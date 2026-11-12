//! A game of Magic, Commander or constructed, as a state machine that
//! stops at every decision (ADR-0043). The engine runs the turn-based and state-based actions
//! itself and stops when a player has to choose, returning a [`Decision`]
//! with the legal answers; [`Game::step`] takes one of them and runs on to
//! the next. A game replays from its seed and its answers.
//!
//! So far: mulligans, the turn's steps with priority, playing lands,
//! casting spells and paying their mana, mana abilities, activated
//! abilities on the stack (so far the fetchlands' search), the stack and
//! resolving permanent spells, commander tax, drawing, discarding to hand
//! size, and the state-based actions for life, an empty library, toughness
//! and a commander's return. No spell's effect runs yet, no trigger, and no
//! creature attacks.
//!
//! # Hidden cards
//!
//! A card can be hidden: the game holds it, in a library or a hand, but
//! no one has said what it is. A replayed log (`crate::replay`) is where
//! this comes from: the log shows a player's draw only when the card is
//! played, cast, discarded or revealed, so a library is that many hidden
//! cards. A hidden card is a [`CardEntry`] whose [`Face`] is `Hidden`, and
//! its text, its cost and what the engine runs of it live only in the
//! `Known` face, so a hidden card can't be played, cast, discarded or have
//! a characteristic read: there is nothing to read. It can be cast face
//! down, since a face-down spell is the same 2/2 whatever its card (CR
//! 708.2a), and it is then a card with morph whenever it is named. [`Game::reveal`] names
//! it, once, which is sound because nothing in the game could have told
//! the difference before; the cards a deck file builds are known from the
//! start, so a goldfish never has one.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use imbl::{OrdMap, OrdSet, Vector};
use vivisurgeons_insight::ast as i;

use crate::deck::{CardText, DeckCards, Oracle, Side, TwoFaced};
use crate::layers::characteristics;
use crate::lower::{
    self, Activated, CardFilter, Cards, DamageTo, Effect, Lowered, Obj, Program, Refusal, Slot,
    TargetSpec, TriggerEvent, Triggered, TurnStep, Who,
};
use crate::mana::{self as m, short};
pub use crate::mana::{Mana, ManaCost, ManaPool, Shard};
use crate::model::{
    self, Ability, CardType, Chars, Cmp, Color, Counter, Filter, Keyword, ObjectId, PlayerId, Pred,
    Seen, State, Subtype, Supertype, Timestamp, YouControl,
};

/// CR 103.4.
pub const STARTING_LIFE: i32 = 20;
/// CR 103.4c, 903.7.
pub const COMMANDER_LIFE: i32 = 40;
/// CR 103.5, 402.2.
pub const HAND_SIZE: usize = 7;
/// CR 903.8.
pub const COMMANDER_TAX: u32 = 2;

/// A physical card, for the whole game: its text and its owner (CR 108.3).
/// The zones other than the battlefield hold cards; on the battlefield a
/// card is an object of `model::State`, a new one each time it arrives
/// (CR 400.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CardId(pub u32);

#[derive(Clone)]
struct CardEntry {
    owner: PlayerId,
    /// A commander (CR 903.3): a designation of the card, not of an object.
    commander: bool,
    /// A token's text, not a card (CR 111.6): it exists only while on the
    /// battlefield (CR 111.7).
    token: bool,
    face: Face,
}

/// What the game knows a card to be (the module's "Hidden cards"), and of
/// a card with two faces which is up.
#[derive(Clone)]
enum Face {
    Hidden,
    One(Arc<Known>),
    /// A card with two faces the engine plays (CR 712, 715), each read as
    /// a card of its own, and the side up: its front in every zone but the
    /// stack and the battlefield (CR 712.8a, 715.4), the face cast or
    /// played on them (CR 712.8f, 715.3b).
    Two(Arc<TwoFaces>, Side),
}

impl Face {
    /// What the game knows of a card's text.
    fn of(text: CardText) -> Face {
        match (text.face(Side::Front), text.face(Side::Back)) {
            (Some(front), Some(back)) => Face::Two(
                Arc::new(TwoFaces {
                    layout: text.layout().expect("two faces"),
                    text,
                    front: known(front),
                    back: known(back),
                }),
                Side::Front,
            ),
            _ => Face::One(known(text)),
        }
    }

    /// The face up, as what the engine makes of it; `None` while hidden.
    fn up(&self) -> Option<&Arc<Known>> {
        match self {
            Face::Hidden => None,
            Face::One(k) => Some(k),
            Face::Two(two, side) => Some(two.face(*side)),
        }
    }
}

/// A card with two faces: the card's text, and what the engine makes of
/// each face.
struct TwoFaces {
    layout: TwoFaced,
    text: CardText,
    front: Arc<Known>,
    back: Arc<Known>,
}

impl TwoFaces {
    fn face(&self, side: Side) -> &Arc<Known> {
        match side {
            Side::Front => &self.front,
            Side::Back => &self.back,
        }
    }
}

/// A card whose identity is known: its text, and what the engine makes of
/// it.
struct Known {
    text: CardText,
    /// Its printed characteristics, if the layers can read them, whether or
    /// not the game runs the rest of it: what a search matches.
    printed: Option<Chars>,
    runs: Result<Runnable, Unrunnable>,
    cost: Result<Option<ManaCost>, Unrunnable>,
    /// Its morph ability, read whether or not the game runs the rest: a
    /// face-down spell is the same 2/2 whatever its card (CR 702.37c).
    morph: Option<Morph>,
}

/// A card's morph ability (CR 702.37a).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Morph {
    /// Turned face up for this mana.
    Mana(ManaCost),
    /// A cost other than mana, which the engine never pays.
    Other,
}

/// What the engine makes of a card's text, read once per text: a game
/// learns the same cards over and over, and Insight's parse is the cost.
#[comemo::memoize]
fn known(text: CardText) -> Arc<Known> {
    Arc::new(Known::read(text))
}

impl Known {
    fn read(text: CardText) -> Known {
        let lowered = text
            .as_card()
            .map_err(|why| match &text.oracle {
                Oracle::Faces { layout, .. } => {
                    Unrunnable::new(format!("a multi-faced card ({layout})"), why)
                }
                _ => Unrunnable::new("a multi-faced card", why),
            })
            .and_then(|c| lower::card(&c).map_err(unlowered));
        // A card the engine can't run still has a type line, mana value and
        // colours, which is all a card outside the battlefield is matched
        // by: a multi-faced card of a layout not played yet by its front
        // face (CR 712.8a).
        let printed = match &lowered {
            Ok(l) => Some(l.chars.clone()),
            Err(_) => lower::printed(&text).ok(),
        };
        let morph = lowered.as_ref().ok().and_then(|l| {
            l.unread.iter().find_map(|a| match a {
                i::Ability::Keywords(ks) => ks.iter().find_map(morph_cost),
                _ => None,
            })
        });
        let morph = morph.map(|cost| cost.map_or(Morph::Other, Morph::Mana));
        let runs = lowered.and_then(runnable).and_then(|r| {
            // A token it creates must run too, or it couldn't enter.
            let refused = r
                .tokens()
                .find_map(|t| known(t.clone()).runs.as_ref().err().cloned());
            match refused {
                Some(why) => Err(Unrunnable::new(
                    format!("a token: {}", why.node),
                    format!("a token it creates: {why}"),
                )),
                None => Ok(r),
            }
        });
        let cost =
            ManaCost::parse(&text.mana_cost).map_err(|why| Unrunnable::new(why.clone(), why));
        Known {
            text,
            printed,
            runs,
            cost,
            morph,
        }
    }

    /// Whether a search for `filter` finds this card: Ok if it matches and,
    /// when it `enters` the battlefield, the engine can put it there; the
    /// refusal if it matches and the engine can't, and `Ok(false)` if it
    /// doesn't match.
    fn found_by(&self, filter: &CardFilter, enters: bool) -> Result<bool, Unrunnable> {
        if !self.printed.as_ref().is_some_and(|c| filter.matches(c)) {
            return Ok(false);
        }
        if !enters {
            return Ok(true);
        }
        self.runs.as_ref().map(|_| true).map_err(Clone::clone)
    }
}

/// A static ability of a permanent's that says something of its
/// controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ForYou {
    /// "You have hexproof" (CR 702.11c).
    Hexproof,
    /// "You have shroud" (CR 702.18a).
    Shroud,
    /// "Skip your draw step" (CR 614.1b, 614.10).
    SkipDrawStep,
    /// "Prevent all damage that would be dealt to you" (CR 615.1a).
    PreventAllDamage,
}

impl ForYou {
    fn read(parts: &[i::Static]) -> Option<ForYou> {
        let you = |r: &i::Recipient| matches!(r, i::Recipient::Player(i::Player::You));
        match parts {
            [i::Static::YouHave(ks)] => match ks.as_slice() {
                [k] if k.param.is_none() && k.name.eq_ignore_ascii_case("hexproof") => {
                    Some(ForYou::Hexproof)
                }
                [k] if k.param.is_none() && k.name.eq_ignore_ascii_case("shroud") => {
                    Some(ForYou::Shroud)
                }
                _ => None,
            },
            [i::Static::SkipStep(i::Step::DrawStep(i::Whose::Yours))] => Some(ForYou::SkipDrawStep),
            [i::Static::PreventDamage {
                combat_only: false,
                to: Some(to),
                by: None,
            }] if matches!(to.as_slice(), [r] if you(r)) => Some(ForYou::PreventAllDamage),
            _ => None,
        }
    }
}

/// An action a card in an opening hand allows (CR 103.6).
#[derive(Clone, Debug)]
enum Opening {
    /// "If this card is in your opening hand, you may begin the game with
    /// it on the battlefield" (CR 103.6a).
    Begins,
    /// "You may reveal this card from your opening hand. If you do, at the
    /// beginning of the first upkeep, …": the delayed trigger's
    /// instructions (CR 103.6b).
    Reveals(Program),
}

/// What the game can run of a card: its lowered characteristics and, of
/// the abilities the layers carry unread, the ones this engine runs, by
/// their index (`model::Ability::Unread`).
struct Runnable {
    lowered: Lowered,
    mana: BTreeMap<u16, m::ManaAbility>,
    /// Its other activated abilities (CR 602): the ones the engine runs,
    /// and why it can't run the rest, which are never offered.
    activated: BTreeMap<u16, Result<Activated, Unrunnable>>,
    enters: BTreeMap<u16, Enters>,
    /// Its triggered abilities (CR 603), all of which the engine runs: a
    /// card with one it can't is refused.
    triggered: BTreeMap<u16, Triggered>,
    /// An instant's or sorcery's instructions (CR 113.3a), its paragraphs
    /// in order.
    spell: Option<Program>,
    flash: bool,
    /// What it lets its owner do from their opening hand (CR 103.6).
    opening: Option<Opening>,
    /// Cycling (CR 702.29a), by its index, and its cost and instruction.
    cycling: Option<(u16, Activated)>,
    /// What its statics say of its controller ("you have hexproof", "skip
    /// your draw step"), by the ability's index, so each works only while
    /// the permanent has it.
    for_you: BTreeMap<u16, ForYou>,
    /// Statics that change what spells cost (CR 601.2f), by index, so
    /// each works only while the permanent has it.
    cost_changes: BTreeMap<u16, lower::CostChange>,
    /// The costs it may be cast for rather than its mana cost, and from
    /// where (CR 118.9): flashback, Fireblast's sacrifice. By the index of
    /// the ability that allows it.
    alternatives: BTreeMap<u16, lower::Alternative>,
    /// Kicker's cost, the additional cost it may be cast with (CR 702.33a).
    kicker: Option<ManaCost>,
    /// An Aura's enchant ability: what it targets as a spell and may
    /// enchant (CR 303.4a, 702.5a), read from its controller.
    enchant: Option<Filter>,
    /// Its triggered mana abilities (CR 605.1b), by index.
    mana_triggers: BTreeMap<u16, lower::ManaTrigger>,
    /// "You may play an additional land on each of your turns" (CR
    /// 305.2a), by index: how many more.
    extra_lands: BTreeMap<u16, u32>,
    /// The abilities it carries that nothing offers to activate yet.
    dormant: Vec<String>,
}

impl Runnable {
    /// The tokens its instructions create, as their text.
    fn tokens(&self) -> impl Iterator<Item = &CardText> {
        fn walk<'a>(effects: &'a [Effect], out: &mut Vec<&'a CardText>) {
            for e in effects {
                if let Effect::Create { token, .. } = e {
                    out.push(token);
                }
                if let Some(inner) = e.inner() {
                    walk(inner, out);
                }
            }
        }
        let mut out = Vec::new();
        if let Some(p) = &self.spell {
            walk(&p.effects, &mut out);
        }
        for t in self.triggered.values() {
            walk(&t.program.effects, &mut out);
        }
        for a in self.activated.values().flatten() {
            walk(&a.program.effects, &mut out);
        }
        out.into_iter()
    }
}

/// A replacement effect on how its own permanent enters (CR 614.1c–d).
#[derive(PartialEq)]
enum Enters {
    /// "This land enters tapped."
    Tapped,
    /// "This land enters tapped unless you control a legendary green
    /// creature", "… two or fewer other lands", "… a Mountain or a Plains".
    TappedUnless(YouControl),
    /// "If you control two or more other lands, this land enters tapped."
    TappedIf(YouControl),
    /// "As this land enters, you may pay 2 life. If you don't, it enters
    /// tapped."
    TappedUnlessPaysLife(u32),
    /// Mox Diamond's "If this artifact would enter, you may discard a
    /// land card instead. If you do, put this artifact onto the
    /// battlefield. If you don't, put it into its owner's graveyard."
    DiscardOrGraveyard(CardFilter),
    /// "This land enters with three mining counters on it" (CR 614.1c,
    /// 122.6).
    WithCounters(Counter, u32),
    /// "As this enchantment enters, choose a creature type" (CR 614.12a).
    ChooseCreatureType,
}

/// Why the engine refuses a card: the node of Insight's tree (or the rule)
/// it can't run yet, and the whole reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unrunnable {
    pub node: String,
    pub why: String,
}

impl fmt::Display for Unrunnable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.why)
    }
}

impl Unrunnable {
    fn new(node: impl Into<String>, why: impl Into<String>) -> Unrunnable {
        Unrunnable {
            node: node.into(),
            why: why.into(),
        }
    }
}

/// A turn's steps, and the main phases, which have none (CR 500.1). With
/// no attackers the declare blockers and combat damage steps are skipped
/// (CR 508.8), and the first-strike damage step happens only when an
/// attacking or blocking creature has first strike or double strike (CR
/// 510.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    Untap,
    Upkeep,
    Draw,
    PrecombatMain,
    BeginningOfCombat,
    DeclareAttackers,
    DeclareBlockers,
    FirstStrikeDamage,
    CombatDamage,
    EndOfCombat,
    PostcombatMain,
    End,
    Cleanup,
}

impl Step {
    pub fn is_main(self) -> bool {
        matches!(self, Step::PrecombatMain | Step::PostcombatMain)
    }

    /// The step after this one in the turn, or `None` after cleanup.
    fn next(self) -> Option<Step> {
        Some(match self {
            Step::Untap => Step::Upkeep,
            Step::Upkeep => Step::Draw,
            Step::Draw => Step::PrecombatMain,
            Step::PrecombatMain => Step::BeginningOfCombat,
            Step::BeginningOfCombat => Step::DeclareAttackers,
            Step::DeclareAttackers => Step::DeclareBlockers,
            Step::DeclareBlockers => Step::FirstStrikeDamage,
            Step::FirstStrikeDamage => Step::CombatDamage,
            Step::CombatDamage => Step::EndOfCombat,
            Step::EndOfCombat => Step::PostcombatMain,
            Step::PostcombatMain => Step::End,
            Step::End => Step::Cleanup,
            Step::Cleanup => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Player {
    life: i32,
    /// The top is the front.
    library: Vector<CardId>,
    hand: Vector<CardId>,
    graveyard: Vector<CardId>,
    pool: ManaPool,
    lands_played: u32,
    /// CR 704.5b: drew from an empty library since state-based actions
    /// were last checked.
    drew_from_empty: bool,
    /// The turn number their most recent turn began on (CR 302.6).
    turn_began: Option<u32>,
    /// The turn number the turn before it began on: their last upkeep's,
    /// for echo (CR 702.30a).
    last_turn: Option<u32>,
    /// Whether they have lost life this turn (CR 119.3).
    lost_life: bool,
    /// The cards they drew this turn (CR 121.1), wherever they are now.
    drawn: Vector<CardId>,
    /// Their commanders' colour identity (CR 903.4), or `None` with no
    /// commander (CR 903.4f).
    identity: Option<Vec<Color>>,
    /// Mulligans taken (CR 103.5).
    mulligans: u32,
    /// Declared they keep, so takes no more mulligans (CR 103.5).
    kept: bool,
    left: bool,
}

impl Player {
    pub fn life(&self) -> i32 {
        self.life
    }
    pub fn library(&self) -> &Vector<CardId> {
        &self.library
    }
    pub fn hand(&self) -> &Vector<CardId> {
        &self.hand
    }
    pub fn graveyard(&self) -> &Vector<CardId> {
        &self.graveyard
    }
    pub fn pool(&self) -> &ManaPool {
        &self.pool
    }
    pub fn lands_played(&self) -> u32 {
        self.lands_played
    }
    pub fn mulligans(&self) -> u32 {
        self.mulligans
    }
    /// Lost, and so left the game (CR 104.3, 800.4a).
    pub fn has_left(&self) -> bool {
        self.left
    }
}

/// A permanent's status (CR 110.5) and since when it has been there; what
/// it is comes from the layers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Permanent {
    pub card: CardId,
    pub tapped: bool,
    /// The turn it came under its controller's control (CR 302.6).
    pub since: u32,
    /// Damage marked on it this turn (CR 120.3e), and whether a source
    /// with deathtouch dealt any (CR 704.5h).
    pub damage: u32,
    pub deathtouched: bool,
    /// Face down (CR 708.2): its card may be hidden, and nothing of it but
    /// a 2/2 creature with no name is seen until it is turned face up.
    pub face_down: bool,
}

/// What a target is (CR 115.1): a player, or a permanent, which is a new
/// object each time it enters (CR 400.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    Player(PlayerId),
    Object(ObjectId),
    /// A spell on the stack, by its card.
    Spell(CardId),
}

/// What can be on the stack (CR 405.1): spells, activated abilities other
/// than mana abilities, and triggered abilities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackObject {
    /// A card on the stack (CR 112.1), controlled by who cast it (CR
    /// 601.2a), with the targets chosen as it was cast (CR 601.2c), by
    /// slot.
    Spell {
        card: CardId,
        controller: PlayerId,
        targets: Vec<Target>,
        /// How it was cast: for its mana cost, or an alternative cost
        /// (CR 601.2b).
        via: Via,
        /// Its kicker was paid (CR 702.33d).
        kicked: bool,
    },
    /// A triggered ability (CR 603.3): the card whose ability it is, by
    /// its index among that card's unread abilities, the permanent it
    /// triggered from (CR 113.7a), and the targets chosen as it was put on
    /// the stack (CR 603.3d). `seq` counts the triggered abilities put on
    /// the stack this game, so each is told apart.
    Trigger {
        source: CardId,
        /// The permanent it triggered from; none for a delayed trigger.
        object: Option<ObjectId>,
        ability: TriggerOf,
        controller: PlayerId,
        targets: Vec<Target>,
        /// The object its event names, exalted's attacking creature.
        triggering: Option<ObjectId>,
        /// The amount its event names.
        amount: Option<u32>,
        /// The player its event names: a spell's caster.
        player: Option<PlayerId>,
        seq: u32,
    },
    /// An activated ability (CR 602.2a): the card whose ability it is, by
    /// its index among that card's unread abilities, and who activated
    /// it. It exists independently of its source (CR 113.7a), so a source
    /// sacrificed to pay for it changes nothing.
    Ability {
        source: CardId,
        /// The permanent it was activated from, "this artifact", or none
        /// for one activated from a hand (cycling, CR 702.29a).
        object: Option<ObjectId>,
        ability: u16,
        controller: PlayerId,
        /// Chosen as it was activated (CR 602.2b, 601.2c), by slot.
        targets: Vec<Target>,
    },
}

/// How a spell was cast (CR 601.2b, 601.2f): for its mana cost, from a
/// hand or the command zone, or for one of its card's alternative costs
/// (CR 118.9), by the index of the ability that allows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Via {
    ManaCost,
    Alternative(u16),
    /// Face down for {3} by its morph ability, a 2/2 with no text, name,
    /// subtypes or mana cost (CR 702.37c, 708.4).
    FaceDown,
}

/// Whose instructions are resolving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Instructions {
    Spell {
        card: CardId,
        via: Via,
        kicked: bool,
    },
    Activated(CardId, u16),
    Triggered(CardId, TriggerOf),
}

/// Which triggered ability: one a card prints, by its index among the
/// card's unread abilities, or a delayed one a resolving spell or ability
/// created (CR 603.7a), by its place in `Game::delayed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TriggerOf {
    Printed(u16),
    Delayed(u32),
}

/// A delayed triggered ability (CR 603.7a): whose, and what it waits for,
/// its instructions those of the effect that created it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Delayed {
    source: CardId,
    controller: PlayerId,
    step: TurnStep,
    /// The turn it was created in: it waits for a later one's step.
    created: u32,
    program: Program,
    /// It triggers once (CR 603.7c).
    fired: bool,
}

impl Instructions {
    /// The card whose spell or ability it is.
    fn card(self) -> CardId {
        match self {
            Instructions::Spell { card: c, .. }
            | Instructions::Activated(c, _)
            | Instructions::Triggered(c, _) => c,
        }
    }
}

/// Where a spell's or ability's resolution stands when it stops for a
/// choice: whose instructions, its targets, and the instruction after the
/// one being followed (CR 608.2c).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Resuming {
    what: Instructions,
    controller: PlayerId,
    /// The permanent whose ability it is, "this creature".
    object: Option<ObjectId>,
    /// Each target slot's target, or `None` if it is illegal now: an
    /// instruction won't act on an illegal target (CR 608.2b).
    targets: Vec<Option<Target>>,
    /// The object the trigger event named.
    triggering: Option<ObjectId>,
    /// The amount the trigger event named, "that much".
    amount: Option<u32>,
    /// The player the trigger event named, "that player".
    player: Option<PlayerId>,
    /// The instruction to follow next: its index in the program's list,
    /// then in the list of each "may" being followed inside it.
    next: Vec<usize>,
    /// The cards its instructions have found, by the slot each binds
    /// (`lower::Slot`).
    found: OrdMap<Slot, Vec<CardId>>,
    /// The card name an instruction chose (CR 201.4), "that name".
    name: Option<String>,
}

/// A triggered ability that has triggered and waits to be put on the stack
/// the next time a player would receive priority (CR 603.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pending {
    pub source: CardId,
    pub object: Option<ObjectId>,
    pub ability: TriggerOf,
    pub controller: PlayerId,
    /// The object its event names, exalted's attacking creature.
    pub triggering: Option<ObjectId>,
    /// The amount its event names, the damage dealt.
    pub amount: Option<u32>,
    /// The player its event names, the caster of a spell.
    pub player: Option<PlayerId>,
    /// Its controller has put it in order among their others (CR 603.3b).
    ordered: bool,
}

/// What a mana cost being paid is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Paying {
    /// A spell, from the command zone or not (CR 601.2g–h).
    Spell { from_command: bool },
    /// An activated ability of the card's (CR 602.2b).
    Ability,
    /// The morph cost of this face-down permanent, turning it face up, a
    /// special action (CR 702.37e, 116.2b).
    FaceUp(ObjectId),
}

/// What happened that a triggered ability may be waiting for (CR 603.2).
enum Event {
    /// A permanent entered the battlefield (CR 603.6a).
    Entered(ObjectId),
    /// These creatures were declared as attackers (CR 508.3a).
    Attacked(Vec<ObjectId>),
    /// A player cast this spell (CR 601.2i).
    Cast { card: CardId, controller: PlayerId },
    /// These permanents were dealt this much damage, at once.
    Damaged(BTreeMap<ObjectId, u32>),
    /// This permanent dealt damage to a player, combat damage or not.
    DamagedPlayer { source: ObjectId, combat: bool },
    /// This permanent dealt this much damage, to anything, at once.
    DealtBy { source: ObjectId, amount: u32 },
    /// This permanent was turned face up (CR 702.37e).
    TurnedFaceUp(ObjectId),
    /// A source this player controls dealt damage to that player, a spell
    /// or a permanent (CR 120.3).
    PlayerDamaged {
        player: PlayerId,
        controller: PlayerId,
    },
    /// A step of the active player's turn began.
    Begins(TurnStep),
    /// This creature died: it is gone, as it last existed (CR 700.4).
    Died(ObjectId),
    /// This player drew a card (CR 121.2).
    Drew(PlayerId),
    /// This permanent became tapped (CR 701.26a).
    BecameTapped(ObjectId),
}

/// The combat phase's state (CR 506): who attacks whom, who blocks what.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Combat {
    /// Each attacking creature and the player it attacks (CR 508.1b).
    attackers: OrdMap<ObjectId, PlayerId>,
    /// Each blocking creature and the attacker it blocks (CR 509.1a).
    blockers: OrdMap<ObjectId, ObjectId>,
    /// The attackers blocked, which stay blocked when their blockers go
    /// (CR 509.1h).
    blocked: OrdSet<ObjectId>,
    /// The creatures that dealt first-strike damage (CR 510.4).
    struck: OrdSet<ObjectId>,
}

/// Combat damage assigned before it is dealt (CR 510.1–510.2): the source,
/// what it is dealt to, and how much.
type Assigned = Vec<(ObjectId, Target, u32)>;

/// What targets are being chosen for (CR 601.2c, 603.3d).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Choosing {
    /// The spell on top of the stack, then its cost is paid.
    Spell { owed: ManaCost, from_command: bool },
    /// The activated ability on top of the stack, from `object`, then its
    /// mana cost, if it has one, is paid (CR 602.2b).
    Ability {
        object: ObjectId,
        owed: Option<ManaCost>,
    },
    /// A triggered ability being put on the stack, then `then` gets
    /// priority.
    Trigger { pending: Pending, then: PlayerId },
}

/// What happens once a permanent has entered.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Then {
    /// This player gets priority (CR 117.3b, 117.3c).
    Priority(PlayerId),
    /// The resolving ability goes on.
    Resume(Resuming),
}

/// What a search found (CR 701.23).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// Nothing: a search for a card with a stated quality needn't find one
    /// (CR 701.23b).
    Nothing,
    /// A card of the library whose name is known.
    Card(CardId),
    /// A hidden card of the library, named as it is found: it was some
    /// card all along, and nothing in the game could tell which until now
    /// (`game`'s "Hidden cards"). It must match what was searched for.
    Hidden(CardId, Box<CardText>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// CR 104.2a.
    Won(PlayerId),
    /// CR 104.4a.
    Draw,
}

