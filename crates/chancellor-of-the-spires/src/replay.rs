//! A real game, from PDBot's Magic Online log, replayed through the engine
//! (ADR-0043, "Real games replayed are the acceptance test").
//!
//! Each line of the log becomes the engine's answers: a land played, a
//! spell cast and paid for, priority passed until the turn and step the
//! next line is in. Magic Online already judged every line legal, so the
//! first one the engine won't follow ends the replay with a typed
//! [`Stop`]: a card the engine refuses by name, an answer it calls illegal,
//! a state that disagrees with the log, or a kind of line the replay
//! doesn't follow yet. No line is skipped or faked to keep going; lines
//! that aren't game actions (chat, spectators, the client's notices) are
//! the only ones passed over.
//!
//! What the log hides stays hidden (`game`'s "Hidden cards"): each library
//! is [`DECK_SIZE`] cards no one has named, a card is named as it enters a
//! hand only when the log first shows it (played, cast, discarded), and
//! which lands paid a cost, and what a search found, are chosen for, never
//! read: the first choice that works, gone back on when a later spell
//! can't be paid for.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crate::deck::{CardText, DeckCards};
use crate::game::{
    Action, Activation, Answer, CardId, Decision, Format, Found, Game, ManaActivation, ManaCost,
    ManaPool, Payment, Rejected, Setup, StackObject, Step, HAND_SIZE,
};
use crate::log::{self, Entry, Event, Object};
use crate::mana::short;
use crate::model::{ObjectId, PlayerId};

/// The deck size assumed: the log never says, and every format these
/// games are played in (constructed, Penny Dreadful) has a 60-card
/// minimum that most decks sit at (CR 100.2a).
pub const DECK_SIZE: usize = 60;

/// How many earlier choices (payments, searches) the replay may go back
/// and make again when a later spell can't be paid for, per game.
const BACKTRACKS: usize = 64;

/// One game replayed: what the engine did for each line, how it ended,
/// and the engine's game where it ended (`None` if it was never set up).
#[derive(Clone)]
pub struct Replay {
    pub trace: Vec<Traced>,
    pub end: End,
    pub game: Option<Game>,
}

/// A log line followed, by its index in the game's entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Traced {
    pub entry: usize,
    pub did: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// Every line followed, to a concession or the end of the log.
    Finished,
    /// The first line the engine wouldn't follow, by its index.
    Stopped { entry: usize, stop: Stop },
}

impl End {
    pub fn stop(&self) -> Option<&Stop> {
        match self {
            End::Finished => None,
            End::Stopped { stop, .. } => Some(stop),
        }
    }
}

/// Why a replay ended before the log did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// The game can't be set up from the log.
    Setup(SetupStop),
    /// The engine refuses a card by the node of its text it can't run yet.
    Refused {
        card: String,
        node: String,
        why: String,
    },
    /// The engine calls the log's play illegal.
    Illegal { what: String },
    /// The engine's game and the log's disagree.
    Diverged(Divergence),
    /// A kind of line the replay doesn't follow yet: an activated ability,
    /// a trigger, an attack. `card` is the card it names, if any.
    Unfollowed { event: String, card: Option<String> },
    /// The log names a card the card data doesn't have.
    UnknownCard(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupStop {
    /// Only two-player games are replayed.
    Players(usize),
    /// Nothing says who started: no "chooses to play first", skipped
    /// first draw or first turn.
    NoStarter,
    /// Those disagree.
    StarterDisagrees,
    /// The log begins after the first turn: PDBot started watching late.
    MidGame,
    /// A player mulliganed without the London mulligan's bottoming (CR
    /// 103.5): an older log, or one that doesn't say what they kept.
    NotLondon,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Divergence {
    /// The log's turn isn't the engine's next one.
    Turn { log: u32, engine: u32 },
    /// The player has no card in hand that is, or could be, the one named.
    NotInHand { card: String },
    /// The engine never offers the play before the turn ends.
    NeverOffered { what: String },
    /// No way to pay for the spell with what the player has.
    Unpayable { card: String },
    /// The engine has a player discard to hand size and the log doesn't.
    UnloggedDiscard { count: usize },
    /// The log discards a different number of cards than the engine asks.
    Discards { log: usize, engine: usize },
    /// The engine's game ended and the log's goes on.
    EngineOver,
    /// The log's game ended by the rules and the engine's goes on.
    LogOver,
    /// A choice the engine needs and the log doesn't show (the legend rule).
    Unlogged { what: String },
    /// The log's spell has targets and the engine's has none.
    Targets { card: String },
    /// A step skipped where the engine doesn't skip it.
    Skip,
    /// A draw the engine doesn't make: not the active player's in their
    /// draw step (CR 504.1), or a second one.
    Draw,
    /// A search the log doesn't say failed, with no card the player could
    /// have found.
    Unfound,
}

impl Stop {
    /// The reason without the card, for ranking reasons by games.
    pub fn kind(&self) -> String {
        match self {
            Stop::Setup(s) => format!("setup: {s:?}"),
            Stop::Refused { node, .. } => format!("refused: {node}"),
            Stop::Illegal { .. } => "illegal".into(),
            Stop::Diverged(d) => format!("diverged: {}", short(d)),
            Stop::Unfollowed { event, .. } => format!("unfollowed: {event}"),
            Stop::UnknownCard(_) => "unknown card".into(),
        }
    }

    /// The card the stop is about, if any.
    pub fn card(&self) -> Option<&str> {
        match self {
            Stop::Refused { card, .. } | Stop::UnknownCard(card) => Some(card),
            Stop::Unfollowed { card, .. } => card.as_deref(),
            Stop::Diverged(
                Divergence::NotInHand { card }
                | Divergence::Unpayable { card }
                | Divergence::Targets { card },
            ) => Some(card),
            _ => None,
        }
    }
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stop::Setup(s) => write!(f, "can't set the game up: {s:?}"),
            Stop::Refused { card, why, .. } => write!(f, "refuses {card}: {why}"),
            Stop::Illegal { what } => write!(f, "illegal: {what}"),
            Stop::Diverged(d) => write!(f, "diverged: {d:?}"),
            Stop::Unfollowed { event, card } => match card {
                Some(c) => write!(f, "doesn't follow {event} ({c})"),
                None => write!(f, "doesn't follow {event}"),
            },
            Stop::UnknownCard(c) => write!(f, "no card data for {c}"),
        }
    }
}

