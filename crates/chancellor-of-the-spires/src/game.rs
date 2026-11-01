//! A game of Commander as a state machine that stops at every decision
//! (ADR-0043). The engine runs the turn-based and state-based actions
//! itself and stops when a player has to choose, returning a [`Decision`]
//! with the legal answers; [`Game::step`] takes one of them and runs on to
//! the next. A game replays from its seed and its answers.
//!
//! So far: the turn's steps with priority, playing lands, casting spells
//! and paying their mana, mana abilities, the stack and resolving permanent
//! spells, commander tax, drawing, discarding to hand size, and the
//! state-based actions for life, an empty library, toughness and a
//! commander's return. No spell's effect runs yet, no trigger, and no
//! creature attacks.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use imbl::{OrdMap, Vector};
use vivisurgeons_insight::ast as i;

use crate::deck::{CardText, DeckCards};
use crate::layers::characteristics;
use crate::lower::{self, Lowered, Refusal};
use crate::mana::{self as m, short};
pub use crate::mana::{Mana, ManaCost, ManaPool, Shard};
use crate::model::{
    Ability, CardType, Chars, Color, Filter, Keyword, ObjectId, PlayerId, Seen, State, Subtype,
    Supertype,
};

/// CR 903.7.
pub const STARTING_LIFE: i32 = 40;
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

struct CardEntry {
    text: CardText,
    owner: PlayerId,
    /// A commander (CR 903.3): a designation of the card, not of an object.
    commander: bool,
    runs: Result<Runnable, Unrunnable>,
    cost: Result<Option<ManaCost>, Unrunnable>,
}

/// What the game can run of a card: its lowered characteristics and, of
/// the abilities the layers carry unread, the ones this engine runs, by
/// their index (`model::Ability::Unread`).
struct Runnable {
    lowered: Lowered,
    mana: BTreeMap<u16, m::ManaAbility>,
    enters: BTreeMap<u16, Enters>,
    flash: bool,
    /// The abilities it carries that nothing offers to activate yet.
    dormant: Vec<String>,
}

/// A replacement effect on how its own permanent enters (CR 614.1c–d).
enum Enters {
    /// "This land enters tapped."
    Tapped,
    /// "This land enters tapped unless you control a legendary green
    /// creature."
    TappedUnlessYouControl(Filter),
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

/// A turn's steps, and the main phases, which have none (CR 500.1). Combat
/// is its beginning, the declaration of attackers and its end: with no
/// attackers the declare blockers and combat damage steps are skipped
/// (CR 508.8), and nothing can attack yet, so they aren't here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    Untap,
    Upkeep,
    Draw,
    PrecombatMain,
    BeginningOfCombat,
    DeclareAttackers,
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
            Step::DeclareAttackers => Step::EndOfCombat,
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
}

/// What can be on the stack (CR 405.1): spells so far, since no ability
/// other than a mana ability is activated and nothing triggers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackObject {
    /// A card on the stack (CR 112.1), controlled by who cast it (CR
    /// 601.2a).
    Spell { card: CardId, controller: PlayerId },
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
        from_command: bool,
    },
    /// CR 514.1.
    Discard {
        player: PlayerId,
        count: usize,
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
    Over(Outcome),
}

/// Who has to choose, and the legal answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
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
    Over(Outcome),
}

/// Activating a mana ability, and the mana it adds of the ways its text
/// allows (CR 605.3b). Its source's ability is `ability`, among those the
/// layers say it has now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ManaActivation {
    pub source: ObjectId,
    pub ability: Ability,
    pub adds: ManaPool,
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
    Act(Action),
    Pay(Payment),
    Discard(Vec<CardId>),
    /// CR 903.9a: put the commander into the command zone, or leave it.
    CommandZone(bool),
    /// CR 704.5j: the legendary permanent kept.
    Keep(ObjectId),
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
    cards: Arc<Vec<CardEntry>>,
    players: Vec<Player>,
    /// The battlefield as the layers read it (ADR-0037).
    battlefield: State,
    permanents: OrdMap<ObjectId, Permanent>,
    /// The top is the back (CR 405.2).
    stack: Vector<StackObject>,
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
    /// Where the setup's shuffles left it, for the shuffles during a game
    /// (mulligans, searches), which are later slices.
    #[expect(dead_code, reason = "nothing shuffles after setup yet")]
    rng: Rng,
}