/// What the game is waiting on.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Waiting {
    /// CR 103.5: `player` declares whether they take a mulligan; `taking`
    /// are those who declared one earlier this round.
    Mulligan {
        player: PlayerId,
        taking: Vec<PlayerId>,
    },
    /// CR 103.5: `player` took a mulligan and puts `count` cards of their
    /// new hand on the bottom; `then` are the others who took one this
    /// round, still to do so.
    Bottom {
        player: PlayerId,
        count: usize,
        then: Vec<PlayerId>,
    },
    /// CR 103.6: `player` takes the actions their opening hand's cards
    /// allow, the starting player first.
    OpeningHand {
        player: PlayerId,
    },
    /// `passes` players have passed in succession before this one (CR 117.4).
    Priority {
        player: PlayerId,
        passes: usize,
    },
    /// Casting `spell`, which is on the stack, with `owed` left to pay (CR
    /// 601.2g–h).
    Paying {
        player: PlayerId,
        spell: CardId,
        owed: ManaCost,
        paying: Paying,
    },
    /// CR 601.2b: `player` casting `card`, whose cost so far is `base`,
    /// announces whether they pay its kicker.
    Kicker {
        player: PlayerId,
        card: CardId,
        base: ManaCost,
        from_command: bool,
    },
    /// CR 514.1, or an instruction (CR 701.9a), which `then` goes on with,
    /// the cards discarded bound to its slot.
    Discard {
        player: PlayerId,
        count: usize,
        then: Option<(Resuming, Slot)>,
    },
    /// `player` chooses targets for each of `slots` (CR 601.2c, 603.3d).
    Targets {
        player: PlayerId,
        card: CardId,
        slots: Vec<TargetSpec>,
        choosing: Choosing,
    },
    /// CR 603.3b: `player` puts these triggered abilities on the stack in
    /// the order they choose, then `then` gets priority.
    Order {
        player: PlayerId,
        pending: Vec<Pending>,
        then: PlayerId,
    },
    /// A resolving instruction has `chooser` choose a card `filter`
    /// matches from `whose` revealed hand, to be discarded.
    ChooseFromHand {
        chooser: PlayerId,
        whose: PlayerId,
        filter: CardFilter,
        resume: Resuming,
    },
    /// CR 701.22a.
    Scry {
        player: PlayerId,
        cards: Vec<CardId>,
        resume: Resuming,
    },
    /// CR 608.2d, 603.5: `player` chooses whether to follow the "may"
    /// instruction `at` points to.
    May {
        player: PlayerId,
        at: Resuming,
    },
    /// Cards revealed from a library that no one had named: the game
    /// names them now (the module's "Hidden cards").
    Reveal {
        player: PlayerId,
        cards: Vec<CardId>,
        resume: Resuming,
    },
    /// `player` chooses `n` of `cards`, bound to `into`.
    ChooseAmong {
        player: PlayerId,
        cards: Vec<CardId>,
        n: usize,
        into: Slot,
        resume: Resuming,
    },
    /// `player` orders `cards` to put on top or bottom of a library.
    Arrange {
        player: PlayerId,
        cards: Vec<CardId>,
        top: bool,
        resume: Resuming,
    },
    /// `player` chooses a card of their hand `filter` matches to put onto
    /// the battlefield.
    PutFromHand {
        player: PlayerId,
        filter: CardFilter,
        tapped: bool,
        resume: Resuming,
    },
    /// CR 508.1: the active player declares attackers.
    Attackers {
        player: PlayerId,
    },
    /// CR 509.1: `player` declares blockers; `rest` are the other
    /// defending players, still to.
    Blockers {
        player: PlayerId,
        rest: Vec<PlayerId>,
    },
    /// CR 510.1c, 702.19b: `source`'s controller divides its combat damage
    /// among `to`; `left` are the creatures still to assign theirs, and
    /// `assigned` what is assigned so far, in the first-strike step or not.
    Assign {
        player: PlayerId,
        source: ObjectId,
        to: Vec<Target>,
        left: Vec<ObjectId>,
        assigned: Assigned,
        first: bool,
    },
    /// CR 704.5j, before `then` gets priority.
    LegendRule {
        player: PlayerId,
        among: Vec<ObjectId>,
        then: PlayerId,
        passes: usize,
    },
    /// CR 903.9a, before `then` gets priority.
    ReturnCommander {
        owner: PlayerId,
        card: CardId,
        then: PlayerId,
        passes: usize,
    },
    /// CR 614.12a: `card` is about to enter under `player`'s control, who
    /// chooses a creature type for it first; then `then`.
    ChooseCreatureType {
        player: PlayerId,
        card: CardId,
        tapped: bool,
        then: Then,
    },
    /// CR 614.1c, 614.12a: `card` is about to enter under `player`'s
    /// control, who may pay `life` or have it enter tapped; then `then`.
    PayOrEnterTapped {
        player: PlayerId,
        card: CardId,
        life: u32,
        /// Whether it enters tapped all the same.
        tapped: bool,
        then: Then,
    },
    /// CR 614.1a, 614.12: `card` would enter under `player`'s control,
    /// who may discard a card `filter` matches instead; it enters if they
    /// do and goes to its owner's graveyard if not.
    DiscardToEnter {
        player: PlayerId,
        card: CardId,
        filter: CardFilter,
        tapped: bool,
        then: Then,
    },
    /// A resolving ability has `player` search their library (CR 701.23a)
    /// for a card `filter` matches, bound to `into`.
    Search {
        player: PlayerId,
        filter: CardFilter,
        /// Whether what it finds is put onto the battlefield, so must be
        /// a card the engine runs.
        enters: bool,
        reveals: bool,
        into: Slot,
        resume: Resuming,
    },
    /// CR 601.2h: `player` sacrifices `count` permanents `filter`
    /// matches to pay for `spell`, the rest of its cost paid.
    Sacrifice {
        player: PlayerId,
        spell: CardId,
        count: u32,
        filter: Filter,
    },
    /// CR 601.2h: `player` returns `count` permanents `filter` matches to
    /// their owner's hand to pay for `spell`, Gush's cost.
    ReturnToHand {
        player: PlayerId,
        spell: CardId,
        count: u32,
        filter: Filter,
    },
    /// CR 119.4: `player` may pay `life` for the "pay life or" instruction
    /// `at` points to.
    PayLifeOr {
        player: PlayerId,
        life: u32,
        at: Resuming,
    },
    /// CR 118.12a: `player` may pay `owed` for the "unless" instruction
    /// `at` points to.
    PayUnless {
        player: PlayerId,
        owed: ManaCost,
        at: Resuming,
    },
    /// CR 118.12a: `player` may discard a card `filter` matches for the
    /// "unless" instruction `at` points to.
    DiscardUnless {
        player: PlayerId,
        filter: CardFilter,
        at: Resuming,
    },
    /// CR 118.12a: `player` may sacrifice permanents `filter` matches
    /// whose powers add up to at least `power` for the "unless"
    /// instruction `at` points to.
    SacrificeUnless {
        player: PlayerId,
        filter: Filter,
        power: u32,
        at: Resuming,
    },
    /// CR 106.3: `player` chooses which of `options` the resolving
    /// instruction adds to their pool.
    ChooseMana {
        player: PlayerId,
        options: Vec<ManaPool>,
        resume: Resuming,
    },
    /// CR 201.4: `player` names a card `filter` matches for the resolving
    /// instruction.
    ChooseName {
        player: PlayerId,
        filter: CardFilter,
        resume: Resuming,
    },
    Over(Outcome),
}

/// Who has to choose, and the legal answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// CR 103.5: take a mulligan (`Answer::Mulligan(true)`) or keep.
    Mulligan {
        player: PlayerId,
    },
    /// CR 103.5: put exactly `count` distinct cards of `hand` on the bottom
    /// of the library, in the order given (`Answer::Bottom`).
    Bottom {
        player: PlayerId,
        count: usize,
        hand: Vec<CardId>,
    },
    /// CR 103.6, 103.6a–b: take the action any of `cards`, of `player`'s
    /// opening hand, allows, answered `Answer::Begin`: put onto the
    /// battlefield, or revealed for a delayed trigger at the first upkeep.
    /// Asked only of a player with such a card: a hidden one has to be
    /// named first, as the hand is kept.
    OpeningHand {
        player: PlayerId,
        cards: Vec<CardId>,
    },
    Priority {
        player: PlayerId,
        actions: Vec<Action>,
    },
    /// CR 601.2g–h: `spell` is on the stack and its total cost is
    /// determined; activate mana abilities, then pay `owed` from the pool,
    /// or cancel the cast (CR 733.1).
    Pay {
        player: PlayerId,
        spell: CardId,
        owed: ManaCost,
        actions: Vec<Payment>,
    },
    /// Discard exactly `count` distinct cards of `hand` (CR 514.1). The
    /// legal answers are every such set, given by their rule rather than
    /// listed.
    Discard {
        player: PlayerId,
        count: usize,
        hand: Vec<CardId>,
    },
    /// CR 903.9a: the commander went to a graveyard or exile, and its
    /// owner may put it into the command zone. Answered with
    /// `Answer::CommandZone`.
    ReturnCommander {
        player: PlayerId,
        card: CardId,
    },
    /// CR 704.5j: `player` controls these legendary permanents with the
    /// same name, and keeps one, answered with `Answer::Keep`; the rest go
    /// to their owners' graveyards.
    LegendRule {
        player: PlayerId,
        among: Vec<ObjectId>,
    },
    /// CR 601.2b, 702.33a: `player` casting `card` announces whether they
    /// pay its kicker, `cost` more, answered `Answer::Kick`.
    Kicker {
        player: PlayerId,
        card: CardId,
        cost: ManaCost,
    },
    /// CR 614.12a, 205.3m: as `card` enters, `player` chooses a creature
    /// type, answered `Answer::CreatureType`.
    ChooseCreatureType {
        player: PlayerId,
        card: CardId,
    },
    /// CR 614.1c, 614.12a: as `card` enters, `player` may pay `life`
    /// (`Answer::PayLife(true)`); if they don't, it enters tapped. A shock
    /// land's "As this land enters, you may pay 2 life".
    PayOrEnterTapped {
        player: PlayerId,
        card: CardId,
        life: u32,
    },
    /// CR 614.1a: `card` enters only if `player` discards a card of
    /// `legal` instead, answered `Pick`, `None` to put it into its
    /// owner's graveyard; `unsure` counts the hidden cards in hand.
    DiscardToEnter {
        player: PlayerId,
        card: CardId,
        legal: Vec<CardId>,
        unsure: usize,
    },
    /// CR 601.2c, 603.3d: choose a target for each slot, among its legal
    /// ones, answered with `Answer::Targets`.
    Targets {
        player: PlayerId,
        card: CardId,
        slots: Vec<Vec<Target>>,
    },
    /// CR 603.3b: put these triggered abilities, of these cards, on the
    /// stack in an order, answered with `Answer::Order`: a permutation of
    /// their indices, the first put on the stack first.
    Order {
        player: PlayerId,
        triggers: Vec<CardId>,
    },
    /// CR 701.20a: `whose` hand is revealed, and `player` chooses a card of
    /// it `legal` lists, answered with `Answer::Pick`; `None` is legal
    /// only when no card can match: none listed, none hidden and none the
    /// engine can't read.
    ChooseFromHand {
        player: PlayerId,
        whose: PlayerId,
        hand: Vec<CardId>,
        legal: Vec<CardId>,
        unsure: usize,
    },
    /// CR 608.2d, 603.5: follow an instruction of `card`'s that says "you
    /// may", or not, answered with `Answer::May`.
    May {
        player: PlayerId,
        card: CardId,
    },
    /// Hidden cards of `player`'s library are revealed (CR 701.20a), and
    /// what each is has to be said now, answered `Answer::Reveal` with a
    /// card for each, in order. A deck file's cards are all known, so
    /// only a replay is asked this (the module's "Hidden cards").
    Reveal {
        player: PlayerId,
        cards: Vec<CardId>,
    },
    /// "Put one of them into your hand": choose exactly `n` of `cards`,
    /// answered `Answer::Choose`.
    ChooseAmong {
        player: PlayerId,
        cards: Vec<CardId>,
        n: usize,
    },
    /// "Put them back in any order" (CR 401.4): `cards` go on top of
    /// their library, or the bottom, in the order `Answer::Arrange` gives,
    /// the first topmost.
    Arrange {
        player: PlayerId,
        cards: Vec<CardId>,
        top: bool,
    },
    /// "Put a Goblin permanent card from your hand onto the battlefield":
    /// `player` chooses a card of their `hand` that `legal` lists,
    /// answered with `Answer::Pick`; `None` is legal only when no card can
    /// match: none listed, none hidden and none the engine can't read.
    PutFromHand {
        player: PlayerId,
        hand: Vec<CardId>,
        legal: Vec<CardId>,
        unsure: usize,
    },
    /// CR 701.22a: look at `cards`, the top of the library, and put any
    /// number on the bottom and the rest on top in any order, answered
    /// with `Answer::Scry`.
    Scry {
        player: PlayerId,
        cards: Vec<CardId>,
    },
    /// CR 508.1a: the creatures `player` may attack with, and whom.
    /// Answered with `Answer::Attack`, none for no attack.
    Attackers {
        player: PlayerId,
        can: Vec<ObjectId>,
        defenders: Vec<PlayerId>,
    },
    /// CR 509.1a: the creatures `player` may block with, and the attackers
    /// attacking them. Answered with `Answer::Block`, legal when flying
    /// and menace are obeyed (CR 702.9b, 702.111b).
    Blockers {
        player: PlayerId,
        can: Vec<ObjectId>,
        attackers: Vec<ObjectId>,
    },
    /// CR 510.1c, 702.19b: `source` assigns `power` combat damage among
    /// `to`, answered with `Answer::Assign`, an amount for each. With
    /// trample `to` ends with the player it attacks, who gets damage only
    /// once each blocker is assigned `lethal` (CR 702.19b).
    Assign {
        player: PlayerId,
        source: ObjectId,
        power: u32,
        to: Vec<Target>,
        lethal: Vec<u32>,
    },
    /// CR 701.23a–b: search your library for a card `filter` matches,
    /// answered with `Answer::Find`. `known` are the library's cards whose
    /// names are known that match; any of its `hidden` cards might too.
    /// `reveals` if an instruction after reveals what it finds.
    Search {
        player: PlayerId,
        filter: CardFilter,
        reveals: bool,
        known: Vec<CardId>,
        hidden: usize,
    },
    /// CR 601.2h, 701.21a: sacrifice exactly `count` of `legal`, the
    /// permanents `player` controls that the cost names, to pay for
    /// `spell`, answered `Answer::Sacrifice`.
    Sacrifice {
        player: PlayerId,
        spell: CardId,
        count: u32,
        legal: Vec<ObjectId>,
    },
    /// CR 601.2h: return exactly `count` of `legal`, the permanents
    /// `player` controls that the cost names, to their owner's hand to pay
    /// for `spell`, answered `Answer::ReturnToHand`.
    ReturnToHand {
        player: PlayerId,
        spell: CardId,
        count: u32,
        legal: Vec<ObjectId>,
    },
    /// CR 119.4: `card`'s resolving instruction says "pay `life` life or
    /// …", answered `Answer::PayLife`: `true` pays it, `false` has the rest
    /// followed. Asked only of a player with that much life.
    PayLifeOr {
        player: PlayerId,
        card: CardId,
        life: u32,
    },
    /// CR 118.12a: pay `owed` for `card`'s "unless", activating mana
    /// abilities first, or not: answered `Answer::Pay`, `Payment::Cancel`
    /// declining, so what follows the "unless" happens.
    PayUnless {
        player: PlayerId,
        card: CardId,
        owed: ManaCost,
        actions: Vec<Payment>,
    },
    /// CR 118.12a, 701.9a: discard a card of `legal` for `card`'s
    /// "unless", answered `Pick`, `None` declining, so what follows the
    /// "unless" happens; `unsure` counts the hidden cards in hand.
    DiscardUnless {
        player: PlayerId,
        card: CardId,
        legal: Vec<CardId>,
        unsure: usize,
    },
    /// CR 118.12a, 701.21a: sacrifice any number of `legal` whose powers
    /// add up to at least `power` for `card`'s "unless", answered
    /// `Answer::Sacrifice`, none declining.
    SacrificeUnless {
        player: PlayerId,
        card: CardId,
        legal: Vec<ObjectId>,
        power: u32,
    },
    /// CR 106.3: choose the mana `card`'s instruction adds, one of
    /// `options`, answered `Answer::Mana`.
    ChooseMana {
        player: PlayerId,
        card: CardId,
        options: Vec<ManaPool>,
    },
    /// CR 201.4, 201.4a: name a card of the Oracle card reference that
    /// `filter` matches, for `card`'s instruction, answered
    /// `Answer::Name`.
    ChooseName {
        player: PlayerId,
        card: CardId,
        filter: CardFilter,
    },
    Over(Outcome),
}

/// Activating an activated ability that isn't a mana ability (CR 602):
/// `source`'s `ability`, among those the layers say it has now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Activation {
    pub source: ObjectId,
    pub ability: Ability,
}

/// Activating a mana ability, and the mana it adds of the ways its text
/// allows (CR 605.3b). Its source's ability is `ability`, among those the
/// layers say it has now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ManaActivation {
    pub source: ObjectId,
    pub ability: Ability,
    pub adds: ManaPool,
    /// The mana spent from the pool for its mana cost, the way chosen to
    /// pay a hybrid or generic symbol (CR 605.3a, 601.2h); none for one
    /// without.
    pub pays: ManaPool,
    /// The permanent its cost sacrifices, if it sacrifices one (CR
    /// 701.21a).
    pub sacrifices: Option<ObjectId>,
}

/// What a player with priority can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// CR 117.3d.
    Pass,
    /// A special action (CR 116.2a, 305.1).
    PlayLand(CardId),
    /// Begin casting a spell from hand, or a commander from the command
    /// zone (CR 601.2, 903.8). Listed when its timing allows (CR 117.1a).
    /// A two-faced card is cast by its front (CR 712.11).
    Cast(CardId),
    /// Play a modal double-faced card by its back face, a land (CR
    /// 712.12, 305.1).
    PlayBack(CardId),
    /// Cast a modal double-faced card by its back face (CR 712.11b), or
    /// an adventurer card as its Adventure (CR 715.3).
    CastBack(CardId),
    /// Begin casting a spell for its card's alternative cost `n` (the
    /// index of the ability that allows it), from the zone that cost
    /// allows: flashback from a graveyard (CR 118.9a, 601.2b, 702.34a).
    CastAlternative(CardId, u16),
    /// A mana ability, which doesn't use the stack (CR 605.3b).
    Mana(ManaActivation),
    /// Any other activated ability, which goes on the stack (CR 602.2,
    /// 117.1b).
    Activate(Activation),
    /// A card's cycling from its owner's hand: "[Cost], Discard this card:
    /// Draw a card" (CR 702.29a).
    Cycle(CardId),
    /// Begin casting a card from hand face down by its morph ability, at
    /// a creature's timing (CR 702.37c, 708.4). A hidden card is offered
    /// too: nothing in the game can tell which card is cast face down.
    CastFaceDown(CardId),
    /// Turn a face-down permanent face up by paying its morph cost, a
    /// special action (CR 702.37e, 116.2b).
    TurnFaceUp(ObjectId),
}

/// What a player paying a spell's cost can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Payment {
    /// CR 601.2g.
    Mana(ManaActivation),
    /// Spend exactly this mana from the pool on the whole cost (CR 601.2h,
    /// 118.3a). Every amount that pays it is offered.
    Spend(ManaPool),
    /// Stop casting: the spell returns where it was, and the mana abilities
    /// activated while casting it are reversed (CR 733.1–733.2).
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// CR 103.5: `true` takes a mulligan, `false` keeps.
    Mulligan(bool),
    /// CR 103.5: these cards go on the bottom, the first one first.
    Bottom(Vec<CardId>),
    Act(Action),
    Pay(Payment),
    Discard(Vec<CardId>),
    /// CR 903.9a: put the commander into the command zone, or leave it.
    CommandZone(bool),
    /// CR 704.5j: the legendary permanent kept.
    Keep(ObjectId),
    /// CR 701.23: what the search found.
    Find(Found),
    /// CR 614.12a, 119.4: pay the life a permanent's replacement effect
    /// offers as it enters, or not.
    PayLife(bool),
    /// CR 601.2b: `true` pays the spell's kicker.
    Kick(bool),
    /// CR 601.2c: one target per slot.
    Targets(Vec<Target>),
    /// CR 603.3b.
    Order(Vec<usize>),
    /// CR 701.20a: the card chosen from a revealed hand, if any can be;
    /// or from a hand to put onto the battlefield.
    Pick(Option<CardId>),
    /// CR 608.2d: follow the "may" instruction, or not.
    May(bool),
    /// CR 103.6a–b: the cards of the opening hand whose action is taken,
    /// put onto the battlefield or revealed.
    Begin(Vec<CardId>),
    /// What each hidden card revealed is, in the decision's order.
    Reveal(Vec<CardText>),
    /// The cards chosen.
    Choose(Vec<CardId>),
    /// The cards in the order they go, the first topmost.
    Arrange(Vec<CardId>),
    /// CR 701.22a: the cards put back on top, the first on top, and those
    /// put on the bottom, the first nearest the top.
    Scry {
        top: Vec<CardId>,
        bottom: Vec<CardId>,
    },
    /// CR 508.1a–b: each attacker and the player it attacks.
    Attack(Vec<(ObjectId, PlayerId)>),
    /// CR 509.1a: each blocker and the attacker it blocks.
    Block(Vec<(ObjectId, ObjectId)>),
    /// CR 510.1c: the damage assigned to each of the decision's `to`.
    Assign(Vec<u32>),
    /// CR 701.21a: the permanents sacrificed.
    Sacrifice(Vec<ObjectId>),
    /// The permanents returned to their owner's hand to pay a cost.
    ReturnToHand(Vec<ObjectId>),
    /// A creature type, as the Oracle text names it: "Beast" (CR 205.3m).
    CreatureType(String),
    /// CR 201.4: the card named, as the Oracle card reference has it.
    Name(Box<CardText>),
    /// CR 106.3: the mana added.
    Mana(ManaPool),
}

#[derive(Debug)]
pub enum Rejected {
    /// Not among the legal answers to the decision.
    Illegal(Answer),
    /// Legal, but the card's text has no lowering yet, so the engine
    /// refuses rather than play it wrong. The game is unchanged.
    Refused { card: String, refusal: Unrunnable },
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rejected::Illegal(a) => write!(f, "{a:?} is not a legal answer"),
            Rejected::Refused { card, refusal } => write!(f, "can't play {card}: {}", refusal.why),
        }
    }
}

/// splitmix64: small, seeded, and the same on every target, so a game
/// replays from its seed.
#[derive(Clone, Debug)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Below `n`, by rejection so every value is equally likely.
    fn below(&mut self, n: u64) -> u64 {
        let zone = u64::MAX - u64::MAX % n;
        loop {
            let x = self.next();
            if x < zone {
                return x % n;
            }
        }
    }

    /// Fisher–Yates.
    fn shuffle(&mut self, cards: &mut Vector<CardId>) {
        for i in (1..cards.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            cards.swap(i, j);
        }
    }
}

/// The whole state of a game (ADR-0043). Cloning is cheap: the zones and
/// the battlefield are persistent, and the cards' text is shared.
#[derive(Clone)]
pub struct Game {
    cards: Vector<CardEntry>,
    players: Vec<Player>,
    /// The battlefield as the layers read it (ADR-0037).
    battlefield: State,
    permanents: OrdMap<ObjectId, Permanent>,
    /// The top is the back (CR 405.2).
    stack: Vector<StackObject>,
    /// Triggered abilities waiting to be put on the stack (CR 603.3).
    pending: Vector<Pending>,
    /// How many triggered abilities have been put on the stack.
    triggers: u32,
    /// Every delayed triggered ability created this game (CR 603.7a).
    delayed: Vector<Delayed>,
    /// The card whose spell or ability most recently finished resolving.
    last_resolved: Option<CardId>,
    /// The combat this turn, from the declaration of attackers to the end
    /// of combat (CR 506, 511.3).
    combat: Option<Combat>,
    /// The "until end of turn" effects to end in cleanup (CR 514.2).
    until_end_of_turn: Vec<Timestamp>,
    /// Cards an instruction put on the bottom of a library this turn.
    bottomed: u32,
    /// Libraries an instruction shuffled this turn.
    shuffled: u32,
    /// Permanents returned to a hand this turn, by a cost or an
    /// instruction.
    returned: u32,
    /// Tokens created this turn.
    created: u32,
    /// Cards discarded this turn by an instruction that chose none of
    /// them ("discards all cards with that name").
    discarded: u32,
    /// The abilities limited to once each turn activated this turn, by
    /// their permanent and index (CR 602.5b).
    used: OrdSet<(ObjectId, u16)>,
    /// The counters each permanent that left had as it last existed, for
    /// an ability of its that counts them (CR 608.2h, 113.7a).
    last_known: OrdMap<ObjectId, LastKnown>,
    exile: Vector<CardId>,
    /// Adventurer cards exiled as their Adventure resolved, each with the
    /// player who may cast it from exile while it stays there (CR 715.3d).
    adventuring: OrdMap<CardId, PlayerId>,
    command: Vector<CardId>,
    /// How many times each commander has been cast from the command zone
    /// (CR 903.8).
    commander_casts: OrdMap<CardId, u32>,
    /// Commanders put into a graveyard or exile since state-based actions
    /// were last checked (CR 903.9a).
    commanders_moved: Vector<CardId>,
    /// The game as it was before the spell being cast was proposed, to
    /// return to if the cast is cancelled (CR 733.1).
    before_cast: Option<Box<Game>>,
    turn: u32,
    active: PlayerId,
    step: Step,
    waiting: Waiting,
    /// Where the setup's shuffles left it, for mulligans and, later,
    /// searches.
    rng: Rng,
}

/// Which rules set up the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// CR 903: commanders in the command zone, 40 life.
    Commander,
    /// A two-player constructed game: 20 life (CR 103.4), no command zone.
    Constructed,
}

/// How a game starts.
#[derive(Clone, Debug)]
pub struct Setup {
    pub format: Format,
    /// In turn order, the starting player first (who starts is agreed
    /// outside the game, CR 103.1).
    pub decks: Vec<DeckCards>,
    /// Whether players are asked to mulligan (CR 103.5). Without, everyone
    /// keeps their seven, as a scripted test wants.
    pub mulligans: bool,
    /// The shuffles' seed: a game replays from its seed and its answers.
    pub seed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupError {
    NoPlayers,
    /// Only Commander has commanders (CR 903.3).
    CommanderOutsideCommander,
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetupError::NoPlayers => f.write_str("a game has at least one player"),
            SetupError::CommanderOutsideCommander => {
                f.write_str("a deck has a commander outside a Commander game")
            }
        }
    }
}

/// Why a card can't be named.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unnameable {
    /// It isn't hidden: it is this card.
    AlreadyKnown(String),
    /// It was cast face down, so it is a card with a morph ability the
    /// engine runs, and this card isn't (CR 702.37c).
    NotMorph(String),
}

impl Game {
    /// A game of Commander between these decks, the first starting, every
    /// player keeping their seven.
    pub fn new(decks: Vec<DeckCards>, seed: u64) -> Game {
        Game::start(Setup {
            format: Format::Commander,
            decks,
            mulligans: false,
            seed,
        })
        .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Each player puts their commander in the command zone (CR 903.6),
    /// shuffles the rest into their library (CR 103.3), sets their life (CR
    /// 103.4, 903.7) and draws seven (CR 103.5). Then the mulligans, if
    /// asked, and the first turn begins.
    pub fn start(setup: Setup) -> Result<Game, SetupError> {
        if setup.decks.is_empty() {
            return Err(SetupError::NoPlayers);
        }
        let commander = setup.format == Format::Commander;
        if !commander && setup.decks.iter().any(|d| !d.commander.is_empty()) {
            return Err(SetupError::CommanderOutsideCommander);
        }
        let mut cards = Vector::new();
        let mut players = Vec::new();
        let mut command = Vector::new();
        let mut rng = Rng(setup.seed);
        for (p, deck) in setup.decks.into_iter().enumerate() {
            let owner = PlayerId(p as u8);
            let identity = (!deck.commander.is_empty()).then(|| {
                let colors: BTreeSet<Color> = deck
                    .commander
                    .iter()
                    .flat_map(|c| c.identity.iter().copied())
                    .collect();
                colors.into_iter().collect()
            });
            let mut add = |face: Face, commander: bool| {
                cards.push_back(CardEntry {
                    owner,
                    commander,
                    token: false,
                    face,
                });
                CardId(cards.len() as u32 - 1)
            };
            let known = Face::of;
            for c in deck.commander {
                command.push_back(add(known(c), true));
            }
            let mut library: Vector<CardId> = deck
                .library
                .into_iter()
                .map(|c| add(known(c), false))
                .collect();
            for _ in 0..deck.hidden {
                library.push_back(add(Face::Hidden, false));
            }
            rng.shuffle(&mut library);
            players.push(Player {
                life: if commander {
                    COMMANDER_LIFE
                } else {
                    STARTING_LIFE
                },
                library,
                hand: Vector::new(),
                graveyard: Vector::new(),
                pool: ManaPool::default(),
                lands_played: 0,
                drew_from_empty: false,
                turn_began: None,
                last_turn: None,
                lost_life: false,
                drawn: Vector::new(),
                identity,
                mulligans: 0,
                kept: !setup.mulligans,
                left: false,
            });
        }
        let mut game = Game {
            cards,
            players,
            battlefield: State::default(),
            permanents: OrdMap::new(),
            stack: Vector::new(),
            pending: Vector::new(),
            triggers: 0,
            last_resolved: None,
            combat: None,
            until_end_of_turn: Vec::new(),
            delayed: Vector::new(),
            last_known: OrdMap::new(),
            bottomed: 0,
            returned: 0,
            shuffled: 0,
            created: 0,
            discarded: 0,
            used: OrdSet::new(),
            exile: Vector::new(),
            adventuring: OrdMap::new(),
            command,
            commander_casts: OrdMap::new(),
            commanders_moved: Vector::new(),
            before_cast: None,
            turn: 0,
            active: PlayerId(0),
            step: Step::Untap,
            waiting: Waiting::Over(Outcome::Draw),
            rng,
        };
        for p in 0..game.players.len() {
            for _ in 0..HAND_SIZE {
                game.draw(PlayerId(p as u8));
            }
        }
        game.declare_from(0, Vec::new());
        Ok(game)
    }

    /// CR 103.5: the next player from `from` in turn order who hasn't kept
    /// declares; once all have, those taking a mulligan take it at once,
    /// and with no one taking one the first turn begins.
    fn declare_from(&mut self, from: usize, taking: Vec<PlayerId>) {
        if let Some(p) = (from..self.players.len()).find(|p| !self.players[*p].kept) {
            self.waiting = Waiting::Mulligan {
                player: PlayerId(p as u8),
                taking,
            };
            return;
        }
        if taking.is_empty() {
            return self.opening_hand_from(0);
        }
        for p in &taking {
            let player = &mut self.players[p.0 as usize];
            let hand = std::mem::take(&mut player.hand);
            player.library.append(hand);
            player.mulligans += 1;
            let mut library = std::mem::take(&mut player.library);
            self.rng.shuffle(&mut library);
            self.players[p.0 as usize].library = library;
            for _ in 0..HAND_SIZE {
                self.draw(*p);
            }
        }
        self.bottom_next(taking);
    }

    /// CR 103.6: once the mulligans are done, each player from `from` in
    /// turn order, the starting player first, who may take an action
    /// with a card of their opening hand is asked; then the first turn
    /// begins.
    fn opening_hand_from(&mut self, from: usize) {
        for p in from..self.players.len() {
            let Decision::OpeningHand { cards, .. } = ({
                self.waiting = Waiting::OpeningHand {
                    player: PlayerId(p as u8),
                };
                self.decision()
            }) else {
                unreachable!("asking for the opening hand")
            };
            if !cards.is_empty() {
                return;
            }
        }
        self.begin_turn(PlayerId(0))
    }

    /// What a card lets its owner do from their opening hand (CR 103.6),
    /// if it is known and the engine runs it.
    fn opening(&self, c: CardId) -> Option<&Opening> {
        self.known(c)
            .and_then(|k| k.runs.as_ref().ok())
            .and_then(|r| r.opening.as_ref())
    }

    /// Whether a permanent `q` controls says `what` of them and has that
    /// ability now: hexproof (CR 702.11c), shroud (CR 702.18a), a draw
    /// step skipped (CR 614.10), all damage to them prevented (CR 615.1a).
    fn you_have(&self, q: PlayerId, what: ForYou) -> bool {
        self.characteristics().iter().any(|(id, c)| {
            c.controller == q
                && self
                    .known(self.permanents[id].card)
                    .and_then(|k| k.runs.as_ref().ok())
                    .is_some_and(|r| {
                        r.for_you
                            .iter()
                            .any(|(n, w)| *w == what && c.abilities.contains(&Ability::Unread(*n)))
                    })
        })
    }

    /// Whether `p`'s next draw step is skipped (CR 614.10), as a
    /// permanent of theirs says now.
    pub fn skips_draw_step(&self, p: PlayerId) -> bool {
        self.you_have(p, ForYou::SkipDrawStep)
    }

    /// CR 103.5, 103.5c: each player who took a mulligan puts one card on
    /// the bottom for each they have taken, the first free in a
    /// multiplayer game. A player whose opening hand would be no cards
    /// takes no more.
    fn bottom_next(&mut self, mut rest: Vec<PlayerId>) {
        while !rest.is_empty() {
            let p = rest.remove(0);
            let free = u32::from(self.players.len() > 2);
            let player = &mut self.players[p.0 as usize];
            let count = player.mulligans.saturating_sub(free) as usize;
            if count >= HAND_SIZE {
                player.kept = true;
            }
            let count = count.min(player.hand.len());
            if count > 0 {
                self.waiting = Waiting::Bottom {
                    player: p,
                    count,
                    then: rest,
                };
                return;
            }
        }
        self.declare_from(0, Vec::new());
    }

    pub fn turn(&self) -> u32 {
        self.turn
    }
    pub fn active(&self) -> PlayerId {
        self.active
    }
    pub fn step_now(&self) -> Step {
        self.step
    }
    pub fn players(&self) -> &[Player] {
        &self.players
    }
    pub fn player(&self, p: PlayerId) -> &Player {
        &self.players[p.0 as usize]
    }
    /// The card's text, or `None` while it is hidden.
    pub fn card(&self, c: CardId) -> Option<&CardText> {
        match &self.cards[c.0 as usize].face {
            Face::Two(two, _) => Some(&two.text),
            face => face.up().map(|k| &k.text),
        }
    }
    /// The face of the card that is up (CR 712.8).
    fn known(&self, c: CardId) -> Option<&Known> {
        self.cards[c.0 as usize].face.up().map(|k| &**k)
    }
    /// A two-faced card's faces.
    fn two_faces(&self, c: CardId) -> Option<&TwoFaces> {
        match &self.cards[c.0 as usize].face {
            Face::Two(two, _) => Some(two),
            _ => None,
        }
    }
    /// Which side of a two-faced card is up; a card with one face's is
    /// its front.
    pub fn side_up(&self, c: CardId) -> Side {
        match &self.cards[c.0 as usize].face {
            Face::Two(_, side) => *side,
            _ => Side::Front,
        }
    }
    /// Turn a two-faced card's `side` up, as it is cast or played (CR
    /// 712.11b, 712.12), or its front as it leaves the stack or the
    /// battlefield (CR 712.8a). A card with one face has only its front.
    fn turn_up(&mut self, c: CardId, side: Side) {
        if let Face::Two(_, up) = &mut self.cards[c.0 as usize].face {
            *up = side;
        }
    }
    /// Name a hidden card (the module's "Hidden cards"). A card already
    /// known keeps what it is, and one cast face down is a card with
    /// morph.
    pub fn reveal(&mut self, c: CardId, text: CardText) -> Result<(), Unnameable> {
        if let Some(k) = self.known(c) {
            return Err(Unnameable::AlreadyKnown(k.text.name.clone()));
        }
        let face_down = self.permanents.values().any(|p| p.card == c && p.face_down)
            || self.stack.iter().any(
                |o| matches!(o, StackObject::Spell { card, via: Via::FaceDown, .. } if *card == c),
            );
        let face = Face::of(text);
        let k = face.up().expect("named");
        if face_down && k.morph.is_none() {
            return Err(Unnameable::NotMorph(k.text.name.clone()));
        }
        self.cards[c.0 as usize].face = face;
        Ok(())
    }
    /// How many cards the game has, every `CardId` below it.
    pub fn card_count(&self) -> usize {
        self.cards.len()
    }
    pub fn owner(&self, c: CardId) -> PlayerId {
        self.cards[c.0 as usize].owner
    }
    pub fn command(&self) -> &Vector<CardId> {
        &self.command
    }
    pub fn exile(&self) -> &Vector<CardId> {
        &self.exile
    }
    /// The adventurer cards in exile `p` may cast, their Adventure having
    /// resolved (CR 715.3d).
    pub fn adventuring(&self, p: PlayerId) -> impl Iterator<Item = CardId> + '_ {
        self.adventuring
            .iter()
            .filter(move |(c, q)| **q == p && self.exile.contains(c))
            .map(|(c, _)| *c)
    }
    pub fn stack(&self) -> &Vector<StackObject> {
        &self.stack
    }
    /// What an Aura or Equipment on the battlefield is attached to.
    pub fn attached_to(&self, id: ObjectId) -> Option<ObjectId> {
        self.battlefield.objects.get(&id)?.attached_to
    }