/// Replay one game of a match. `cards` gives a card's text by the name the
/// log uses, or `None` if there is no such card.
pub fn replay(
    m: &log::Match,
    g: &log::Game,
    cards: &mut dyn FnMut(&str) -> Option<CardText>,
) -> Replay {
    let mut r = Driver {
        cards,
        named: HashMap::new(),
        trace: Vec::new(),
        seats: Vec::new(),
        game: None,
        drawn: 0,
        checkpoints: Vec::new(),
        at: 0,
        lands: Vec::new(),
        demand: Vec::new(),
    };
    let end = r.run(m, g);
    Replay {
        trace: r.trace,
        end,
        game: r.game,
    }
}

/// A point to go back to: the game as it was at a choice the log doesn't
/// show (a payment, a search), the other ways to make it, and the line to
/// go on from once one is made.
struct Checkpoint {
    game: Game,
    entry: usize,
    trace: usize,
    resume: usize,
    others: Vec<Retry>,
}

#[derive(Clone)]
enum Retry {
    Pay(Vec<ManaActivation>),
    Answer(Answer, String),
}

struct Driver<'a> {
    cards: &'a mut dyn FnMut(&str) -> Option<CardText>,
    named: HashMap<String, Option<CardText>>,
    trace: Vec<Traced>,
    /// The engine's player for each of the log's.
    seats: Vec<PlayerId>,
    game: Option<Game>,
    /// The last turn whose draw step's draw the log has shown.
    drawn: u32,
    checkpoints: Vec<Checkpoint>,
    /// The line being followed.
    at: usize,
    /// For each engine player, the lands the log shows them playing in any
    /// game of the match: cards their deck has.
    lands: Vec<Vec<String>>,
    /// For each engine player, how many of each colour's mana symbols the
    /// spells they cast this game have, W U B R G.
    demand: Vec<[u32; 5]>,
}

type Followed = Result<(), Stop>;