impl Game {
    /// A game of Commander between these decks, in this turn order, the
    /// first player starting (who starts is agreed outside the game,
    /// CR 103.1). Each player puts their commander in the command zone and
    /// shuffles the rest into their library (CR 903.6), then sets their
    /// life to 40 and draws seven (CR 903.7). Every player keeps: mulligans
    /// are a later slice. Then the first turn begins.
    pub fn new(decks: Vec<DeckCards>, seed: u64) -> Game {
        assert!(!decks.is_empty(), "a game has at least one player");
        let mut cards = Vec::new();
        let mut players = Vec::new();
        let mut command = Vector::new();
        let mut rng = Rng(seed);
        for (p, deck) in decks.into_iter().enumerate() {
            let owner = PlayerId(p as u8);
            let identity = (!deck.commander.is_empty()).then(|| {
                let colors: BTreeSet<Color> = deck
                    .commander
                    .iter()
                    .flat_map(|c| c.identity.iter().copied())
                    .collect();
                colors.into_iter().collect()
            });
            let mut add = |text: CardText, commander: bool| {
                let runs = text
                    .as_card()
                    .map_err(|why| Unrunnable::new("a multi-faced card", why))
                    .and_then(|c| lower::card(&c).map_err(unlowered))
                    .and_then(runnable);
                let cost = ManaCost::parse(&text.mana_cost)
                    .map_err(|why| Unrunnable::new(why.clone(), why));
                cards.push(CardEntry {
                    text,
                    owner,
                    commander,
                    runs,
                    cost,
                });
                CardId(cards.len() as u32 - 1)
            };
            for c in deck.commander {
                command.push_back(add(c, true));
            }
            let mut library: Vector<CardId> =
                deck.library.into_iter().map(|c| add(c, false)).collect();
            rng.shuffle(&mut library);
            players.push(Player {
                life: STARTING_LIFE,
                library,
                hand: Vector::new(),
                graveyard: Vector::new(),
                pool: ManaPool::default(),
                lands_played: 0,
                drew_from_empty: false,
                turn_began: None,
                identity,
                left: false,
            });
        }
        let mut game = Game {
            cards: Arc::new(cards),
            players,
            battlefield: State::default(),
            permanents: OrdMap::new(),
            stack: Vector::new(),
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
        game.begin_turn(PlayerId(0));
        game
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
    pub fn card(&self, c: CardId) -> &CardText {
        &self.cards[c.0 as usize].text
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
    /// What each permanent is now, through the layers (CR 613).
    pub fn characteristics(&self) -> BTreeMap<ObjectId, Chars> {
        characteristics(&self.battlefield)
    }

    /// Whether the engine can run this card: why it refuses it if not. A
    /// land is checked as it would be played, any other card as it would
    /// be cast.
    pub fn refusal(&self, c: CardId) -> Option<Unrunnable> {
        let entry = &self.cards[c.0 as usize];
        match &entry.runs {
            Err(r) => Some(r.clone()),
            Ok(_) if is_land(&entry.text.type_line) => None,
            Ok(runs) => self.cast_refusal(entry, runs).err(),
        }
    }

    /// The abilities a card the engine runs carries that nothing offers
    /// to activate yet: its non-mana activated abilities and those that
    /// work from a hand.
    pub fn dormant(&self, c: CardId) -> &[String] {
        match &self.cards[c.0 as usize].runs {
            Ok(r) => &r.dormant,
            Err(_) => &[],
        }
    }

    /// What the game is waiting on, with the legal answers.
    pub fn decision(&self) -> Decision {
        match &self.waiting {
            Waiting::Over(o) => Decision::Over(*o),
            Waiting::Discard { player, count } => Decision::Discard {
                player: *player,
                count: *count,
                hand: self.player(*player).hand.iter().copied().collect(),
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
        }
    }

    /// Answer the decision and run on to the next one. An answer that isn't
    /// legal leaves the game as it was.
    pub fn step(&mut self, answer: Answer) -> Result<Decision, Rejected> {
        match (self.waiting.clone(), &answer) {
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
                    from_command,
                },
                Answer::Pay(pay),
            ) if self.payments(player, &owed).contains(pay) => match *pay {
                Payment::Mana(a) => self.activate(player, a),
                Payment::Spend(spent) => {
                    let pool = &mut self.players[player.0 as usize].pool;
                    *pool = pool.minus(&spent);
                    if from_command {
                        *self.commander_casts.entry(spell).or_insert(0) += 1;
                    }
                    // CR 601.2i: the spell becomes cast, and its caster had
                    // priority, so gets it again.
                    self.before_cast = None;
                    self.give_priority(player, 0);
                }
                Payment::Cancel => {
                    let before = self.before_cast.take().expect("a cast to cancel");
                    *self = *before;
                }
            },
            (Waiting::Discard { player, count }, Answer::Discard(cards))
                if self.can_discard(player, count, cards) =>
            {
                self.discard(player, cards);
                // The excess is gone, so beginning cleanup again discards
                // nothing and runs the rest of it (CR 514.2–514.3).
                self.run_from(Step::Cleanup);
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
                    .filter(|c| is_land(&self.card(**c).type_line))
                    .map(|c| Action::PlayLand(*c)),
            );
        }
        let from_command = self
            .command
            .iter()
            .filter(|c| self.owner(**c) == p && self.cards[c.0 as usize].commander);
        for c in player.hand.iter().chain(from_command) {
            let entry = &self.cards[c.0 as usize];
            // CR 305.9; a card with no mana cost can't be paid for, and
            // alternative costs aren't modelled (CR 118.6).
            if is_land(&entry.text.type_line) || matches!(entry.cost, Ok(None)) {
                continue;
            }
            if sorcery_speed || self.instant_speed(entry) {
                actions.push(Action::Cast(*c));
            }
        }
        actions.extend(self.mana_activations(p).into_iter().map(Action::Mana));
        actions
    }

    /// CR 117.1a, 304.1, 702.8a: an instant, or a card with flash.
    fn instant_speed(&self, entry: &CardEntry) -> bool {
        match &entry.runs {
            Ok(r) => r.flash || r.lowered.chars.is(CardType::Instant),
            Err(_) => type_words(&entry.text.type_line).any(|w| w == "Instant"),
        }
    }

    /// Every mana ability `p` can activate now, once for each choice of
    /// mana it gives.
    fn mana_activations(&self, p: PlayerId) -> Vec<ManaActivation> {
        let player = self.player(p);
        let began = player.turn_began;
        let mut out = Vec::new();
        for (id, chars) in self.characteristics() {
            if chars.controller != p {
                continue;
            }
            let perm = &self.permanents[&id];
            // CR 302.6, 702.10c: a creature's {T} ability needs it
            // controlled since its controller's most recent turn began.
            let sick = chars.is(CardType::Creature)
                && !chars.abilities.contains(&Ability::Keyword(Keyword::Haste))
                && began.is_none_or(|t| perm.since >= t);
            let runs = self.cards[perm.card.0 as usize].runs.as_ref().ok();
            for ability in &chars.abilities {
                let (tap, life, options) = match ability {
                    // CR 305.6.
                    Ability::Intrinsic(t) => (true, 0, vec![ManaPool::of([Mana::of(*t)])]),
                    Ability::Unread(n) => match runs.and_then(|r| r.mana.get(n)) {
                        Some(a) => (a.tap, a.life, a.options(player.identity.as_deref())),
                        None => continue,
                    },
                    _ => continue,
                };
                // CR 118.3, 119.4.
                if tap && (perm.tapped || sick) || life > 0 && player.life < life as i32 {
                    continue;
                }
                let mut seen = BTreeSet::new();
                for adds in options {
                    if seen.insert(adds) {
                        out.push(ManaActivation {
                            source: id,
                            ability: *ability,
                            adds,
                        });
                    }
                }
            }
        }
        out
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
                self.enter(card, p);
                // CR 117.3c.
                self.give_priority(p, 0);
            }
            Action::Cast(card) => self.cast(p, card)?,
            Action::Mana(a) => {
                self.activate(p, a);
                // CR 117.3c.
                self.give_priority(p, 0);
            }
        }
        Ok(())
    }

