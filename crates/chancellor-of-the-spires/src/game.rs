//! A game of Commander as a state machine that stops at every decision
//! (ADR-0043). The engine runs the turn-based and state-based actions
//! itself and stops when a player has to choose, returning a [`Decision`]
//! with the legal answers; [`Game::step`] takes one of them and runs on to
//! the next. A game replays from its seed and its answers.
//!
//! So far: the turn's steps with priority, playing lands, basic lands'
//! mana abilities, mana pools emptying, drawing, discarding to hand size,
//! and the state-based actions for life and an empty library. Nothing goes
//! on the stack yet, and no creature attacks.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use imbl::{OrdMap, Vector};
use vivisurgeons_insight::ast as i;

use crate::deck::{CardText, DeckCards};
use crate::layers::characteristics;
use crate::lower::{self, Lowered};
use crate::model::{BasicLandType, CardType, Chars, ObjectId, PlayerId, State, Subtype};

/// CR 903.7.
pub const STARTING_LIFE: i32 = 40;
/// CR 103.5, 402.2.
pub const HAND_SIZE: usize = 7;

/// A physical card, for the whole game: its text and its owner (CR 108.3).
/// The zones other than the battlefield hold cards; on the battlefield a
/// card is an object of `model::State`, a new one each time it arrives
/// (CR 400.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CardId(pub u32);

struct CardEntry {
    text: CardText,
    owner: PlayerId,
    lowered: Result<Lowered, String>,
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

/// One mana's type (CR 106.1b): a colour or colourless.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mana {
    White,
    Blue,
    Black,
    Red,
    Green,
    Colorless,
}

impl Mana {
    /// What a basic land type's intrinsic ability adds (CR 305.6).
    pub fn of(land: BasicLandType) -> Mana {
        match land {
            BasicLandType::Plains => Mana::White,
            BasicLandType::Island => Mana::Blue,
            BasicLandType::Swamp => Mana::Black,
            BasicLandType::Mountain => Mana::Red,
            BasicLandType::Forest => Mana::Green,
        }
    }
}

/// CR 106.4.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ManaPool(BTreeMap<Mana, u32>);

impl ManaPool {
    pub fn get(&self, m: Mana) -> u32 {
        self.0.get(&m).copied().unwrap_or(0)
    }

    pub fn total(&self) -> u32 {
        self.0.values().sum()
    }

    fn add(&mut self, m: Mana) {
        *self.0.entry(m).or_insert(0) += 1;
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

/// What can be on the stack. Nothing yet: no spell is cast and no
/// non-mana ability activated, so the stack is always empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StackObject {}

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
    /// CR 514.1.
    Discard {
        player: PlayerId,
        count: usize,
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
    /// Discard exactly `count` distinct cards of `hand` (CR 514.1). The
    /// legal answers are every such set, given by their rule rather than
    /// listed.
    Discard {
        player: PlayerId,
        count: usize,
        hand: Vec<CardId>,
    },
    Over(Outcome),
}

/// What a player with priority can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// CR 117.3d.
    Pass,
    /// A special action (CR 116.2a, 305.1).
    PlayLand(CardId),
    /// A basic land type's intrinsic mana ability (CR 305.6), which doesn't
    /// use the stack (CR 605.3b). A land with two basic land types has two.
    TapForMana { land: ObjectId, adds: Mana },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Act(Action),
    Discard(Vec<CardId>),
}

#[derive(Debug)]
pub enum Rejected {
    /// Not among the legal answers to the decision.
    Illegal(Answer),
    /// Legal, but the card's text has no lowering yet, so the engine
    /// refuses rather than play it wrong. The game is unchanged.
    Refused { card: String, refusal: String },
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Rejected::Illegal(a) => write!(f, "{a:?} is not a legal answer"),
            Rejected::Refused { card, refusal } => write!(f, "can't play {card}: {refusal}"),
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
    stack: Vector<StackObject>,
    exile: Vector<CardId>,
    command: Vector<CardId>,
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
            let mut add = |text: CardText| {
                let lowered = text
                    .as_card()
                    .and_then(|c| lower::card(&c).map_err(|r| r.to_string()))
                    .and_then(runnable);
                cards.push(CardEntry {
                    text,
                    owner,
                    lowered,
                });
                CardId(cards.len() as u32 - 1)
            };
            for c in deck.commander {
                command.push_back(add(c));
            }
            let mut library: Vector<CardId> = deck.library.into_iter().map(&mut add).collect();
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
    /// What each permanent is now, through the layers (CR 613).
    pub fn characteristics(&self) -> BTreeMap<ObjectId, Chars> {
        characteristics(&self.battlefield)
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
            (Waiting::Discard { player, count }, Answer::Discard(cards))
                if self.can_discard(player, count, cards) =>
            {
                self.discard(player, cards);
                // The excess is gone, so beginning cleanup again discards
                // nothing and runs the rest of it (CR 514.2–514.3).
                self.run_from(Step::Cleanup);
            }
            _ => return Err(Rejected::Illegal(answer)),
        }
        Ok(self.decision())
    }