impl Driver<'_> {
    fn game(&mut self) -> &mut Game {
        self.game.as_mut().expect("set up")
    }

    fn seat(&self, p: log::Player) -> PlayerId {
        self.seats[p.0 as usize]
    }

    fn text(&mut self, name: &str) -> Result<CardText, Stop> {
        if !self.named.contains_key(name) {
            let found = (self.cards)(name);
            self.named.insert(name.to_string(), found);
        }
        self.named[name]
            .clone()
            .ok_or_else(|| Stop::UnknownCard(name.to_string()))
    }

    fn run(&mut self, m: &log::Match, g: &log::Game) -> End {
        if let Err(stop) = self.set_up(m, g) {
            return End::Stopped { entry: 0, stop };
        }
        let mut backtracks = 0;
        // The failure that got furthest, which is the one reported if no
        // other payment gets past it.
        let mut furthest: Option<(usize, Stop, Vec<Traced>)> = None;
        let mut i = 0;
        while i < g.entries.len() {
            let entry = &g.entries[i];
            self.at = i;
            match self.follow(g, i, entry) {
                Ok(Next::Line(n)) => i = n,
                Ok(Next::Done) => return End::Finished,
                Err(stop) => {
                    let unpayable = matches!(
                        stop,
                        Stop::Diverged(Divergence::Unpayable { .. } | Divergence::EngineOver)
                    );
                    if furthest.as_ref().is_none_or(|f| i > f.0) {
                        furthest = Some((i, stop.clone(), self.trace.clone()));
                    }
                    // Only a choice made earlier can make a later payment
                    // impossible, or lose the game early, so only then is
                    // it worth going back.
                    let retry = unpayable && backtracks < BACKTRACKS;
                    match self.checkpoints.last_mut().filter(|_| retry) {
                        Some(cp) => {
                            backtracks += 1;
                            let other = cp.others.remove(0);
                            let mut game = cp.game.clone();
                            let (entry, len, resume) = (cp.entry, cp.trace, cp.resume);
                            if cp.others.is_empty() {
                                self.checkpoints.pop();
                            }
                            self.trace.truncate(len);
                            let did = match other {
                                Retry::Pay(payment) => pay_with(&mut game, &payment)
                                    .map(|()| format!("paid again, {}", describe(&payment))),
                                Retry::Answer(answer, did) => game
                                    .step(answer)
                                    .map(|_| format!("chose again: {did}"))
                                    .map_err(|_| Stop::Illegal {
                                        what: "an answer tried before".into(),
                                    }),
                            };
                            match did {
                                Ok(did) => {
                                    self.trace.push(Traced { entry, did });
                                    self.game = Some(game);
                                    i = resume;
                                }
                                Err(_) => continue,
                            }
                        }
                        None => {
                            let (at, stop, trace) = furthest.take().expect("recorded");
                            self.trace = trace;
                            return End::Stopped { entry: at, stop };
                        }
                    }
                }
            }
        }
        End::Finished
    }

    /// Two players, who starts, and the mulligans the log shows, answered
    /// in the engine's order (CR 103.5).
    fn set_up(&mut self, m: &log::Match, g: &log::Game) -> Followed {
        if m.players.len() != 2 {
            return Err(Stop::Setup(SetupStop::Players(m.players.len())));
        }
        let other = |p: log::Player| log::Player(1 - p.0);
        let chose = g.entries.iter().find_map(|e| match e.event {
            Event::PlaysFirst { player, first } => Some(if first { player } else { other(player) }),
            _ => None,
        });
        let first = g.entries.iter().find_map(|e| match e.event {
            Event::TurnBegins(t) => Some(t),
            _ => None,
        });
        if first.is_some_and(|t| t.number != 1) {
            return Err(Stop::Setup(SetupStop::MidGame));
        }
        let first_turn = first.map(|t| t.active);
        // CR 103.8a: only the starting player skips a draw before a turn
        // line, when the log prints it early.
        let skipped = g
            .entries
            .iter()
            .take_while(|e| e.turn.is_none())
            .find_map(|e| match e.event {
                Event::Skips {
                    player,
                    step: log::Step::Draw,
                } => Some(player),
                _ => None,
            });
        let said: Vec<log::Player> = [chose, skipped, first_turn].into_iter().flatten().collect();
        let Some(&starter) = said.first() else {
            return Err(Stop::Setup(SetupStop::NoStarter));
        };
        if said.iter().any(|p| *p != starter) {
            return Err(Stop::Setup(SetupStop::StarterDisagrees));
        }
        self.seats = vec![PlayerId(0); 2];
        self.seats[starter.0 as usize] = PlayerId(0);
        self.seats[other(starter).0 as usize] = PlayerId(1);
        self.lands = vec![Vec::new(); 2];
        for e in m.games.iter().flat_map(|g| &g.entries) {
            if let Event::PlaysLand {
                player,
                card: Object::Card(c),
            } = &e.event
            {
                let seat = self.seat(*player).0 as usize;
                let lands = &mut self.lands[seat];
                if !lands.iter().any(|l| l == c.name()) {
                    lands.push(c.name().to_string());
                }
            }
        }
        self.demand = vec![[0; 5]; 2];
        for e in &g.entries {
            if let Event::Casts(cast) = &e.event {
                if let Object::Card(c) = &cast.card {
                    let p = self.seat(cast.player).0 as usize;
                    if let Some(text) = (self.cards)(c.name()) {
                        for (k, symbol) in ["W", "U", "B", "R", "G"].iter().enumerate() {
                            self.demand[p][k] +=
                                text.mana_cost.matches(&format!("{symbol}}}")).count() as u32;
                        }
                    }
                }
            }
        }

        // Mulligans taken, and the London mulligan's bottoming checked.
        let mut taken = [0u32; 2];
        let mut kept: [Option<(u32, u32)>; 2] = [None, None];
        for e in g.entries.iter().take_while(|e| e.turn.is_none()) {
            match e.event {
                Event::Mulligan { player, .. } => taken[self.seat(player).0 as usize] += 1,
                Event::Keeps {
                    player,
                    hand,
                    bottomed,
                } => kept[self.seat(player).0 as usize] = Some((hand, bottomed)),
                _ => {}
            }
        }
        for p in 0..2 {
            let london = match kept[p] {
                Some((hand, bottomed)) => {
                    bottomed == taken[p] && hand as usize + taken[p] as usize == HAND_SIZE
                }
                // Still mulliganing when the game ended, or an older log
                // that doesn't say what anyone kept.
                None => taken[p] == 0 || first_turn.is_none(),
            };
            if !london {
                return Err(Stop::Setup(SetupStop::NotLondon));
            }
        }
        let hidden = || DeckCards {
            commander: Vec::new(),
            library: Vec::new(),
            hidden: DECK_SIZE,
        };
        let mut game = Game::start(Setup {
            format: Format::Constructed,
            decks: vec![hidden(), hidden()],
            mulligans: true,
            seed: g.id,
        })
        .expect("two constructed decks set up");
        let mut asked = [0u32; 2];
        loop {
            let answer = match game.decision() {
                Decision::Mulligan { player } => {
                    let p = player.0 as usize;
                    // The log ends before they keep: so does the setup.
                    if kept[p].is_none() && asked[p] == taken[p] && taken[p] > 0 {
                        break;
                    }
                    let take = asked[p] < taken[p];
                    asked[p] += take as u32;
                    Answer::Mulligan(take)
                }
                Decision::Bottom { count, hand, .. } => Answer::Bottom(hand[..count].to_vec()),
                _ => break,
            };
            game.step(answer)
                .expect("a mulligan answer the engine offered");
        }
        self.trace.push(Traced {
            entry: 0,
            did: format!(
                "set up: {} starts; mulligans {} and {}; libraries of {DECK_SIZE} hidden cards",
                m.players[starter.0 as usize], taken[0], taken[1]
            ),
        });
        self.game = Some(game);
        Ok(())
    }

    fn did(&mut self, entry: usize, did: impl Into<String>) {
        self.trace.push(Traced {
            entry,
            did: did.into(),
        });
    }

    fn follow(&mut self, g: &log::Game, i: usize, entry: &Entry) -> Result<Next, Stop> {
        use Event::*;
        let unfollowed = |card: Option<&str>| Stop::Unfollowed {
            event: short(&entry.event),
            card: card.map(str::to_string),
        };
        match &entry.event {
            // Not game actions.
            Chat { .. }
            | Watching { .. }
            | Joined(_)
            | Left(_)
            | Disconnected(_)
            | Notice(_)
            | Blank(_) => {}
            PlaysFirst { .. } | Mulligan { .. } | Keeps { .. } => {
                self.did(i, "followed in the setup")
            }
            TurnBegins(t) => {
                if t.extra {
                    return Err(unfollowed(None));
                }
                let seat = self.seat(t.active);
                let expected = 2 * t.number - u32::from(seat == PlayerId(0));
                self.pass_to_turn(expected)?;
                let game = self.game();
                if game.active() != seat {
                    return Err(Stop::Diverged(Divergence::Turn {
                        log: expected,
                        engine: game.turn(),
                    }));
                }
                let step = game.step_now();
                self.did(i, format!("turn {expected} begins; at {step:?}"));
            }
            // CR 103.8a: the engine skips it too.
            Skips {
                player,
                step: log::Step::Draw,
            } => {
                let seat = self.seat(*player);
                let game = self.game();
                if game.turn() != 1 || seat != PlayerId(0) {
                    return Err(Stop::Diverged(Divergence::Skip));
                }
                self.did(i, "the starting player skips their first draw (CR 103.8a)");
            }
            // Newer logs show the draw step's draw (CR 504.1).
            Draws {
                player,
                count: 1,
                with: None,
            } => {
                let p = self.seat(*player);
                self.draw_step(p)?;
                self.did(i, "drew in the draw step (CR 504.1)");
            }
            PlaysLand {
                player,
                card: Object::Card(card),
            } => {
                let p = self.seat(*player);
                let c = self.offered(p, card.name(), Kind::Land, false)?;
                self.act(Answer::Act(Action::PlayLand(c)))?;
                self.did(i, format!("played {}", card.name()));
            }
            Casts(cast) => {
                let Object::Card(card) = &cast.card else {
                    return Err(unfollowed(None));
                };
                // Flashback, suspend, kicker, an alternative cost: the line
                // shows which.
                if cast.via.is_some() {
                    return Err(Stop::Unfollowed {
                        event: "Casts other than from hand for its mana cost".into(),
                        card: Some(card.name().to_string()),
                    });
                }
                let p = self.seat(cast.player);
                let on_stack = cast.targets.iter().any(|a| self.targets_stack(&a.target));
                let c = self.offered(p, card.name(), Kind::Cast, on_stack)?;
                self.act(Answer::Act(Action::Cast(c)))?;
                let payments = self.payments()?;
                let Some((first, rest)) = payments.split_first() else {
                    return Err(Stop::Diverged(Divergence::Unpayable {
                        card: card.name().to_string(),
                    }));
                };
                if !rest.is_empty() {
                    let game = self.game().clone();
                    self.checkpoints.push(Checkpoint {
                        game,
                        entry: i,
                        trace: self.trace.len(),
                        resume: i + 1,
                        others: rest.iter().cloned().map(Retry::Pay).collect(),
                    });
                }
                pay_with(self.game(), first)?;
                // Nothing the engine casts yet has a target: one that has
                // is refused as it is read.
                if !cast.targets.is_empty() {
                    return Err(Stop::Diverged(Divergence::Targets {
                        card: card.name().to_string(),
                    }));
                }
                self.did(i, format!("cast {}, {}", card.name(), describe(first)));
            }
            Discards { player, cards } => {
                let p = self.seat(*player);
                let game = self.game();
                // CR 514.1: only the active player with more than seven
                // cards discards in cleanup; any other discard is a cost
                // (cycling) or an effect the engine doesn't run.
                if game.active() != p || game.player(p).hand().len() <= HAND_SIZE {
                    let card = cards.iter().find_map(|c| match c {
                        Object::Card(c) => Some(c.name()),
                        _ => None,
                    });
                    return Err(unfollowed(card));
                }
                return self.discard(g, i, p);
            }
            Concedes(_) | LosesToInaction(_) | OutOfTime(_) => {
                self.did(i, "the game ends outside the rules");
                return Ok(Next::Done);
            }
            Wins(_) | Loses(_) => {
                return Err(Stop::Diverged(Divergence::LogOver));
            }
            Activates(a) => {
                let Object::Card(card) = &a.card else {
                    return Err(unfollowed(None));
                };
                let p = self.seat(a.player);
                let on_stack = a.targets.iter().any(|t| self.targets_stack(&t.target));
                let activation = self.activation(p, card.name(), on_stack)?;
                self.act(Answer::Act(Action::Activate(activation)))?;
                // Nothing the engine activates yet has a target: one that
                // has is never offered.
                if !a.targets.is_empty() {
                    return Err(Stop::Diverged(Divergence::Targets {
                        card: card.name().to_string(),
                    }));
                }
                self.did(i, format!("activated {}'s ability", card.name()));
            }
            // CR 701.23b: the search the ability on the stack makes finds
            // nothing.
            FailsToFind { player, .. } => {
                let p = self.seat(*player);
                let turn = self.game().turn();
                loop {
                    let game = self.game();
                    match game.decision() {
                        Decision::Search { player, .. } if player == p => break,
                        Decision::Priority { .. }
                            if !game.stack().is_empty() && game.turn() == turn =>
                        {
                            self.pass()?
                        }
                        _ => return Err(unfollowed(None)),
                    }
                }
                self.act(Answer::Find(Found::Nothing))?;
                self.did(i, "found nothing (CR 701.23b)");
            }
            Trigger(t) => {
                return Err(match &t.source {
                    log::Source::Card(c) => unfollowed(Some(c.name())),
                    log::Source::Named(n) => unfollowed(Some(n)),
                })
            }
            other => {
                let card = other.cards().first().map(|c| c.name().to_string());
                return Err(unfollowed(card.as_deref()));
            }
        }
        Ok(Next::Line(i + 1))
    }

    /// Pass priority to `p`'s draw step this turn, where the engine has
    /// drawn for them.
    fn draw_step(&mut self, p: PlayerId) -> Followed {
        let turn = self.game().turn();
        loop {
            let drawn = self.drawn;
            let game = self.game();
            let step = game.step_now();
            if game.turn() != turn || game.active() != p || step > Step::Draw || drawn == turn {
                return Err(Stop::Diverged(Divergence::Draw));
            }
            if step == Step::Draw {
                self.drawn = turn;
                return Ok(());
            }
            self.pass()?;
        }
    }

    /// Whether the log's target is a spell on the stack.
    fn targets_stack(&self, t: &log::Target) -> bool {
        let log::Target::Object(Object::Card(c)) = t else {
            return false;
        };
        let game = self.game.as_ref().expect("set up");
        game.stack().iter().any(|o| match o {
            StackObject::Spell { card, .. } | StackObject::Ability { source: card, .. } => {
                game.card(*card).is_some_and(|t| t.name == c.name())
            }
        })
    }

    fn act(&mut self, answer: Answer) -> Followed {
        match self.game().step(answer) {
            Ok(_) => Ok(()),
            Err(Rejected::Illegal(a)) => Err(Stop::Illegal {
                what: format!("{a:?}"),
            }),
            Err(Rejected::Refused { card, refusal }) => Err(Stop::Refused {
                card,
                node: refusal.node,
                why: refusal.why,
            }),
        }
    }

    /// Pass priority until the engine's turn is `turn`.
    fn pass_to_turn(&mut self, turn: u32) -> Followed {
        loop {
            let game = self.game();
            if game.turn() == turn {
                return Ok(());
            }
            if game.turn() > turn {
                return Err(Stop::Diverged(Divergence::Turn {
                    log: turn,
                    engine: game.turn(),
                }));
            }
            self.pass()?;
        }
    }

    /// Answer the decision with a pass, or stop if it isn't priority.
    fn pass(&mut self) -> Followed {
        match self.game().decision() {
            Decision::Priority { .. } => self.act(Answer::Act(Action::Pass)),
            Decision::Discard { count, .. } => {
                Err(Stop::Diverged(Divergence::UnloggedDiscard { count }))
            }
            Decision::LegendRule { .. } => Err(Stop::Diverged(Divergence::Unlogged {
                what: "the legend rule".into(),
            })),
            Decision::ReturnCommander { .. } => Err(Stop::Diverged(Divergence::Unlogged {
                what: "a commander's return".into(),
            })),
            Decision::Over(_) => Err(Stop::Diverged(Divergence::EngineOver)),
            Decision::Search {
                player,
                known,
                hidden,
                ..
            } => self.search(player, known, hidden),
            d @ (Decision::Pay { .. } | Decision::Mulligan { .. } | Decision::Bottom { .. }) => {
                unreachable!("the replay answers {d:?} where it arises")
            }
        }
    }

    /// The card in `p`'s hand that is `name`, naming a hidden one if none
    /// is known to be.
    fn in_hand(&mut self, p: PlayerId, name: &str) -> Result<Option<CardId>, Stop> {
        let text = self.text(name)?;
        let game = self.game();
        let hand = game.player(p).hand().clone();
        if let Some(c) = hand
            .iter()
            .find(|c| game.card(**c).is_some_and(|t| t.name == text.name))
        {
            return Ok(Some(*c));
        }
        let Some(c) = hand.iter().find(|c| game.card(**c).is_none()) else {
            return Ok(None);
        };
        game.reveal(*c, text).expect("a hidden card");
        Ok(Some(*c))
    }

    /// Pass priority until `p` may play or cast `name` from their hand,
    /// within this turn, and say which card it is. The stack resolves first
    /// unless the play targets a spell on it: the log doesn't say when a
    /// spell resolved, and a play that answers one targets it.
    fn offered(
        &mut self,
        p: PlayerId,
        name: &str,
        kind: Kind,
        respond: bool,
    ) -> Result<CardId, Stop> {
        self.text(name)?;
        let turn = self.game().turn();
        if !respond {
            while !self.game().stack().is_empty() {
                self.pass()?;
                if self.game().turn() != turn {
                    break;
                }
            }
        }
        let mut seen = None;
        loop {
            let game = self.game();
            if game.turn() != turn {
                let Some(c) = seen else {
                    return Err(Stop::Diverged(Divergence::NotInHand {
                        card: name.to_string(),
                    }));
                };
                // A card the engine never offers may be one it refuses by
                // name, such as one with no mana cost (CR 118.6).
                if let Some(r) = game.refusal(c) {
                    return Err(Stop::Refused {
                        card: name.to_string(),
                        node: r.node,
                        why: r.why,
                    });
                }
                return Err(Stop::Diverged(Divergence::NeverOffered {
                    what: format!("{} {name}", kind.verb()),
                }));
            }
            if let Decision::Priority { player, .. } = game.decision() {
                if player == p {
                    if let Some(c) = self.in_hand(p, name)? {
                        seen = Some(c);
                        let wanted = match kind {
                            Kind::Land => Action::PlayLand(c),
                            Kind::Cast => Action::Cast(c),
                        };
                        // Asked after naming it: a hidden card isn't offered.
                        let Decision::Priority { actions, .. } = self.game().decision() else {
                            unreachable!("naming a card changes no decision")
                        };
                        if actions.contains(&wanted) {
                            return Ok(c);
                        }
                    }
                }
            }
            self.pass()?;
        }
    }

    /// Pass priority until `p` may activate an ability of a permanent named
    /// `name` they control, within this turn, as `offered` does for a
    /// play. Which of its abilities is the one the engine offers: a card
    /// whose abilities it offers more than one of isn't followed yet.
    fn activation(&mut self, p: PlayerId, name: &str, respond: bool) -> Result<Activation, Stop> {
        let text = self.text(name)?;
        let turn = self.game().turn();
        if !respond {
            while !self.game().stack().is_empty() {
                self.pass()?;
                if self.game().turn() != turn {
                    break;
                }
            }
        }
        loop {
            let game = self.game();
            let chars = game.characteristics();
            let named: Vec<CardId> = game
                .permanents()
                .iter()
                .filter(|(id, _)| chars[id].controller == p)
                .map(|(_, perm)| perm.card)
                .filter(|c| game.card(*c).is_some_and(|t| t.name == text.name))
                .collect();
            // An ability the engine never offers isn't waited for.
            let refused = named.iter().map(|c| game.activation_refusal(*c));
            let refused: Option<Vec<_>> = refused.collect();
            if let Some(r) = refused.and_then(|r| r.into_iter().next()) {
                return Err(Stop::Refused {
                    card: name.to_string(),
                    node: r.node,
                    why: r.why,
                });
            }
            if named.is_empty() || game.turn() != turn {
                return Err(Stop::Diverged(Divergence::NeverOffered {
                    what: format!("activate {name}"),
                }));
            }
            if let Decision::Priority { player, actions } = game.decision() {
                if player == p {
                    let offered: Vec<Activation> = actions
                        .iter()
                        .filter_map(|a| match a {
                            Action::Activate(a)
                                if named.contains(&game.permanents()[&a.source].card) =>
                            {
                                Some(*a)
                            }
                            _ => None,
                        })
                        .collect();
                    if let Some(first) = offered.first() {
                        if offered.iter().any(|a| a.ability != first.ability) {
                            return Err(Stop::Unfollowed {
                                event: "Activates one of several abilities".into(),
                                card: Some(name.to_string()),
                            });
                        }
                        return Ok(*first);
                    }
                }
            }
            self.pass()?;
        }
    }

    /// Answer a search the log doesn't show the result of. The log says
    /// when a search fails (`FailsToFind`), so this one found a card; which
    /// card isn't said, so the replay chooses, as it chooses a payment, and
    /// goes back to choose again if a later spell can't be paid for. The
    /// candidates are the library's known cards, then a hidden card named
    /// as each basic land, the colours the player's spells this game need
    /// most first, then each land the log shows the player playing in the
    /// match. The engine checks each matches what was searched for.
    fn search(&mut self, p: PlayerId, known: Vec<CardId>, hidden: usize) -> Followed {
        let mut tries: Vec<(Answer, String)> = Vec::new();
        for c in known {
            let name = self
                .game()
                .card(c)
                .map(|t| t.name.clone())
                .unwrap_or_default();
            tries.push((Answer::Find(Found::Card(c)), format!("found {name}")));
        }
        let game = self.game.as_ref().expect("set up");
        let hidden_card = game
            .player(p)
            .library()
            .iter()
            .copied()
            .find(|c| game.card(*c).is_none());
        if let Some(c) = hidden_card.filter(|_| hidden > 0) {
            let demand = self.demand[p.0 as usize];
            let mut basics: Vec<(usize, &str)> =
                ["Plains", "Island", "Swamp", "Mountain", "Forest"]
                    .into_iter()
                    .enumerate()
                    .collect();
            basics.sort_by_key(|(k, _)| std::cmp::Reverse(demand[*k]));
            let names: Vec<String> = basics
                .into_iter()
                .map(|(_, n)| n.to_string())
                .chain(std::iter::once("Wastes".to_string()))
                .chain(self.lands[p.0 as usize].clone())
                .collect();
            for n in names {
                let Ok(text) = self.text(&n) else { continue };
                tries.push((
                    Answer::Find(Found::Hidden(c, Box::new(text))),
                    format!("found {n}, from the hidden library"),
                ));
            }
        }
        let mut legal = Vec::new();
        let mut refused = None;
        for (answer, did) in tries {
            match self.game().clone().step(answer.clone()) {
                Ok(_) => legal.push((answer, did)),
                Err(Rejected::Refused { card, refusal }) => {
                    refused.get_or_insert(Stop::Refused {
                        card,
                        node: refusal.node,
                        why: refusal.why,
                    });
                }
                Err(Rejected::Illegal(_)) => {}
            }
        }
        let Some(((first, did), rest)) = legal.split_first() else {
            return Err(refused.unwrap_or(Stop::Diverged(Divergence::Unfound)));
        };
        if !rest.is_empty() {
            let game = self.game().clone();
            self.checkpoints.push(Checkpoint {
                game,
                entry: self.at,
                trace: self.trace.len(),
                resume: self.at,
                others: rest
                    .iter()
                    .map(|(a, d)| Retry::Answer(a.clone(), d.clone()))
                    .collect(),
            });
        }
        self.act(first.clone())?;
        let at = self.at;
        self.did(at, did.clone());
        Ok(())
    }

    /// The payments the solver finds for the spell being cast, best first.
    fn payments(&mut self) -> Result<Vec<Vec<ManaActivation>>, Stop> {
        let game = self.game();
        let Decision::Pay {
            player,
            owed,
            actions,
            ..
        } = game.decision()
        else {
            unreachable!("a cast waits for its payment")
        };
        let pool = *game.player(player).pool();
        Ok(solve(&owed, pool, &actions))
    }

    /// The discard to hand size in cleanup (CR 514.1), of the cards the
    /// log names over its next lines.
    fn discard(&mut self, g: &log::Game, i: usize, p: PlayerId) -> Result<Next, Stop> {
        let turn = self.game().turn();
        let count = loop {
            if self.game().turn() != turn {
                return Err(Stop::Diverged(Divergence::NeverOffered {
                    what: "a discard".into(),
                }));
            }
            match self.game().decision() {
                Decision::Discard { player, count, .. } if player == p => break count,
                Decision::Priority { .. } | Decision::Search { .. } => self.pass()?,
                _ => {
                    return Err(Stop::Diverged(Divergence::NeverOffered {
                        what: "a discard".into(),
                    }))
                }
            }
        };
        // The discards and the chat between them.
        let mut names = Vec::new();
        let mut j = i;
        while j < g.entries.len() && names.len() < count {
            match &g.entries[j].event {
                Event::Discards { player, cards } if self.seat(*player) == p => {
                    for c in cards {
                        match c {
                            Object::Card(c) => names.push(c.name().to_string()),
                            _ => {
                                return Err(Stop::Unfollowed {
                                    event: "Discards".into(),
                                    card: None,
                                })
                            }
                        }
                    }
                }
                Event::Chat { .. } | Event::Watching { .. } => {}
                _ => break,
            }
            j += 1;
        }
        if names.len() != count {
            return Err(Stop::Diverged(Divergence::Discards {
                log: names.len(),
                engine: count,
            }));
        }
        let mut chosen = Vec::new();
        for n in &names {
            // A card discarded twice over is two cards of that name.
            let text = self.text(n)?;
            let game = self.game();
            let hand = game.player(p).hand().clone();
            let known = hand.iter().find(|c| {
                !chosen.contains(*c) && game.card(**c).is_some_and(|t| t.name == text.name)
            });
            let c = match known {
                Some(c) => *c,
                None => {
                    let Some(c) = hand
                        .iter()
                        .find(|c| !chosen.contains(*c) && game.card(**c).is_none())
                    else {
                        return Err(Stop::Diverged(Divergence::NotInHand { card: n.clone() }));
                    };
                    game.reveal(*c, text).expect("a hidden card");
                    *c
                }
            };
            chosen.push(c);
        }
        self.act(Answer::Discard(chosen))?;
        for k in i..j {
            self.did(k, "discarded to hand size (CR 514.1)");
        }
        Ok(Next::Line(j))
    }
}