    pub fn permanents(&self) -> &OrdMap<ObjectId, Permanent> {
        &self.permanents
    }
    /// Whether the card is a face-down spell on the stack (CR 708.4).
    pub fn face_down_on_stack(&self, c: CardId) -> bool {
        self.stack
            .iter()
            .any(|o| matches!(o, StackObject::Spell { card, via: Via::FaceDown, .. } if *card == c))
    }
    /// How many times this commander has been cast from the command zone.
    pub fn commander_casts(&self, c: CardId) -> u32 {
        self.commander_casts.get(&c).copied().unwrap_or(0)
    }
    /// Whether a known card's spell has a target slot a spell fills.
    pub fn targets_spells(&self, c: CardId) -> bool {
        self.known(c)
            .and_then(|k| k.runs.as_ref().ok())
            .and_then(|r| r.spell.as_ref())
            .is_some_and(|p| p.targets.iter().any(|t| matches!(t, TargetSpec::Spell(_))))
    }

    /// Whether an activated ability of a known card has a target slot a
    /// spell fills.
    pub fn activation_targets_spells(&self, c: CardId) -> bool {
        self.known(c)
            .and_then(|k| k.runs.as_ref().ok())
            .is_some_and(|r| {
                r.activated.values().flatten().any(|a| {
                    a.program
                        .targets
                        .iter()
                        .any(|t| matches!(t, TargetSpec::Spell(_)))
                })
            })
    }

    /// How many cards an instruction that chose none of them discarded
    /// this turn ("discards all cards with that name").
    pub fn discarded(&self) -> u32 {
        self.discarded
    }

    /// How many tokens have been created this turn.
    pub fn created(&self) -> u32 {
        self.created
    }

    /// How many cards an instruction has put on the bottom of a library
    /// this turn.
    pub fn bottomed(&self) -> u32 {
        self.bottomed
    }

    /// How many permanents have been returned to a hand this turn.
    pub fn returned(&self) -> u32 {
        self.returned
    }

    /// How many libraries an instruction has shuffled this turn.
    pub fn shuffled(&self) -> u32 {
        self.shuffled
    }

    /// How many counters of this kind are on the permanent, or were as it
    /// last existed (CR 608.2h).
    pub fn counters_on(&self, id: ObjectId, kind: &Counter) -> u32 {
        self.battlefield
            .objects
            .get(&id)
            .map(|o| &o.counters)
            .or_else(|| self.last_known.get(&id).map(|l| &l.counters))
            .and_then(|c| c.get(kind).copied())
            .unwrap_or(0)
    }

    /// What each permanent is now, through the layers (CR 613).
    pub fn characteristics(&self) -> BTreeMap<ObjectId, Chars> {
        characteristics(&self.layer_state())
    }

    /// The battlefield with what else of the game a static's condition
    /// reads: the size of each graveyard. Cloning it is cheap, its maps
    /// being persistent.
    fn layer_state(&self) -> State {
        let mut state = self.battlefield.clone();
        state.graveyards = self
            .players
            .iter()
            .enumerate()
            .map(|(q, p)| (PlayerId(q as u8), p.graveyard.len() as u32))
            .collect();
        state
    }

    /// Whether the engine can run this card: why it refuses it if not. A
    /// land is checked as it would be played, any other card as it would
    /// be cast.
    pub fn refusal(&self, c: CardId) -> Option<Unrunnable> {
        self.refusal_of(c, self.side_up(c))
    }

    /// As `refusal`, of one side of a card: a two-faced card's face (CR
    /// 712.11c, 715.3a), a card with one face's only one.
    pub fn refusal_of(&self, c: CardId, side: Side) -> Option<Unrunnable> {
        let face = match &self.cards[c.0 as usize].face {
            Face::Two(two, _) => Some(&**two.face(side)),
            _ => self.known(c),
        };
        let Some(known) = face else {
            return Some(Unrunnable::new(
                "a hidden card",
                "no one has said what it is",
            ));
        };
        Game::face_refusal(known)
    }

    fn face_refusal(known: &Known) -> Option<Unrunnable> {
        match &known.runs {
            Err(r) => Some(r.clone()),
            Ok(_) if is_land(&known.text.type_line) => None,
            Ok(runs) => Game::spell_refusal_any(known, runs),
        }
    }

    /// Why a card can be cast no way at all: for its mana cost, or for an
    /// alternative cost when it has one.
    fn spell_refusal_any(known: &Known, runs: &Runnable) -> Option<Unrunnable> {
        let refused = Game::cast_refusal(known, runs).err()?;
        if runs.alternatives.is_empty() {
            return Some(refused);
        }
        Game::spell_refusal(runs).err()
    }

    /// What the engine makes of a card before any game: why it refuses
    /// it, as `refusal` would once it is known, and the abilities it
    /// carries that nothing offers (`dormant`).
    /// A card with two faces is refused if either face is, its front's
    /// refusal first.
    pub fn reads(text: &CardText) -> (Option<Unrunnable>, Vec<String>) {
        let faces = match Face::of(text.clone()) {
            Face::Two(two, _) => vec![Arc::clone(&two.front), Arc::clone(&two.back)],
            face => vec![Arc::clone(face.up().expect("known"))],
        };
        let refused = faces.iter().find_map(|k| Game::face_refusal(k));
        let dormant = faces
            .iter()
            .filter_map(|k| k.runs.as_ref().ok())
            .flat_map(|r| r.dormant.iter().cloned())
            .collect();
        (refused, dormant)
    }

    /// The abilities a card the engine runs carries that nothing offers
    /// to activate yet: its non-mana activated abilities and those that
    /// work from a hand.
    pub fn dormant(&self, c: CardId) -> &[String] {
        match self.known(c).map(|k| &k.runs) {
            Some(Ok(r)) => &r.dormant,
            _ => &[],
        }
    }

    /// The Oracle line a card's unread ability `n` was read from.
    pub fn ability_text(&self, c: CardId, n: u16) -> Option<&str> {
        match &self.known(c)?.runs {
            Ok(r) => r.lowered.unread_text.get(n as usize).map(String::as_str),
            Err(_) => None,
        }
    }

    /// What the game is waiting on, with the legal answers.
    pub fn decision(&self) -> Decision {
        match &self.waiting {
            Waiting::Over(o) => Decision::Over(*o),
            Waiting::Mulligan { player, .. } => Decision::Mulligan { player: *player },
            Waiting::OpeningHand { player } => {
                let hand = &self.player(*player).hand;
                Decision::OpeningHand {
                    player: *player,
                    cards: hand
                        .iter()
                        .copied()
                        .filter(|c| self.opening(*c).is_some())
                        .collect(),
                }
            }
            Waiting::Bottom { player, count, .. } => Decision::Bottom {
                player: *player,
                count: *count,
                hand: self.player(*player).hand.iter().copied().collect(),
            },
            Waiting::Discard { player, count, .. } => Decision::Discard {
                player: *player,
                count: *count,
                hand: self.player(*player).hand.iter().copied().collect(),
            },
            Waiting::Targets {
                player,
                card,
                slots,
                choosing,
            } => {
                let (controller, object) = match choosing {
                    Choosing::Spell { .. } => (*player, None),
                    Choosing::Ability { object, .. } => (*player, Some(*object)),
                    Choosing::Trigger { pending, .. } => (pending.controller, pending.object),
                };
                let source = self.source_chars(*card, object);
                Decision::Targets {
                    player: *player,
                    card: *card,
                    slots: slots
                        .iter()
                        .map(|s| self.legal_targets(s, controller, source.as_ref()))
                        .collect(),
                }
            }
            Waiting::Order {
                player, pending, ..
            } => Decision::Order {
                player: *player,
                triggers: pending.iter().map(|p| p.source).collect(),
            },
            Waiting::ChooseFromHand {
                chooser,
                whose,
                filter,
                ..
            } => {
                let hand: Vec<CardId> = self.player(*whose).hand.iter().copied().collect();
                let read = |c: &CardId| self.known(*c).and_then(|k| k.printed.as_ref());
                Decision::ChooseFromHand {
                    player: *chooser,
                    whose: *whose,
                    legal: hand
                        .iter()
                        .copied()
                        .filter(|c| read(c).is_some_and(|ch| filter.matches(ch)))
                        .collect(),
                    unsure: hand.iter().filter(|c| read(c).is_none()).count(),
                    hand,
                }
            }
            Waiting::Scry { player, cards, .. } => Decision::Scry {
                player: *player,
                cards: cards.clone(),
            },
            Waiting::Reveal { player, cards, .. } => Decision::Reveal {
                player: *player,
                cards: cards.clone(),
            },
            Waiting::ChooseAmong {
                player, cards, n, ..
            } => Decision::ChooseAmong {
                player: *player,
                cards: cards.clone(),
                n: *n,
            },
            Waiting::Arrange {
                player, cards, top, ..
            } => Decision::Arrange {
                player: *player,
                cards: cards.clone(),
                top: *top,
            },
            Waiting::May { player, at } => Decision::May {
                player: *player,
                card: at.what.card(),
            },
            Waiting::PutFromHand { player, filter, .. } => {
                let hand: Vec<CardId> = self.player(*player).hand.iter().copied().collect();
                Decision::PutFromHand {
                    player: *player,
                    legal: hand
                        .iter()
                        .copied()
                        .filter(|c| self.finds(*c, filter))
                        .collect(),
                    unsure: hand
                        .iter()
                        .filter(|c| match self.known(**c) {
                            None => true,
                            Some(k) => k.found_by(filter, true).is_err(),
                        })
                        .count(),
                    hand,
                }
            }
            Waiting::Attackers { player } => Decision::Attackers {
                player: *player,
                can: self.can_attack(*player),
                defenders: (0..self.players.len())
                    .map(|q| PlayerId(q as u8))
                    .filter(|q| q != player && !self.player(*q).left)
                    .collect(),
            },
            Waiting::Blockers { player, .. } => Decision::Blockers {
                player: *player,
                can: self.can_block(*player),
                attackers: self
                    .combat
                    .as_ref()
                    .map(|c| {
                        c.attackers
                            .iter()
                            .filter(|(a, d)| *d == player && self.permanents.contains_key(*a))
                            .map(|(a, _)| *a)
                            .collect()
                    })
                    .unwrap_or_default(),
            },
            Waiting::Assign {
                player, source, to, ..
            } => Decision::Assign {
                player: *player,
                source: *source,
                power: self.power(*source),
                to: to.clone(),
                lethal: to.iter().map(|t| self.lethal(*source, *t)).collect(),
            },
            Waiting::Priority { player, .. } => Decision::Priority {
                player: *player,
                actions: self.actions(*player),
            },
            Waiting::Paying {
                player,
                spell,
                owed,
                ..
            } => Decision::Pay {
                player: *player,
                spell: *spell,
                owed: owed.clone(),
                actions: self.payments(*player, owed),
            },
            Waiting::ReturnCommander { owner, card, .. } => Decision::ReturnCommander {
                player: *owner,
                card: *card,
            },
            Waiting::LegendRule { player, among, .. } => Decision::LegendRule {
                player: *player,
                among: among.clone(),
            },
            Waiting::DiscardToEnter {
                player,
                card,
                filter,
                ..
            } => {
                let hand = &self.player(*player).hand;
                Decision::DiscardToEnter {
                    player: *player,
                    card: *card,
                    legal: hand
                        .iter()
                        .copied()
                        .filter(|c| {
                            self.known(*c)
                                .and_then(|k| k.printed.as_ref())
                                .is_some_and(|ch| filter.matches(ch))
                        })
                        .collect(),
                    unsure: hand.iter().filter(|c| self.known(**c).is_none()).count(),
                }
            }
            Waiting::Kicker { player, card, .. } => Decision::Kicker {
                player: *player,
                card: *card,
                cost: self.runs_of(*card).kicker.clone().expect("a kicker"),
            },
            Waiting::ChooseCreatureType { player, card, .. } => Decision::ChooseCreatureType {
                player: *player,
                card: *card,
            },
            Waiting::PayOrEnterTapped {
                player, card, life, ..
            } => Decision::PayOrEnterTapped {
                player: *player,
                card: *card,
                life: *life,
            },
            Waiting::Sacrifice {
                player,
                spell,
                count,
                filter,
            } => Decision::Sacrifice {
                player: *player,
                spell: *spell,
                count: *count,
                legal: self.sacrificable(*player, filter),
            },
            Waiting::ReturnToHand {
                player,
                spell,
                count,
                filter,
            } => Decision::ReturnToHand {
                player: *player,
                spell: *spell,
                count: *count,
                legal: self.sacrificable(*player, filter),
            },
            Waiting::PayLifeOr { player, life, at } => Decision::PayLifeOr {
                player: *player,
                card: at.what.card(),
                life: *life,
            },
            Waiting::PayUnless { player, owed, at } => Decision::PayUnless {
                player: *player,
                card: at.what.card(),
                owed: owed.clone(),
                actions: self.payments(*player, owed),
            },
            Waiting::DiscardUnless { player, filter, at } => {
                let (legal, unsure) = self.discardable(*player, filter);
                Decision::DiscardUnless {
                    player: *player,
                    card: at.what.card(),
                    legal,
                    unsure,
                }
            }
            Waiting::SacrificeUnless {
                player,
                filter,
                power,
                at,
            } => Decision::SacrificeUnless {
                player: *player,
                card: at.what.card(),
                legal: self.sacrificable(*player, filter),
                power: *power,
            },
            Waiting::ChooseMana {
                player,
                options,
                resume,
            } => Decision::ChooseMana {
                player: *player,
                card: resume.what.card(),
                options: options.clone(),
            },
            Waiting::ChooseName {
                player,
                filter,
                resume,
            } => Decision::ChooseName {
                player: *player,
                card: resume.what.card(),
                filter: filter.clone(),
            },
            Waiting::Search {
                player,
                filter,
                enters,
                reveals,
                resume,
                ..
            } => {
                let library = &self.player(*player).library;
                let found = Self::found_already(resume);
                Decision::Search {
                    player: *player,
                    filter: filter.clone(),
                    reveals: *reveals,
                    known: library
                        .iter()
                        .copied()
                        .filter(|c| {
                            !found.contains(c)
                                && self
                                    .known(*c)
                                    .is_some_and(|k| k.found_by(filter, *enters) == Ok(true))
                        })
                        .collect(),
                    hidden: library.iter().filter(|c| self.known(**c).is_none()).count(),
                }
            }
        }
    }

    /// The cards of `p`'s hand `filter` matches, which they may discard
    /// (CR 701.9a), and how many cards of it are hidden.
    fn discardable(&self, p: PlayerId, filter: &CardFilter) -> (Vec<CardId>, usize) {
        let hand = &self.player(p).hand;
        let legal = hand
            .iter()
            .copied()
            .filter(|c| {
                self.known(*c)
                    .and_then(|k| k.printed.as_ref())
                    .is_some_and(|ch| filter.matches(ch))
            })
            .collect();
        (
            legal,
            hand.iter().filter(|c| self.known(**c).is_none()).count(),
        )
    }

    /// The permanents `p` controls that `filter` matches, which they may
    /// sacrifice (CR 701.21a).
    fn sacrificable(&self, p: PlayerId, filter: &Filter) -> Vec<ObjectId> {
        let all = self.characteristics();
        all.iter()
            .filter(|(o, c)| {
                c.controller == p
                    && filter.matches(&Seen {
                        id: **o,
                        chars: c,
                        source: ObjectId(u32::MAX),
                        source_chars: c,
                        source_attached_to: None,
                        source_chosen: None,
                        token: self.battlefield.objects[*o].token,
                        attacking: self.battlefield.attacking.contains(*o),
                    })
            })
            .map(|(o, _)| *o)
            .collect()
    }

    /// Whether `filter` matches this card, which must be known and one the
    /// engine can put onto the battlefield.
    fn finds(&self, c: CardId, filter: &CardFilter) -> bool {
        self.known(c)
            .is_some_and(|k| k.found_by(filter, true) == Ok(true))
    }

