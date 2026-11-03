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
//! a characteristic read: there is nothing to read. [`Game::reveal`] names
//! it, once, which is sound because nothing in the game could have told
//! the difference before; the cards a deck file builds are known from the
//! start, so a goldfish never has one.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use imbl::{OrdMap, OrdSet, Vector};
use vivisurgeons_insight::ast as i;

use crate::deck::{CardText, DeckCards};
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

/// What the game knows a card to be (the module's "Hidden cards").
#[derive(Clone)]
enum Face {
    Hidden,
    Known(Arc<Known>),
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
}

impl Known {
    fn read(text: CardText) -> Known {
        let lowered = text
            .as_card()
            .map_err(|why| Unrunnable::new("a multi-faced card", why))
            .and_then(|c| lower::card(&c).map_err(unlowered));
        // A card the engine can't run still has a type line, mana value and
        // colours, which is all a card outside the battlefield is matched
        // by: a multi-faced card by its front face (CR 712.8a).
        let printed = match &lowered {
            Ok(l) => Some(l.chars.clone()),
            Err(_) => lower::printed(&text).ok(),
        };
        let runs = lowered.and_then(runnable).and_then(|r| {
            // A token it creates must run too, or it couldn't enter.
            let refused = r.tokens().find_map(|t| Known::read(t.clone()).runs.err());
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
    /// "If this card is in your opening hand, you may begin the game with
    /// it on the battlefield" (CR 103.6a).
    begins: bool,
    /// "You have hexproof" (CR 702.11c), by the ability's index, so it
    /// works only while the permanent has it.
    hexproof: BTreeSet<u16>,
    /// Statics that change what spells cost (CR 601.2f), by index, so
    /// each works only while the permanent has it.
    cost_changes: BTreeMap<u16, lower::CostChange>,
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
    },
    /// A triggered ability (CR 603.3): the card whose ability it is, by
    /// its index among that card's unread abilities, the permanent it
    /// triggered from (CR 113.7a), and the targets chosen as it was put on
    /// the stack (CR 603.3d). `seq` counts the triggered abilities put on
    /// the stack this game, so each is told apart.
    Trigger {
        source: CardId,
        object: ObjectId,
        ability: u16,
        controller: PlayerId,
        targets: Vec<Target>,
        /// The object its event names, exalted's attacking creature.
        triggering: Option<ObjectId>,
        /// The amount its event names.
        amount: Option<u32>,
        seq: u32,
    },
    /// An activated ability (CR 602.2a): the card whose ability it is, by
    /// its index among that card's unread abilities, and who activated
    /// it. It exists independently of its source (CR 113.7a), so a source
    /// sacrificed to pay for it changes nothing.
    Ability {
        source: CardId,
        /// The permanent it was activated from, "this artifact".
        object: ObjectId,
        ability: u16,
        controller: PlayerId,
        /// Chosen as it was activated (CR 602.2b, 601.2c), by slot.
        targets: Vec<Target>,
    },
}

/// Whose instructions are resolving.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Instructions {
    Spell(CardId),
    Activated(CardId, u16),
    Triggered(CardId, u16),
}

impl Instructions {
    /// The card whose spell or ability it is.
    fn card(self) -> CardId {
        match self {
            Instructions::Spell(c)
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
    /// The instruction to follow next: its index in the program's list,
    /// then in the list of each "may" being followed inside it.
    next: Vec<usize>,
    /// The cards its instructions have found, by the slot each binds
    /// (`lower::Slot`).
    found: OrdMap<Slot, Vec<CardId>>,
}

/// A triggered ability that has triggered and waits to be put on the stack
/// the next time a player would receive priority (CR 603.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pending {
    pub source: CardId,
    pub object: ObjectId,
    pub ability: u16,
    pub controller: PlayerId,
    /// The object its event names, exalted's attacking creature.
    pub triggering: Option<ObjectId>,
    /// The amount its event names, the damage dealt.
    pub amount: Option<u32>,
    /// Its controller has put it in order among their others (CR 603.3b).
    ordered: bool,
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
    /// A step of the active player's turn began.
    Begins(TurnStep),
    /// This creature died: it is gone, as it last existed (CR 700.4).
    Died(ObjectId),
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
        /// A spell, from the command zone or not, or an activated
        /// ability of `spell`'s (CR 602.2b).
        ability: bool,
        from_command: bool,
    },
    /// CR 514.1, or an instruction (CR 701.9a), which `then` goes on with.
    Discard {
        player: PlayerId,
        count: usize,
        then: Option<Resuming>,
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
    /// A resolving ability has `player` search their library (CR 701.23a)
    /// for a card `filter` matches, bound to `into`.
    Search {
        player: PlayerId,
        filter: CardFilter,
        /// Whether what it finds is put onto the battlefield, so must be
        /// a card the engine runs.
        enters: bool,
        into: Slot,
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
    /// CR 103.6, 103.6a: put any of `cards`, of `player`'s opening hand,
    /// onto the battlefield, answered `Answer::Begin`. Asked only of a
    /// player with such a card: a hidden one has to be named first, as
    /// the hand is kept.
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
    /// CR 614.1c, 614.12a: as `card` enters, `player` may pay `life`
    /// (`Answer::PayLife(true)`); if they don't, it enters tapped. A shock
    /// land's "As this land enters, you may pay 2 life".
    PayOrEnterTapped {
        player: PlayerId,
        card: CardId,
        life: u32,
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
    Search {
        player: PlayerId,
        filter: CardFilter,
        known: Vec<CardId>,
        hidden: usize,
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
    Cast(CardId),
    /// A mana ability, which doesn't use the stack (CR 605.3b).
    Mana(ManaActivation),
    /// Any other activated ability, which goes on the stack (CR 602.2,
    /// 117.1b).
    Activate(Activation),
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
    /// CR 601.2c: one target per slot.
    Targets(Vec<Target>),
    /// CR 603.3b.
    Order(Vec<usize>),
    /// CR 701.20a: the card chosen from a revealed hand, if any can be;
    /// or from a hand to put onto the battlefield.
    Pick(Option<CardId>),
    /// CR 608.2d: follow the "may" instruction, or not.
    May(bool),
    /// CR 103.6a: the cards of the opening hand put onto the battlefield.
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
    /// The card whose spell or ability most recently finished resolving.
    last_resolved: Option<CardId>,
    /// The combat this turn, from the declaration of attackers to the end
    /// of combat (CR 506, 511.3).
    combat: Option<Combat>,
    /// The "until end of turn" effects to end in cleanup (CR 514.2).
    until_end_of_turn: Vec<Timestamp>,
    /// Cards an instruction put on the bottom of a library this turn.
    bottomed: u32,
    /// Tokens created this turn.
    created: u32,
    /// The counters each permanent that left had as it last existed, for
    /// an ability of its that counts them (CR 608.2h, 113.7a).
    last_known: OrdMap<ObjectId, LastKnown>,
    exile: Vector<CardId>,
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

/// Naming a card that isn't hidden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlreadyKnown(pub String);

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
            let known = |text| Face::Known(Arc::new(Known::read(text)));
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
            last_known: OrdMap::new(),
            bottomed: 0,
            created: 0,
            exile: Vector::new(),
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

    /// Whether a card begins the game on the battlefield if its owner
    /// says so (CR 103.6a): known, and run by the engine.
    fn begins(&self, c: CardId) -> bool {
        self.known(c)
            .and_then(|k| k.runs.as_ref().ok())
            .is_some_and(|r| r.begins)
    }

    /// Whether a player has hexproof (CR 702.11c): a permanent they
    /// control says so and has that ability now.
    fn has_hexproof(&self, q: PlayerId) -> bool {
        self.characteristics().iter().any(|(id, c)| {
            c.controller == q
                && self
                    .known(self.permanents[id].card)
                    .and_then(|k| k.runs.as_ref().ok())
                    .is_some_and(|r| {
                        r.hexproof
                            .iter()
                            .any(|n| c.abilities.contains(&Ability::Unread(*n)))
                    })
        })
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
        self.known(c).map(|k| &k.text)
    }
    fn known(&self, c: CardId) -> Option<&Known> {
        match &self.cards[c.0 as usize].face {
            Face::Known(k) => Some(k),
            Face::Hidden => None,
        }
    }
    /// Name a hidden card (the module's "Hidden cards"). A card already
    /// known keeps what it is.
    pub fn reveal(&mut self, c: CardId, text: CardText) -> Result<(), AlreadyKnown> {
        if let Some(k) = self.known(c) {
            return Err(AlreadyKnown(k.text.name.clone()));
        }
        self.cards[c.0 as usize].face = Face::Known(Arc::new(Known::read(text)));
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
    pub fn stack(&self) -> &Vector<StackObject> {
        &self.stack
    }
    pub fn permanents(&self) -> &OrdMap<ObjectId, Permanent> {
        &self.permanents
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

    /// How many tokens have been created this turn.
    pub fn created(&self) -> u32 {
        self.created
    }

    /// How many cards an instruction has put on the bottom of a library
    /// this turn.
    pub fn bottomed(&self) -> u32 {
        self.bottomed
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
        characteristics(&self.battlefield)
    }

    /// Whether the engine can run this card: why it refuses it if not. A
    /// land is checked as it would be played, any other card as it would
    /// be cast.
    pub fn refusal(&self, c: CardId) -> Option<Unrunnable> {
        let Some(known) = self.known(c) else {
            return Some(Unrunnable::new(
                "a hidden card",
                "no one has said what it is",
            ));
        };
        match &known.runs {
            Err(r) => Some(r.clone()),
            Ok(_) if is_land(&known.text.type_line) => None,
            Ok(runs) => Game::cast_refusal(known, runs).err(),
        }
    }

    /// What the engine makes of a card before any game: why it refuses
    /// it, as `refusal` would once it is known, and the abilities it
    /// carries that nothing offers (`dormant`).
    pub fn reads(text: &CardText) -> (Option<Unrunnable>, Vec<String>) {
        let known = Known::read(text.clone());
        match &known.runs {
            Err(r) => (Some(r.clone()), Vec::new()),
            Ok(runs) if is_land(&known.text.type_line) => (None, runs.dormant.clone()),
            Ok(runs) => (Game::cast_refusal(&known, runs).err(), runs.dormant.clone()),
        }
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
                    cards: hand.iter().copied().filter(|c| self.begins(*c)).collect(),
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
                    Choosing::Trigger { pending, .. } => (pending.controller, Some(pending.object)),
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
            Waiting::PayOrEnterTapped {
                player, card, life, ..
            } => Decision::PayOrEnterTapped {
                player: *player,
                card: *card,
                life: *life,
            },
            Waiting::Search {
                player,
                filter,
                enters,
                ..
            } => {
                let library = &self.player(*player).library;
                Decision::Search {
                    player: *player,
                    filter: filter.clone(),
                    known: library
                        .iter()
                        .copied()
                        .filter(|c| {
                            self.known(*c)
                                .is_some_and(|k| k.found_by(filter, *enters) == Ok(true))
                        })
                        .collect(),
                    hidden: library.iter().filter(|c| self.known(**c).is_none()).count(),
                }
            }
        }
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
                        .all(|c| self.begins(*c) && self.player(player).hand.contains(c))
                {
                    return Err(Rejected::Illegal(answer));
                }
                for c in cards {
                    self.players[player.0 as usize].hand.retain(|x| x != c);
                    self.arrive(*c, player, false);
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
                    ability,
                    from_command,
                },
                Answer::Pay(pay),
            ) if self.payments(player, &owed).contains(pay) => match *pay {
                Payment::Mana(a) => self.activate(player, a),
                // CR 602.2b: an ability activated, its activator gets
                // priority again (CR 117.3c).
                Payment::Spend(spent) if ability => {
                    let pool = &mut self.players[player.0 as usize].pool;
                    *pool = pool.minus(&spent);
                    self.before_cast = None;
                    self.give_priority(player, 0);
                }
                Payment::Spend(spent) => {
                    let pool = &mut self.players[player.0 as usize].pool;
                    *pool = pool.minus(&spent);
                    if from_command {
                        *self.commander_casts.entry(spell).or_insert(0) += 1;
                    }
                    // CR 601.2i: the spell becomes cast, what waits for a
                    // spell cast triggers, and its caster had priority, so
                    // gets it again.
                    self.before_cast = None;
                    self.trigger_on(Event::Cast {
                        card: spell,
                        controller: player,
                    });
                    self.give_priority(player, 0);
                }
                Payment::Cancel => {
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
                    Some(at) => self.follow(at),
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
                    Choosing::Trigger { pending, .. } => (pending.controller, Some(pending.object)),
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
                            ability: false,
                            from_command,
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
                    self.cards[c.0 as usize].face =
                        Face::Known(Arc::new(Known::read(text.clone())));
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
                    self.players[player.0 as usize].life -= life as i32;
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
                },
                Answer::Find(found),
            ) => {
                let library = &self.player(player).library;
                // What it is, if it is in the library and its name known
                // or given now for a hidden card.
                let read = match found {
                    Found::Nothing => None,
                    Found::Card(c) if library.contains(c) => match self.known(*c) {
                        Some(_) => Some((*c, None)),
                        None => return Err(Rejected::Illegal(answer)),
                    },
                    Found::Hidden(c, text) if library.contains(c) && self.known(*c).is_none() => {
                        Some((*c, Some(Arc::new(Known::read((**text).clone())))))
                    }
                    _ => return Err(Rejected::Illegal(answer)),
                };
                let card = match read {
                    None => None,
                    Some((c, named)) => {
                        let known =
                            named
                                .clone()
                                .unwrap_or_else(|| match &self.cards[c.0 as usize].face {
                                    Face::Known(k) => Arc::clone(k),
                                    Face::Hidden => unreachable!("checked known"),
                                });
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
                            self.cards[c.0 as usize].face = Face::Known(named);
                        }
                        Some(c)
                    }
                };
                // Found, it stays where it is until an instruction moves it.
                resume.found.insert(into, card.into_iter().collect());
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
        // One land a turn (CR 305.2, 305.2a); nothing raises it yet.
        if sorcery_speed && player.lands_played < 1 {
            actions.extend(
                player
                    .hand
                    .iter()
                    .filter(|c| self.card(**c).is_some_and(|t| is_land(&t.type_line)))
                    .map(|c| Action::PlayLand(*c)),
            );
        }
        let from_command = self
            .command
            .iter()
            .filter(|c| self.owner(**c) == p && self.cards[c.0 as usize].commander);
        for c in player.hand.iter().chain(from_command) {
            // A hidden card can't be cast: nothing says what it is.
            let Some(known) = self.known(*c) else {
                continue;
            };
            // CR 305.9; a card with no mana cost can't be paid for, and
            // alternative costs aren't modelled (CR 118.6).
            if is_land(&known.text.type_line) || matches!(known.cost, Ok(None)) {
                continue;
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
            if (sorcery_speed || self.instant_speed(known)) && targetable {
                actions.push(Action::Cast(*c));
            }
        }
        actions.extend(self.mana_activations(p).into_iter().map(Action::Mana));
        actions.extend(self.activations(p).into_iter().map(Action::Activate));
        actions
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
                if def.cost.tap && (perm.tapped || sick)
                    || def.cost.life > 0 && player.life < def.cost.life as i32
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
                let (tap, life, mana, options) = match ability {
                    // CR 305.6.
                    Ability::Intrinsic(t) => (true, 0, None, vec![ManaPool::of([Mana::of(*t)])]),
                    Ability::Unread(n) => match runs.and_then(|r| r.mana.get(n)) {
                        Some(a) => (
                            a.tap,
                            a.life,
                            a.mana.as_ref(),
                            a.options(player.identity.as_deref(), |f| {
                                self.count(f, id, chars, &all)
                            }),
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
                let mut seen = BTreeSet::new();
                for adds in options {
                    for pays in &ways {
                        if seen.insert((adds, *pays)) {
                            out.push(ManaActivation {
                                source: id,
                                ability: *ability,
                                adds,
                                pays: *pays,
                            });
                        }
                    }
                }
            }
        }
        out
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
            Action::PlayLand(card) => {
                self.runs(card)?;
                let player = &mut self.players[p.0 as usize];
                player.hand.retain(|c| *c != card);
                player.lands_played += 1;
                // CR 117.3c.
                self.enter(card, p, false, Then::Priority(p));
            }
            Action::Cast(card) => self.cast(p, card)?,
            Action::Mana(a) => {
                self.activate(p, a);
                // CR 117.3c.
                self.give_priority(p, 0);
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
                self.before_cast = Some(Box::new(self.clone()));
                // CR 602.2a: on the stack first, then its targets chosen
                // and its costs paid (CR 602.2b, 601.2c, 601.2h).
                self.stack.push_back(StackObject::Ability {
                    source: card,
                    object: a.source,
                    ability: n,
                    controller: p,
                    targets: Vec::new(),
                });
                if cost.tap {
                    if let Some(perm) = self.permanents.get_mut(&a.source) {
                        perm.tapped = true;
                    }
                }
                // CR 119.4.
                self.players[p.0 as usize].life -= cost.life as i32;
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
                    ability: true,
                    from_command: false,
                }
            }
            None => {
                self.before_cast = None;
                self.give_priority(p, 0);
            }
        }
    }

    /// Only a known card is ever offered to play or cast.
    fn runs(&self, card: CardId) -> Result<&Runnable, Rejected> {
        let known = self.known(card).expect("only a known card is offered");
        known.runs.as_ref().map_err(|r| Rejected::Refused {
            card: known.text.name.clone(),
            refusal: r.clone(),
        })
    }

    /// What this engine can't cast yet, beyond the card's abilities.
    fn cast_refusal(entry: &Known, runs: &Runnable) -> Result<ManaCost, Unrunnable> {
        let cost = entry.cost.clone()?.ok_or_else(|| {
            Unrunnable::new("no mana cost", "it has no mana cost to pay (CR 118.6)")
        })?;
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
        // CR 303.4a: an Aura spell targets.
        if chars.subtypes.contains(&Subtype::Other("Aura".into())) {
            return Err(Unrunnable::new("Aura", "an Aura spell targets"));
        }
        Ok(cost)
    }

    /// CR 601.2a–f: move the card to the stack, then determine its total
    /// cost and wait for it to be paid. The card has no modes, targets, X
    /// or additional costs to announce, since one that has any is refused
    /// when read.
    fn cast(&mut self, p: PlayerId, card: CardId) -> Result<(), Rejected> {
        let entry = self.known(card).expect("only a known card is offered");
        let runs = self.runs(card)?;
        let cost = Game::cast_refusal(entry, runs).map_err(|refusal| Rejected::Refused {
            card: entry.text.name.clone(),
            refusal,
        })?;
        let from_command = self.command.contains(&card);
        let before = self.clone();
        if from_command {
            self.command.retain(|c| *c != card);
        } else {
            self.players[p.0 as usize].hand.retain(|c| *c != card);
        }
        let slots = self.runs_of(card).spell.as_ref().map(|s| s.targets.clone());
        self.stack.push_back(StackObject::Spell {
            card,
            controller: p,
            targets: Vec::new(),
        });
        let owed = if from_command {
            cost.plus_generic(COMMANDER_TAX * self.commander_casts(card))
        } else {
            cost
        };
        let owed = self.total_cost(p, card, owed);
        self.before_cast = Some(Box::new(before));
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
                ability: false,
                from_command,
            },
        };
        Ok(())
    }

    /// CR 601.2f: the spell's cost with every cost change on the
    /// battlefield that applies to it, increases added and reductions
    /// taken from the generic component, never below {0} (CR 118.7a).
    fn total_cost(&self, p: PlayerId, card: CardId, cost: ManaCost) -> ManaCost {
        let Some(spell) = self.known(card).and_then(|k| k.printed.as_ref()) else {
            return cost;
        };
        let (mut more, mut less) = (0, 0);
        for (id, chars) in self.characteristics() {
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
                if caster && change.spells.matches(spell) {
                    if change.less {
                        less += change.generic;
                    } else {
                        more += change.generic;
                    }
                }
            }
        }
        ManaCost {
            generic: (cost.generic + more).saturating_sub(less),
            shards: cost.shards,
        }
    }

    /// Pay a mana ability's cost and resolve it at once (CR 605.3b).
    fn activate(&mut self, p: PlayerId, a: ManaActivation) {
        let chars = self.characteristics();
        let source = &chars[&a.source];
        let perm = &self.permanents[&a.source];
        let (tap, life, damage) = match a.ability {
            Ability::Intrinsic(_) => (true, 0, 0),
            Ability::Unread(n) => {
                let runs = self.known(perm.card).and_then(|k| k.runs.as_ref().ok());
                let def = &runs.expect("a listed ability runs").mana[&n];
                (def.tap, def.life, def.damage_to_you)
            }
            _ => unreachable!("only mana abilities are listed"),
        };
        let lifelink = source
            .abilities
            .contains(&Ability::Keyword(Keyword::Lifelink));
        if tap {
            if let Some(perm) = self.permanents.get_mut(&a.source) {
                perm.tapped = true;
            }
        }
        let player = &mut self.players[p.0 as usize];
        // CR 119.4.
        player.life -= life as i32;
        player.pool = player.pool.minus(&a.pays);
        player.pool.add_all(&a.adds);
        // CR 120.3a, 120.3f: its damage to its controller is life lost,
        // and gained back if it has lifelink.
        player.life -= damage as i32;
        if lifelink {
            player.life += damage as i32;
        }
    }

    /// The top of the stack resolves (CR 608), and the active player gets
    /// priority (CR 117.3b).
    fn resolve_top(&mut self) {
        let then = Then::Priority(self.active);
        match self.stack.pop_back() {
            Some(StackObject::Spell {
                card,
                controller,
                targets,
            }) => {
                let permanent = self
                    .known(card)
                    .and_then(|k| k.runs.as_ref().ok())
                    .is_some_and(|r| is_permanent(&r.lowered.chars));
                if permanent {
                    // CR 608.3a.
                    self.enter(card, controller, false, then);
                } else {
                    self.begin_resolving(
                        Instructions::Spell(card),
                        controller,
                        (None, None, None),
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
                (Some(object), None, None),
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
                ..
            }) => self.begin_resolving(
                Instructions::Triggered(source, ability),
                controller,
                (Some(object), triggering, amount),
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
        (object, triggering, amount): (Option<ObjectId>, Option<ObjectId>, Option<u32>),
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
            next: vec![0],
            found: OrdMap::new(),
        };
        if !targets.is_empty() && at.targets.iter().all(Option::is_none) {
            return self.done(&at);
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
            Instructions::Spell(c) => self.runs_of(c).spell.as_ref().unwrap_or(&NONE),
            Instructions::Triggered(c, n) => &self.runs_of(c).triggered[&n].program,
            Instructions::Activated(c, n) => match self.runs_of(c).activated.get(&n) {
                Some(Ok(def)) => &def.program,
                _ => &NONE,
            },
        }
    }

    /// The spell or ability is done: a spell goes to its owner's graveyard
    /// (CR 608.2n), and the active player gets priority (CR 117.3b).
    fn done(&mut self, at: &Resuming) {
        self.last_resolved = Some(match at.what {
            Instructions::Spell(c)
            | Instructions::Activated(c, _)
            | Instructions::Triggered(c, _) => c,
        });
        if let Instructions::Spell(card) = at.what {
            let owner = self.owner(card);
            self.players[owner.0 as usize].graveyard.push_back(card);
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
            };
            match e {
                Effect::Search {
                    who: w,
                    filter,
                    into,
                    enters,
                } => {
                    let Some(q) = who(w) else { continue };
                    self.waiting = Waiting::Search {
                        player: q,
                        filter,
                        enters,
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
                }
                Effect::Draw { who: w, n } => {
                    if let Some(q) = who(w) {
                        for _ in 0..n {
                            self.draw(q);
                        }
                    }
                }
                Effect::Discard { who: w, n } => {
                    let Some(q) = who(w) else { continue };
                    let count = (n as usize).min(self.player(q).hand.len());
                    if count > 0 {
                        self.waiting = Waiting::Discard {
                            player: q,
                            count,
                            then: Some(resume),
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
                        self.players[q.0 as usize].life -= n as i32;
                    }
                }
                Effect::GainLife { who: w, n } => {
                    let n = self.amount(&at, n);
                    if let Some(q) = who(w) {
                        self.players[q.0 as usize].life += n as i32;
                    }
                }
                Effect::Destroy(o) => {
                    if let Some(o) = self.object(&at, o) {
                        self.put_in_graveyard(o);
                    }
                }
                Effect::Exile(o) => {
                    if let Some(o) = self.object(&at, o) {
                        self.leave_battlefield(o, Zone::Exile);
                    }
                }
                // CR 701.26a.
                Effect::Tap(o) | Effect::Untap(o) => {
                    let tapped = matches!(e, Effect::Tap(_));
                    if let Some(perm) = self
                        .object(&at, o)
                        .and_then(|o| self.permanents.get_mut(&o))
                    {
                        perm.tapped = tapped;
                    }
                }
                Effect::ToOwnersHand(o) => {
                    if let Some(o) = self.object(&at, o) {
                        self.leave_battlefield(o, Zone::Hand);
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
                    for _ in 0..n {
                        self.cards.push_back(CardEntry {
                            owner: p,
                            commander: false,
                            token: true,
                            face: Face::Known(Arc::new(Known::read((*token).clone()))),
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
                            self.stack.remove(k);
                            let owner = self.owner(c);
                            self.players[owner.0 as usize].graveyard.push_back(c);
                        }
                    }
                }
                Effect::Damage { to, n } => {
                    let to = match to {
                        DamageTo::You => Some(Target::Player(p)),
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
                        self.damage_triggers(hit, by, false);
                    }
                }
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
                    if let Some(o) = self.object(&at, on) {
                        self.battlefield.put_counters(o, counter, n);
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
                    if let Some(o) = self.object(&at, on) {
                        let t = self.battlefield.resolve([o], ops);
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
                Effect::ToLibrary { cards, top } => {
                    let cards = self.still_in_library(&at, cards);
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

    /// The cards an instruction names that are still in a library, where
    /// the instruction found them: one moved since is a new object (CR
    /// 400.7).
    fn still_in_library(&self, at: &Resuming, cards: Cards) -> Vec<CardId> {
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
        };
        named
            .into_iter()
            .filter(|c| self.player(self.owner(*c)).library.contains(c))
            .collect()
    }

    /// Put these cards of a library on its top or bottom, the first
    /// topmost (CR 401.4).
    fn put_in_library(&mut self, cards: &[CardId], top: bool) {
        for c in cards {
            let owner = self.owner(*c);
            self.players[owner.0 as usize].library.retain(|x| x != c);
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

    /// The object an instruction names, whether or not it is still there.
    fn named(&self, at: &Resuming, on: Obj) -> Option<ObjectId> {
        match on {
            Obj::Itself => at.object,
            Obj::Triggering => at.triggering,
            Obj::Target(s) => match at.targets[s] {
                Some(Target::Object(o)) => Some(o),
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
                _ => false,
            };
            if *n == 0 || prevented {
                continue;
            }
            total += n;
            match to {
                // CR 120.3a.
                Target::Player(q) => {
                    self.players[q.0 as usize].life -= *n as i32;
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
        dealt
    }

    /// What waits for damage dealt at once triggers: a permanent dealt
    /// some, and the permanent `by` that dealt it to each player.
    fn damage_triggers(&mut self, hit: Dealt, by: Option<ObjectId>, combat: bool) {
        if !hit.objects.is_empty() {
            self.trigger_on(Event::Damaged(hit.objects));
        }
        if let Some(source) = by {
            for _ in hit.players {
                self.trigger_on(Event::DamagedPlayer { source, combat });
            }
        }
    }

    /// Every legal choice for a target slot of a spell or ability `p`
    /// controls (CR 115.1, 115.4): players still in the game, and
    /// permanents. Nothing on the engine's battlefield has hexproof, shroud
    /// or protection, which are refused as they are read.
    fn legal_targets(&self, spec: &TargetSpec, p: PlayerId, source: Option<&Chars>) -> Vec<Target> {
        // CR 115.4, 702.11c: a player with hexproof can't be the target
        // of an opponent's spell or ability.
        let players = || {
            (0..self.players.len())
                .map(|q| PlayerId(q as u8))
                .filter(|q| !self.player(*q).left && (*q == p || !self.has_hexproof(*q)))
        };
        // CR 702.16b: not a permanent with protection from the source.
        let permanents = |f: &dyn Fn(&Chars) -> bool| -> Vec<Target> {
            self.characteristics()
                .into_iter()
                .filter(|(_, c)| f(c) && !source.is_some_and(|s| c.protected_from(s)))
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
                                && self
                                    .known(*card)
                                    .and_then(|k| k.printed.as_ref())
                                    .is_some_and(|c| filter.matches(c)) =>
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
                let source = self.source_chars(first.source, Some(first.object));
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
            seq: self.triggers,
        });
    }

    /// The card whose spell or ability is resolving and waits on a choice
    /// its instructions ask for, if any.
    pub fn resolving(&self) -> Option<CardId> {
        let at = match &self.waiting {
            Waiting::Discard { then: Some(at), .. }
            | Waiting::ChooseFromHand { resume: at, .. }
            | Waiting::Scry { resume: at, .. }
            | Waiting::Search { resume: at, .. }
            | Waiting::May { at, .. }
            | Waiting::Reveal { resume: at, .. }
            | Waiting::ChooseAmong { resume: at, .. }
            | Waiting::Arrange { resume: at, .. }
            | Waiting::PutFromHand { resume: at, .. } => at,
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
                    (TriggerEvent::Dies(f), Event::Died(id)) => {
                        let l = &self.last_known[id];
                        f.matches(&Seen {
                            id: *id,
                            chars: &l.chars,
                            source: *src,
                            source_chars: chars,
                            source_attached_to: self.battlefield.objects[src].attached_to,
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
                        .then_some((None, None))
                        .into_iter()
                        .collect(),
                    (TriggerEvent::Attacks(f), Event::Attacked(ids)) => ids
                        .iter()
                        .filter(|id| f.matches(&seen(**id)))
                        .map(|id| (Some(*id), None))
                        .collect(),
                    (TriggerEvent::AttacksAlone(f), Event::Attacked(ids)) => match ids.as_slice() {
                        [id] if f.matches(&seen(*id)) => vec![(Some(*id), None)],
                        _ => vec![],
                    },
                    (TriggerEvent::YouCast(f), Event::Cast { card, controller }) => {
                        let printed = self.known(*card).and_then(|k| k.printed.as_ref());
                        (*controller == chars.controller && printed.is_some_and(|c| f.matches(c)))
                            .then_some((None, None))
                            .into_iter()
                            .collect()
                    }
                    _ => vec![],
                };
                for (triggering, amount) in objects {
                    triggered.push(Pending {
                        source: card,
                        object: *src,
                        ability: *n,
                        controller: chars.controller,
                        triggering,
                        amount,
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
                        token: l.token,
                        attacking: false,
                    };
                    if l.chars.abilities.contains(&Ability::Unread(*n)) && f.matches(&seen) {
                        triggered.push(Pending {
                            source: l.card,
                            object: *id,
                            ability: *n,
                            controller: l.chars.controller,
                            triggering: None,
                            amount: None,
                            ordered: false,
                        });
                    }
                }
            }
        }
        self.pending.extend(triggered);
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
                if let Some(perm) = self.permanents.get_mut(a) {
                    perm.tapped = true;
                }
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
        let mut hit_players: Vec<ObjectId> = Vec::new();
        for s in &sources {
            let hits: Vec<(Target, u32)> = assigned
                .iter()
                .filter(|(x, _, _)| x == s)
                .map(|(_, t, n)| (*t, *n))
                .collect();
            let source = chars.get(s).cloned();
            let controller = source.as_ref().map_or(self.active, |c| c.controller);
            let hit = self.deal(source.as_ref(), controller, &hits);
            for (o, n) in hit.objects {
                *dealt.entry(o).or_insert(0) += n;
            }
            for _ in hit.players {
                hit_players.push(*s);
            }
        }
        // One event for all of it: a creature dealt damage by two at once
        // is dealt it once (CR 510.2).
        self.damage_triggers(
            Dealt {
                objects: dealt,
                players: BTreeSet::new(),
            },
            None,
            true,
        );
        for s in hit_players {
            self.trigger_on(Event::DamagedPlayer {
                source: s,
                combat: true,
            });
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
        let (tapped, pays) = self.entering(card, controller);
        let tapped = tapped || put_tapped;
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
    fn entering(&self, card: CardId, controller: PlayerId) -> (bool, Option<u32>) {
        let runs = self.runs_of(card);
        let mut chars = runs.lowered.chars.clone();
        chars.controller = controller;
        let mut would = self.battlefield.clone();
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
        for (n, e) in &runs.enters {
            if !me.abilities.contains(&Ability::Unread(*n)) {
                continue;
            }
            match e {
                Enters::Tapped => tapped = true,
                Enters::TappedUnless(cond) => tapped |= !holds(cond),
                Enters::TappedIf(cond) => tapped |= holds(cond),
                Enters::TappedUnlessPaysLife(life) => pays = Some(*life),
            }
        }
        (tapped, pays)
    }

    fn runs_of(&self, card: CardId) -> &Runnable {
        match &self.cards[card.0 as usize].face {
            Face::Known(k) => k.runs.as_ref().expect("only a card that runs enters"),
            Face::Hidden => unreachable!("only a known card enters"),
        }
    }

    /// The card is put onto the battlefield, a new object (CR 400.7).
    fn place(&mut self, card: CardId, controller: PlayerId, tapped: bool, then: Then) {
        self.arrive(card, controller, tapped);
        self.proceed(then);
    }

    /// The card arrives on the battlefield, and what waits for that
    /// triggers.
    fn arrive(&mut self, card: CardId, controller: PlayerId, tapped: bool) {
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
            },
        );
        self.trigger_on(Event::Entered(id));
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

    /// CR 121.1, 121.4.
    fn draw(&mut self, p: PlayerId) {
        let player = &mut self.players[p.0 as usize];
        match player.library.pop_front() {
            Some(c) => player.hand.push_back(c),
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
        self.created = 0;
        self.active = p;
        self.players[p.0 as usize].turn_began = Some(self.turn);
        for player in &mut self.players {
            player.lands_played = 0;
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
    /// only for a creature with first or double strike in combat.
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
                    c.pt.is_some_and(|p| {
                        p.toughness <= 0
                            || c.is(CardType::Creature)
                                && (perm.damage >= p.toughness as u32 || perm.deathtouched)
                                && perm.damage > 0
                    })
                })
                .map(|(id, _)| id)
                .collect();
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
        let owner = self.owner(perm.card);
        let player = &mut self.players[owner.0 as usize];
        match to {
            Zone::Graveyard => player.graveyard.push_back(perm.card),
            Zone::Hand => player.hand.push_back(perm.card),
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

/// Abilities that work only from a hand or a graveyard, which nothing
/// offers yet (CR 702.29a, 702.29e, 702.53a, 702.34a).
fn hand_keyword(k: &i::Keyword) -> bool {
    let name = k.name.to_lowercase();
    name.ends_with("cycling") || name == "transmute" || name == "flashback"
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
        begins: false,
        hexproof: BTreeSet::new(),
        cost_changes: BTreeMap::new(),
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
        let bound = !bind.is_empty();
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
                    } else if let Some(t) = lower::keyword_trigger(k) {
                        runs.triggered.insert(n, t);
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
            i::Ability::Static(parts)
                if matches!(parts.as_slice(), [i::Static::YouHave(ks)]
                    if matches!(ks.as_slice(), [k] if k.param.is_none() && k.name.eq_ignore_ascii_case("hexproof"))) =>
            {
                runs.hexproof.insert(n);
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
            }) if if_you_do.is_empty() => runs.begins = true,
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