    fn actions(&self, p: PlayerId) -> Vec<Action> {
        let mut actions = vec![Action::Pass];
        let player = self.player(p);
        if p == self.active
            && self.step.is_main()
            && self.stack.is_empty()
            // One land a turn (CR 305.2, 305.2a); nothing raises it yet.
            && player.lands_played < 1
        {
            actions.extend(
                player
                    .hand
                    .iter()
                    .filter(|c| is_land(&self.card(**c).type_line))
                    .map(|c| Action::PlayLand(*c)),
            );
        }
        let began = player.turn_began;
        for (id, chars) in self.characteristics() {
            let perm = &self.permanents[&id];
            // CR 302.6: a creature's {T} ability needs it controlled since
            // its controller's most recent turn began.
            let sick = chars.is(CardType::Creature) && began.is_none_or(|t| perm.since >= t);
            if chars.controller != p || perm.tapped || sick || !chars.is(CardType::Land) {
                continue;
            }
            for s in &chars.subtypes {
                if let Subtype::BasicLand(t) = s {
                    actions.push(Action::TapForMana {
                        land: id,
                        adds: Mana::of(*t),
                    });
                }
            }
        }
        actions
    }

    fn act(&mut self, p: PlayerId, passes: usize, action: Action) -> Result<(), Rejected> {
        match action {
            Action::Pass => {
                let passes = passes + 1;
                if passes >= self.in_game() {
                    // CR 117.4, 500.2: the stack is empty (nothing can be
                    // on it yet), so the step ends.
                    self.end_step();
                } else {
                    let next = self.next_in_game(p);
                    self.give_priority(next, passes);
                }
            }
            Action::PlayLand(card) => {
                let entry = &self.cards[card.0 as usize];
                let lowered = entry.lowered.as_ref().map_err(|r| Rejected::Refused {
                    card: entry.text.name.clone(),
                    refusal: r.clone(),
                })?;
                let mut chars = lowered.chars.clone();
                chars.controller = p;
                let statics = lowered.statics.clone();
                let player = &mut self.players[p.0 as usize];
                player.hand.retain(|c| *c != card);
                player.lands_played += 1;
                let id = self.battlefield.enter(chars, statics);
                self.permanents.insert(
                    id,
                    Permanent {
                        card,
                        tapped: false,
                        since: self.turn,
                    },
                );
                // CR 117.3c.
                self.give_priority(p, 0);
            }
            Action::TapForMana { land, adds } => {
                if let Some(perm) = self.permanents.get_mut(&land) {
                    perm.tapped = true;
                }
                self.players[p.0 as usize].pool.add(adds);
                self.give_priority(p, 0);
            }
        }
        Ok(())
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
        if self.state_based_actions() == Checked::GameOver {
            return;
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
    /// is left.
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
            if losers.is_empty() {
                return checked;
            }
            checked = Checked::Performed;
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

/// A card whose abilities the game can carry on the battlefield so far.
/// The layers carry triggered abilities and statics such as "enters
/// tapped" unread (ADR-0037), and nothing here runs them yet: played, the
/// card would skip them, so it is refused by name instead. An activated
/// ability is safe to carry, since nothing activates one yet, and so is
/// cycling, which works only from a hand (CR 702.29a, 702.29e).
fn runnable(lowered: Lowered) -> Result<Lowered, String> {
    fn inert(a: &i::Ability) -> bool {
        match a {
            i::Ability::Activated { .. } => true,
            i::Ability::Keywords(ks) => ks
                .iter()
                .all(|k| k.name.to_lowercase().ends_with("cycling")),
            i::Ability::Let { ability, .. } | i::Ability::Labelled { ability, .. } => {
                inert(ability)
            }
            _ => false,
        }
    }
    match lowered.unread.iter().find(|a| !inert(a)) {
        None => Ok(lowered),
        Some(a) => Err(format!(
            "the game can't run its {} yet",
            match a {
                i::Ability::Triggered { .. } => "triggered ability",
                i::Ability::Static(_) => "static ability",
                _ => "ability",
            }
        )),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Checked {
    Nothing,
    Performed,
    GameOver,
}

/// Whether a type line, front face first, has the land card type.
fn is_land(type_line: &str) -> bool {
    let front = type_line.split("//").next().unwrap_or("");
    let types = front.split('—').next().unwrap_or("");
    types.split_whitespace().any(|w| w == "Land")
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