    fn runs(&self, card: CardId) -> Result<&Runnable, Rejected> {
        let entry = &self.cards[card.0 as usize];
        entry.runs.as_ref().map_err(|r| Rejected::Refused {
            card: entry.text.name.clone(),
            refusal: r.clone(),
        })
    }

    /// What this engine can't cast yet, beyond the card's abilities.
    fn cast_refusal(&self, entry: &CardEntry, runs: &Runnable) -> Result<ManaCost, Unrunnable> {
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
        let entry = &self.cards[card.0 as usize];
        let runs = self.runs(card)?;
        let cost = self
            .cast_refusal(entry, runs)
            .map_err(|refusal| Rejected::Refused {
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
        self.stack.push_back(StackObject::Spell {
            card,
            controller: p,
        });
        let owed = if from_command {
            cost.plus_generic(COMMANDER_TAX * self.commander_casts(card))
        } else {
            cost
        };
        self.before_cast = Some(Box::new(before));
        self.waiting = Waiting::Paying {
            player: p,
            spell: card,
            owed,
            from_command,
        };
        Ok(())
    }

    /// Pay a mana ability's cost and resolve it at once (CR 605.3b).
    fn activate(&mut self, p: PlayerId, a: ManaActivation) {
        let chars = self.characteristics();
        let source = &chars[&a.source];
        let perm = &self.permanents[&a.source];
        let (tap, life, damage) = match a.ability {
            Ability::Intrinsic(_) => (true, 0, 0),
            Ability::Unread(n) => {
                let runs = self.cards[perm.card.0 as usize].runs.as_ref().ok();
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
        if let Some(StackObject::Spell { card, controller }) = self.stack.pop_back() {
            let permanent = self.cards[card.0 as usize]
                .runs
                .as_ref()
                .is_ok_and(|r| is_permanent(&r.lowered.chars));
            if permanent {
                // CR 608.3a.
                self.enter(card, controller);
            } else {
                // CR 608.2n. Only a spell with no instructions gets here.
                let owner = self.owner(card);
                self.players[owner.0 as usize].graveyard.push_back(card);
            }
        }
        self.give_priority(self.active, 0);
    }

    /// The card enters the battlefield under `controller`'s control, tapped
    /// if its own replacement effect says so. Which of those apply is read
    /// off the permanent as it would exist on the battlefield (CR 614.12),
    /// so a land that enters having lost its abilities enters untapped.
    fn enter(&mut self, card: CardId, controller: PlayerId) {
        let runs = self.cards[card.0 as usize]
            .runs
            .as_ref()
            .expect("only a card that runs enters");
        let mut chars = runs.lowered.chars.clone();
        chars.controller = controller;
        let id = self.battlefield.enter(chars, runs.lowered.statics.clone());
        self.permanents.insert(
            id,
            Permanent {
                card,
                tapped: false,
                since: self.turn,
            },
        );
        let now = self.characteristics();
        let has = &now[&id].abilities;
        let tapped = runs.enters.iter().any(|(n, e)| {
            has.contains(&Ability::Unread(*n))
                && match e {
                    Enters::Tapped => true,
                    Enters::TappedUnlessYouControl(filter) => !now.iter().any(|(o, c)| {
                        *o != id
                            && c.controller == controller
                            && filter.matches(&Seen {
                                id: *o,
                                chars: c,
                                source: *o,
                                source_chars: c,
                                source_attached_to: None,
                                token: self.battlefield.objects[o].token,
                            })
                    }),
                }
        });
        if let Some(perm) = self.permanents.get_mut(&id) {
            perm.tapped = tapped;
        }
    }

    fn can_discard(&self, p: PlayerId, count: usize, cards: &[CardId]) -> bool {
        let hand = &self.player(p).hand;
        let distinct: BTreeSet<CardId> = cards.iter().copied().collect();
        distinct.len() == count && cards.len() == count && cards.iter().all(|c| hand.contains(c))
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
                    // CR 504.1.
                    self.draw(self.active);
                    return self.give_priority(self.active, 0);
                }
                Step::Cleanup => {
                    let player = self.player(self.active);
                    let count = player.hand.len().saturating_sub(HAND_SIZE);
                    if count > 0 && !player.left {
                        self.waiting = Waiting::Discard {
                            player: self.active,
                            count,
                        };
                        return;
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
        match step.next() {
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
            let losers: Vec<PlayerId> = (0..self.players.len())
                .map(|p| PlayerId(p as u8))
                .filter(|p| {
                    let player = self.player(*p);
                    // CR 704.5a, 704.5b.
                    !player.left && (player.life <= 0 || player.drew_from_empty)
                })
                .collect();
            // CR 704.5f.
            let dying: Vec<ObjectId> = self
                .characteristics()
                .into_iter()
                .filter(|(_, c)| c.pt.is_some_and(|p| p.toughness <= 0))
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
                let name = self.card(self.permanents[&id].card).name.as_str();
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
        let Some(perm) = self.permanents.remove(&id) else {
            return;
        };
        self.battlefield.leave(id);
        let owner = self.owner(perm.card);
        self.players[owner.0 as usize]
            .graveyard
            .push_back(perm.card);
        if self.cards[perm.card.0 as usize].commander {
            self.commanders_moved.push_back(perm.card);
        }
    }

    /// CR 800.4a: everything the player owns leaves the game. Nothing
    /// changes control yet, so that is everything they control too.
    fn leave(&mut self, p: PlayerId) {
        let cards = Arc::clone(&self.cards);
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
        self.stack.retain(|o| match o {
            StackObject::Spell { card, .. } => !owned(card),
        });
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

/// Keywords that only matter once creatures attack and block, which
/// nothing does yet (CR 702.3b, 702.4a, 702.7a, 702.17a, 702.111a), and
/// partner, which works before the game (CR 702.124a).
fn combat_keyword(k: &i::Keyword) -> bool {
    let name = k.name.to_lowercase();
    matches!(
        name.as_str(),
        "reach" | "menace" | "first strike" | "double strike" | "defender"
    ) || name == "partner" && k.param.is_none()
}

/// Abilities that work only from a hand, which nothing activates yet (CR
/// 702.29a, 702.29e, 702.53a).
fn hand_keyword(k: &i::Keyword) -> bool {
    let name = k.name.to_lowercase();
    name.ends_with("cycling") || name == "transmute"
}

/// What of a card's abilities the game runs, or the first it can't. The
/// layers carry triggered abilities and statics such as "enters tapped"
/// unread (ADR-0037); one the game can't run is refused by name rather
/// than skipped. A non-mana activated ability is safe to carry, since
/// nothing offers to activate one yet.
fn runnable(lowered: Lowered) -> Result<Runnable, Unrunnable> {
    let mut runs = Runnable {
        mana: BTreeMap::new(),
        enters: BTreeMap::new(),
        flash: false,
        dormant: Vec::new(),
        lowered,
    };
    for (n, a) in runs.lowered.unread.iter().enumerate() {
        let n = n as u16;
        // An ability word adds no rules (CR 207.2c), and a `Let` binds the
        // targets and objects the ability names.
        let mut a = a;
        let mut bound = false;
        loop {
            match a {
                i::Ability::Labelled { ability, .. } => a = ability,
                i::Ability::Let { ability, .. } => {
                    bound = true;
                    a = ability
                }
                _ => break,
            }
        }
        match a {
            // With a target it isn't a mana ability (CR 605.5a).
            i::Ability::Activated { .. } if bound => {
                runs.dormant.push("an activated ability".into())
            }
            i::Ability::Activated { .. } => match m::mana_ability(a) {
                None => runs.dormant.push("an activated ability".into()),
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
                    } else if hand_keyword(k) {
                        runs.dormant.push(k.name.to_lowercase());
                    } else if !combat_keyword(k) {
                        return Err(Unrunnable::new(
                            format!("keyword {}", k.name),
                            format!("the game can't run {} yet", k.name),
                        ));
                    }
                }
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
            i::Ability::Spell(_) => {
                return Err(Unrunnable::new(
                    "Spell",
                    "the game can't run a spell's effects yet",
                ))
            }
            i::Ability::Triggered { .. } => {
                return Err(Unrunnable::new(
                    "Triggered",
                    "the game can't run its triggered ability yet",
                ))
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
        }] if itself(what) => Ok(Enters::TappedUnlessYouControl(lower::condition_objects(
            objects,
        )?)),
        [one] => Err(short(one)),
        _ => Err("a static ability of several parts".into()),
    }
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
    [
        CardType::Artifact,
        CardType::Battle,
        CardType::Creature,
        CardType::Enchantment,
        CardType::Land,
        CardType::Planeswalker,
    ]
    .iter()
    .any(|t| chars.is(*t))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forests(n: usize) -> DeckCards {
        DeckCards {
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