enum Next {
    Line(usize),
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Land,
    Cast,
}

impl Kind {
    fn verb(self) -> &'static str {
        match self {
            Kind::Land => "play",
            Kind::Cast => "cast",
        }
    }
}

/// Activate these mana abilities, then spend from the pool what pays the
/// cost.
fn pay_with(game: &mut Game, payment: &[ManaActivation]) -> Followed {
    let illegal = |a: Answer| Stop::Illegal {
        what: format!("{a:?}"),
    };
    for a in payment {
        game.step(Answer::Pay(Payment::Mana(*a)))
            .map_err(|_| illegal(Answer::Pay(Payment::Mana(*a))))?;
    }
    let Decision::Pay { actions, .. } = game.decision() else {
        unreachable!("still paying")
    };
    let spend = actions
        .iter()
        .find(|a| matches!(a, Payment::Spend(_)))
        .copied()
        .ok_or_else(|| Stop::Illegal {
            what: "a payment the solver found doesn't pay".into(),
        })?;
    game.step(Answer::Pay(spend))
        .map_err(|_| illegal(Answer::Pay(spend)))?;
    Ok(())
}

fn describe(payment: &[ManaActivation]) -> String {
    if payment.is_empty() {
        return "paid from the pool".into();
    }
    let mana: Vec<String> = payment
        .iter()
        .map(|a| format!("#{} for {}", a.source.0, a.adds))
        .collect();
    format!("tapping {}", mana.join(", "))
}