    /// Answer the decision and run on to the next one. An answer that isn't
    /// legal leaves the game as it was.
    pub fn step(&mut self, answer: Answer) -> Result<Decision, Rejected> {
        match (self.waiting.clone(), &answer) {
            (Waiting::OpeningHand { player }, Answer::Begin(cards)) => {
                let distinct: BTreeSet<&CardId> = cards.iter().collect();
                if distinct.len() != cards.len()
                    || !cards
                        .iter()
                        .all(|c| self.opening(*c).is_some() && self.player(player).hand.contains(c))
                {
                    return Err(Rejected::Illegal(answer));
                }
                for c in cards {
                    match self.opening(*c).cloned() {
                        Some(Opening::Begins) => {
                            self.players[player.0 as usize].hand.retain(|x| x != c);
                            self.arrive(*c, player, false);
                        }
                        // CR 103.6b, 603.7: revealed, it stays in the hand,
                        // and the first upkeep, of turn 1, is of a later
                        // turn than this.
                        Some(Opening::Reveals(program)) => self.delayed.push_back(Delayed {
                            source: *c,
                            controller: player,
                            step: TurnStep::Upkeep,
                            created: self.turn,
                            program,
                            fired: false,
                        }),
                        None => unreachable!("checked above"),
                    }
                }
                self.opening_hand_from(player.0 as usize + 1);
            }
            (Waiting::Mulligan { player, mut taking }, Answer::Mulligan(take)) => {
                if *take {
                    taking.push(player);
                } else {
                    self.players[player.0 as usize].kept = true;
                }
                self.declare_from(player.0 as usize + 1, taking);
            }
            (
                Waiting::Bottom {
                    player,
                    count,
                    then,
                },
                Answer::Bottom(cards),
            ) if self.chosen_from_hand(player, count, cards) => {
                let p = &mut self.players[player.0 as usize];
                p.hand.retain(|c| !cards.contains(c));
                p.library.extend(cards.iter().copied());
                self.bottom_next(then);
            }
            (Waiting::Priority { player, passes }, Answer::Act(action))
                if self.actions(player).contains(action) =>
            {
                self.act(player, passes, *action)?
            }
            (
                Waiting::Paying {
                    player,
                    spell,
                    owed,
                    paying,
                },
                Answer::Pay(pay),
            ) if self.payments(player, &owed).contains(pay) => match (*pay, paying) {
                (Payment::Mana(a), _) => self.activate(player, a),
                // CR 602.2b: an ability activated, its activator gets
                // priority again (CR 117.3c).
                (Payment::Spend(spent), Paying::Ability) => {
                    let pool = &mut self.players[player.0 as usize].pool;
                    *pool = pool.minus(&spent);
                    self.before_cast = None;
                    self.give_priority(player, 0);
                }
                // CR 702.37e, 116.3: turned face up without the stack, and
                // its controller gets priority again.
                (Payment::Spend(spent), Paying::FaceUp(id)) => {
                    let pool = &mut self.players[player.0 as usize].pool;
                    *pool = pool.minus(&spent);
                    self.before_cast = None;
                    self.turn_face_up(id);
                    self.give_priority(player, 0);
                }
                (Payment::Spend(spent), Paying::Spell { from_command }) => {
                    let pool = &mut self.players[player.0 as usize].pool;
                    *pool = pool.minus(&spent);
                    if from_command {
                        *self.commander_casts.entry(spell).or_insert(0) += 1;
                    }
                    // CR 601.2h: the rest of its total cost, paid with the
                    // mana in any order.
                    let other = self.other_costs(spell);
                    self.lose_life(player, other.life);
                    match other.sacrifice {
                        Some((count, filter)) => {
                            self.waiting = Waiting::Sacrifice {
                                player,
                                spell,
                                count,
                                filter,
                            }
                        }
                        None => self.return_or_cast(player, spell),
                    }
                }
                (Payment::Cancel, _) => {
                    let before = self.before_cast.take().expect("a cast to cancel");
                    *self = *before;
                }
            },
            (
                Waiting::Discard {
                    player,
                    count,
                    then,
                },
                Answer::Discard(cards),
            ) if self.can_discard(player, count, cards) => {
                self.discard(player, cards);
                match then {
                    // Bound for "discarded this way".
                    Some((mut at, into)) => {
                        at.found.insert(into, cards.clone());
                        self.follow(at)
                    }
                    // The excess is gone, so beginning cleanup again
                    // discards nothing and runs the rest of it (CR
                    // 514.2–514.3).
                    None => self.run_from(Step::Cleanup),
                }
            }
            (
                Waiting::Targets {
                    player,
                    card,
                    slots,
                    choosing,
                },
                Answer::Targets(chosen),
            ) => {
                let (controller, object) = match &choosing {
                    Choosing::Spell { .. } => (player, None),
                    Choosing::Ability { object, .. } => (player, Some(*object)),
                    Choosing::Trigger { pending, .. } => (pending.controller, pending.object),
                };
                let source = self.source_chars(card, object);
                let legal = chosen.len() == slots.len()
                    && slots.iter().zip(chosen).all(|(s, t)| {
                        self.legal_targets(s, controller, source.as_ref())
                            .contains(t)
                    });
                if !legal {
                    return Err(Rejected::Illegal(answer));
                }
                match choosing {
                    Choosing::Ability { owed, .. } => {
                        if let Some(StackObject::Ability {
                            targets, source, ..
                        }) = self.stack.back_mut()
                        {
                            *targets = chosen.clone();
                            let spell = *source;
                            self.pay_ability(player, spell, owed);
                        }
                    }
                    Choosing::Spell { owed, from_command } => {
                        if let Some(StackObject::Spell { targets, .. }) = self.stack.back_mut() {
                            *targets = chosen.clone();
                        }
                        // CR 601.2f–g.
                        self.waiting = Waiting::Paying {
                            player,
                            spell: match self.stack.back() {
                                Some(StackObject::Spell { card, .. }) => *card,
                                _ => unreachable!("the spell is on top"),
                            },
                            owed,
                            paying: Paying::Spell { from_command },
                        };
                    }
                    Choosing::Trigger { pending, then } => {
                        self.push_trigger(pending, chosen.clone());
                        self.give_priority(then, 0);
                    }
                }
            }
            (Waiting::Order { pending, then, .. }, Answer::Order(order)) => {
                let mut sorted = order.clone();
                sorted.sort_unstable();
                if sorted != (0..pending.len()).collect::<Vec<_>>() {
                    return Err(Rejected::Illegal(answer));
                }
                // Back in front of the others, in the order chosen, to be
                // put on the stack one by one.
                for k in order.iter().rev() {
                    self.pending.push_front(Pending {
                        ordered: true,
                        ..pending[*k]
                    });
                }
                self.give_priority(then, 0);
            }
            (Waiting::ChooseFromHand { whose, resume, .. }, Answer::Pick(pick)) => {
                let Decision::ChooseFromHand { legal, unsure, .. } = self.decision() else {
                    unreachable!("waiting on a choice from a hand")
                };
                match pick {
                    Some(c) if legal.contains(c) => self.discard(whose, &[*c]),
                    None if legal.is_empty() && unsure == 0 => {}
                    _ => return Err(Rejected::Illegal(answer)),
                }
                self.follow(resume);
            }
            (Waiting::Reveal { cards, resume, .. }, Answer::Reveal(named))
                if named.len() == cards.len() =>
            {
                for (c, text) in cards.iter().zip(named) {
                    self.cards[c.0 as usize].face = Face::of(text.clone());
                }
                self.follow(resume);
            }
            (
                Waiting::ChooseAmong {
                    cards,
                    n,
                    into,
                    mut resume,
                    ..
                },
                Answer::Choose(chosen),
            ) => {
                let distinct: BTreeSet<&CardId> = chosen.iter().collect();
                if chosen.len() != n
                    || distinct.len() != n
                    || !chosen.iter().all(|c| cards.contains(c))
                {
                    return Err(Rejected::Illegal(answer));
                }
                resume.found.insert(into, chosen.clone());
                self.follow(resume);
            }
            (
                Waiting::Arrange {
                    cards, top, resume, ..
                },
                Answer::Arrange(order),
            ) => {
                let mut given = order.clone();
                given.sort_unstable();
                let mut looked = cards.clone();
                looked.sort_unstable();
                if given != looked {
                    return Err(Rejected::Illegal(answer));
                }
                self.put_in_library(order, top);
                self.follow(resume);
            }
            (Waiting::May { at, .. }, Answer::May(yes)) => {
                let mut at = at;
                if *yes {
                    at.next.push(0);
                } else {
                    *at.next.last_mut().expect("a cursor") += 1;
                }
                self.follow(at);
            }
            (
                Waiting::PutFromHand {
                    player,
                    tapped,
                    resume,
                    ..
                },
                Answer::Pick(pick),
            ) => {
                let Decision::PutFromHand { legal, unsure, .. } = self.decision() else {
                    unreachable!("waiting on a card from a hand")
                };
                match pick {
                    Some(c) if legal.contains(c) => {
                        self.players[player.0 as usize].hand.retain(|x| x != c);
                        self.enter(*c, player, tapped, Then::Resume(resume));
                    }
                    None if legal.is_empty() && unsure == 0 => self.follow(resume),
                    _ => return Err(Rejected::Illegal(answer)),
                }
            }
            (Waiting::Attackers { player }, Answer::Attack(declared)) => {
                let Decision::Attackers { can, defenders, .. } = self.decision() else {
                    unreachable!("declaring attackers")
                };
                let mut seen = BTreeSet::new();
                if !declared
                    .iter()
                    .all(|(a, d)| can.contains(a) && defenders.contains(d) && seen.insert(*a))
                {
                    return Err(Rejected::Illegal(answer));
                }
                self.declare_attackers(player, declared);
            }
            (Waiting::Blockers { player, rest }, Answer::Block(declared)) => {
                if !self.legal_blocks(player, declared) {
                    return Err(Rejected::Illegal(answer));
                }
                let combat = self.combat.get_or_insert_with(Combat::default);
                for (b, a) in declared {
                    combat.blockers.insert(*b, *a);
                    combat.blocked.insert(*a);
                }
                self.declare_blockers(rest);
            }
            (
                Waiting::Assign {
                    source,
                    to,
                    left,
                    mut assigned,
                    first,
                    ..
                },
                Answer::Assign(amounts),
            ) => {
                let Decision::Assign { power, lethal, .. } = self.decision() else {
                    unreachable!("assigning damage")
                };
                let to_player = to.iter().position(|t| matches!(t, Target::Player(_)));
                let blockers_lethal = to
                    .iter()
                    .zip(amounts)
                    .zip(&lethal)
                    .all(|((t, n), l)| matches!(t, Target::Player(_)) || n >= l);
                let legal = amounts.len() == to.len()
                    && amounts.iter().sum::<u32>() == power
                    && to_player.is_none_or(|k| amounts[k] == 0 || blockers_lethal);
                if !legal {
                    return Err(Rejected::Illegal(answer));
                }
                for (t, n) in to.iter().zip(amounts) {
                    if *n > 0 {
                        assigned.push((source, *t, *n));
                    }
                }
                self.assign_from(left, assigned, first);
            }
            (
                Waiting::Scry {
                    player,
                    cards,
                    resume,
                },
                Answer::Scry { top, bottom },
            ) => {
                let mut given: Vec<CardId> = top.iter().chain(bottom).copied().collect();
                given.sort_unstable();
                let mut looked = cards.clone();
                looked.sort_unstable();
                if given != looked {
                    return Err(Rejected::Illegal(answer));
                }
                let library = &mut self.players[player.0 as usize].library;
                library.retain(|c| !cards.contains(c));
                for c in top.iter().rev() {
                    library.push_front(*c);
                }
                library.extend(bottom.iter().copied());
                self.follow(resume);
            }
            (
                Waiting::ReturnCommander {
                    owner,
                    card,
                    then,
                    passes,
                },
                Answer::CommandZone(to_command),
            ) => {
                self.commanders_moved.retain(|c| *c != card);
                if *to_command {
                    self.players[owner.0 as usize]
                        .graveyard
                        .retain(|c| *c != card);
                    self.exile.retain(|c| *c != card);
                    self.command.push_back(card);
                }
                self.give_priority(then, passes);
            }
            (
                Waiting::LegendRule {
                    among,
                    then,
                    passes,
                    ..
                },
                Answer::Keep(kept),
            ) if among.contains(kept) => {
                for id in among.iter().filter(|id| *id != kept) {
                    self.put_in_graveyard(*id);
                }
                self.give_priority(then, passes);
            }
            (
                Waiting::DiscardToEnter {
                    player,
                    card,
                    tapped,
                    then,
                    ..
                },
                Answer::Pick(pick),
            ) => {
                let Decision::DiscardToEnter { legal, .. } = self.decision() else {
                    unreachable!("waiting on a discard to enter")
                };
                match pick {
                    Some(c) if legal.contains(c) => {
                        self.discard(player, &[*c]);
                        self.place(card, player, tapped, then);
                    }
                    None => {
                        let owner = self.owner(card);
                        self.players[owner.0 as usize].graveyard.push_back(card);
                        self.proceed(then);
                    }
                    _ => return Err(Rejected::Illegal(answer)),
                }
            }
            (
                Waiting::Kicker {
                    player,
                    card,
                    base,
                    from_command,
                },
                Answer::Kick(kick),
            ) => {
                let base = if *kick {
                    if let Some(StackObject::Spell { kicked, .. }) = self.stack.back_mut() {
                        *kicked = true;
                    }
                    base.plus(self.runs_of(card).kicker.as_ref().expect("a kicker"))
                } else {
                    base
                };
                self.choose_targets(player, card, base, from_command);
            }
            (
                Waiting::ChooseCreatureType {
                    player,
                    card,
                    tapped,
                    then,
                },
                Answer::CreatureType(t),
            ) if vivisurgeons_insight::vocab::vocab()
                .creature_types
                .contains(&t.to_lowercase()) =>
            {
                let chosen = Subtype::Creature(lower::capitalised(t));
                self.arrive_chosen(card, player, tapped, chosen);
                self.proceed(then);
            }
            (
                Waiting::PayOrEnterTapped {
                    player,
                    card,
                    life,
                    tapped,
                    then,
                },
                Answer::PayLife(pay),
            ) => {
                if *pay {
                    // CR 119.4.
                    self.lose_life(player, life);
                }
                self.place(card, player, tapped || !pay, then);
            }
            (
                Waiting::Search {
                    player,
                    filter,
                    enters,
                    into,
                    mut resume,
                    ..
                },
                Answer::Find(found),
            ) => {
                let library = &self.player(player).library;
                // What it is, if it is in the library and its name known
                // or given now for a hidden card, and no search of this
                // instruction found it already.
                let read = match found {
                    Found::Card(c) | Found::Hidden(c, _)
                        if Self::found_already(&resume).contains(c) =>
                    {
                        return Err(Rejected::Illegal(answer))
                    }
                    Found::Nothing => None,
                    Found::Card(c) if library.contains(c) => match self.known(*c) {
                        Some(_) => Some((*c, None)),
                        None => return Err(Rejected::Illegal(answer)),
                    },
                    Found::Hidden(c, text) if library.contains(c) && self.known(*c).is_none() => {
                        Some((*c, Some(Face::of((**text).clone()))))
                    }
                    _ => return Err(Rejected::Illegal(answer)),
                };
                let card = match read {
                    None => None,
                    Some((c, named)) => {
                        let face = named
                            .clone()
                            .unwrap_or_else(|| self.cards[c.0 as usize].face.clone());
                        let known = Arc::clone(face.up().expect("checked known"));
                        match known.found_by(&filter, enters) {
                            Ok(true) => {}
                            Ok(false) => return Err(Rejected::Illegal(answer)),
                            Err(refusal) => {
                                return Err(Rejected::Refused {
                                    card: known.text.name.clone(),
                                    refusal,
                                })
                            }
                        }
                        if let Some(named) = named {
                            self.cards[c.0 as usize].face = named;
                        }
                        Some(c)
                    }
                };
                // Found, it stays where it is until an instruction moves it.
                resume.found.insert(into, card.into_iter().collect());
                self.follow(resume);
            }
            (
                Waiting::Sacrifice {
                    player,
                    spell,
                    count,
                    filter,
                },
                Answer::Sacrifice(chosen),
            ) => {
                let legal = self.sacrificable(player, &filter);
                let distinct: BTreeSet<&ObjectId> = chosen.iter().collect();
                if chosen.len() != count as usize
                    || distinct.len() != chosen.len()
                    || !chosen.iter().all(|o| legal.contains(o))
                {
                    return Err(Rejected::Illegal(answer));
                }
                for o in chosen {
                    self.put_in_graveyard(*o);
                }
                self.return_or_cast(player, spell);
            }
            (
                Waiting::ReturnToHand {
                    player,
                    spell,
                    count,
                    filter,
                },
                Answer::ReturnToHand(chosen),
            ) => {
                let legal = self.sacrificable(player, &filter);
                let distinct: BTreeSet<&ObjectId> = chosen.iter().collect();
                if chosen.len() != count as usize
                    || distinct.len() != chosen.len()
                    || !chosen.iter().all(|o| legal.contains(o))
                {
                    return Err(Rejected::Illegal(answer));
                }
                for o in chosen {
                    self.leave_battlefield(*o, Zone::Hand);
                }
                self.cast_done(player, spell);
            }
            (Waiting::PayLifeOr { player, life, at }, Answer::PayLife(pay)) => {
                let mut at = at;
                if *pay {
                    self.lose_life(player, life);
                    *at.next.last_mut().expect("a cursor") += 1;
                } else {
                    at.next.push(0);
                }
                self.follow(at);
            }
            (Waiting::PayUnless { player, owed, at }, Answer::Pay(pay))
                if self.payments(player, &owed).contains(pay) =>
            {
                let mut at = at;
                match *pay {
                    Payment::Mana(a) => self.activate(player, a),
                    Payment::Spend(spent) => {
                        let pool = &mut self.players[player.0 as usize].pool;
                        *pool = pool.minus(&spent);
                        *at.next.last_mut().expect("a cursor") += 1;
                        self.follow(at);
                    }
                    Payment::Cancel => {
                        at.next.push(0);
                        self.follow(at);
                    }
                }
            }
            (Waiting::DiscardUnless { player, at, .. }, Answer::Pick(pick)) => {
                let Decision::DiscardUnless { legal, .. } = self.decision() else {
                    unreachable!("waiting on a discard")
                };
                let mut at = at;
                match pick {
                    Some(c) if legal.contains(c) => {
                        self.discard(player, &[*c]);
                        *at.next.last_mut().expect("a cursor") += 1;
                    }
                    None => at.next.push(0),
                    Some(_) => return Err(Rejected::Illegal(answer)),
                }
                self.follow(at);
            }
            (
                Waiting::SacrificeUnless {
                    player,
                    filter,
                    power,
                    at,
                },
                Answer::Sacrifice(chosen),
            ) => {
                let legal = self.sacrificable(player, &filter);
                let distinct: BTreeSet<&ObjectId> = chosen.iter().collect();
                let total: u32 = chosen.iter().map(|o| self.power(*o)).sum();
                if distinct.len() != chosen.len()
                    || !chosen.iter().all(|o| legal.contains(o))
                    || (!chosen.is_empty() && total < power)
                {
                    return Err(Rejected::Illegal(answer));
                }
                let mut at = at;
                if chosen.is_empty() {
                    at.next.push(0);
                } else {
                    for o in chosen {
                        self.put_in_graveyard(*o);
                    }
                    *at.next.last_mut().expect("a cursor") += 1;
                }
                self.follow(at);
            }
            (
                Waiting::ChooseMana {
                    player,
                    options,
                    resume,
                },
                Answer::Mana(mana),
            ) if options.contains(mana) => {
                self.players[player.0 as usize].pool.add_all(mana);
                self.follow(resume);
            }
            (Waiting::ChooseName { filter, resume, .. }, Answer::Name(text)) => {
                // CR 201.4a: a card whose Oracle text has the
                // characteristics, not a token.
                if !lower::printed(text).is_ok_and(|c| filter.matches(&c)) {
                    return Err(Rejected::Illegal(answer));
                }
                let mut resume = resume;
                resume.name = Some(text.name.clone());
                self.follow(resume);
            }
            _ => return Err(Rejected::Illegal(answer)),
        }
        Ok(self.decision())
    }

    fn actions(&self, p: PlayerId) -> Vec<Action> {
        let mut actions = vec![Action::Pass];
        let player = self.player(p);
        // CR 117.1a: a noninstant spell, and a land, only in your main
        // phase with the stack empty.
        let sorcery_speed = p == self.active && self.step.is_main() && self.stack.is_empty();
        // One land a turn, more for each static that lets its controller
        // play an additional one (CR 305.2, 305.2a).
        if sorcery_speed && player.lands_played < 1 + self.extra_lands(p) {
            actions.extend(
                player
                    .hand
                    .iter()
                    .filter(|c| self.card(**c).is_some_and(|t| is_land(&t.type_line)))
                    .map(|c| Action::PlayLand(*c)),
            );
            // CR 712.12: a modal double-faced card played by its land face.
            actions.extend(
                player
                    .hand
                    .iter()
                    .filter(|c| {
                        self.two_faces(**c).is_some_and(|two| {
                            two.layout == TwoFaced::Modal && is_land(&two.back.text.type_line)
                        })
                    })
                    .map(|c| Action::PlayBack(*c)),
            );
        }
        // CR 702.29a: cycling any time its owner has priority.
        actions.extend(
            player
                .hand
                .iter()
                .filter(|c| {
                    self.known(**c)
                        .and_then(|k| k.runs.as_ref().ok())
                        .is_some_and(|r| r.cycling.is_some())
                })
                .map(|c| Action::Cycle(*c)),
        );
        let from_command = self
            .command
            .iter()
            .filter(|c| self.owner(**c) == p && self.cards[c.0 as usize].commander);
        let castable = |known: &Known| {
            // CR 305.9; a card with no mana cost can't be paid for, and
            // alternative costs aren't modelled (CR 118.6).
            if is_land(&known.text.type_line) || matches!(known.cost, Ok(None)) {
                return false;
            }
            // CR 601.2c: a spell with a target slot nothing can fill can't
            // be cast.
            let targetable = match &known.runs {
                Ok(Runnable {
                    spell: Some(program),
                    ..
                }) => program
                    .targets
                    .iter()
                    .all(|t| !self.legal_targets(t, p, known.printed.as_ref()).is_empty()),
                _ => true,
            };
            (sorcery_speed || self.instant_speed(known)) && targetable
        };
        for c in player.hand.iter().chain(from_command) {
            // A hidden card can't be cast: nothing says what it is.
            let Some(known) = self.known(*c) else {
                continue;
            };
            if castable(known) {
                actions.push(Action::Cast(*c));
            }
            // CR 712.11b: a modal double-faced card cast by its back face,
            // only that face evaluated (CR 712.11c); CR 715.3: an
            // adventurer card as an Adventure, only its alternative
            // characteristics (CR 715.3a).
            if self.two_faces(*c).is_some_and(|two| castable(&two.back)) {
                actions.push(Action::CastBack(*c));
            }
        }
        // CR 715.3d: an adventurer card cast from exile, though not as an
        // Adventure.
        for c in self.adventuring(p) {
            if self.known(c).is_some_and(castable) {
                actions.push(Action::Cast(c));
            }
        }
        // CR 118.9, 601.2b: a card cast for an alternative cost, from the
        // zone that cost allows, with the timing its type has (CR
        // 117.1a); one whose other costs can't be paid isn't (CR 601.2h).
        let zones = [
            (lower::CastFrom::Hand, &player.hand),
            (lower::CastFrom::Graveyard, &player.graveyard),
        ];
        for (from, cards) in zones {
            for c in cards.iter() {
                let Some(known) = self.known(*c) else {
                    continue;
                };
                let Ok(runs) = &known.runs else { continue };
                if is_land(&known.text.type_line)
                    || Game::spell_refusal(runs).is_err()
                    || !(sorcery_speed || self.instant_speed(known))
                {
                    continue;
                }
                let targetable = runs.spell.as_ref().is_none_or(|program| {
                    program
                        .targets
                        .iter()
                        .all(|t| !self.legal_targets(t, p, known.printed.as_ref()).is_empty())
                });
                let instant_or_sorcery = runs.lowered.chars.is(CardType::Instant)
                    || runs.lowered.chars.is(CardType::Sorcery);
                for (n, alt) in &runs.alternatives {
                    let payable = player.life >= alt.other.life as i32
                        && alt
                            .other
                            .sacrifice
                            .as_ref()
                            .is_none_or(|(k, f)| self.sacrificable(p, f).len() >= *k as usize)
                        && alt
                            .other
                            .returned
                            .as_ref()
                            .is_none_or(|(k, f)| self.sacrificable(p, f).len() >= *k as usize);
                    if alt.from == from
                        && (instant_or_sorcery || !alt.instant_or_sorcery)
                        && targetable
                        && payable
                    {
                        actions.push(Action::CastAlternative(*c, *n));
                    }
                }
            }
        }
        // CR 702.37c, 708.4: a face-down spell is a creature spell, cast
        // at sorcery speed; a hidden card may be one with morph.
        if sorcery_speed {
            actions.extend(
                player
                    .hand
                    .iter()
                    .filter(|c| match self.known(**c) {
                        None => true,
                        Some(k) => k.morph.is_some(),
                    })
                    .map(|c| Action::CastFaceDown(*c)),
            );
        }
        // CR 702.37e, 116.2b: any time its controller has priority, if it
        // would have a morph cost face up.
        if self.permanents.values().any(|perm| perm.face_down) {
            let all = self.characteristics();
            actions.extend(
                self.permanents
                    .iter()
                    .filter(|(id, perm)| {
                        perm.face_down
                            && all.get(*id).is_some_and(|c| c.controller == p)
                            && self.face_up_cost(perm.card).is_some()
                    })
                    .map(|(id, _)| Action::TurnFaceUp(*id)),
            );
        }
        actions.extend(self.mana_activations(p).into_iter().map(Action::Mana));
        actions.extend(self.activations(p).into_iter().map(Action::Activate));
        actions
    }

    /// The morph cost turning a card face up costs, if the engine runs
    /// the card and the cost.
    fn face_up_cost(&self, card: CardId) -> Option<ManaCost> {
        let k = self.known(card)?;
        match (&k.morph, &k.runs) {
            (Some(Morph::Mana(cost)), Ok(_)) => Some(cost.clone()),
            _ => None,
        }
    }

    /// How many more lands than one `p` may play this turn (CR 305.2a).
    fn extra_lands(&self, p: PlayerId) -> u32 {
        self.characteristics()
            .iter()
            .filter(|(_, c)| c.controller == p)
            .map(|(id, c)| {
                let Some(runs) = self
                    .known(self.permanents[id].card)
                    .and_then(|k| k.runs.as_ref().ok())
                else {
                    return 0;
                };
                c.abilities
                    .iter()
                    .filter_map(|a| match a {
                        Ability::Unread(n) => runs.extra_lands.get(n).copied(),
                        _ => None,
                    })
                    .sum::<u32>()
            })
            .sum()
    }

    /// Every activated ability other than a mana ability `p` can activate
    /// now (CR 117.1b), costs they can pay (CR 118.3, 119.4) and all.
    fn activations(&self, p: PlayerId) -> Vec<Activation> {
        let player = self.player(p);
        let mut out = Vec::new();
        for (id, chars) in self.characteristics() {
            if chars.controller != p {
                continue;
            }
            let perm = &self.permanents[&id];
            let Some(runs) = self.known(perm.card).and_then(|k| k.runs.as_ref().ok()) else {
                continue;
            };
            for ability in &chars.abilities {
                let Ability::Unread(n) = ability else {
                    continue;
                };
                let Some(Ok(def)) = runs.activated.get(n) else {
                    continue;
                };
                // CR 602.5a.
                let sick = chars.is(CardType::Creature)
                    && !chars.abilities.contains(&Ability::Keyword(Keyword::Haste))
                    && player.turn_began.is_none_or(|t| perm.since >= t);
                // CR 602.5, 602.5b.
                let Ability::Unread(k) = ability else {
                    continue;
                };
                let opponent_lost = self
                    .players
                    .iter()
                    .enumerate()
                    .any(|(q, pl)| q != p.0 as usize && pl.lost_life);
                if def.cost.tap && (perm.tapped || sick)
                    || def.cost.life > 0 && player.life < def.cost.life as i32
                    || def.limits.once_per_turn && self.used.contains(&(id, *k))
                    || def.limits.opponent_lost_life && !opponent_lost
                    || def.limits.sorcery_speed
                        && !(p == self.active && self.step.is_main() && self.stack.is_empty())
                {
                    continue;
                }
                // CR 602.2b, 601.2c: a target slot nothing can fill.
                let source = Some(&chars);
                if def
                    .program
                    .targets
                    .iter()
                    .any(|t| self.legal_targets(t, p, source).is_empty())
                {
                    continue;
                }
                out.push(Activation {
                    source: id,
                    ability: *ability,
                });
            }
        }
        out
    }

    /// Whether a permanent has a triggered ability that triggers on its
    /// becoming tapped (City of Brass).
    pub fn triggers_on_tapping(&self, id: ObjectId) -> bool {
        let Some(perm) = self.permanents.get(&id) else {
            return false;
        };
        match self.known(perm.card).map(|k| &k.runs) {
            Some(Ok(r)) => r
                .triggered
                .values()
                .any(|t| matches!(t.event, TriggerEvent::BecomesTapped(_))),
            _ => false,
        }
    }

    /// The counter a mana ability takes off its source as its cost.
    pub fn mana_removes(&self, a: &ManaActivation) -> Option<Counter> {
        let Ability::Unread(n) = a.ability else {
            return None;
        };
        let card = self.permanents.get(&a.source)?.card;
        match &self.known(card)?.runs {
            Ok(r) => r.mana.get(&n)?.remove_counter.clone(),
            Err(_) => None,
        }
    }

    /// The counter a mana ability puts on its source as its cost.
    pub fn mana_counter(&self, a: &ManaActivation) -> Option<Counter> {
        let Ability::Unread(n) = a.ability else {
            return None;
        };
        let card = self.permanents.get(&a.source)?.card;
        match &self.known(card)?.runs {
            Ok(r) => r.mana.get(&n)?.counter.clone(),
            Err(_) => None,
        }
    }

    /// The zones a card may be cast from by its alternative costs (CR
    /// 118.9), before any game.
    pub fn alternatives_of(text: &CardText) -> Vec<lower::CastFrom> {
        match &known(text.clone()).runs {
            Ok(r) => r.alternatives.values().map(|a| a.from).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// The zones a known card may be cast from by its alternative costs
    /// (CR 118.9).
    pub fn alternatives(&self, c: CardId) -> Vec<lower::CastFrom> {
        match self.known(c).map(|k| &k.runs) {
            Some(Ok(r)) => r.alternatives.values().map(|a| a.from).collect(),
            _ => Vec::new(),
        }
    }

    /// Why the engine never offers any of this card's activated abilities
    /// other than mana abilities: the first it can't run, or `None` if it
    /// runs one of them (or has none).
    pub fn activation_refusal(&self, c: CardId) -> Option<Unrunnable> {
        match &self.known(c)?.runs {
            Err(r) => Some(r.clone()),
            Ok(runs) if runs.activated.values().any(Result::is_ok) => None,
            Ok(runs) => runs.activated.values().find_map(|a| a.clone().err()),
        }
    }

    /// CR 117.1a, 304.1, 702.8a: an instant, or a card with flash.
    fn instant_speed(&self, entry: &Known) -> bool {
        match &entry.runs {
            Ok(r) => r.flash || r.lowered.chars.is(CardType::Instant),
            Err(_) => type_words(&entry.text.type_line).any(|w| w == "Instant"),
        }
    }

    /// Every mana ability `p` can activate now, once for each choice of
    /// mana it gives and each way to pay its mana cost from the pool.
    fn mana_activations(&self, p: PlayerId) -> Vec<ManaActivation> {
        let pool = self.player(p).pool;
        self.mana_options(p)
            .into_iter()
            .filter(|a| pool.contains(&a.pays))
            .collect()
    }

    /// The mana abilities `p` could activate with enough mana in their
    /// pool: every way to pay each one's mana cost from any mana, so a
    /// payment can be planned before the mana it spends is made.
    pub fn mana_options(&self, p: PlayerId) -> Vec<ManaActivation> {
        let player = self.player(p);
        let began = player.turn_began;
        let mut out = Vec::new();
        let all = self.characteristics();
        for (id, chars) in &all {
            let id = *id;
            if chars.controller != p {
                continue;
            }
            let perm = &self.permanents[&id];
            // CR 302.6, 702.10c: a creature's {T} ability needs it
            // controlled since its controller's most recent turn began.
            let sick = chars.is(CardType::Creature)
                && !chars.abilities.contains(&Ability::Keyword(Keyword::Haste))
                && began.is_none_or(|t| perm.since >= t);
            let runs = self.known(perm.card).and_then(|k| k.runs.as_ref().ok());
            for ability in &chars.abilities {
                let (tap, life, mana, options, sacrifice) = match ability {
                    // CR 305.6.
                    Ability::Intrinsic(t) => {
                        (true, 0, None, vec![ManaPool::of([Mana::of(*t)])], None)
                    }
                    // CR 602.5, 602.5b.
                    Ability::Unread(n)
                        if runs
                            .and_then(|r| r.mana.get(n))
                            .and_then(|a| a.only_if.as_ref())
                            .is_some_and(|c| !self.you_control(c, id, chars, &all))
                            || runs
                                .and_then(|r| r.mana.get(n))
                                .is_some_and(|a| a.once_per_turn)
                                && self.used.contains(&(id, *n))
                            // CR 118.3: a counter it hasn't can't be removed.
                            || runs
                                .and_then(|r| r.mana.get(n))
                                .and_then(|a| a.remove_counter.as_ref())
                                .is_some_and(|c| self.counters_on(id, c) == 0) =>
                    {
                        continue
                    }
                    Ability::Unread(n) => match runs.and_then(|r| r.mana.get(n)) {
                        Some(a) => (
                            a.tap,
                            a.life,
                            a.mana.as_ref(),
                            a.options(player.identity.as_deref(), |f| {
                                self.count(f, id, chars, &all)
                            }),
                            a.sacrifice.as_ref(),
                        ),
                        None => continue,
                    },
                    _ => continue,
                };
                // CR 118.3, 119.4.
                if tap && (perm.tapped || sick) || life > 0 && player.life < life as i32 {
                    continue;
                }
                // Each exact way any mana pays its cost.
                let ways = match mana {
                    None => vec![ManaPool::default()],
                    Some(cost) => {
                        let n = cost.generic + 2 * cost.shards.len() as u32;
                        cost.payments(&ManaPool::of(
                            Mana::ALL
                                .iter()
                                .flat_map(|m| std::iter::repeat_n(*m, n as usize)),
                        ))
                    }
                };
                // CR 701.21a: each permanent it could sacrifice.
                let sacrificed: Vec<Option<ObjectId>> = match sacrifice {
                    None => vec![None],
                    Some(m::Sacrifice::Itself) => vec![Some(id)],
                    Some(m::Sacrifice::One(f)) => {
                        self.sacrificable(p, f).into_iter().map(Some).collect()
                    }
                };
                // CR 605.1b, 605.4a: the triggered mana abilities a
                // permanent tapped for mana triggers add theirs with it.
                let extra = if tap {
                    self.tapped_for_mana(id, &all)
                } else {
                    ManaPool::default()
                };
                let mut seen = BTreeSet::new();
                for mut adds in options {
                    adds.add_all(&extra);
                    for pays in &ways {
                        for sacrifices in &sacrificed {
                            if seen.insert((adds, *pays, *sacrifices)) {
                                out.push(ManaActivation {
                                    source: id,
                                    ability: *ability,
                                    adds,
                                    pays: *pays,
                                    sacrifices: *sacrifices,
                                });
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// The mana the triggered mana abilities on the battlefield add when
    /// `id` is tapped for mana (CR 106.12a, 605.1b), each read from its
    /// own source: Wild Growth's {G} for the land it enchants.
    fn tapped_for_mana(&self, id: ObjectId, all: &BTreeMap<ObjectId, Chars>) -> ManaPool {
        let mut extra = ManaPool::default();
        for (src, chars) in all {
            let Some(runs) = self
                .known(self.permanents[src].card)
                .and_then(|k| k.runs.as_ref().ok())
            else {
                continue;
            };
            for (n, t) in &runs.mana_triggers {
                let seen = Seen {
                    id,
                    chars: &all[&id],
                    source: *src,
                    source_chars: chars,
                    source_attached_to: self.battlefield.objects[src].attached_to,
                    source_chosen: self.battlefield.objects[src].chosen_type.as_ref(),
                    token: self.battlefield.objects[&id].token,
                    attacking: self.battlefield.attacking.contains(&id),
                };
                if chars.abilities.contains(&Ability::Unread(*n)) && t.tapped.matches(&seen) {
                    extra.add_all(&t.adds);
                }
            }
        }
        extra
    }

    /// Whether the source's controller controls as many permanents as the
    /// condition counts, read from the source.
    fn you_control(
        &self,
        c: &model::YouControl,
        source: ObjectId,
        source_chars: &Chars,
        all: &BTreeMap<ObjectId, Chars>,
    ) -> bool {
        let n = all
            .iter()
            .filter(|(o, ch)| {
                ch.controller == source_chars.controller
                    && c.any_of.iter().any(|f| {
                        f.matches(&Seen {
                            id: **o,
                            chars: ch,
                            source,
                            source_chars,
                            source_attached_to: self.battlefield.objects[&source].attached_to,
                            source_chosen: self.battlefield.objects[&source].chosen_type.as_ref(),
                            token: self.battlefield.objects[*o].token,
                            attacking: self.battlefield.attacking.contains(*o),
                        })
                    })
            })
            .count();
        c.count.holds(n as u32)
    }

    /// How many permanents `filter` matches, read from `source` (CR 608.2h).
    fn count(
        &self,
        filter: &Filter,
        source: ObjectId,
        source_chars: &Chars,
        all: &BTreeMap<ObjectId, Chars>,
    ) -> u32 {
        all.iter()
            .filter(|(o, c)| {
                filter.matches(&Seen {
                    id: **o,
                    chars: c,
                    source,
                    source_chars,
                    source_attached_to: self.battlefield.objects[&source].attached_to,
                    source_chosen: self.battlefield.objects[&source].chosen_type.as_ref(),
                    token: self.battlefield.objects[*o].token,
                    attacking: self.battlefield.attacking.contains(*o),
                })
            })
            .count() as u32
    }

    fn payments(&self, p: PlayerId, owed: &ManaCost) -> Vec<Payment> {
        let mut out: Vec<Payment> = self
            .mana_activations(p)
            .into_iter()
            .map(Payment::Mana)
            .collect();
        out.extend(
            owed.payments(&self.player(p).pool)
                .into_iter()
                .map(Payment::Spend),
        );
        out.push(Payment::Cancel);
        out
    }

    fn act(&mut self, p: PlayerId, passes: usize, action: Action) -> Result<(), Rejected> {
        match action {
            Action::Pass => {
                let passes = passes + 1;
                if passes < self.in_game() {
                    let next = self.next_in_game(p);
                    self.give_priority(next, passes);
                } else if self.stack.is_empty() {
                    // CR 117.4, 500.2.
                    self.end_step();
                } else {
                    // CR 405.5, 608.1.
                    self.resolve_top();
                }
            }
            Action::PlayLand(card) => self.play_land(p, card, Side::Front)?,
            Action::PlayBack(card) => self.play_land(p, card, Side::Back)?,
            Action::Cast(card) => self.cast(p, card, Via::ManaCost, Side::Front)?,
            Action::CastBack(card) => self.cast(p, card, Via::ManaCost, Side::Back)?,
            Action::CastAlternative(card, n) => {
                self.cast(p, card, Via::Alternative(n), Side::Front)?
            }
            // CR 702.37c: turned face down, put on the stack, and {3} paid
            // rather than its mana cost, with the cost changes that apply
            // to a face-down spell (CR 708.4, 601.2f).
            Action::CastFaceDown(card) => {
                self.before_cast = Some(Box::new(self.clone()));
                self.players[p.0 as usize].hand.retain(|c| *c != card);
                self.stack.push_back(StackObject::Spell {
                    card,
                    controller: p,
                    targets: Vec::new(),
                    via: Via::FaceDown,
                    kicked: false,
                });
                let owed = self.total_cost(p, card, ManaCost::default().plus_generic(3));
                self.waiting = Waiting::Paying {
                    player: p,
                    spell: card,
                    owed,
                    paying: Paying::Spell {
                        from_command: false,
                    },
                };
            }
            // CR 702.37e: show the morph cost, pay it, then turn it face up.
            Action::TurnFaceUp(id) => {
                let card = self.permanents[&id].card;
                let owed = self
                    .face_up_cost(card)
                    .expect("only one with a morph cost is offered");
                self.before_cast = Some(Box::new(self.clone()));
                self.waiting = Waiting::Paying {
                    player: p,
                    spell: card,
                    owed,
                    paying: Paying::FaceUp(id),
                };
            }
            Action::Mana(a) => {
                self.activate(p, a);
                // CR 117.3c.
                self.give_priority(p, 0);
            }
            // CR 702.29a, 602.2: discarding it is part of the cost, paid
            // as the ability goes on the stack.
            Action::Cycle(card) => {
                let Some((n, def)) = self.runs_of(card).cycling.clone() else {
                    unreachable!("only a card with cycling is offered")
                };
                self.before_cast = Some(Box::new(self.clone()));
                self.discard(p, &[card]);
                self.stack.push_back(StackObject::Ability {
                    source: card,
                    object: None,
                    ability: n,
                    controller: p,
                    targets: Vec::new(),
                });
                // "When you cycle this card": from the graveyard it went
                // to, put on the stack the next time a player would get
                // priority (CR 603.3).
                let cycled: Vec<u16> = self
                    .runs_of(card)
                    .triggered
                    .iter()
                    .filter(|(_, t)| t.event == TriggerEvent::CyclesItself)
                    .map(|(n, _)| *n)
                    .collect();
                for k in cycled {
                    self.pending.push_back(Pending {
                        source: card,
                        object: None,
                        ability: TriggerOf::Printed(k),
                        controller: p,
                        triggering: None,
                        amount: None,
                        player: None,
                        ordered: false,
                    });
                }
                self.pay_ability(p, card, def.cost.mana);
            }
            Action::Activate(a) => {
                let card = self.permanents[&a.source].card;
                let Ability::Unread(n) = a.ability else {
                    unreachable!("only an unread ability is offered to activate")
                };
                let Some(Ok(def)) = self
                    .known(card)
                    .and_then(|k| k.runs.as_ref().ok())
                    .and_then(|r| r.activated.get(&n))
                else {
                    unreachable!("only an ability that runs is offered")
                };
                let cost = def.cost.clone();
                let slots = def.program.targets.clone();
                if def.limits.once_per_turn {
                    self.used.insert((a.source, n));
                }
                self.before_cast = Some(Box::new(self.clone()));
                // CR 602.2a: on the stack first, then its targets chosen
                // and its costs paid (CR 602.2b, 601.2c, 601.2h).
                self.stack.push_back(StackObject::Ability {
                    source: card,
                    object: Some(a.source),
                    ability: n,
                    controller: p,
                    targets: Vec::new(),
                });
                if cost.tap {
                    self.tap(a.source);
                }
                // CR 119.4.
                self.lose_life(p, cost.life);
                if cost.sacrifice {
                    // CR 701.21a.
                    self.put_in_graveyard(a.source);
                }
                if slots.is_empty() {
                    self.pay_ability(p, card, cost.mana);
                } else {
                    self.waiting = Waiting::Targets {
                        player: p,
                        card,
                        slots,
                        choosing: Choosing::Ability {
                            object: a.source,
                            owed: cost.mana,
                        },
                    };
                }
            }
        }
        Ok(())
    }

    /// An activated ability's mana cost paid, if it has one (CR 602.2b,
    /// 601.2g–h), or the ability is activated and its activator gets
    /// priority (CR 117.3c).
    fn pay_ability(&mut self, p: PlayerId, card: CardId, owed: Option<ManaCost>) {
        match owed {
            Some(owed) => {
                self.waiting = Waiting::Paying {
                    player: p,
                    spell: card,
                    owed,
                    paying: Paying::Ability,
                }
            }
            None => {
                self.before_cast = None;
                self.give_priority(p, 0);
            }
        }
    }

    /// CR 305.1: a land played by `side`, which is up as it enters (CR
    /// 712.12).
    fn play_land(&mut self, p: PlayerId, card: CardId, side: Side) -> Result<(), Rejected> {
        let known = self.face(card, side);
        known.runs.as_ref().map_err(|r| Rejected::Refused {
            card: known.text.name.clone(),
            refusal: r.clone(),
        })?;
        self.turn_up(card, side);
        let player = &mut self.players[p.0 as usize];
        player.hand.retain(|c| *c != card);
        player.lands_played += 1;
        // CR 117.3c.
        self.enter(card, p, false, Then::Priority(p));
        Ok(())
    }

    /// One side of a known card: a two-faced card's face, a card with one
    /// face's only one.
    fn face(&self, card: CardId, side: Side) -> &Known {
        match &self.cards[card.0 as usize].face {
            Face::Two(two, _) => two.face(side),
            face => face.up().expect("only a known card is offered"),
        }
    }

    /// What this engine can't cast yet for its mana cost, beyond the
    /// card's abilities.
    fn cast_refusal(entry: &Known, runs: &Runnable) -> Result<ManaCost, Unrunnable> {
        Game::spell_refusal(runs)?;
        entry
            .cost
            .clone()?
            .ok_or_else(|| Unrunnable::new("no mana cost", "it has no mana cost to pay (CR 118.6)"))
    }

    /// What this engine can't cast yet however it is cast, beyond the
    /// card's abilities.
    fn spell_refusal(runs: &Runnable) -> Result<(), Unrunnable> {
        let chars = &runs.lowered.chars;
        // CR 306.5b, 310.4b: they enter with counters the card index
        // doesn't keep, and leave without them (CR 704.5i).
        for (t, node) in [
            (CardType::Planeswalker, "Planeswalker"),
            (CardType::Battle, "Battle"),
        ] {
            if chars.is(t) {
                return Err(Unrunnable::new(
                    node,
                    format!("a {} enters with counters", node.to_lowercase()),
                ));
            }
        }
        // CR 303.4a: an Aura spell targets what its enchant ability says.
        if chars.subtypes.contains(&Subtype::Other("Aura".into())) && runs.enchant.is_none() {
            return Err(Unrunnable::new(
                "Aura",
                "an Aura spell targets what its enchant ability says",
            ));
        }
        Ok(())
    }

    /// CR 601.2a–f: move the card to the stack, then determine its total
    /// cost and wait for it to be paid. The card has no modes, targets, X
    /// or additional costs to announce, since one that has any is refused
    /// when read.
    /// A two-faced card is cast by `side`, only that face evaluated and
    /// put on the stack (CR 712.11b–c).
    fn cast(&mut self, p: PlayerId, card: CardId, via: Via, side: Side) -> Result<(), Rejected> {
        let entry = self.face(card, side);
        let refused = |refusal| Rejected::Refused {
            card: entry.text.name.clone(),
            refusal,
        };
        let runs = entry.runs.as_ref().map_err(|r| refused(r.clone()))?;
        let cost = match via {
            Via::ManaCost => Game::cast_refusal(entry, runs).map_err(refused)?,
            Via::Alternative(n) => {
                Game::spell_refusal(runs).map_err(refused)?;
                runs.alternatives[&n].mana.clone()
            }
            Via::FaceDown => unreachable!("cast face down by its own action"),
        };
        let from_command = via == Via::ManaCost && self.command.contains(&card);
        let kicker = runs.kicker.is_some();
        let before = self.clone();
        self.turn_up(card, side);
        // CR 601.2a: from wherever it is.
        self.command.retain(|c| *c != card);
        if self.adventuring.remove(&card).is_some() {
            self.exile.retain(|c| *c != card);
        }
        let player = &mut self.players[p.0 as usize];
        player.hand.retain(|c| *c != card);
        player.graveyard.retain(|c| *c != card);
        self.stack.push_back(StackObject::Spell {
            card,
            controller: p,
            targets: Vec::new(),
            via,
            kicked: false,
        });
        // CR 903.8: commander tax is on a cast from the command zone.
        let owed = if from_command {
            cost.plus_generic(COMMANDER_TAX * self.commander_casts(card))
        } else {
            cost
        };
        self.before_cast = Some(Box::new(before));
        // CR 601.2b: an optional additional cost is announced first.
        if kicker {
            self.waiting = Waiting::Kicker {
                player: p,
                card,
                base: owed,
                from_command,
            };
            return Ok(());
        }
        self.choose_targets(p, card, owed, from_command);
        Ok(())
    }

    /// CR 601.2c–f: the spell on top of the stack, whose cost before cost
    /// changes is `base`, has its targets chosen, then its total cost paid.
    fn choose_targets(&mut self, p: PlayerId, card: CardId, base: ManaCost, from_command: bool) {
        let slots = self.spell_targets(card);
        let owed = self.total_cost(p, card, base);
        self.waiting = match slots.filter(|s| !s.is_empty()) {
            // CR 601.2c: targets are chosen before the cost is determined
            // and paid.
            Some(slots) => Waiting::Targets {
                player: p,
                card,
                slots,
                choosing: Choosing::Spell { owed, from_command },
            },
            None => Waiting::Paying {
                player: p,
                spell: card,
                owed,
                paying: Paying::Spell { from_command },
            },
        };
    }

    /// The target slots of `card` cast as a spell: its instructions', or
    /// an Aura's one, what it can enchant (CR 303.4a).
    fn spell_targets(&self, card: CardId) -> Option<Vec<TargetSpec>> {
        let runs = self.runs_of(card);
        match &runs.enchant {
            Some(f) => Some(vec![TargetSpec::Permanent(f.clone())]),
            None => runs.spell.as_ref().map(|s| s.targets.clone()),
        }
    }

    /// CR 601.2f: the spell's cost with every cost change on the
    /// battlefield that applies to it, increases added and reductions
    /// taken from the generic component, never below {0} (CR 118.7a).
    fn total_cost(&self, p: PlayerId, card: CardId, cost: ManaCost) -> ManaCost {
        let Some(spell) = self.spell_chars(card) else {
            return cost;
        };
        let spell = &spell;
        let (mut more, mut less) = (0, 0);
        let all = self.characteristics();
        for (id, chars) in all.clone() {
            let Some(runs) = self
                .known(self.permanents[&id].card)
                .and_then(|k| k.runs.as_ref().ok())
            else {
                continue;
            };
            for a in &chars.abilities {
                let Ability::Unread(n) = a else { continue };
                let Some(change) = runs.cost_changes.get(n) else {
                    continue;
                };
                let caster = match change.cast_by {
                    lower::CastBy::Anyone => true,
                    lower::CastBy::You => p == chars.controller,
                    lower::CastBy::Opponents => p != chars.controller,
                };
                let now = !change.your_turn || chars.controller == self.active;
                if caster && now && change.spells.matches(spell) {
                    let times = match &change.per {
                        None => 1,
                        Some(f) => self.count(f, id, &chars, &all),
                    };
                    if change.less {
                        less += change.generic * times;
                    } else {
                        more += change.generic * times;
                    }
                }
            }
        }
        ManaCost {
            generic: (cost.generic + more).saturating_sub(less),
            shards: cost.shards,
        }
    }

    /// What a spell on the stack, or a card about to be one, is to what
    /// looks at spells: a face-down spell's characteristics (CR 708.4), or
    /// its card's printed ones; `None` for a hidden card.
    fn spell_chars(&self, card: CardId) -> Option<Chars> {
        let face_down = self.stack.iter().find_map(|o| match o {
            StackObject::Spell {
                card: c,
                controller,
                via: Via::FaceDown,
                ..
            } if *c == card => Some(*controller),
            _ => None,
        });
        match face_down {
            Some(controller) => Some(face_down_chars(controller)),
            None => self.known(card)?.printed.clone(),
        }
    }

    /// The permanents its cost returns to hand, once any it sacrifices are
    /// gone, then the spell becomes cast.
    fn return_or_cast(&mut self, player: PlayerId, spell: CardId) {
        match self.other_costs(spell).returned {
            Some((count, filter)) => {
                self.waiting = Waiting::ReturnToHand {
                    player,
                    spell,
                    count,
                    filter,
                }
            }
            None => self.cast_done(player, spell),
        }
    }

    /// What the spell on the stack costs beyond mana: its alternative
    /// cost's, if it was cast for one (CR 601.2f).
    fn other_costs(&self, spell: CardId) -> lower::OtherCosts {
        let via = self.stack.iter().find_map(|o| match o {
            StackObject::Spell { card, via, .. } if *card == spell => Some(*via),
            _ => None,
        });
        match via {
            Some(Via::Alternative(n)) => self.runs_of(spell).alternatives[&n].other.clone(),
            _ => lower::OtherCosts::default(),
        }
    }

    /// CR 601.2i: the spell becomes cast, what waits for a spell cast
    /// triggers, and its caster had priority, so gets it again.
    fn cast_done(&mut self, player: PlayerId, spell: CardId) {
        self.before_cast = None;
        self.trigger_on(Event::Cast {
            card: spell,
            controller: player,
        });
        self.give_priority(player, 0);
    }

    /// A spell leaves the stack for its owner's graveyard, or exile if it
    /// was cast for flashback (CR 702.34a), or, `resolved` with its
    /// controller, as an Adventure: exiled, and that player may cast it
    /// from there (CR 715.3d).
    fn off_the_stack(&mut self, card: CardId, via: Via, resolved: Option<PlayerId>) {
        let adventure = self.side_up(card) == Side::Back
            && self
                .two_faces(card)
                .is_some_and(|t| t.layout == TwoFaced::Adventure);
        // CR 712.8a, 715.4: its front face, its normal characteristics, in
        // any other zone.
        self.turn_up(card, Side::Front);
        if let (true, Some(controller)) = (adventure, resolved) {
            self.exile.push_back(card);
            self.adventuring.insert(card, controller);
            return;
        }
        let exiled = match via {
            Via::Alternative(n) => self.runs_of(card).alternatives[&n].exiled,
            // CR 708.9: revealed as it leaves the stack.
            Via::ManaCost | Via::FaceDown => false,
        };
        if exiled {
            self.exile.push_back(card);
        } else {
            let owner = self.owner(card);
            self.players[owner.0 as usize].graveyard.push_back(card);
        }
    }

    /// Pay a mana ability's cost and resolve it at once (CR 605.3b).
    fn activate(&mut self, p: PlayerId, a: ManaActivation) {
        let chars = self.characteristics();
        let source = &chars[&a.source];
        let perm = &self.permanents[&a.source];
        let (tap, life, damage, counter, once, removes) = match a.ability {
            Ability::Intrinsic(_) => (true, 0, 0, None, None, None),
            Ability::Unread(n) => {
                let runs = self.known(perm.card).and_then(|k| k.runs.as_ref().ok());
                let def = &runs.expect("a listed ability runs").mana[&n];
                let once = def.once_per_turn.then_some(n);
                let removes = def
                    .remove_counter
                    .clone()
                    .map(|c| (c, def.sacrificed_when_none));
                (
                    def.tap,
                    def.life,
                    def.damage_to_you,
                    def.counter.clone(),
                    once,
                    removes,
                )
            }
            _ => unreachable!("only mana abilities are listed"),
        };
        if let Some((c, _)) = &removes {
            self.battlefield.remove_counters(a.source, c, 1);
        }
        if let Some(n) = once {
            self.used.insert((a.source, n));
        }
        // CR 602.2b: a counter put on it is part of the cost.
        if let Some(c) = counter {
            self.battlefield.put_counters(a.source, c, 1);
        }
        let lifelink = source
            .abilities
            .contains(&Ability::Keyword(Keyword::Lifelink));
        if tap {
            self.tap(a.source);
        }
        // CR 119.4.
        self.lose_life(p, life);
        let player = &mut self.players[p.0 as usize];
        player.pool = player.pool.minus(&a.pays);
        player.pool.add_all(&a.adds);
        // CR 120.3a, 120.3f: its damage to its controller is life lost,
        // and gained back if it has lifelink; CR 615.6: none, prevented.
        let damage = if self.you_have(p, ForYou::PreventAllDamage) {
            0
        } else {
            damage
        };
        self.lose_life(p, damage);
        if lifelink {
            self.players[p.0 as usize].life += damage as i32;
        }
        if let Some(o) = a.sacrifices {
            self.put_in_graveyard(o);
        }
        // "If there are no mining counters on this land, sacrifice it."
        if let Some((c, true)) = removes {
            if self.counters_on(a.source, &c) == 0 {
                self.put_in_graveyard(a.source);
            }
        }
    }

    /// The top of the stack resolves (CR 608), and the active player gets
    /// priority (CR 117.3b).
    fn resolve_top(&mut self) {
        let then = Then::Priority(self.active);
        match self.stack.pop_back() {
            // CR 702.37c: it enters with the face-down spell's
            // characteristics.
            Some(StackObject::Spell {
                card,
                controller,
                via: Via::FaceDown,
                ..
            }) => {
                self.arrive_face_down(card, controller);
                self.proceed(then);
            }
            Some(StackObject::Spell {
                card,
                controller,
                targets,
                via,
                kicked,
            }) => {
                let permanent = self
                    .known(card)
                    .and_then(|k| k.runs.as_ref().ok())
                    .is_some_and(|r| is_permanent(&r.lowered.chars));
                let enchant = self.runs_of(card).enchant.clone();
                if let (true, Some(filter), [to]) = (permanent, enchant, targets.as_slice()) {
                    // CR 608.3b: an Aura whose target is illegal doesn't
                    // resolve; CR 608.3c: one whose target is legal enters
                    // attached to it.
                    let source = self.source_chars(card, None);
                    let spec = TargetSpec::Permanent(filter);
                    match to {
                        Target::Object(o)
                            if self
                                .legal_targets(&spec, controller, source.as_ref())
                                .contains(to) =>
                        {
                            let (tapped, _, _) = self.entering(card, controller);
                            self.arrive_attached(card, controller, tapped, Some(*o));
                            self.proceed(then);
                        }
                        _ => {
                            self.off_the_stack(card, via, None);
                            self.give_priority(self.active, 0);
                        }
                    }
                } else if permanent {
                    // CR 608.3a.
                    self.enter(card, controller, false, then);
                } else {
                    self.begin_resolving(
                        Instructions::Spell { card, via, kicked },
                        controller,
                        (None, None, None, None),
                        targets,
                    );
                }
            }
            Some(StackObject::Ability {
                source,
                object,
                ability,
                controller,
                targets,
            }) => self.begin_resolving(
                Instructions::Activated(source, ability),
                controller,
                (object, None, None, None),
                targets,
            ),
            Some(StackObject::Trigger {
                source,
                object,
                ability,
                controller,
                targets,
                triggering,
                amount,
                player,
                ..
            }) => self.begin_resolving(
                Instructions::Triggered(source, ability),
                controller,
                (object, triggering, amount, player),
                targets,
            ),
            None => self.proceed(then),
        }
    }

    /// CR 608.2b: each target is checked again as the spell or ability
    /// resolves; one with targets, all of them illegal now, doesn't
    /// resolve, and otherwise an illegal one is left alone.
    fn begin_resolving(
        &mut self,
        what: Instructions,
        controller: PlayerId,
        (object, triggering, amount, player): (
            Option<ObjectId>,
            Option<ObjectId>,
            Option<u32>,
            Option<PlayerId>,
        ),
        targets: Vec<Target>,
    ) {
        let slots = self.program(what).targets.clone();
        let source = self.source_chars(what.card(), object);
        let checked: Vec<Option<Target>> = targets
            .iter()
            .zip(&slots)
            .map(|(t, s)| {
                self.legal_targets(s, controller, source.as_ref())
                    .contains(t)
                    .then_some(*t)
            })
            .collect();
        let at = Resuming {
            what,
            controller,
            object,
            targets: checked,
            triggering,
            amount,
            player,
            next: vec![0],
            found: OrdMap::new(),
            name: None,
        };
        if !targets.is_empty() && at.targets.iter().all(Option::is_none) {
            return self.done(&at);
        }
        // CR 603.4: an intervening "if" checked again as it resolves, from
        // its source as it is now or last existed.
        if let Instructions::Triggered(card, TriggerOf::Printed(n)) = what {
            if let Some(c) = self.runs_of(card).triggered[&n].condition.clone() {
                let all = self.characteristics();
                let holds = source.as_ref().is_some_and(|sc| {
                    self.intervening(
                        &c,
                        object.unwrap_or(ObjectId(u32::MAX)),
                        sc,
                        at.triggering,
                        &all,
                    )
                });
                if !holds {
                    return self.done(&at);
                }
            }
        }
        self.follow(at)
    }

    /// The instructions a spell or ability on the stack follows.
    fn program(&self, what: Instructions) -> &Program {
        static NONE: Program = Program {
            targets: Vec::new(),
            effects: Vec::new(),
        };
        match what {
            Instructions::Spell { card: c, .. } => self.runs_of(c).spell.as_ref().unwrap_or(&NONE),
            Instructions::Triggered(c, TriggerOf::Printed(n)) => {
                &self.runs_of(c).triggered[&n].program
            }
            Instructions::Triggered(_, TriggerOf::Delayed(k)) => &self.delayed[k as usize].program,
            Instructions::Activated(c, n) => match self.runs_of(c).activated.get(&n) {
                Some(Ok(def)) => &def.program,
                _ => match &self.runs_of(c).cycling {
                    Some((k, def)) if *k == n => &def.program,
                    _ => &NONE,
                },
            },
        }
    }

    /// The spell or ability is done: a spell goes to its owner's graveyard
    /// (CR 608.2n), and the active player gets priority (CR 117.3b).
    fn done(&mut self, at: &Resuming) {
        self.last_resolved = Some(at.what.card());
        if let Instructions::Spell { card, via, .. } = at.what {
            // CR 608.2b: one whose targets are all illegal doesn't resolve.
            let resolved = at.targets.is_empty() || at.targets.iter().any(Option::is_some);
            self.off_the_stack(card, via, resolved.then_some(at.controller));
        }
        self.give_priority(self.active, 0);
    }

    /// Follow a resolving spell's or ability's instructions in order from
    /// `at.next` (CR 608.2c), stopping at one that asks for a choice.
    fn follow(&mut self, mut at: Resuming) {
        let program = self.program(at.what).effects.clone();
        let p = at.controller;
        loop {
            // The list the cursor is in: the program's, or a "may"'s
            // inside it.
            let (k, outer) = at.next.split_last().expect("a cursor");
            let mut list: &[Effect] = &program;
            for j in outer {
                list = list[*j]
                    .inner()
                    .expect("a cursor enters only what holds instructions");
            }
            let Some(e) = list.get(*k).cloned() else {
                // The end of a "may" goes on after it; of the program,
                // the spell or ability is done.
                if at.next.len() == 1 {
                    break;
                }
                at.next.pop();
                *at.next.last_mut().expect("a cursor") += 1;
                continue;
            };
            let here = at.clone();
            *at.next.last_mut().expect("a cursor") += 1;
            let resume = at.clone();
            // CR 608.2b: an instruction naming an illegal target does
            // nothing to it.
            let defending = at
                .object
                .and_then(|o| self.combat.as_ref()?.attackers.get(&o).copied());
            let who = |w: Who| match w {
                Who::You => Some(p),
                Who::Target(s) => match at.targets[s] {
                    Some(Target::Player(q)) => Some(q),
                    _ => None,
                },
                Who::Defending => defending,
                Who::ControllerOf(o) => self.now_or_last(&at, o).map(|c| c.controller),
                Who::EventPlayer => at.player,
                Who::SpellController(s) => match at.targets[s] {
                    Some(Target::Spell(c)) => self.stack.iter().find_map(|o| match o {
                        StackObject::Spell {
                            card, controller, ..
                        } if *card == c => Some(*controller),
                        _ => None,
                    }),
                    _ => None,
                },
            };
            match e {
                Effect::Search {
                    who: w,
                    filter,
                    into,
                    enters,
                    reveals,
                } => {
                    let Some(q) = who(w) else { continue };
                    self.waiting = Waiting::Search {
                        player: q,
                        filter,
                        enters,
                        reveals,
                        into,
                        resume,
                    };
                    return;
                }
                // One card at a time, each entering as it would alone;
                // what has moved is no longer in the library.
                Effect::ToBattlefield {
                    cards,
                    tapped,
                    who: w,
                } => {
                    let Some(q) = who(w) else { continue };
                    if let Some(c) = self.still_in_library(&at, cards).first().copied() {
                        let owner = self.owner(c);
                        self.players[owner.0 as usize].library.retain(|x| *x != c);
                        self.enter(c, q, tapped, Then::Resume(here));
                        return;
                    }
                }
                // CR 701.24a.
                Effect::Shuffle(w) => {
                    let Some(q) = who(w) else { continue };
                    let mut library = std::mem::take(&mut self.players[q.0 as usize].library);
                    self.rng.shuffle(&mut library);
                    self.players[q.0 as usize].library = library;
                    self.shuffled += 1;
                }
                Effect::Draw { who: w, n } => {
                    if let Some(q) = who(w) {
                        for _ in 0..self.amount(&at, n) {
                            self.draw(q);
                        }
                    }
                }
                Effect::Discard { who: w, n, into } => {
                    let Some(q) = who(w) else { continue };
                    let count = (n as usize).min(self.player(q).hand.len());
                    if count > 0 {
                        self.waiting = Waiting::Discard {
                            player: q,
                            count,
                            then: Some((resume, into)),
                        };
                        return;
                    }
                }
                Effect::RevealChooseDiscard { who: w, filter } => {
                    let Some(q) = who(w) else { continue };
                    self.waiting = Waiting::ChooseFromHand {
                        chooser: p,
                        whose: q,
                        filter,
                        resume,
                    };
                    return;
                }
                // CR 119.3.
                Effect::LoseLife { who: w, n } => {
                    let n = self.amount(&at, n);
                    if let Some(q) = who(w) {
                        self.lose_life(q, n);
                    }
                }
                Effect::GainLife { who: w, n } => {
                    let n = self.amount(&at, n);
                    if let Some(q) = who(w) {
                        self.players[q.0 as usize].life += n as i32;
                    }
                }
                // CR 702.12b.
                Effect::Destroy(on) => {
                    for o in self.objects_on(&at, &on) {
                        if self
                            .characteristics()
                            .get(&o)
                            .is_some_and(|c| !c.has(Keyword::Indestructible))
                        {
                            self.put_in_graveyard(o);
                        }
                    }
                }
                Effect::Exile(on) => {
                    for o in self.objects_on(&at, &on) {
                        if self.permanents.contains_key(&o) {
                            self.leave_battlefield(o, Zone::Exile);
                        }
                    }
                }
                // CR 701.26a.
                Effect::Attach { to } => {
                    let (Some(what), Some(to)) = (at.object, self.named(&at, to)) else {
                        continue;
                    };
                    let there = |o| self.permanents.contains_key(&o);
                    if there(what)
                        && there(to)
                        && what != to
                        && self.battlefield.objects[&what].attached_to != Some(to)
                    {
                        self.battlefield.attach(what, to);
                    }
                }
                Effect::Tap(ref on) | Effect::Untap(ref on) => {
                    let tapped = matches!(e, Effect::Tap(_));
                    for o in self.objects_on(&at, on) {
                        if tapped {
                            self.tap(o);
                        } else if let Some(perm) = self.permanents.get_mut(&o) {
                            perm.tapped = false;
                        }
                    }
                }
                Effect::ToOwnersHand(on) => {
                    for o in self.objects_on(&at, &on) {
                        if self.permanents.contains_key(&o) {
                            self.leave_battlefield(o, Zone::Hand);
                        }
                    }
                }
                // CR 701.21a: only a permanent its controller controls.
                Effect::Sacrifice(o) => {
                    if let Some(o) = self.object(&at, o) {
                        if self.characteristics()[&o].controller == p {
                            self.put_in_graveyard(o);
                        }
                    }
                }
                // CR 111.2: its creator owns and controls it.
                Effect::Create { n, token, tapped } => {
                    for _ in 0..self.amount(&at, n) {
                        self.cards.push_back(CardEntry {
                            owner: p,
                            commander: false,
                            token: true,
                            face: Face::of((*token).clone()),
                        });
                        let card = CardId(self.cards.len() as u32 - 1);
                        self.arrive(card, p, tapped);
                        self.created += 1;
                    }
                }
                // CR 701.6a.
                Effect::Counter(slot) => {
                    if let Some(Target::Spell(c)) = at.targets[slot] {
                        let on_stack = self.stack.iter().position(
                            |x| matches!(x, StackObject::Spell { card, .. } if *card == c),
                        );
                        if let Some(k) = on_stack {
                            if let StackObject::Spell { via, .. } = self.stack.remove(k) {
                                self.off_the_stack(c, via, None);
                            }
                        }
                    }
                }
                Effect::Damage {
                    to: DamageTo::Each(f),
                    n,
                } => {
                    let n = self.amount(&at, n);
                    let hits: Vec<(Target, u32)> = self
                        .objects_on(&at, &lower::On::Each(f))
                        .into_iter()
                        .map(|o| (Target::Object(o), n))
                        .collect();
                    let source = self.source_of(&at);
                    let hit = self.deal(source.as_ref(), at.controller, &hits);
                    let by = at.object.filter(|o| self.permanents.contains_key(o));
                    self.damage_triggers(hit, by, false, at.controller);
                }
                Effect::Damage { to, n } => {
                    let to = match to {
                        DamageTo::Each(_) => unreachable!("dealt above"),
                        DamageTo::You => Some(Target::Player(p)),
                        DamageTo::EventPlayer => at.player.map(Target::Player),
                        DamageTo::Target(s) => at.targets[s],
                        DamageTo::Attacked => at
                            .object
                            .and_then(|o| self.combat.as_ref()?.attackers.get(&o).copied())
                            .map(Target::Player),
                    };
                    let n = self.amount(&at, n);
                    if let Some(to) = to {
                        let source = self.source_of(&at);
                        let hit = self.deal(source.as_ref(), at.controller, &[(to, n)]);
                        let by = at.object.filter(|o| self.permanents.contains_key(o));
                        self.damage_triggers(hit, by, false, at.controller);
                    }
                }
                // CR 106.3.
                Effect::AddMana(mana) => self.players[p.0 as usize].pool.add_all(&mana),
                Effect::AddManaOf(options) => {
                    self.waiting = Waiting::ChooseMana {
                        player: p,
                        options,
                        resume,
                    };
                    return;
                }
                // CR 603.7a.
                Effect::Delayed { step, effects } => self.delayed.push_back(Delayed {
                    source: at.what.card(),
                    controller: p,
                    step,
                    created: self.turn,
                    program: Program {
                        targets: Vec::new(),
                        effects,
                    },
                    fired: false,
                }),
                Effect::Scry(n) => {
                    let cards: Vec<CardId> = self
                        .player(p)
                        .library
                        .iter()
                        .take(n as usize)
                        .copied()
                        .collect();
                    if !cards.is_empty() {
                        self.waiting = Waiting::Scry {
                            player: p,
                            cards,
                            resume,
                        };
                        return;
                    }
                }
                Effect::PutCounters { on, counter, n } => {
                    for o in self.objects_on(&at, &on) {
                        self.battlefield.put_counters(o, counter.clone(), n);
                    }
                }
                Effect::UntilEndOfTurn { on, ops, per } => {
                    let ops = match per {
                        None => ops,
                        Some(f) => {
                            let all = self.characteristics();
                            let n = at
                                .object
                                .filter(|o| all.contains_key(o))
                                .map_or(0, |o| self.count(&f, o, &all[&o], &all))
                                as i32;
                            ops.into_iter()
                                .map(|op| match op {
                                    model::Op::ModifyPt(d) => {
                                        model::Op::ModifyPt(model::pt(d.power * n, d.toughness * n))
                                    }
                                    other => other,
                                })
                                .collect()
                        }
                    };
                    // CR 611.2c: the set it affects fixed as it resolves.
                    let set = self.objects_on(&at, &on);
                    if !set.is_empty() {
                        let t = self.battlefield.resolve(set, ops);
                        self.until_end_of_turn.push(t);
                    }
                }
                Effect::May(w, _) => {
                    let Some(q) = who(w) else { continue };
                    self.waiting = Waiting::May {
                        player: q,
                        at: here,
                    };
                    return;
                }
                Effect::PutFromHand {
                    filter,
                    tapped,
                    mana_value,
                } => {
                    let filter = match mana_value {
                        None => filter,
                        // CR 608.2h: counted now, from the source as it
                        // last existed if it has left.
                        Some(kind) => {
                            let n = at.object.map_or(0, |o| self.counters_on(o, &kind));
                            CardFilter(
                                filter
                                    .0
                                    .into_iter()
                                    .map(|mut f| {
                                        f.0.push(Pred::ManaValue(Cmp::Eq, n));
                                        f
                                    })
                                    .collect(),
                            )
                        }
                    };
                    self.waiting = Waiting::PutFromHand {
                        player: p,
                        filter,
                        tapped,
                        resume,
                    };
                    return;
                }
                Effect::RevealTop { whose, n, into } | Effect::LookAtTop { whose, n, into } => {
                    let revealed = matches!(e, Effect::RevealTop { .. });
                    let Some(q) = who(whose) else { continue };
                    let cards: Vec<CardId> = self
                        .player(q)
                        .library
                        .iter()
                        .take(n as usize)
                        .copied()
                        .collect();
                    at.found.insert(into, cards.clone());
                    let hidden: Vec<CardId> = cards
                        .into_iter()
                        .filter(|c| self.known(*c).is_none())
                        .collect();
                    // A card revealed is seen by all (CR 701.20a), so a
                    // hidden one is named; one looked at stays hidden.
                    if revealed && !hidden.is_empty() {
                        self.waiting = Waiting::Reveal {
                            player: q,
                            cards: hidden,
                            resume: at,
                        };
                        return;
                    }
                }
                Effect::Gather { from, into } => {
                    let cards = from
                        .iter()
                        .flat_map(|s| at.found.get(s).cloned().unwrap_or_default())
                        .collect();
                    at.found.insert(into, cards);
                }
                Effect::ChooseAmong { from, n, into } => {
                    let cards = self.still_in_library(&at, Cards::Bound(from));
                    if cards.len() <= n as usize {
                        at.found.insert(into, cards);
                    } else {
                        self.waiting = Waiting::ChooseAmong {
                            player: p,
                            cards,
                            n: n as usize,
                            into,
                            resume: at,
                        };
                        return;
                    }
                }
                Effect::ToHand { cards, who: w } => {
                    let Some(q) = who(w) else { continue };
                    for c in self.still_in_library(&at, cards) {
                        let owner = self.owner(c);
                        self.players[owner.0 as usize].library.retain(|x| *x != c);
                        self.players[q.0 as usize].hand.push_back(c);
                    }
                }
                Effect::ToLibrary { cards, top, from } => {
                    let cards = self.still_found(&at, cards, from);
                    match cards.len() {
                        0 => {}
                        1 => self.put_in_library(&cards, top),
                        _ => {
                            self.waiting = Waiting::Arrange {
                                player: p,
                                cards,
                                top,
                                resume: at,
                            };
                            return;
                        }
                    }
                }
                Effect::IfControl { control, .. } => {
                    let all = self.characteristics();
                    let n = all
                        .iter()
                        .filter(|(o, c)| {
                            c.controller == p
                                && control.any_of.iter().any(|f| {
                                    f.matches(&Seen {
                                        id: **o,
                                        chars: c,
                                        source: ObjectId(u32::MAX),
                                        source_chars: c,
                                        source_attached_to: None,
                                        source_chosen: None,
                                        token: self.battlefield.objects[*o].token,
                                        attacking: self.battlefield.attacking.contains(*o),
                                    })
                                })
                        })
                        .count();
                    if control.count.holds(n as u32) {
                        at = here;
                        at.next.push(0);
                    }
                }
                // CR 118.12a: they may pay; if they don't, what follows.
                Effect::UnlessLife { who: w, life, .. } => {
                    let Some(q) = who(w) else { continue };
                    // CR 119.4: with too little life, the rest is followed.
                    if self.player(q).life < life as i32 {
                        at = here;
                        at.next.push(0);
                        continue;
                    }
                    self.waiting = Waiting::PayLifeOr {
                        player: q,
                        life,
                        at: here,
                    };
                    return;
                }
                Effect::ChooseDrawn { n, into } => {
                    let player = self.player(p);
                    let cards: Vec<CardId> = player
                        .hand
                        .iter()
                        .copied()
                        .filter(|c| player.drawn.contains(c))
                        .collect();
                    if cards.len() <= n as usize {
                        at.found.insert(into, cards);
                    } else {
                        self.waiting = Waiting::ChooseAmong {
                            player: p,
                            cards,
                            n: n as usize,
                            into,
                            resume: at,
                        };
                        return;
                    }
                }
                Effect::Nth { from, k, into } => {
                    let card: Vec<CardId> = at
                        .found
                        .get(&from)
                        .and_then(|v| v.get(k))
                        .copied()
                        .into_iter()
                        .collect();
                    at.found.insert(into, card);
                }
                Effect::Unless { who: w, cost, .. } => {
                    let Some(q) = who(w) else { continue };
                    self.waiting = match cost {
                        lower::UnlessCost::Mana(owed) => Waiting::PayUnless {
                            player: q,
                            owed,
                            at: here,
                        },
                        lower::UnlessCost::Discard(filter) => {
                            // With no card to discard, what follows.
                            let (legal, unsure) = self.discardable(q, &filter);
                            if legal.is_empty() && unsure == 0 {
                                at = here;
                                at.next.push(0);
                                continue;
                            }
                            Waiting::DiscardUnless {
                                player: q,
                                filter,
                                at: here,
                            }
                        }
                        lower::UnlessCost::SacrificePower { filter, power } => {
                            // Nor with too little power to sacrifice.
                            let most: u32 = self
                                .sacrificable(q, &filter)
                                .iter()
                                .map(|o| self.power(*o))
                                .sum();
                            if most < power {
                                at = here;
                                at.next.push(0);
                                continue;
                            }
                            Waiting::SacrificeUnless {
                                player: q,
                                filter,
                                power,
                                at: here,
                            }
                        }
                    };
                    return;
                }
                Effect::ChooseName(filter) => {
                    self.waiting = Waiting::ChooseName {
                        player: p,
                        filter,
                        resume,
                    };
                    return;
                }
                // CR 701.20a: seen by all, so a hidden card of it is named.
                Effect::RevealHand(w) => {
                    let Some(q) = who(w) else { continue };
                    let hidden: Vec<CardId> = self
                        .player(q)
                        .hand
                        .iter()
                        .copied()
                        .filter(|c| self.known(*c).is_none())
                        .collect();
                    if !hidden.is_empty() {
                        self.waiting = Waiting::Reveal {
                            player: q,
                            cards: hidden,
                            resume,
                        };
                        return;
                    }
                }
                // CR 701.9a: no choice in it, the name chosen.
                Effect::DiscardNamed(w) => {
                    let Some(q) = who(w) else { continue };
                    let cards: Vec<CardId> = self
                        .player(q)
                        .hand
                        .iter()
                        .copied()
                        .filter(|c| {
                            at.name
                                .as_ref()
                                .is_some_and(|n| self.card(*c).is_some_and(|t| &t.name == n))
                        })
                        .collect();
                    self.discard(q, &cards);
                    self.discarded += cards.len() as u32;
                }
                Effect::Kicked { was, .. } => {
                    let kicked = matches!(at.what, Instructions::Spell { kicked: true, .. });
                    if kicked == was {
                        at = here;
                        at.next.push(0);
                    }
                }
                Effect::IfCard { card, filter, .. } => {
                    let holds = match at.found.get(&card).map(Vec::as_slice) {
                        Some([c]) => self
                            .known(*c)
                            .and_then(|k| k.printed.as_ref())
                            .is_some_and(|ch| filter.matches(ch)),
                        _ => false,
                    };
                    if holds {
                        at = here;
                        at.next.push(0);
                    }
                }
            }
        }
        self.done(&at)
    }

    /// The cards the instructions resolving have found so far: a search
    /// for "up to two" finds each once (CR 701.23a).
    fn found_already(at: &Resuming) -> BTreeSet<CardId> {
        at.found.values().flatten().copied().collect()
    }

    /// The cards an instruction names that are still in a library, where
    /// the instruction found them: one moved since is a new object (CR
    /// 400.7).
    fn still_in_library(&self, at: &Resuming, cards: Cards) -> Vec<CardId> {
        self.still_found(at, cards, lower::FoundIn::Library)
    }

    /// The cards an instruction names that are still in the zone `from`
    /// it found them in (CR 400.7).
    fn still_found(&self, at: &Resuming, cards: Cards, from: lower::FoundIn) -> Vec<CardId> {
        let bound = |s: Slot| at.found.get(&s).cloned().unwrap_or_default();
        let named = match cards {
            Cards::Bound(s) => bound(s),
            Cards::Rest { of, from } => {
                let of = bound(of);
                bound(from)
                    .into_iter()
                    .filter(|c| !of.contains(c))
                    .collect()
            }
            Cards::Matching { from, filter } => bound(from)
                .into_iter()
                .filter(|c| {
                    self.known(*c)
                        .and_then(|k| k.printed.as_ref())
                        .is_some_and(|ch| filter.matches(ch))
                })
                .collect(),
        };
        named
            .into_iter()
            .filter(|c| {
                let owner = self.player(self.owner(*c));
                match from {
                    lower::FoundIn::Library => owner.library.contains(c),
                    lower::FoundIn::Hand => owner.hand.contains(c),
                }
            })
            .collect()
    }

    /// Put these cards of a library or a hand on the library's top or
    /// bottom, the first topmost (CR 401.4).
    fn put_in_library(&mut self, cards: &[CardId], top: bool) {
        for c in cards {
            let owner = self.owner(*c);
            let player = &mut self.players[owner.0 as usize];
            player.library.retain(|x| x != c);
            player.hand.retain(|x| x != c);
        }
        if top {
            for c in cards.iter().rev() {
                let owner = self.owner(*c);
                self.players[owner.0 as usize].library.push_front(*c);
            }
        } else {
            for c in cards {
                let owner = self.owner(*c);
                self.players[owner.0 as usize].library.push_back(*c);
                self.bottomed += 1;
            }
        }
    }

    /// How much an amount is as the instruction is followed.
    fn amount(&self, at: &Resuming, n: lower::Amount) -> u32 {
        match n {
            lower::Amount::N(n) => n,
            lower::Amount::CountOf(f) => self.objects_on(at, &lower::On::Each(f)).len() as u32,
            lower::Amount::PerFound { each, slot, filter } => {
                let matching = at
                    .found
                    .get(&slot)
                    .map(|cards| {
                        cards
                            .iter()
                            .filter(|c| {
                                self.known(**c)
                                    .and_then(|k| k.printed.as_ref())
                                    .is_some_and(|ch| filter.matches(ch))
                            })
                            .count()
                    })
                    .unwrap_or(0);
                each * matching as u32
            }
            lower::Amount::CardsInGraveyards {
                whose,
                named,
                filter,
            } => self
                .players
                .iter()
                .enumerate()
                .filter(|(q, _)| match whose {
                    lower::Whose::Yours => *q == at.controller.0 as usize,
                    lower::Whose::Opponents => *q != at.controller.0 as usize,
                    lower::Whose::Each => true,
                })
                .flat_map(|(_, player)| &player.graveyard)
                .filter(|c| {
                    self.known(**c).is_some_and(|k| {
                        named.as_ref().is_none_or(|n| k.text.name == *n)
                            && k.printed.as_ref().is_some_and(|ch| filter.matches(ch))
                    })
                })
                .count() as u32,
            lower::Amount::ThatMuch => at.amount.unwrap_or(0),
            lower::Amount::PowerOf(o) => self
                .now_or_last(at, o)
                .and_then(|c| c.pt)
                .map_or(0, |p| p.power.max(0) as u32),
            lower::Amount::ManaValueOf(s) => match at.found.get(&s).map(Vec::as_slice) {
                Some([c]) => self
                    .known(*c)
                    .and_then(|k| k.printed.as_ref())
                    .map_or(0, |ch| ch.mana_value),
                _ => 0,
            },
        }
    }

    /// The permanent an instruction names, if it is still on the
    /// battlefield: one that left is a new object, and the instruction
    /// does nothing to it (CR 400.7).
    fn object(&self, at: &Resuming, on: Obj) -> Option<ObjectId> {
        self.named(at, on)
            .filter(|o| self.permanents.contains_key(o))
    }

    /// The permanents an instruction acts on: the one it names, if it is
    /// still there, or each the filter matches now, read from the
    /// ability's source; a spell's, off the battlefield, is its card with
    /// its controller (CR 608.2h).
    fn objects_on(&self, at: &Resuming, on: &lower::On) -> Vec<ObjectId> {
        match on {
            lower::On::One(o) => self.object(at, *o).into_iter().collect(),
            lower::On::Each(f) => {
                let all = self.characteristics();
                let here = at.object.filter(|o| all.contains_key(o));
                let mut card = self.known(at.what.card()).and_then(|k| k.printed.clone());
                if let Some(c) = &mut card {
                    c.controller = at.controller;
                }
                let Some(source_chars) = here.map(|o| &all[&o]).or(card.as_ref()) else {
                    return Vec::new();
                };
                let source = here.unwrap_or(ObjectId(u32::MAX));
                all.iter()
                    .filter(|(o, c)| {
                        f.matches(&Seen {
                            id: **o,
                            chars: c,
                            source,
                            source_chars,
                            source_attached_to: here
                                .and_then(|s| self.battlefield.objects[&s].attached_to),
                            source_chosen: here
                                .and_then(|s| self.battlefield.objects[&s].chosen_type.as_ref()),
                            token: self.battlefield.objects[*o].token,
                            attacking: self.battlefield.attacking.contains(*o),
                        })
                    })
                    .map(|(o, _)| *o)
                    .collect()
            }
        }
    }

    /// The object an instruction names, whether or not it is still there.
    fn named(&self, at: &Resuming, on: Obj) -> Option<ObjectId> {
        match on {
            Obj::Itself => at.object,
            Obj::Triggering => at.triggering,
            Obj::Target(s) => match at.targets[s] {
                Some(Target::Object(o)) => Some(o),
                _ => None,
            },
            Obj::Found(s) => match at.found.get(&s).map(Vec::as_slice) {
                Some([c]) => self
                    .permanents
                    .iter()
                    .find(|(_, p)| p.card == *c)
                    .map(|(o, _)| *o),
                _ => None,
            },
        }
    }

    /// What an object an instruction names is, or was as it last existed
    /// (CR 608.2h).
    fn now_or_last(&self, at: &Resuming, on: Obj) -> Option<Chars> {
        let o = self.named(at, on)?;
        self.characteristics()
            .remove(&o)
            .or_else(|| self.last_known.get(&o).map(|l| l.chars.clone()))
    }

    /// What the resolving spell or ability's source is now, or as it last
    /// existed (CR 113.7a).
    fn source_of(&self, at: &Resuming) -> Option<Chars> {
        at.object
            .and_then(|o| self.characteristics().remove(&o))
            .or_else(|| self.known(at.what.card()).and_then(|k| k.printed.clone()))
    }

    /// A spell's or ability's source, for what its quality is (CR
    /// 702.16b): the permanent, as it is now or last existed (CR 113.7a),
    /// or the card itself, a spell on the stack having its card's
    /// characteristics.
    fn source_chars(&self, card: CardId, object: Option<ObjectId>) -> Option<Chars> {
        object
            .and_then(|o| {
                self.characteristics()
                    .remove(&o)
                    .or_else(|| self.last_known.get(&o).map(|l| l.chars.clone()))
            })
            .or_else(|| self.known(card).and_then(|k| k.printed.clone()))
    }

    /// Damage from one source, dealt at once (CR 120.3): a player loses
    /// that much life, a creature has it marked; lifelink gains the
    /// source's controller the total (CR 702.15b), and deathtouch makes it
    /// lethal (CR 702.2b).
    /// The permanents dealt damage, and how much, are returned for what
    /// waits for it to trigger.
    fn deal(
        &mut self,
        source: Option<&Chars>,
        controller: PlayerId,
        hits: &[(Target, u32)],
    ) -> Dealt {
        let has = |k: Keyword| source.is_some_and(|c| c.has(k));
        let now = self.characteristics();
        let mut total = 0;
        let mut dealt = Dealt::default();
        for (to, n) in hits {
            // CR 702.16e: damage from a source with the quality is
            // prevented.
            let prevented = match (to, source) {
                (Target::Object(o), Some(s)) => now.get(o).is_some_and(|c| c.protected_from(s)),
                // CR 615.1a.
                (Target::Player(q), _) => self.you_have(*q, ForYou::PreventAllDamage),
                _ => false,
            };
            if *n == 0 || prevented {
                continue;
            }
            total += n;
            match to {
                // CR 120.3a.
                Target::Player(q) => {
                    self.lose_life(*q, *n);
                    dealt.players.insert(*q);
                }
                // CR 120.3e.
                Target::Object(o) => {
                    if let Some(perm) = self.permanents.get_mut(o) {
                        perm.damage += n;
                        perm.deathtouched |= has(Keyword::Deathtouch);
                        *dealt.objects.entry(*o).or_insert(0) += n;
                    }
                }
                // No instruction deals damage to a spell.
                Target::Spell(_) => {}
            }
        }
        if has(Keyword::Lifelink) {
            let controller = source.map_or(controller, |c| c.controller);
            self.players[controller.0 as usize].life += total as i32;
        }
        dealt.total = total;
        dealt
    }

    /// What waits for damage dealt at once triggers: a permanent dealt
    /// some, and the permanent `by` that dealt it to each player.
    fn damage_triggers(
        &mut self,
        hit: Dealt,
        by: Option<ObjectId>,
        combat: bool,
        controller: PlayerId,
    ) {
        if let Some(source) = by.filter(|_| hit.total > 0) {
            self.trigger_on(Event::DealtBy {
                source,
                amount: hit.total,
            });
        }
        if !hit.objects.is_empty() {
            self.trigger_on(Event::Damaged(hit.objects));
        }
        for player in hit.players {
            if let Some(source) = by {
                self.trigger_on(Event::DamagedPlayer { source, combat });
            }
            self.trigger_on(Event::PlayerDamaged { player, controller });
        }
    }

    /// Every legal choice for a target slot of a spell or ability `p`
    /// controls (CR 115.1, 115.4): players still in the game, and
    /// permanents. Nothing on the engine's battlefield has hexproof, shroud
    /// or protection, which are refused as they are read.
    fn legal_targets(&self, spec: &TargetSpec, p: PlayerId, source: Option<&Chars>) -> Vec<Target> {
        // CR 115.4, 702.11c: a player with hexproof can't be the target
        // of an opponent's spell or ability; CR 702.18a: one with shroud
        // can't be anyone's.
        let players = || {
            (0..self.players.len())
                .map(|q| PlayerId(q as u8))
                .filter(|q| {
                    !self.player(*q).left
                        && (*q == p || !self.you_have(*q, ForYou::Hexproof))
                        && !self.you_have(*q, ForYou::Shroud)
                })
        };
        // CR 702.16b: not a permanent with protection from the source;
        // CR 702.18a, 702.11b: nor one with shroud, nor with hexproof
        // unless its controller's.
        let permanents = |f: &dyn Fn(&Chars) -> bool| -> Vec<Target> {
            self.characteristics()
                .into_iter()
                .filter(|(_, c)| {
                    f(c) && !source.is_some_and(|s| c.protected_from(s))
                        && !c.has(Keyword::Shroud)
                        && !(c.has(Keyword::Hexproof) && c.controller != p)
                })
                .map(|(o, _)| Target::Object(o))
                .collect()
        };
        match spec {
            TargetSpec::Player { opponent } => players()
                .filter(|q| !opponent || *q != p)
                .map(Target::Player)
                .collect(),
            TargetSpec::Permanent(filter) => permanents(&|c| {
                filter.0.iter().all(|pred| match pred {
                    Pred::YouControl => c.controller == p,
                    Pred::OpponentsControl => c.controller != p,
                    other => Filter(vec![other.clone()]).matches_card(c),
                })
            }),
            TargetSpec::Any => players()
                .map(Target::Player)
                .chain(permanents(&|c| {
                    c.is(CardType::Creature)
                        || c.is(CardType::Planeswalker)
                        || c.is(CardType::Battle)
                }))
                .collect(),
            TargetSpec::PlayerOrPlaneswalker => players()
                .map(Target::Player)
                .chain(permanents(&|c| c.is(CardType::Planeswalker)))
                .collect(),
            // CR 115.5: a spell can't target itself, and the one whose
            // targets are being chosen is on top of the stack.
            TargetSpec::Spell(filter) => {
                let casting = match &self.waiting {
                    Waiting::Targets {
                        card,
                        choosing: Choosing::Spell { .. },
                        ..
                    } => Some(*card),
                    _ => None,
                };
                self.stack
                    .iter()
                    .filter_map(|o| match o {
                        StackObject::Spell { card, .. }
                            if Some(*card) != casting
                                && self.spell_chars(*card).is_some_and(|c| filter.matches(&c)) =>
                        {
                            Some(Target::Spell(*card))
                        }
                        _ => None,
                    })
                    .collect()
            }
        }
    }

    /// CR 603.3b: the triggered abilities waiting, the active player's
    /// first, then each other player's in turn order, each player's in the
    /// order they choose. Whether any went on the stack, or the game now
    /// waits on a choice.
    fn put_triggers(&mut self, then: PlayerId) -> Put {
        let mut put = Put::Nothing;
        let n = self.players.len();
        for k in 0..n {
            let p = PlayerId(((self.active.0 as usize + k) % n) as u8);
            loop {
                let mine: Vec<Pending> = self
                    .pending
                    .iter()
                    .filter(|t| t.controller == p)
                    .copied()
                    .collect();
                let Some(first) = mine.first().copied() else {
                    break;
                };
                if mine.len() > 1 && !first.ordered {
                    self.pending.retain(|t| t.controller != p);
                    self.waiting = Waiting::Order {
                        player: p,
                        pending: mine,
                        then,
                    };
                    return Put::Waiting;
                }
                let at = self
                    .pending
                    .iter()
                    .position(|t| *t == first)
                    .expect("pending");
                self.pending.remove(at);
                let slots = self
                    .program(Instructions::Triggered(first.source, first.ability))
                    .targets
                    .clone();
                if slots.is_empty() {
                    self.push_trigger(first, Vec::new());
                    put = Put::Some;
                    continue;
                }
                // CR 603.3d: one with a target slot nothing can fill is
                // removed from the stack.
                let source = self.source_chars(first.source, first.object);
                if slots.iter().any(|s| {
                    self.legal_targets(s, first.controller, source.as_ref())
                        .is_empty()
                }) {
                    continue;
                }
                self.waiting = Waiting::Targets {
                    player: first.controller,
                    card: first.source,
                    slots,
                    choosing: Choosing::Trigger {
                        pending: first,
                        then,
                    },
                };
                return Put::Waiting;
            }
        }
        put
    }

    fn push_trigger(&mut self, t: Pending, targets: Vec<Target>) {
        self.triggers += 1;
        self.stack.push_back(StackObject::Trigger {
            source: t.source,
            object: t.object,
            ability: t.ability,
            controller: t.controller,
            targets,
            triggering: t.triggering,
            amount: t.amount,
            player: t.player,
            seq: self.triggers,
        });
    }

    /// The card whose spell or ability is resolving and waits on a choice
    /// its instructions ask for, if any.
    pub fn resolving(&self) -> Option<CardId> {
        let at = match &self.waiting {
            Waiting::Discard {
                then: Some((at, _)),
                ..
            }
            | Waiting::ChooseFromHand { resume: at, .. }
            | Waiting::Scry { resume: at, .. }
            | Waiting::Search { resume: at, .. }
            | Waiting::May { at, .. }
            | Waiting::Reveal { resume: at, .. }
            | Waiting::ChooseAmong { resume: at, .. }
            | Waiting::Arrange { resume: at, .. }
            | Waiting::PutFromHand { resume: at, .. }
            | Waiting::ChooseMana { resume: at, .. }
            | Waiting::PayUnless { at, .. }
            | Waiting::DiscardUnless { at, .. }
            | Waiting::SacrificeUnless { at, .. }
            | Waiting::PayLifeOr { at, .. }
            | Waiting::ChooseName { resume: at, .. } => at,
            _ => return None,
        };
        Some(at.what.card())
    }

    /// The creatures attacking, and whom (CR 506.3).
    pub fn attacks(&self) -> Vec<(ObjectId, PlayerId)> {
        self.combat
            .as_ref()
            .map(|c| c.attackers.iter().map(|(a, d)| (*a, *d)).collect())
            .unwrap_or_default()
    }

    /// The creatures blocking, and what each blocks.
    pub fn blocks(&self) -> Vec<(ObjectId, ObjectId)> {
        self.combat
            .as_ref()
            .map(|c| c.blockers.iter().map(|(b, a)| (*b, *a)).collect())
            .unwrap_or_default()
    }

    /// The card whose spell or ability most recently finished resolving.
    pub fn last_resolved(&self) -> Option<CardId> {
        self.last_resolved
    }

    /// The triggered abilities waiting to be put on the stack.
    pub fn pending(&self) -> &Vector<Pending> {
        &self.pending
    }

    /// Each triggered ability on the battlefield waiting for what
    /// happened triggers (CR 603.2): an enters ability whose filter the
    /// permanent matches, its own included (CR 603.6a); an attacks ability
    /// for each attacker it matches (CR 508.3a), and exalted for one that
    /// attacks alone (CR 506.5); prowess for its controller's spell.
    fn trigger_on(&mut self, event: Event) {
        let all = self.characteristics();
        let mut triggered = Vec::new();
        for (src, chars) in &all {
            let card = self.permanents[src].card;
            let Some(runs) = self.known(card).and_then(|k| k.runs.as_ref().ok()) else {
                continue;
            };
            let seen = |id: ObjectId| Seen {
                id,
                chars: &all[&id],
                source: *src,
                source_chars: chars,
                source_attached_to: self.battlefield.objects[src].attached_to,
                source_chosen: self.battlefield.objects[src].chosen_type.as_ref(),
                token: self.battlefield.objects[&id].token,
                attacking: self.battlefield.attacking.contains(&id),
            };
            for (n, t) in &runs.triggered {
                if !chars.abilities.contains(&Ability::Unread(*n)) {
                    continue;
                }
                let objects: Vec<(Option<ObjectId>, Option<u32>)> = match (&t.event, &event) {
                    (TriggerEvent::DealtDamage(f), Event::Damaged(hit)) => hit
                        .iter()
                        .filter(|(id, _)| all.contains_key(*id) && f.matches(&seen(**id)))
                        .map(|(id, n)| (Some(*id), Some(*n)))
                        .collect(),
                    (
                        TriggerEvent::DealsDamageToPlayer { what, combat },
                        Event::DamagedPlayer { source, combat: c },
                    ) if all.contains_key(source) && (*c || !combat) => what
                        .matches(&seen(*source))
                        .then_some((None, None))
                        .into_iter()
                        .collect(),
                    (TriggerEvent::DealsDamage(f), Event::DealtBy { source, amount }) => {
                        (all.contains_key(source) && f.matches(&seen(*source)))
                            .then_some((None, Some(*amount)))
                            .into_iter()
                            .collect()
                    }
                    (TriggerEvent::TurnedFaceUp(f), Event::TurnedFaceUp(id)) => f
                        .matches(&seen(*id))
                        .then_some((Some(*id), None))
                        .into_iter()
                        .collect(),
                    (TriggerEvent::Dies(f), Event::Died(id)) => {
                        let l = &self.last_known[id];
                        f.matches(&Seen {
                            id: *id,
                            chars: &l.chars,
                            source: *src,
                            source_chars: chars,
                            source_attached_to: self.battlefield.objects[src].attached_to,
                            source_chosen: self.battlefield.objects[src].chosen_type.as_ref(),
                            token: l.token,
                            attacking: false,
                        })
                        .then_some((None, None))
                        .into_iter()
                        .collect()
                    }
                    (TriggerEvent::Begins { step, whose }, Event::Begins(s)) => {
                        let mine = chars.controller == self.active;
                        let holds = match whose {
                            lower::Whose::Yours => mine,
                            lower::Whose::Each => true,
                            lower::Whose::Opponents => !mine,
                        };
                        (step == s && holds)
                            .then_some((None, None))
                            .into_iter()
                            .collect()
                    }
                    (TriggerEvent::Enters(f), Event::Entered(id)) => f
                        .matches(&seen(*id))
                        .then_some((Some(*id), None))
                        .into_iter()
                        .collect(),
                    (TriggerEvent::Attacks(f), Event::Attacked(ids)) => ids
                        .iter()
                        .filter(|id| f.matches(&seen(**id)))
                        .map(|id| (Some(*id), None))
                        .collect(),
                    (TriggerEvent::AttacksWith { filter, n }, Event::Attacked(ids)) => {
                        let with = ids
                            .iter()
                            .filter(|id| {
                                all.get(id)
                                    .is_some_and(|c| c.controller == chars.controller)
                                    && filter.matches(&seen(**id))
                            })
                            .count();
                        (with >= *n as usize)
                            .then_some((None, None))
                            .into_iter()
                            .collect()
                    }
                    // CR 702.149a: power as the layers have it now.
                    (TriggerEvent::Trains, Event::Attacked(ids)) => {
                        let power = |id: &ObjectId| {
                            all.get(id).and_then(|c| c.pt).map_or(i32::MIN, |p| p.power)
                        };
                        let mine = power(src);
                        (ids.contains(src) && ids.iter().any(|id| id != src && power(id) > mine))
                            .then_some((None, None))
                            .into_iter()
                            .collect()
                    }
                    (TriggerEvent::AttacksAlone(f), Event::Attacked(ids)) => match ids.as_slice() {
                        [id] if f.matches(&seen(*id)) => vec![(Some(*id), None)],
                        _ => vec![],
                    },
                    (
                        TriggerEvent::YourSourceDamages { opponent },
                        Event::PlayerDamaged { player, controller },
                    ) => (*controller == chars.controller
                        && (!opponent || *player != chars.controller))
                        .then_some((None, None))
                        .into_iter()
                        .collect(),
                    (TriggerEvent::BecomesTapped(f), Event::BecameTapped(id)) => f
                        .matches(&seen(*id))
                        .then_some((Some(*id), None))
                        .into_iter()
                        .collect(),
                    (TriggerEvent::Draws { by }, Event::Drew(q)) => {
                        let drew = match by {
                            lower::CastBy::Anyone => true,
                            lower::CastBy::You => *q == chars.controller,
                            lower::CastBy::Opponents => *q != chars.controller,
                        };
                        drew.then_some((None, None)).into_iter().collect()
                    }
                    (TriggerEvent::Casts { by, spells }, Event::Cast { card, controller }) => {
                        let printed = self.spell_chars(*card);
                        let caster = match by {
                            lower::CastBy::Anyone => true,
                            lower::CastBy::You => *controller == chars.controller,
                            lower::CastBy::Opponents => *controller != chars.controller,
                        };
                        (caster && printed.is_some_and(|c| spells.matches(&c)))
                            .then_some((None, None))
                            .into_iter()
                            .collect()
                    }
                    _ => vec![],
                };
                // A spell cast names its caster, "that player", and a draw
                // who drew.
                let player = match &event {
                    Event::Cast { controller, .. } => Some(*controller),
                    Event::Drew(q) => Some(*q),
                    _ => None,
                };
                // CR 603.4: an intervening "if" checked as it triggers.
                let objects = objects.into_iter().filter(|(triggering, _)| {
                    t.condition
                        .as_ref()
                        .is_none_or(|c| self.intervening(c, *src, chars, *triggering, &all))
                });
                for (triggering, amount) in objects {
                    triggered.push(Pending {
                        source: card,
                        object: Some(*src),
                        ability: TriggerOf::Printed(*n),
                        controller: chars.controller,
                        triggering,
                        amount,
                        player,
                        ordered: false,
                    });
                }
            }
        }
        // CR 603.10a: a creature's own "when this creature dies" looks
        // back at it as it last existed.
        if let Event::Died(id) = &event {
            let l = self.last_known[id].clone();
            if let Some(runs) = self.known(l.card).and_then(|k| k.runs.as_ref().ok()) {
                for (n, t) in &runs.triggered {
                    let TriggerEvent::Dies(f) = &t.event else {
                        continue;
                    };
                    let seen = Seen {
                        id: *id,
                        chars: &l.chars,
                        source: *id,
                        source_chars: &l.chars,
                        source_attached_to: None,
                        source_chosen: None,
                        token: l.token,
                        attacking: false,
                    };
                    if l.chars.abilities.contains(&Ability::Unread(*n)) && f.matches(&seen) {
                        triggered.push(Pending {
                            source: l.card,
                            object: Some(*id),
                            ability: TriggerOf::Printed(*n),
                            controller: l.chars.controller,
                            triggering: None,
                            amount: None,
                            player: None,
                            ordered: false,
                        });
                    }
                }
            }
        }
        // CR 603.7b–c: a delayed trigger waiting for this step, of a
        // later turn than the one it was created in, triggers once.
        if let Event::Begins(step) = &event {
            for (k, d) in self.delayed.iter_mut().enumerate() {
                if !d.fired && d.step == *step && d.created < self.turn {
                    d.fired = true;
                    triggered.push(Pending {
                        source: d.source,
                        object: None,
                        ability: TriggerOf::Delayed(k as u32),
                        controller: d.controller,
                        triggering: None,
                        amount: None,
                        player: None,
                        ordered: false,
                    });
                }
            }
        }
        self.pending.extend(triggered);
    }

    /// Whether an intervening "if" holds (CR 603.4): the permanents the
    /// source's controller controls that it counts, read from the source,
    /// less the object the event names when it says "other"; or the source
    /// having come under its controller's control since their last upkeep
    /// began, which is their previous turn's (CR 702.30a).
    fn intervening(
        &self,
        c: &lower::Intervening,
        source: ObjectId,
        source_chars: &Chars,
        triggering: Option<ObjectId>,
        all: &BTreeMap<ObjectId, Chars>,
    ) -> bool {
        let (control, excluding) = match c {
            lower::Intervening::Control { control, excluding } => (control, *excluding),
            lower::Intervening::ArrivedSinceLastUpkeep => {
                let last = self.player(source_chars.controller).last_turn;
                return self
                    .permanents
                    .get(&source)
                    .is_some_and(|p| last.is_none_or(|t| p.since >= t));
            }
        };
        let n = all
            .iter()
            .filter(|(o, ch)| {
                ch.controller == source_chars.controller
                    && !(excluding && Some(**o) == triggering)
                    && control.any_of.iter().any(|f| {
                        f.matches(&Seen {
                            id: **o,
                            chars: ch,
                            source,
                            source_chars,
                            source_attached_to: self
                                .battlefield
                                .objects
                                .get(&source)
                                .and_then(|x| x.attached_to),
                            source_chosen: self
                                .battlefield
                                .objects
                                .get(&source)
                                .and_then(|x| x.chosen_type.as_ref()),
                            token: self.battlefield.objects[*o].token,
                            attacking: self.battlefield.attacking.contains(*o),
                        })
                    })
            })
            .count();
        control.count.holds(n as u32)
    }

    /// CR 508.1a, 302.6, 702.3b: untapped creatures `p` controls that have
    /// been theirs since their turn began, or have haste, and lack
    /// defender.
    fn can_attack(&self, p: PlayerId) -> Vec<ObjectId> {
        let began = self.player(p).turn_began;
        self.characteristics()
            .iter()
            .filter(|(id, c)| {
                let perm = &self.permanents[*id];
                c.controller == p
                    && c.is(CardType::Creature)
                    && !perm.tapped
                    && (began.is_some_and(|t| perm.since < t) || c.has(Keyword::Haste))
                    && !c.has(Keyword::Defender)
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// CR 509.1a: untapped creatures `p` controls.
    fn can_block(&self, p: PlayerId) -> Vec<ObjectId> {
        self.characteristics()
            .iter()
            .filter(|(id, c)| {
                c.controller == p && c.is(CardType::Creature) && !self.permanents[*id].tapped
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// CR 509.1a–c: each blocker one of `p`'s untapped creatures, blocking
    /// one creature attacking `p`; a creature with flying blocked only by
    /// creatures with flying or reach (CR 702.9b), one with menace by two
    /// or more (CR 702.111b), and one with protection by none with its
    /// quality (CR 702.16f).
    fn legal_blocks(&self, p: PlayerId, declared: &[(ObjectId, ObjectId)]) -> bool {
        let Decision::Blockers { can, attackers, .. } = self.decision() else {
            return false;
        };
        let chars = self.characteristics();
        let mut seen = BTreeSet::new();
        let shape = declared.iter().all(|(b, a)| {
            can.contains(b) && attackers.contains(a) && seen.insert(*b) && chars[b].controller == p
        });
        shape
            && attackers.iter().all(|a| {
                let by: Vec<ObjectId> = declared
                    .iter()
                    .filter(|(_, x)| x == a)
                    .map(|(b, _)| *b)
                    .collect();
                let flying = chars[a].has(Keyword::Flying);
                let menace = chars[a].has(Keyword::Menace);
                // CR 702.16f.
                (!menace || by.is_empty() || by.len() >= 2)
                    && by.iter().all(|b| !chars[a].protected_from(&chars[b]))
                    // CR 702.28b.
                    && by
                        .iter()
                        .all(|b| chars[a].has(Keyword::Shadow) == chars[b].has(Keyword::Shadow))
                    && (!flying
                        || by
                            .iter()
                            .all(|b| chars[b].has(Keyword::Flying) || chars[b].has(Keyword::Reach)))
            })
    }

    /// CR 508.1b–f, 508.1m: the attackers attack, tapping unless they
    /// have vigilance (CR 702.20b), and what waits for them triggers; then
    /// the active player gets priority (CR 508.2).
    fn declare_attackers(&mut self, p: PlayerId, declared: &[(ObjectId, PlayerId)]) {
        let chars = self.characteristics();
        let mut combat = Combat::default();
        for (a, d) in declared {
            combat.attackers.insert(*a, *d);
            if !chars[a].has(Keyword::Vigilance) {
                self.tap(*a);
            }
        }
        self.battlefield.attacking = combat.attackers.keys().copied().collect();
        self.combat = Some(combat);
        if !declared.is_empty() {
            self.trigger_on(Event::Attacked(declared.iter().map(|(a, _)| *a).collect()));
        }
        self.give_priority(p, 0);
    }

    /// CR 509.1: each defending player in turn order from the active
    /// player declares blockers, then the active player gets priority (CR
    /// 509.2).
    fn declare_blockers(&mut self, mut rest: Vec<PlayerId>) {
        while !rest.is_empty() {
            let p = rest.remove(0);
            if !self.can_block(p).is_empty() {
                self.waiting = Waiting::Blockers { player: p, rest };
                return;
            }
        }
        self.give_priority(self.active, 0);
    }

    /// A creature's power as it assigns damage, 0 if it has none.
    fn power(&self, id: ObjectId) -> u32 {
        self.characteristics()
            .get(&id)
            .and_then(|c| c.pt)
            .map_or(0, |p| p.power.max(0) as u32)
    }

    /// The damage that is lethal to `to` from `source` (CR 702.19b): the
    /// creature's toughness less the damage on it, 1 from a source with
    /// deathtouch (CR 702.2c), 0 for a player.
    fn lethal(&self, source: ObjectId, to: Target) -> u32 {
        let Target::Object(o) = to else { return 0 };
        let chars = self.characteristics();
        if chars
            .get(&source)
            .is_some_and(|c| c.has(Keyword::Deathtouch))
        {
            return 1;
        }
        let toughness = chars.get(&o).and_then(|c| c.pt).map_or(0, |p| p.toughness);
        let marked = self.permanents.get(&o).map_or(0, |p| p.damage as i32);
        (toughness - marked).max(0) as u32
    }

    /// CR 510.1, 510.4: the creatures that assign combat damage in this
    /// step, attackers first, then each assigns it.
    fn combat_damage(&mut self, first: bool) {
        let Some(combat) = self.combat.clone() else {
            return self.give_priority(self.active, 0);
        };
        let chars = self.characteristics();
        let strikes = |id: &ObjectId| {
            let c = &chars[id];
            let fs = c.has(Keyword::FirstStrike);
            let ds = c.has(Keyword::DoubleStrike);
            if first {
                fs || ds
            } else {
                !combat.struck.contains(id) || ds
            }
        };
        let left: Vec<ObjectId> = combat
            .attackers
            .keys()
            .chain(combat.blockers.keys())
            .filter(|id| chars.contains_key(id) && strikes(id) && self.power(**id) > 0)
            .copied()
            .collect();
        self.assign_from(left, Vec::new(), first);
    }

    /// Each creature in `left` assigns its combat damage (CR 510.1a–d): an
    /// unblocked attacker to the player it attacks, a blocked one to its
    /// blocker, its controller dividing it among two or more or, with
    /// trample, past them to the player (CR 702.19b); a blocker to the
    /// attacker it blocks. Then it is dealt at once (CR 510.2).
    fn assign_from(&mut self, mut left: Vec<ObjectId>, mut assigned: Assigned, first: bool) {
        let combat = self.combat.clone().unwrap_or_default();
        while !left.is_empty() {
            let id = left.remove(0);
            let power = self.power(id);
            if let Some(defender) = combat.attackers.get(&id) {
                let blockers: Vec<ObjectId> = combat
                    .blockers
                    .iter()
                    .filter(|(b, a)| **a == id && self.permanents.contains_key(*b))
                    .map(|(b, _)| *b)
                    .collect();
                let chars = self.characteristics();
                let trample = chars[&id].has(Keyword::Trample);
                let player = Target::Player(*defender);
                match blockers.as_slice() {
                    // CR 510.1b.
                    _ if !combat.blocked.contains(&id) => assigned.push((id, player, power)),
                    // CR 510.1c, 702.19e.
                    [] if trample => assigned.push((id, player, power)),
                    [] => {}
                    [b] if !trample => assigned.push((id, Target::Object(*b), power)),
                    _ => {
                        let mut to: Vec<Target> =
                            blockers.iter().map(|b| Target::Object(*b)).collect();
                        if trample {
                            to.push(player);
                        }
                        self.waiting = Waiting::Assign {
                            player: chars[&id].controller,
                            source: id,
                            to,
                            left,
                            assigned,
                            first,
                        };
                        return;
                    }
                }
            } else if let Some(a) = combat.blockers.get(&id) {
                // CR 510.1d.
                if self.permanents.contains_key(a) && combat.attackers.contains_key(a) {
                    assigned.push((id, Target::Object(*a), power));
                }
            }
        }
        // CR 510.2: dealt simultaneously, each source's at once.
        let chars = self.characteristics();
        let sources: BTreeSet<ObjectId> = assigned.iter().map(|(s, _, _)| *s).collect();
        let mut dealt: BTreeMap<ObjectId, u32> = BTreeMap::new();
        let mut hit_players: Vec<(ObjectId, PlayerId, PlayerId)> = Vec::new();
        let mut dealt_by: Vec<(ObjectId, u32)> = Vec::new();
        for s in &sources {
            let hits: Vec<(Target, u32)> = assigned
                .iter()
                .filter(|(x, _, _)| x == s)
                .map(|(_, t, n)| (*t, *n))
                .collect();
            let source = chars.get(s).cloned();
            let controller = source.as_ref().map_or(self.active, |c| c.controller);
            let hit = self.deal(source.as_ref(), controller, &hits);
            if hit.total > 0 {
                dealt_by.push((*s, hit.total));
            }
            for (o, n) in hit.objects {
                *dealt.entry(o).or_insert(0) += n;
            }
            for q in hit.players {
                hit_players.push((*s, controller, q));
            }
        }
        // One event for all of it: a creature dealt damage by two at once
        // is dealt it once (CR 510.2).
        self.damage_triggers(
            Dealt {
                objects: dealt,
                players: BTreeSet::new(),
                total: 0,
            },
            None,
            true,
            self.active,
        );
        for (source, amount) in dealt_by {
            self.trigger_on(Event::DealtBy { source, amount });
        }
        for (s, controller, player) in hit_players {
            self.trigger_on(Event::DamagedPlayer {
                source: s,
                combat: true,
            });
            self.trigger_on(Event::PlayerDamaged { player, controller });
        }
        if first {
            if let Some(c) = self.combat.as_mut() {
                c.struck.extend(sources);
            }
        }
        self.give_priority(self.active, 0);
    }

    fn proceed(&mut self, then: Then) {
        match then {
            Then::Priority(p) => self.give_priority(p, 0),
            Then::Resume(at) => self.follow(at),
        }
    }

    /// The card enters the battlefield under `controller`'s control,
    /// tapped if the instruction putting it there says so or its own
    /// replacement effect does, then `then`. Which of those apply is read
    /// off the permanent as it would exist on the battlefield (CR 614.12),
    /// so a land that enters having lost its abilities enters untapped.
    fn enter(&mut self, card: CardId, controller: PlayerId, put_tapped: bool, then: Then) {
        let (tapped, pays, discard) = self.entering(card, controller);
        let tapped = tapped || put_tapped;
        if let Some(filter) = discard {
            self.waiting = Waiting::DiscardToEnter {
                player: controller,
                card,
                filter,
                tapped,
                then,
            };
            return;
        }
        match pays {
            // CR 614.12a: the choice is made before it enters. Paying is
            // a choice even when something else has it enter tapped.
            Some(life) if self.player(controller).life >= life as i32 => {
                self.waiting = Waiting::PayOrEnterTapped {
                    player: controller,
                    card,
                    life,
                    tapped,
                    then,
                };
            }
            // CR 119.4: unable to pay the life, so nothing to choose.
            Some(_) => self.place(card, controller, true, then),
            None => self.place(card, controller, tapped, then),
        }
    }

    /// Which of the card's own replacement effects on how it enters apply,
    /// read off the permanent as it would exist on the battlefield (CR
    /// 614.12): whether one has it enter tapped, and the life one offers to
    /// pay instead. A condition counts the permanents its controller
    /// controls, never itself, since it isn't on the battlefield yet.
    fn entering(
        &self,
        card: CardId,
        controller: PlayerId,
    ) -> (bool, Option<u32>, Option<CardFilter>) {
        let runs = self.runs_of(card);
        let mut chars = runs.lowered.chars.clone();
        chars.controller = controller;
        let mut would = self.layer_state();
        let id = would.enter(chars, runs.lowered.statics.clone());
        let now = characteristics(&would);
        let me = &now[&id];
        let holds = |cond: &YouControl| {
            let n = now
                .iter()
                .filter(|(o, c)| {
                    **o != id
                        && c.controller == controller
                        && cond.any_of.iter().any(|f| {
                            f.matches(&Seen {
                                id: **o,
                                chars: c,
                                source: id,
                                source_chars: me,
                                source_attached_to: None,
                                source_chosen: None,
                                token: would.objects[*o].token,
                                attacking: would.attacking.contains(*o),
                            })
                        })
                })
                .count();
            cond.count.holds(n as u32)
        };
        let mut tapped = false;
        let mut pays = None;
        let mut discard = None;
        for (n, e) in &runs.enters {
            if !me.abilities.contains(&Ability::Unread(*n)) {
                continue;
            }
            match e {
                Enters::Tapped => tapped = true,
                Enters::TappedUnless(cond) => tapped |= !holds(cond),
                Enters::TappedIf(cond) => tapped |= holds(cond),
                Enters::TappedUnlessPaysLife(life) => pays = Some(*life),
                Enters::DiscardOrGraveyard(f) => discard = Some(f.clone()),
                Enters::WithCounters(..) | Enters::ChooseCreatureType => {}
            }
        }
        (tapped, pays, discard)
    }

    /// Whether the card's own "as this enters, choose a creature type"
    /// applies as it would enter (CR 614.12).
    fn chooses_type(&self, card: CardId, controller: PlayerId) -> bool {
        let runs = self.runs_of(card);
        let mut chars = runs.lowered.chars.clone();
        chars.controller = controller;
        let mut would = self.layer_state();
        let id = would.enter(chars, runs.lowered.statics.clone());
        let me = &characteristics(&would)[&id];
        runs.enters.iter().any(|(n, e)| {
            *e == Enters::ChooseCreatureType && me.abilities.contains(&Ability::Unread(*n))
        })
    }

    fn runs_of(&self, card: CardId) -> &Runnable {
        match self.cards[card.0 as usize].face.up() {
            Some(k) => k.runs.as_ref().expect("only a card that runs enters"),
            None => unreachable!("only a known card enters"),
        }
    }

    /// The card is put onto the battlefield, a new object (CR 400.7).
    fn place(&mut self, card: CardId, controller: PlayerId, tapped: bool, then: Then) {
        // CR 614.12a: the type is chosen before it enters.
        if self.chooses_type(card, controller) {
            self.waiting = Waiting::ChooseCreatureType {
                player: controller,
                card,
                tapped,
                then,
            };
            return;
        }
        self.arrive(card, controller, tapped);
        self.proceed(then);
    }

    /// The card arrives on the battlefield, and what waits for that
    /// triggers.
    fn arrive(&mut self, card: CardId, controller: PlayerId, tapped: bool) {
        self.arrive_attached(card, controller, tapped, None);
    }

    /// As `arrive`, with the creature type chosen as it entered, there
    /// before anything sees it enter.
    fn arrive_chosen(&mut self, card: CardId, controller: PlayerId, tapped: bool, chosen: Subtype) {
        self.arrive_with(card, controller, tapped, None, Some(chosen));
    }

    /// As `arrive`, attached to `to` as it enters, as an Aura spell
    /// resolves (CR 608.3c).
    fn arrive_attached(
        &mut self,
        card: CardId,
        controller: PlayerId,
        tapped: bool,
        to: Option<ObjectId>,
    ) {
        self.arrive_with(card, controller, tapped, to, None);
    }

    fn arrive_with(
        &mut self,
        card: CardId,
        controller: PlayerId,
        tapped: bool,
        to: Option<ObjectId>,
        chosen: Option<Subtype>,
    ) {
        let runs = self.runs_of(card);
        let mut chars = runs.lowered.chars.clone();
        chars.controller = controller;
        let statics = runs.lowered.statics.clone();
        let id = if self.cards[card.0 as usize].token {
            self.battlefield.create_token(chars, statics)
        } else {
            self.battlefield.enter(chars, statics)
        };
        self.permanents.insert(
            id,
            Permanent {
                card,
                tapped,
                since: self.turn,
                damage: 0,
                deathtouched: false,
                face_down: false,
            },
        );
        // CR 614.1c, 122.6: with its counters as it enters, before what
        // waits for it to enter looks.
        let counters: Vec<(Counter, u32)> = self
            .runs_of(card)
            .enters
            .values()
            .filter_map(|e| match e {
                Enters::WithCounters(c, n) => Some((c.clone(), *n)),
                _ => None,
            })
            .collect();
        for (c, n) in counters {
            self.battlefield.put_counters(id, c, n);
        }
        if let Some(to) = to {
            self.battlefield.attach(id, to);
        }
        if let Some(o) = self.battlefield.objects.get_mut(&id) {
            o.chosen_type = chosen;
        }
        self.trigger_on(Event::Entered(id));
    }

    /// A face-down spell becomes a face-down permanent: a 2/2 with no
    /// abilities, so nothing of its own applies as it enters (CR 708.3),
    /// what waits for a creature to enter triggering.
    fn arrive_face_down(&mut self, card: CardId, controller: PlayerId) {
        let id = self
            .battlefield
            .enter(face_down_chars(controller), Vec::new());
        self.permanents.insert(
            id,
            Permanent {
                card,
                tapped: false,
                since: self.turn,
                damage: 0,
                deathtouched: false,
                face_down: true,
            },
        );
        self.trigger_on(Event::Entered(id));
    }

    /// A face-down permanent turns face up: its card's characteristics and
    /// abilities, the same object, and abilities relating to it entering
    /// neither trigger nor apply (CR 708.8, 702.37e). What waits for it to
    /// be turned face up triggers.
    fn turn_face_up(&mut self, id: ObjectId) {
        let Some(perm) = self.permanents.get_mut(&id) else {
            return;
        };
        perm.face_down = false;
        let card = perm.card;
        let controller = self.battlefield.objects[&id].printed.controller;
        let runs = self.runs_of(card);
        let mut chars = runs.lowered.chars.clone();
        chars.controller = controller;
        let statics = runs.lowered.statics.clone();
        self.battlefield.turn_face_up(id, chars, statics);
        self.trigger_on(Event::TurnedFaceUp(id));
    }

    /// Exactly `count` distinct cards of `p`'s hand.
    fn chosen_from_hand(&self, p: PlayerId, count: usize, cards: &[CardId]) -> bool {
        let hand = &self.player(p).hand;
        let distinct: BTreeSet<CardId> = cards.iter().copied().collect();
        distinct.len() == count && cards.len() == count && cards.iter().all(|c| hand.contains(c))
    }

    /// A discarded card goes to a public zone, the graveyard (CR 400.2),
    /// so it is one whose name is known.
    fn can_discard(&self, p: PlayerId, count: usize, cards: &[CardId]) -> bool {
        self.chosen_from_hand(p, count, cards) && cards.iter().all(|c| self.known(*c).is_some())
    }

    fn discard(&mut self, p: PlayerId, cards: &[CardId]) {
        let player = &mut self.players[p.0 as usize];
        player.hand.retain(|c| !cards.contains(c));
        player.graveyard.extend(cards.iter().copied());
    }

    /// CR 121.1, 121.4: a card drawn, which what waits for it triggers
    /// on; from an empty library, none is.
    fn draw(&mut self, p: PlayerId) {
        let player = &mut self.players[p.0 as usize];
        match player.library.pop_front() {
            Some(c) => {
                player.hand.push_back(c);
                player.drawn.push_back(c);
                self.trigger_on(Event::Drew(p));
            }
            None => player.drew_from_empty = true,
        }
    }

    fn in_game(&self) -> usize {
        self.players.iter().filter(|p| !p.left).count()
    }

    /// The next player after `p` in turn order who is still in the game.
    fn next_in_game(&self, p: PlayerId) -> PlayerId {
        let n = self.players.len();
        (1..=n)
            .map(|i| PlayerId(((p.0 as usize + i) % n) as u8))
            .find(|q| !self.player(*q).left)
            .unwrap_or(p)
    }

    fn begin_turn(&mut self, p: PlayerId) {
        self.turn += 1;
        self.battlefield.active = Some(p);
        self.bottomed = 0;
        self.returned = 0;
        self.shuffled = 0;
        self.created = 0;
        self.discarded = 0;
        self.used = OrdSet::new();
        self.active = p;
        let player = &mut self.players[p.0 as usize];
        player.last_turn = player.turn_began;
        player.turn_began = Some(self.turn);
        for player in &mut self.players {
            player.lands_played = 0;
            player.lost_life = false;
            player.drawn = Vector::new();
        }
        self.run_from(Step::Untap);
    }

    /// Begin `step` and run the turn-based actions and the steps no one
    /// gets priority in, until a player has to decide.
    fn run_from(&mut self, mut step: Step) {
        loop {
            self.step = step;
            match step {
                // CR 502.3, and no one gets priority (CR 502.4). Phasing and
                // day/night (CR 502.1–502.2) aren't modelled.
                Step::Untap => {
                    let active = self.active;
                    let chars = self.characteristics();
                    for (id, c) in chars {
                        if c.controller == active {
                            if let Some(perm) = self.permanents.get_mut(&id) {
                                perm.tapped = false;
                            }
                        }
                    }
                }
                Step::Draw => {
                    // CR 504.1, then what waits for the step triggers (CR
                    // 504.2).
                    self.draw(self.active);
                    self.trigger_on(Event::Begins(TurnStep::Draw));
                    return self.give_priority(self.active, 0);
                }
                // CR 503.1a, 513.1a.
                Step::Upkeep | Step::End => {
                    let s = if step == Step::Upkeep {
                        TurnStep::Upkeep
                    } else {
                        TurnStep::End
                    };
                    self.trigger_on(Event::Begins(s));
                    return self.give_priority(self.active, 0);
                }
                // CR 508.1: declaring no attackers is no choice when no
                // creature can attack.
                Step::DeclareAttackers => {
                    let p = self.active;
                    if self.can_attack(p).is_empty() {
                        self.combat = Some(Combat::default());
                        return self.give_priority(p, 0);
                    }
                    self.waiting = Waiting::Attackers { player: p };
                    return;
                }
                Step::DeclareBlockers => {
                    let attacked: BTreeSet<PlayerId> = self
                        .combat
                        .as_ref()
                        .map(|c| c.attackers.values().copied().collect())
                        .unwrap_or_default();
                    let n = self.players.len();
                    let rest = (1..n)
                        .map(|k| PlayerId(((self.active.0 as usize + k) % n) as u8))
                        .filter(|q| attacked.contains(q))
                        .collect();
                    return self.declare_blockers(rest);
                }
                Step::FirstStrikeDamage => return self.combat_damage(true),
                Step::CombatDamage => return self.combat_damage(false),
                Step::Cleanup => {
                    let player = self.player(self.active);
                    let count = player.hand.len().saturating_sub(HAND_SIZE);
                    if count > 0 && !player.left {
                        self.waiting = Waiting::Discard {
                            player: self.active,
                            count,
                            then: None,
                        };
                        return;
                    }
                    // CR 514.2.
                    let ended = std::mem::take(&mut self.until_end_of_turn);
                    self.battlefield.end(&ended);
                    let ids: Vec<ObjectId> = self.permanents.keys().copied().collect();
                    for id in ids {
                        if let Some(perm) = self.permanents.get_mut(&id) {
                            perm.damage = 0;
                            perm.deathtouched = false;
                        }
                    }
                    // CR 514.3a: no one gets priority unless state-based
                    // actions were performed.
                    if self.state_based_actions() != Checked::Nothing {
                        return self.give_priority(self.active, 0);
                    }
                }
                // CR 507.1: what waits for the beginning of combat.
                Step::BeginningOfCombat => {
                    self.trigger_on(Event::Begins(TurnStep::BeginningOfCombat));
                    return self.give_priority(self.active, 0);
                }
                _ => return self.give_priority(self.active, 0),
            }
            match self.after(step) {
                Some(next) => step = next,
                None => return,
            }
        }
    }

    /// End the step with priority: all players passed with the stack empty.
    fn end_step(&mut self) {
        // A cleanup step with priority in it is followed by another
        // (CR 514.3a).
        let next = match self.step {
            Step::Cleanup => {
                self.empty_pools();
                Some(Step::Cleanup)
            }
            step => self.after(step),
        };
        if let Some(next) = next {
            self.run_from(next);
        }
    }

    /// Empty the pools as `step` ends (CR 500.5) and say which step is
    /// next, beginning the next turn after cleanup and returning `None`.
    fn after(&mut self, step: Step) -> Option<Step> {
        self.empty_pools();
        // CR 511.3.
        if step == Step::EndOfCombat {
            self.combat = None;
            self.battlefield.attacking.clear();
        }
        let mut next = step.next();
        while let Some(s) = next.filter(|s| self.skips(*s)) {
            next = s.next();
        }
        match next {
            // CR 103.8a: in a two-player game the starting player skips
            // their first draw step. In other multiplayer games no one
            // does (CR 103.8c).
            Some(Step::Draw) if self.turn == 1 && self.players.len() == 2 => {
                Some(Step::PrecombatMain)
            }
            Some(next) => Some(next),
            None => {
                // CR 800.4k: a player who left begins no turn.
                let next = self.next_in_game(self.active);
                self.begin_turn(next);
                None
            }
        }
    }

    /// CR 508.8: with no attackers, the declare blockers and combat damage
    /// steps are skipped; CR 510.4: the first-strike damage step happens
    /// only for a creature with first or double strike in combat; CR
    /// 614.10: a draw step a permanent of the active player's skips.
    fn skips(&self, s: Step) -> bool {
        let fighting: Vec<ObjectId> = self
            .combat
            .as_ref()
            .map(|c| {
                c.attackers
                    .keys()
                    .chain(c.blockers.keys())
                    .copied()
                    .collect()
            })
            .unwrap_or_default();
        let live: Vec<&ObjectId> = fighting
            .iter()
            .filter(|id| self.permanents.contains_key(*id))
            .collect();
        match s {
            Step::DeclareBlockers | Step::CombatDamage => fighting.is_empty(),
            Step::FirstStrikeDamage => {
                let chars = self.characteristics();
                !live.iter().any(|id| {
                    chars[*id].has(Keyword::FirstStrike) || chars[*id].has(Keyword::DoubleStrike)
                })
            }
            // CR 614.10: decided as the step would begin, so a permanent
            // gone in the upkeep skips nothing.
            Step::Draw => self.you_have(self.active, ForYou::SkipDrawStep),
            _ => false,
        }
    }

    fn empty_pools(&mut self) {
        for p in &mut self.players {
            p.pool = ManaPool::default();
        }
    }

    /// Give `p` priority, after state-based actions (CR 704.3). If they have
    /// left, the next player in turn order gets it (CR 800.4a, 800.4j).
    fn give_priority(&mut self, p: PlayerId, passes: usize) {
        match self.state_based_actions() {
            Checked::GameOver => return,
            Checked::Commander(card) => {
                self.waiting = Waiting::ReturnCommander {
                    owner: self.owner(card),
                    card,
                    then: p,
                    passes,
                };
                return;
            }
            Checked::Legend(player, among) => {
                self.waiting = Waiting::LegendRule {
                    player,
                    among,
                    then: p,
                    passes,
                };
                return;
            }
            Checked::Nothing | Checked::Performed => {}
        }
        // CR 603.3: then triggered abilities go on the stack, and with one
        // put there the passes start again.
        let passes = match self.put_triggers(p) {
            Put::Waiting => return,
            Put::Some => 0,
            Put::Nothing => passes,
        };
        let p = if self.player(p).left {
            self.next_in_game(p)
        } else {
            p
        };
        self.waiting = Waiting::Priority { player: p, passes };
    }

    /// Perform state-based actions until none apply (CR 704.3). A player
    /// who loses leaves the game, and the game ends when one player or none
    /// is left. A commander's owner choosing where it goes stops the check
    /// with a decision (CR 903.9a).
    fn state_based_actions(&mut self) -> Checked {
        let mut checked = Checked::Nothing;
        loop {
            // CR 704.5d, 111.7: a token in a zone other than the
            // battlefield ceases to exist.
            let cards = self.cards.clone();
            let token = |c: &CardId| cards[c.0 as usize].token;
            let elsewhere = self.players.iter().any(|p| {
                p.hand
                    .iter()
                    .chain(&p.graveyard)
                    .chain(&p.library)
                    .any(token)
            }) || self.exile.iter().any(token);
            if elsewhere {
                for p in &mut self.players {
                    p.hand.retain(|c| !token(c));
                    p.graveyard.retain(|c| !token(c));
                    p.library.retain(|c| !token(c));
                }
                self.exile.retain(|c| !token(c));
                checked = Checked::Performed;
            }
            let losers: Vec<PlayerId> = (0..self.players.len())
                .map(|p| PlayerId(p as u8))
                .filter(|p| {
                    let player = self.player(*p);
                    // CR 704.5a, 704.5b.
                    !player.left && (player.life <= 0 || player.drew_from_empty)
                })
                .collect();
            // CR 704.5f, 704.5g, 704.5h.
            let dying: Vec<ObjectId> = self
                .characteristics()
                .into_iter()
                .filter(|(id, c)| {
                    let perm = &self.permanents[id];
                    // CR 702.12b: lethal damage doesn't destroy one
                    // with indestructible; toughness 0 isn't destruction.
                    c.pt.is_some_and(|p| {
                        p.toughness <= 0
                            || c.is(CardType::Creature)
                                && !c.has(Keyword::Indestructible)
                                && (perm.damage >= p.toughness as u32 || perm.deathtouched)
                                && perm.damage > 0
                    })
                })
                .map(|(id, _)| id)
                .chain(self.illegal_auras())
                .collect();
            let loose = self.illegal_equipment();
            if !loose.is_empty() {
                for id in loose {
                    self.battlefield.detach(id);
                }
                checked = Checked::Performed;
            }
            if losers.is_empty() && dying.is_empty() {
                if let Some(legends) = self.legends() {
                    return legends;
                }
                return match self.commanders_moved.front() {
                    Some(c) => Checked::Commander(*c),
                    None => checked,
                };
            }
            checked = Checked::Performed;
            for id in dying {
                self.put_in_graveyard(id);
            }
            for p in losers {
                self.leave(p);
            }
            let remaining: Vec<PlayerId> = (0..self.players.len())
                .map(|p| PlayerId(p as u8))
                .filter(|p| !self.player(*p).left)
                .collect();
            match remaining.as_slice() {
                [] => {
                    self.waiting = Waiting::Over(Outcome::Draw);
                    return Checked::GameOver;
                }
                [winner] => {
                    self.waiting = Waiting::Over(Outcome::Won(*winner));
                    return Checked::GameOver;
                }
                _ => {}
            }
        }
    }

    /// CR 704.5m, 303.4c–d: Auras attached to nothing, to an object gone
    /// from the battlefield, to themselves, or to one their enchant ability
    /// or protection (CR 702.16c) says they can't enchant.
    fn illegal_auras(&self) -> Vec<ObjectId> {
        let all = self.characteristics();
        all.iter()
            .filter(|(id, c)| {
                c.subtypes.contains(&Subtype::Other("Aura".into())) && {
                    let enchant = self
                        .known(self.permanents[*id].card)
                        .and_then(|k| k.runs.as_ref().ok())
                        .and_then(|r| r.enchant.as_ref());
                    let on = self.battlefield.objects[*id].attached_to;
                    match (enchant, on.and_then(|o| all.get(&o).map(|oc| (o, oc)))) {
                        (Some(f), Some((o, oc))) if o != **id => {
                            oc.protected_from(c)
                                || !f.0.iter().all(|pred| match pred {
                                    Pred::YouControl => oc.controller == c.controller,
                                    Pred::OpponentsControl => oc.controller != c.controller,
                                    other => Filter(vec![other.clone()]).matches_card(oc),
                                })
                        }
                        _ => true,
                    }
                }
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// CR 704.5n, 301.5c: Equipment attached to an object gone from the
    /// battlefield, to itself, to a permanent that isn't a creature, or to
    /// one with protection from it (CR 702.16c), which become unattached
    /// and stay where they are.
    fn illegal_equipment(&self) -> Vec<ObjectId> {
        let all = self.characteristics();
        all.iter()
            .filter(|(id, c)| {
                c.subtypes.contains(&Subtype::Other("Equipment".into()))
                    && self.battlefield.objects[*id].attached_to.is_some_and(|o| {
                        match all.get(&o) {
                            Some(oc) => {
                                o == **id || !oc.is(CardType::Creature) || oc.protected_from(c)
                            }
                            None => true,
                        }
                    })
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// CR 704.5j: the first player controlling two or more legendary
    /// permanents with the same name, and those permanents.
    fn legends(&self) -> Option<Checked> {
        let mut named: BTreeMap<(PlayerId, &str), Vec<ObjectId>> = BTreeMap::new();
        for (id, c) in self.characteristics() {
            if c.supertypes.contains(&Supertype::Legendary) {
                let Some(text) = self.card(self.permanents[&id].card) else {
                    continue;
                };
                let name = text.name.as_str();
                named.entry((c.controller, name)).or_default().push(id);
            }
        }
        named
            .into_iter()
            .find(|(_, ids)| ids.len() > 1)
            .map(|((p, _), ids)| Checked::Legend(p, ids))
    }

    /// A player loses life, paid or dealt as damage (CR 119.3, 119.4,
    /// 120.3a), and has lost life this turn.
    fn lose_life(&mut self, q: PlayerId, n: u32) {
        if n > 0 {
            let player = &mut self.players[q.0 as usize];
            player.life -= n as i32;
            player.lost_life = true;
        }
    }

    /// An untapped permanent becomes tapped (CR 701.26a), which what waits
    /// for that triggers on.
    fn tap(&mut self, id: ObjectId) {
        if let Some(perm) = self.permanents.get_mut(&id) {
            if !perm.tapped {
                perm.tapped = true;
                self.trigger_on(Event::BecameTapped(id));
            }
        }
    }

    /// A permanent is put into its owner's graveyard.
    fn put_in_graveyard(&mut self, id: ObjectId) {
        self.leave_battlefield(id, Zone::Graveyard);
    }

    /// A permanent leaves the battlefield for a zone of its owner's, kept
    /// as it last existed (CR 608.2h); a creature put into a graveyard
    /// dies (CR 700.4), which what waits for that triggers, looking back
    /// at the battlefield as it was (CR 603.10a).
    fn leave_battlefield(&mut self, id: ObjectId, to: Zone) {
        let Some(chars) = self.characteristics().remove(&id) else {
            return;
        };
        let Some(perm) = self.permanents.remove(&id) else {
            return;
        };
        let object = self.battlefield.objects.get(&id).cloned();
        let token = object.as_ref().is_some_and(|o| o.token);
        self.last_known.insert(
            id,
            LastKnown {
                chars: chars.clone(),
                counters: object.map(|o| o.counters).unwrap_or_default(),
                card: perm.card,
                token,
            },
        );
        self.battlefield.leave(id);
        // CR 712.8a: its front face in any other zone.
        self.turn_up(perm.card, Side::Front);
        let owner = self.owner(perm.card);
        let player = &mut self.players[owner.0 as usize];
        match to {
            Zone::Graveyard => player.graveyard.push_back(perm.card),
            Zone::Hand => {
                player.hand.push_back(perm.card);
                self.returned += 1;
            }
            Zone::Exile => self.exile.push_back(perm.card),
        }
        if to != Zone::Hand && self.cards[perm.card.0 as usize].commander {
            self.commanders_moved.push_back(perm.card);
        }
        if to == Zone::Graveyard && chars.is(CardType::Creature) {
            self.trigger_on(Event::Died(id));
        }
    }

    /// CR 800.4a: everything the player owns leaves the game. Nothing
    /// changes control yet, so that is everything they control too.
    fn leave(&mut self, p: PlayerId) {
        let cards = self.cards.clone();
        let owned = |c: &CardId| cards[c.0 as usize].owner == p;
        let player = &mut self.players[p.0 as usize];
        player.left = true;
        player.drew_from_empty = false;
        player.library.clear();
        player.hand.clear();
        player.graveyard.clear();
        player.pool = ManaPool::default();
        self.command.retain(|c| !owned(c));
        self.exile.retain(|c| !owned(c));
        self.commanders_moved.retain(|c| !owned(c));
        // CR 800.4a: their spells and abilities on the stack cease to exist.
        self.stack.retain(|o| match o {
            StackObject::Spell { card, .. } => !owned(card),
            StackObject::Ability { controller, .. } | StackObject::Trigger { controller, .. } => {
                *controller != p
            }
        });
        self.pending.retain(|t| t.controller != p);
        let gone: Vec<ObjectId> = self
            .permanents
            .iter()
            .filter(|(_, perm)| owned(&perm.card))
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.permanents.remove(&id);
            self.battlefield.leave(id);
        }
    }
}

fn unlowered(r: Refusal) -> Unrunnable {
    let node = match &r {
        Refusal::TypeLine(_) => "type line".to_string(),
        Refusal::Unparsed { .. } => "Insight breaks".to_string(),
        Refusal::Unlowered { node, .. } => node.clone(),
    };
    Unrunnable::new(node, r.to_string())
}

/// Partner, which works before the game (CR 702.124a); the keywords
/// the engine runs are the layers' (`model::Keyword`).
fn before_the_game(k: &i::Keyword) -> bool {
    k.name.eq_ignore_ascii_case("partner") && k.param.is_none()
}

/// "Morph {2}{W}{W}": `None` if the keyword isn't morph, and its cost if
/// that is all mana (CR 702.37a). Megamorph's counter isn't run, so it
/// stays refused.
fn morph_cost(k: &i::Keyword) -> Option<Option<ManaCost>> {
    if !k.name.eq_ignore_ascii_case("morph") {
        return None;
    }
    Some(match k.param.as_ref() {
        Some(i::KeywordParam::Cost(c)) => match c.as_slice() {
            [i::Cost::Mana(m)] => ManaCost::from_symbols(m).ok(),
            _ => None,
        },
        _ => None,
    })
}

/// The characteristics of a face-down spell or permanent: a 2/2 creature
/// with no text, name, subtypes or mana cost (CR 702.37c, 708.2a).
fn face_down_chars(controller: PlayerId) -> Chars {
    Chars {
        types: BTreeSet::from([CardType::Creature]),
        supertypes: BTreeSet::new(),
        subtypes: BTreeSet::new(),
        colors: BTreeSet::new(),
        abilities: BTreeSet::new(),
        pt: Some(model::pt(2, 2)),
        mana_value: 0,
        controller,
    }
}

/// Abilities that work only from a hand or a graveyard, which nothing
/// offers yet (CR 702.29a, 702.29e, 702.53a; eternalize, embalm, unearth,
/// scavenge and plot, CR 702.129a, 702.128a, 702.84a, 702.97a, 702.170a),
/// and flashback whose cost the engine can't pay (CR 702.34a).
fn hand_keyword(k: &i::Keyword) -> bool {
    let name = k.name.to_lowercase();
    name.ends_with("cycling")
        || matches!(
            name.as_str(),
            "transmute" | "flashback" | "eternalize" | "embalm" | "unearth" | "scavenge" | "plot"
        )
}

/// "Cycling {2}": its mana cost, if that is all its cost is; landcycling
/// and the like search instead, and stay carried.
fn cycling_cost(k: &i::Keyword) -> Option<ManaCost> {
    if !k.name.eq_ignore_ascii_case("cycling") {
        return None;
    }
    match k.param.as_ref()? {
        i::KeywordParam::Cost(c) => match c.as_slice() {
            [i::Cost::Mana(m)] => ManaCost::from_symbols(m).ok(),
            _ => None,
        },
        _ => None,
    }
}

/// What of a card's abilities the game runs, or the first it can't. The
/// layers carry triggered abilities and statics such as "enters tapped"
/// unread (ADR-0037); one the game can't run is refused by name rather
/// than skipped. A non-mana activated ability is safe to carry, since
/// nothing offers to activate one yet.
fn runnable(lowered: Lowered) -> Result<Runnable, Unrunnable> {
    let mut runs = Runnable {
        mana: BTreeMap::new(),
        activated: BTreeMap::new(),
        enters: BTreeMap::new(),
        triggered: BTreeMap::new(),
        spell: None,
        flash: false,
        opening: None,
        for_you: BTreeMap::new(),
        cycling: None,
        cost_changes: BTreeMap::new(),
        alternatives: BTreeMap::new(),
        kicker: None,
        enchant: None,
        mana_triggers: BTreeMap::new(),
        extra_lands: BTreeMap::new(),
        dormant: Vec::new(),
        lowered,
    };
    for (n, a) in runs.lowered.unread.iter().enumerate() {
        let n = n as u16;
        // An ability word adds no rules (CR 207.2c), and a `Let` binds the
        // targets and objects the ability names.
        let mut a = a;
        let mut bind: &[i::Objects] = &[];
        loop {
            match a {
                i::Ability::Labelled { ability, .. } => a = ability,
                i::Ability::Let { ability, bind: b } => {
                    bind = b;
                    a = ability
                }
                _ => break,
            }
        }
        // A `Let` may bind only a description ("a creature spell of the
        // chosen type" its mana is spent on), which is no target.
        let bound = bind.iter().any(|o| {
            matches!(
                o.quant,
                i::Quant::Target { .. } | i::Quant::AnyTarget { .. }
            )
        });
        match a {
            // With a target it isn't a mana ability (CR 605.5a).
            i::Ability::Activated {
                cost,
                effect,
                limits,
            } => match m::mana_ability(a).filter(|_| !bound) {
                None => {
                    let def = lower::activated(bind, cost, effect, limits).map_err(|node| {
                        Unrunnable::new(
                            node.clone(),
                            format!("the game can't run its activated ability: {node}"),
                        )
                    });
                    if let Err(r) = &def {
                        runs.dormant
                            .push(format!("an activated ability: {}", r.node));
                    }
                    runs.activated.insert(n, def);
                }
                Some(Ok(def)) => {
                    runs.mana.insert(n, def);
                }
                Some(Err(node)) => {
                    return Err(Unrunnable::new(
                        node.clone(),
                        format!("the game can't run its mana ability: {node}"),
                    ))
                }
            },
            i::Ability::Keywords(ks) => {
                for k in ks {
                    if k.name.eq_ignore_ascii_case("flash") {
                        runs.flash = true;
                    } else if let Some(cost) = cycling_cost(k) {
                        runs.cycling = Some((
                            n,
                            Activated {
                                cost: lower::ActivationCost {
                                    mana: Some(cost),
                                    ..Default::default()
                                },
                                program: lower::Program {
                                    targets: Vec::new(),
                                    effects: vec![Effect::Draw {
                                        who: lower::Who::You,
                                        n: lower::Amount::N(1),
                                    }],
                                },
                                limits: Default::default(),
                            },
                        ));
                    } else if let Some(t) = lower::keyword_trigger(k) {
                        runs.triggered.insert(n, t);
                    } else if let Some(alt) = lower::flashback(k) {
                        // One the engine can't pay for is carried and
                        // never offered: the card is still cast from hand.
                        match alt {
                            Ok(alt) => {
                                runs.alternatives.insert(n, alt);
                            }
                            Err(node) => runs.dormant.push(format!("flashback: {node}")),
                        }
                    } else if let Some(equip) = lower::equip(k) {
                        let def = equip.map_err(|node| {
                            Unrunnable::new(node.clone(), format!("the game can't run {node} yet"))
                        });
                        if let Err(r) = &def {
                            runs.dormant.push(format!("equip: {}", r.node));
                        }
                        runs.activated.insert(n, def);
                    } else if let Some(enchant) = lower::enchant(k) {
                        let filter = enchant.map_err(|node| {
                            Unrunnable::new(node.clone(), format!("the game can't run {node} yet"))
                        })?;
                        // CR 702.5c: each instance applies; one is run.
                        if runs.enchant.replace(filter).is_some() {
                            return Err(Unrunnable::new(
                                "a second enchant",
                                "the game can't run two enchant abilities yet",
                            ));
                        }
                    } else if let Some(kicker) = lower::kicker(k) {
                        // One the engine can't pay for is carried and
                        // never offered: the card is cast unkicked.
                        match kicker {
                            Ok(_) if runs.kicker.is_some() => {
                                return Err(Unrunnable::new(
                                    "a second kicker",
                                    "the game can't run two kickers yet",
                                ))
                            }
                            Ok(cost) => runs.kicker = Some(cost),
                            Err(node) => runs.dormant.push(format!("kicker: {node}")),
                        }
                    } else if let Some(cost) = morph_cost(k) {
                        // Read with the card (`Known::morph`); one the
                        // engine can't pay is never turned face up.
                        if cost.is_none() {
                            runs.dormant.push("morph: a cost other than mana".into());
                        }
                    } else if hand_keyword(k) {
                        runs.dormant.push(k.name.to_lowercase());
                    } else if !before_the_game(k) {
                        return Err(Unrunnable::new(
                            format!("keyword {}", k.name),
                            format!("the game can't run {} yet", k.name),
                        ));
                    }
                }
            }
            i::Ability::Static(parts) if ForYou::read(parts).is_some() => {
                runs.for_you
                    .insert(n, ForYou::read(parts).expect("read above"));
            }
            // CR 305.2a.
            i::Ability::Static(parts)
                if matches!(parts.as_slice(), [i::Static::ExtraLand(i::Amount::N(_))]) =>
            {
                if let [i::Static::ExtraLand(i::Amount::N(k))] = parts.as_slice() {
                    runs.extra_lands.insert(n, *k);
                }
            }
            i::Ability::Static(parts) if lower::alternative_cost(parts).is_some() => {
                match lower::alternative_cost(parts).expect("an alternative cost") {
                    Ok(alt) => {
                        runs.alternatives.insert(n, alt);
                    }
                    Err(node) => runs.dormant.push(format!("an alternative cost: {node}")),
                }
            }
            i::Ability::Static(parts) if lower::cost_change(parts).is_some() => {
                let change = lower::cost_change(parts)
                    .expect("a cost change")
                    .map_err(|node| {
                        Unrunnable::new(
                            node.clone(),
                            format!("the game can't run its cost change: {node}"),
                        )
                    })?;
                runs.cost_changes.insert(n, change);
            }
            i::Ability::OpeningHandAction(i::OpeningHandAction::BeginOnBattlefield {
                not_starting_player: false,
                counters: None,
                if_you_do,
            }) if if_you_do.is_empty() => runs.opening = Some(Opening::Begins),
            i::Ability::OpeningHandAction(i::OpeningHandAction::Reveal(effects)) => {
                let program = lower::opening_reveal(effects).map_err(|node| {
                    Unrunnable::new(
                        node.clone(),
                        format!("the game can't run its reveal from the opening hand: {node}"),
                    )
                })?;
                runs.opening = Some(Opening::Reveals(program));
            }
            i::Ability::Static(parts) => {
                let e = enters(parts).map_err(|node| {
                    Unrunnable::new(
                        node.clone(),
                        format!("the game can't run its static ability {node} yet"),
                    )
                })?;
                runs.enters.insert(n, e);
            }
            // A spell's instructions (CR 113.3a), which run as it resolves:
            // lowering them into actions is the next slice.
            i::Ability::Spell(effects) if effects.is_empty() => {}
            i::Ability::Spell(effects) => {
                let program = lower::spell(bind, effects).map_err(|node| {
                    Unrunnable::new(
                        node.clone(),
                        format!("the game can't run a spell's {node} yet"),
                    )
                })?;
                match &mut runs.spell {
                    None => runs.spell = Some(program),
                    // CR 608.2c: its paragraphs one after the other; a
                    // second with targets would number its slots anew.
                    Some(first) if program.targets.is_empty() => {
                        first.effects.extend(program.effects)
                    }
                    Some(_) => {
                        return Err(Unrunnable::new(
                            "a spell of several paragraphs with targets",
                            "the game can't run a later paragraph's targets yet",
                        ))
                    }
                }
            }
            i::Ability::Triggered {
                trigger,
                effect,
                limits,
            } if lower::mana_trigger(bind, trigger, effect, limits).is_some() => {
                let def = lower::mana_trigger(bind, trigger, effect, limits)
                    .expect("a triggered mana ability")
                    .map_err(|node| {
                        Unrunnable::new(
                            node.clone(),
                            format!("the game can't run its triggered mana ability: {node}"),
                        )
                    })?;
                runs.mana_triggers.insert(n, def);
            }
            i::Ability::Triggered {
                trigger,
                effect,
                limits,
            } => {
                let def = lower::triggered(bind, trigger, effect, limits).map_err(|node| {
                    Unrunnable::new(
                        node.clone(),
                        format!("the game can't run its triggered ability: {node}"),
                    )
                })?;
                runs.triggered.insert(n, def);
            }
            other => {
                let node = short(other);
                return Err(Unrunnable::new(
                    node.clone(),
                    format!("the game can't run its {node} ability yet"),
                ));
            }
        }
    }
    Ok(runs)
}

/// An "enters tapped" static of the card's own (CR 614.1d).
fn enters(parts: &[i::Static]) -> Result<Enters, String> {
    let itself = |o: &i::Objects| o.quant == i::Quant::SelfRef;
    match parts {
        [i::Static::EntersTapped(what)] if itself(what) => Ok(Enters::Tapped),
        [i::Static::EntersTappedUnless {
            what,
            unless: i::Condition::YouControl(objects),
        }] if itself(what) => Ok(Enters::TappedUnless(lower::you_control(objects)?)),
        [i::Static::If {
            cond: i::Condition::YouControl(objects),
            then,
            ..
        }] if matches!(then.as_slice(), [i::Static::EntersTapped(what)] if itself(what)) => {
            Ok(Enters::TappedIf(lower::you_control(objects)?))
        }
        [i::Static::Replacement { event, instead }] => lower::discard_to_enter(event, instead)
            .map(Enters::DiscardOrGraveyard)
            .ok_or_else(|| "Replacement".to_string()),
        [i::Static::EntersWithCounters {
            what,
            n: i::Amount::N(n),
            kind,
        }] if itself(what) => lower::counter(kind)
            .map(|c| Enters::WithCounters(c, *n))
            .ok_or_else(|| "EntersWithCounters".to_string()),
        [i::Static::AsEnters { what, then }]
            if itself(what) && lower::chooses_creature_type(then) =>
        {
            Ok(Enters::ChooseCreatureType)
        }
        [i::Static::AsEnters { what, then }] if itself(what) => lower::pays_life_or_tapped(then)
            .map(Enters::TappedUnlessPaysLife)
            .ok_or_else(|| "AsEnters".to_string()),
        [one] => Err(short(one)),
        _ => Err("a static ability of several parts".into()),
    }
}

/// A zone a permanent can leave the battlefield for, its owner's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Zone {
    Graveyard,
    Exile,
    Hand,
}

/// A permanent as it last existed on the battlefield (CR 608.2h, 113.7a).
#[derive(Clone, Debug)]
struct LastKnown {
    chars: Chars,
    counters: OrdMap<Counter, u32>,
    card: CardId,
    token: bool,
}

/// Damage one source dealt at once (CR 120.3): to each permanent, how
/// much, and the players dealt any.
#[derive(Default)]
struct Dealt {
    objects: BTreeMap<ObjectId, u32>,
    players: BTreeSet<PlayerId>,
    /// All the damage dealt, to players and permanents.
    total: u32,
}

/// What putting triggered abilities on the stack did.
enum Put {
    Nothing,
    Some,
    /// The game waits on a choice: an order or targets.
    Waiting,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Checked {
    Nothing,
    Performed,
    /// A commander's owner chooses whether it goes to the command zone.
    Commander(CardId),
    /// A player keeps one of these legendary permanents.
    Legend(PlayerId, Vec<ObjectId>),
    GameOver,
}

/// The card types before the dash, front face first.
fn type_words(type_line: &str) -> impl Iterator<Item = &str> {
    let front = type_line.split("//").next().unwrap_or("");
    let types = front.split('—').next().unwrap_or("");
    types.split_whitespace()
}

/// Whether a type line, front face first, has the land card type.
fn is_land(type_line: &str) -> bool {
    type_words(type_line).any(|w| w == "Land")
}

/// CR 110.4b; a kindred card is the other type it has (CR 308.1).
fn is_permanent(chars: &Chars) -> bool {
    lower::PERMANENT_TYPES.iter().any(|t| chars.is(*t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forests(n: usize) -> DeckCards {
        DeckCards {
            hidden: 0,
            commander: Vec::new(),
            library: (0..n).map(|_| CardText::basic("Forest")).collect(),
        }
    }

    /// CR 704.5a: a player at 0 life loses the next time a player would
    /// receive priority, and with one player left the game ends (CR 104.2a).
    /// Nothing deals damage yet, so the life is set by hand.
    #[test]
    fn zero_life_loses_when_priority_would_be_given() {
        let mut game = Game::new(vec![forests(20), forests(20)], 1);
        game.players[1].life = 0;
        let d = game.step(Answer::Act(Action::Pass)).unwrap();
        assert_eq!(d, Decision::Over(Outcome::Won(PlayerId(0))));
        assert!(game.player(PlayerId(1)).has_left());
    }

    /// CR 800.4, 800.4a: in a multiplayer game the rest play on, and the
    /// player's permanents leave with them.
    #[test]
    fn a_multiplayer_game_goes_on_without_the_loser() {
        let mut game = Game::new(vec![forests(20), forests(20), forests(20)], 1);
        game.players[2].life = -1;
        let d = game.step(Answer::Act(Action::Pass)).unwrap();
        assert_eq!(
            d,
            Decision::Priority {
                player: PlayerId(1),
                actions: vec![Action::Pass]
            }
        );
        assert!(game.player(PlayerId(2)).has_left());
        assert!(game.player(PlayerId(2)).hand().is_empty());
    }
}