/// How many nodes the payment search visits before it settles for what it
/// has found.
const SEARCH: usize = 20_000;

/// Every set of mana abilities, at most one per source, whose mana with
/// the pool's pays `owed` and none of which is needed, the fewest
/// activations first, then those leaving the most flexible sources
/// untapped. The log never says which lands paid, so the replay tries
/// these in order.
fn solve(owed: &ManaCost, pool: ManaPool, actions: &[Payment]) -> Vec<Vec<ManaActivation>> {
    let mut sources: BTreeMap<ObjectId, Vec<ManaActivation>> = BTreeMap::new();
    for a in actions {
        if let Payment::Mana(m) = a {
            sources.entry(m.source).or_default().push(*m);
        }
    }
    let sources: Vec<Vec<ManaActivation>> = sources.into_values().collect();
    let most = owed.generic
        + owed
            .shards
            .iter()
            .map(|s| match s {
                crate::mana::Shard::TwoOr(_) => 2,
                _ => 1,
            })
            .sum::<u32>();
    struct Search<'a> {
        owed: &'a ManaCost,
        sources: &'a [Vec<ManaActivation>],
        most: u32,
        visited: usize,
        out: Vec<Vec<ManaActivation>>,
    }
    impl Search<'_> {
        fn go(&mut self, i: usize, pool: ManaPool, chosen: &mut Vec<ManaActivation>) {
            self.visited += 1;
            if self.visited > SEARCH {
                return;
            }
            if !self.owed.payments(&pool).is_empty() {
                self.out.push(chosen.clone());
                return;
            }
            if i == self.sources.len() || pool.total() >= self.most {
                return;
            }
            for a in &self.sources[i] {
                let mut more = pool;
                more.add_all(&a.adds);
                chosen.push(*a);
                self.go(i + 1, more, chosen);
                chosen.pop();
            }
            self.go(i + 1, pool, chosen);
        }
    }
    let mut s = Search {
        owed,
        sources: &sources,
        most,
        visited: 0,
        out: Vec::new(),
    };
    s.go(0, pool, &mut Vec::new());
    let options: HashMap<ObjectId, usize> = sources
        .iter()
        .filter_map(|v| Some((v.first()?.source, v.len())))
        .collect();
    let mut out = s.out;
    out.sort_by_key(|p| (p.len(), p.iter().map(|a| options[&a.source]).sum::<usize>()));
    out
}
