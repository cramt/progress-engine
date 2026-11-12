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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

use crate::deck::{CardText, DeckCards, Side};
use crate::game::{
    Action, Activation, Answer, CardId, Decision, Format, Found, Game, Mana, ManaActivation,
    ManaCost, ManaPool, Outcome, Payment, Rejected, Setup, StackObject, Step, Target, HAND_SIZE,
};
use crate::log::{self, Entry, Event, Object};
use crate::lower::CastFrom;
use crate::mana::short;
use crate::model::{Ability, Counter, ObjectId, PlayerId};

/// The deck size assumed: the log never says, and every format these
/// games are played in (constructed, Penny Dreadful) has a 60-card
/// minimum that most decks sit at (CR 100.2a).
pub const DECK_SIZE: usize = 60;

/// How many earlier choices (payments, searches) the replay may go back
/// and make again when a later spell can't be paid for, per game. It was
/// 64 until a fetchland's find twelve lines before an unpayable Mana Leak
/// sat behind more later choices than that (game 933380242).
const BACKTRACKS: usize = 512;

/// One game replayed: what the engine did for each line, how it ended,
/// and the engine's game where it ended (`None` if it was never set up).
#[derive(Clone)]
pub struct Replay {
    pub trace: Vec<Traced>,
    pub end: End,
    pub game: Option<Game>,
    /// The lands a search found that the log never names anywhere in the
    /// match, named as a hypothesis (`UnseenLand`), which the replay as it
    /// ended relies on.
    pub unseen: Vec<String>,
}

/// A real land the log may never name, which a search the log doesn't
/// show the result of may find as a hypothesis: one legal in the game's
/// format, and the colours of mana it makes, W U B R G, to try first the
/// lands making the colours the player's spells need.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnseenLand {
    pub name: String,
    pub makes: [bool; 5],
}

/// How many lands the log never names a search may try, best first, before
/// it gives up on the hypothesis.
const UNSEEN_TRIES: usize = 8;

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
    /// The engine put a triggered ability on the stack that the log never
    /// shows.
    UnloggedTrigger { card: String },
    /// The log shows a triggered ability the engine never triggered.
    Untriggered { card: String },
    /// A revealed hand has a different number of cards than the engine's.
    HandSize { log: usize, engine: usize },
    /// The log scries a different number of cards than the engine looks at.
    Scry { log: usize, engine: usize },
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
                | Divergence::Targets { card }
                | Divergence::UnloggedTrigger { card }
                | Divergence::Untriggered { card },
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
    unseen: &[UnseenLand],
) -> Replay {
    let mentioned = m
        .games
        .iter()
        .flat_map(|g| &g.entries)
        .flat_map(|e| e.event.cards())
        .map(|c| c.name().to_string())
        .collect();
    let mut r = Driver {
        cards,
        unseen_lands: unseen,
        mentioned,
        alternatives: HashMap::new(),
        unseen: Vec::new(),
        activated: (0, Vec::new()),
        pick: None,
        aim_pick: None,
        early: None,
        late: None,
        counted: BTreeMap::new(),
        named: HashMap::new(),
        trace: Vec::new(),
        seats: Vec::new(),
        game: None,
        drawn: 0,
        checkpoints: Vec::new(),
        at: 0,
        lands: Vec::new(),
        demand: Vec::new(),
        matched: BTreeSet::new(),
        consumed: BTreeSet::new(),
        bottomed: (0, 0),
        created: (0, 0),
        shuffled: (0, 0),
        discarded: (0, 0),
        returned: (0, 0),
        transformed: (0, 0),
        face_down: BTreeMap::new(),
        entries: g.entries.clone(),
        respond: None,
    };
    let end = r.run(m, g);
    Replay {
        trace: r.trace,
        end,
        game: r.game,
        unseen: r.unseen,
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
    /// What the replay knew of the log there.
    knew: Knew,
    /// What later stop is reason to come back here.
    back: Back,
}

/// The stops a choice the log doesn't show can cause, so the stops that
/// are reason to go back and make it otherwise.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Back {
    /// A payment or a search: a later spell or play it leaves impossible.
    Unplayable,
    /// A "may" the log shows no answer to, whose answer is known only
    /// from what follows: any line that can't be followed.
    Any,
    /// A spell cast after what is on the stack resolves, or in response
    /// to it: a trigger the log does or doesn't show.
    Triggers,
}

/// The replay's own record of the lines it has matched, put back with the
/// game when it goes back to a checkpoint.
#[derive(Clone)]
struct Knew {
    drawn: u32,
    matched: BTreeSet<u32>,
    consumed: BTreeSet<usize>,
    bottomed: (u32, u32),
    created: (u32, u32),
    shuffled: (u32, u32),
    discarded: (u32, u32),
    returned: (u32, u32),
    transformed: (u32, u32),
    respond: Option<usize>,
    unseen: Vec<String>,
    activated: (u32, Vec<ObjectId>),
    pick: Option<(usize, usize)>,
    aim_pick: Option<(usize, usize)>,
    early: Option<usize>,
    late: Option<usize>,
    counted: BTreeMap<(ObjectId, Counter), u32>,
}

#[derive(Clone)]
enum Retry {
    Pay(Vec<ManaActivation>),
    Answer(Answer, String),
    /// Follow the line again, casting its spell in response to what is on
    /// the stack rather than after it resolves.
    Respond,
    /// Follow the line again, activating the ability of this one of the
    /// permanents of its name (`which_source`).
    Pick(usize),
    /// Follow the line again, its spell targeting this one of the
    /// permanents of the name it targets (`aim`).
    Aim(usize),
    /// Follow the line again, casting its spell in its caster's beginning
    /// phase rather than their main phase.
    Early,
    /// Follow the line again, casting its spell in its caster's postcombat
    /// main phase rather than in combat.
    Late,
    /// Go on from the checkpoint's game as it is, the choice made there
    /// already: what it was.
    Take(String),
}

struct Driver<'a> {
    cards: &'a mut dyn FnMut(&str) -> Option<CardText>,
    /// The lands a search may find that the log never names.
    unseen_lands: &'a [UnseenLand],
    /// Every card name the match's log names.
    mentioned: HashSet<String>,
    /// The zones each card the log casts may be cast from by an
    /// alternative cost, as read.
    alternatives: HashMap<String, Vec<CastFrom>>,
    /// The lands the log never names that a search has found so far.
    unseen: Vec<String>,
    /// The turn, and the permanents the replay activated an ability of in
    /// it.
    activated: (u32, Vec<ObjectId>),
    /// The line, and which of the permanents of its name to activate, when
    /// it is followed again (`Retry::Pick`).
    pick: Option<(usize, usize)>,
    /// The line, and which of the permanents of a name its spell targets,
    /// when it is followed again (`Retry::Aim`).
    aim_pick: Option<(usize, usize)>,
    /// The line whose spell is cast in its caster's beginning phase
    /// (`Retry::Early`).
    early: Option<usize>,
    /// The line whose spell is cast in its caster's postcombat main phase
    /// rather than in combat (`Retry::Late`).
    late: Option<usize>,
    /// The counters the log has shown put on each permanent, by kind.
    counted: BTreeMap<(ObjectId, Counter), u32>,
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
    /// The triggered abilities on the stack a log line has shown, by their
    /// `seq`.
    matched: BTreeSet<u32>,
    /// The lines a choice was answered from before the replay reached
    /// them: "chooses to use", "declines to use", a card revealed.
    consumed: BTreeSet<usize>,
    /// The turn, and how many cards the log has shown going to the bottom
    /// of a library by an instruction in it.
    bottomed: (u32, u32),
    /// The turn, and how many tokens the log has shown created in it.
    created: (u32, u32),
    /// The turn, and how many shuffles by an instruction the log has
    /// shown in it.
    shuffled: (u32, u32),
    /// The turn, and how many cards the log has shown discarded in it by
    /// an instruction with no choice in it.
    discarded: (u32, u32),
    /// The turn, and how many permanents the log has shown returned to a
    /// hand in it.
    returned: (u32, u32),
    /// The turn, and how many permanents the log has shown transform in
    /// it.
    transformed: (u32, u32),
    /// The card cast as each "face-down creature #N", by N. Following the
    /// cast line again after going back to before it casts it again, so
    /// this is never put back.
    face_down: BTreeMap<u32, CardId>,
    /// The game's lines, for the choices a later line shows (a trigger's
    /// targets, the order triggers go on the stack).
    entries: Vec<Entry>,
    /// The line whose spell is cast in response to what is on the stack,
    /// gone back to when casting it after what it answers can't be
    /// followed.
    respond: Option<usize>,
}

type Followed = Result<(), Stop>;

impl Driver<'_> {
    fn game(&mut self) -> &mut Game {
        self.game.as_mut().expect("set up")
    }

    fn knew(&self) -> Knew {
        Knew {
            drawn: self.drawn,
            matched: self.matched.clone(),
            consumed: self.consumed.clone(),
            bottomed: self.bottomed,
            created: self.created,
            shuffled: self.shuffled,
            discarded: self.discarded,
            returned: self.returned,
            transformed: self.transformed,
            respond: self.respond,
            unseen: self.unseen.clone(),
            activated: self.activated.clone(),
            pick: self.pick,
            aim_pick: self.aim_pick,
            early: self.early,
            late: self.late,
            counted: self.counted.clone(),
        }
    }

    /// Put back what the replay knew of the log, with the game it knew it
    /// of.
    fn restore(&mut self, knew: Knew) {
        self.drawn = knew.drawn;
        self.matched = knew.matched;
        self.consumed = knew.consumed;
        self.bottomed = knew.bottomed;
        self.shuffled = knew.shuffled;
        self.created = knew.created;
        self.discarded = knew.discarded;
        self.returned = knew.returned;
        self.transformed = knew.transformed;
        self.respond = knew.respond;
        self.unseen = knew.unseen;
        self.activated = knew.activated;
        self.pick = knew.pick;
        self.aim_pick = knew.aim_pick;
        self.early = knew.early;
        self.late = knew.late;
        self.counted = knew.counted;
    }

    fn seat(&self, p: log::Player) -> PlayerId {
        self.seats[p.0 as usize]
    }

    /// The log's player in this seat.
    fn player_of(&self, seat: PlayerId) -> Option<log::Player> {
        self.seats
            .iter()
            .position(|s| *s == seat)
            .map(|k| log::Player(k as u8))
    }

    /// The name of the permanent a line names: a token's is the one the
    /// log gives it, "Goblin Token", which the engine names it too (CR
    /// 111.4), and a card's is its card's.
    fn permanent_name(&mut self, c: &log::Card) -> Result<String, Stop> {
        if c.is_token() {
            return Ok(c.name().to_string());
        }
        Ok(self.text(c.name())?.name)
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
            // At the line it couldn't follow: 0, or an opening hand's card.
            return End::Stopped {
                entry: self.at,
                stop,
            };
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
                    // What resolving first would show and the log doesn't:
                    // a trigger, or a choice (Duress's, answered with a
                    // Lightning Bolt).
                    let triggers = matches!(
                        stop,
                        Stop::Diverged(
                            Divergence::UnloggedTrigger { .. }
                                | Divergence::Untriggered { .. }
                                | Divergence::Unlogged { .. }
                        )
                    );
                    let tapped_trigger = matches!(
                        stop,
                        Stop::Diverged(
                            Divergence::UnloggedTrigger { .. } | Divergence::Untriggered { .. }
                        )
                    );
                    // A permanent tapped to pay can't then attack, block or
                    // be tapped again: a play never offered is reason to
                    // go back too.
                    let unpayable = matches!(
                        stop,
                        Stop::Diverged(
                            Divergence::Unpayable { .. }
                                | Divergence::EngineOver
                                | Divergence::NeverOffered { .. }
                                // A target named for what a search found.
                                | Divergence::Targets { .. }
                        )
                    );
                    // Of two stops at one line, a card refused or a line
                    // the replay can't follow yet says more than what
                    // another way of following it ran into: the engine
                    // hasn't disagreed with the log on every way there.
                    let refused = matches!(stop, Stop::Refused { .. } | Stop::Unfollowed { .. });
                    if furthest.as_ref().is_none_or(|f| {
                        i > f.0 || i == f.0 && refused && matches!(f.1, Stop::Diverged(_))
                    }) {
                        furthest = Some((i, stop.clone(), self.trace.clone()));
                    }
                    // Only a choice made earlier can make a later payment
                    // or play impossible, or lose the game early, so only
                    // then is it worth going back.
                    let retry = backtracks < BACKTRACKS;
                    // The latest choice that could have caused this; any
                    // after it are of a game gone back from.
                    let cause = |b: Back| match b {
                        // A land tapped to pay can trigger (City of Brass):
                        // a trigger the log doesn't show, or one it shows
                        // that never came, is reason to pay otherwise.
                        Back::Unplayable => unpayable || tapped_trigger,
                        Back::Any => true,
                        // Or a cost it can't pay once what is on the stack
                        // resolves (Chill's tax on a red spell).
                        Back::Triggers => triggers || unpayable,
                    };
                    while self.checkpoints.last().is_some_and(|cp| !cause(cp.back)) {
                        self.checkpoints.pop();
                    }
                    match self.checkpoints.last_mut().filter(|_| retry) {
                        Some(cp) => {
                            backtracks += 1;
                            let other = cp.others.remove(0);
                            let knew = cp.knew.clone();
                            let mut game = cp.game.clone();
                            let (entry, len, resume) = (cp.entry, cp.trace, cp.resume);
                            if cp.others.is_empty() {
                                self.checkpoints.pop();
                            }
                            self.trace.truncate(len);
                            self.restore(knew);
                            let did = match other {
                                Retry::Respond => {
                                    self.respond = Some(entry);
                                    self.game = Some(game);
                                    i = resume;
                                    continue;
                                }
                                Retry::Early => {
                                    self.early = Some(entry);
                                    self.game = Some(game);
                                    i = resume;
                                    continue;
                                }
                                Retry::Late => {
                                    self.late = Some(entry);
                                    self.game = Some(game);
                                    i = resume;
                                    continue;
                                }
                                Retry::Pick(k) => {
                                    self.pick = Some((entry, k));
                                    self.game = Some(game);
                                    i = resume;
                                    continue;
                                }
                                Retry::Aim(k) => {
                                    self.aim_pick = Some((entry, k));
                                    self.game = Some(game);
                                    i = resume;
                                    continue;
                                }
                                Retry::Take(did) => Ok(did),
                                Retry::Pay(payment) => pay_with(&mut game, &payment)
                                    .map(|()| format!("paid again, {}", describe(&payment))),
                                Retry::Answer(answer, did) => {
                                    self.note_unseen(&answer);
                                    game.step(answer)
                                        .map(|_| format!("chose again: {did}"))
                                        .map_err(|_| Stop::Illegal {
                                            what: "an answer tried before".into(),
                                        })
                                }
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
        let mut begun: [Vec<CardId>; 2] = [Vec::new(), Vec::new()];
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
                    // The hand kept is the opening hand: what the log
                    // puts onto the battlefield from it is named now
                    // (CR 103.6a).
                    if !take {
                        begun[p] = self.name_begun(g, &mut game, player)?;
                    }
                    Answer::Mulligan(take)
                }
                // The cards named stay in the hand.
                Decision::Bottom {
                    player,
                    count,
                    hand,
                } => Answer::Bottom(
                    hand.iter()
                        .copied()
                        .filter(|c| !begun[player.0 as usize].contains(c))
                        .take(count)
                        .collect(),
                ),
                Decision::OpeningHand { player, .. } => {
                    let named: Vec<CardId> = game
                        .player(player)
                        .hand()
                        .iter()
                        .copied()
                        .filter(|c| begun[player.0 as usize].contains(c))
                        .collect();
                    Answer::Begin(named)
                }
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

    /// The cards the log puts onto the battlefield from `p`'s opening hand
    /// or reveals from it before the first turn (CR 103.6a–b), named from
    /// its hidden cards; one the engine can't run is refused.
    fn name_begun(
        &mut self,
        g: &log::Game,
        game: &mut Game,
        p: PlayerId,
    ) -> Result<Vec<CardId>, Stop> {
        let names: Vec<(usize, String)> = g
            .entries
            .iter()
            .enumerate()
            .take_while(|(_, e)| e.turn.is_none())
            .filter_map(|(k, e)| match &e.event {
                Event::PutsOntoBattlefield {
                    player,
                    card,
                    attacking: None,
                } if self.seat(*player) == p => Some((k, card.name().to_string())),
                Event::Reveals {
                    player,
                    cards,
                    how: log::Reveal::OpeningHand,
                } if self.seat(*player) == p => match cards.as_slice() {
                    [card] => Some((k, card.name().to_string())),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let mut chosen = Vec::new();
        for (k, n) in names {
            self.at = k;
            let text = self.text(&n)?;
            let hand = game.player(p).hand().clone();
            let c = match hand.iter().find(|c| {
                !chosen.contains(*c) && game.card(**c).is_some_and(|t| t.name == text.name)
            }) {
                Some(c) => *c,
                None => {
                    let Some(c) = hand.iter().find(|c| game.card(**c).is_none()) else {
                        return Err(Stop::Diverged(Divergence::NotInHand { card: n }));
                    };
                    game.reveal(*c, text).expect("a hidden card");
                    *c
                }
            };
            if let Some(r) = game.refusal(c) {
                return Err(Stop::Refused {
                    card: n,
                    node: r.node,
                    why: r.why,
                });
            }
            chosen.push(c);
        }
        self.at = 0;
        Ok(chosen)
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
            PutsOntoBattlefield { .. } if entry.turn.is_none() => self.did(
                i,
                "begun on the battlefield from the opening hand (CR 103.6a)",
            ),
            Reveals {
                cards,
                how: log::Reveal::OpeningHand,
                ..
            } if entry.turn.is_none() && cards.len() == 1 => {
                self.did(i, "revealed from the opening hand (CR 103.6b)")
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
                // CR 614.10: a permanent of theirs says to skip it
                // (Solitary Confinement).
                if game.active() == seat && game.skips_draw_step(seat) {
                    self.did(i, "skips their draw step, as a permanent says (CR 614.10)");
                    return Ok(Next::Line(i + 1));
                }
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
                let play = self.offered(p, card.name(), Kind::Land, false, false)?;
                self.act(Answer::Act(play))?;
                self.did(i, format!("played {}", card.name()));
            }
            Casts(cast) => {
                let Object::Card(card) = &cast.card else {
                    return Err(unfollowed(None));
                };
                // Flashback, suspend, kicker, an alternative cost: the line
                // shows which, though older logs print nothing for
                // Fireblast's sacrifice. Of these, an alternative cost from
                // a hand or flashback from a graveyard is followed.
                let other = || Stop::Unfollowed {
                    event: "Casts other than from hand for its mana cost".into(),
                    card: Some(card.name().to_string()),
                };
                // "with kicker", "(with kicker) from the graveyard": paid
                // beside whichever cost the rest of the line says.
                let (via, kicked) = match cast.via.as_deref() {
                    Some(v) if v.contains("with kicker") => {
                        let rest =
                            v.replacen("(with kicker)", "", 1)
                                .replacen("with kicker", "", 1);
                        let rest = rest.trim().to_string();
                        ((!rest.is_empty()).then_some(rest), true)
                    }
                    v => (v.map(str::to_string), false),
                };
                let text = self.text(card.name())?;
                let zones = self
                    .alternatives
                    .entry(text.name.clone())
                    .or_insert_with(|| Game::alternatives_of(&text))
                    .clone();
                let routes = match via.as_deref() {
                    // Most logs print nothing for Gush's alternative cost,
                    // but a line beside the cast shows what it returned;
                    // Steam Clean's return line is its own instruction.
                    None if zones.contains(&CastFrom::Hand)
                        && self.returned_for(i, cast.player, card.name()).is_some() =>
                    {
                        vec![Route::Alternative(CastFrom::Hand)]
                    }
                    None => vec![Route::ManaCost, Route::Alternative(CastFrom::Hand)],
                    Some(v)
                        if v == "from the graveyard"
                            || v.starts_with("with Flashback")
                                && v.ends_with("from the graveyard") =>
                    {
                        vec![Route::Alternative(CastFrom::Graveyard)]
                    }
                    Some(v) if v.starts_with("by sacrificing ") || v.starts_with("by return ") => {
                        vec![Route::Alternative(CastFrom::Hand)]
                    }
                    Some(_) => return Err(other()),
                };
                let p = self.seat(cast.player);
                // A spell targeting a spell on the stack, or a permanent a
                // spell on the stack targets, answers it: it is cast
                // before what it answers resolves.
                let on_stack = cast.targets.iter().any(|a| self.targets_stack(&a.target));
                // "targeting Triggered ability from Goblin Lackey": an
                // ability on the stack.
                let at_ability = cast.targets.iter().any(|a| {
                    matches!(&a.target, log::Target::Named(n)
                        if n.starts_with("Triggered ability") || n.starts_with("Activated ability"))
                });
                let answers = cast
                    .targets
                    .iter()
                    .any(|a| self.targeted_on_stack(&a.target))
                    || at_ability
                    || self.respond == Some(i);
                // The log never says it was cast in response: it may have
                // been, if what follows says so (a trigger the spell on
                // the stack resolving first would have caused).
                if !answers && !self.game().stack().is_empty() {
                    let game = self.game().clone();
                    self.checkpoints.push(Checkpoint {
                        game,
                        entry: i,
                        trace: self.trace.len(),
                        resume: i,
                        others: vec![Retry::Respond],
                        knew: self.knew(),
                        back: Back::Triggers,
                    });
                }
                // A target named by a name two permanents share is either:
                // nothing the log says tells them apart (Naturalize on one
                // of two Mox Diamonds), so the other is tried if a later
                // play can't be made.
                let mut alike = 0;
                for a in &cast.targets {
                    if let log::Target::Object(Object::Card(c)) = &a.target {
                        let called = Called::Name(self.permanent_name(c)?);
                        let game = self.game.as_ref().expect("set up");
                        let n = game
                            .permanents()
                            .keys()
                            .filter(|id| is_called(game, id, &called))
                            .count();
                        alike = alike.max(n);
                    }
                }
                if alike > 1 && self.aim_pick.is_none_or(|(at, _)| at != i) {
                    let game = self.game().clone();
                    self.checkpoints.push(Checkpoint {
                        game,
                        entry: i,
                        trace: self.trace.len(),
                        resume: i,
                        others: (1..alike).map(Retry::Aim).collect(),
                        knew: self.knew(),
                        back: Back::Unplayable,
                    });
                }
                // The first route that can be paid for; a line that names
                // none is cast for its mana cost if it can be, and for an
                // alternative cost the card has otherwise.
                let routes: Vec<Route> = routes
                    .into_iter()
                    .filter(|r| match r {
                        Route::ManaCost => true,
                        Route::Alternative(from) => zones.contains(from),
                    })
                    .collect();
                let Some((last, before)) = routes.split_last() else {
                    return Err(other());
                };
                let mut paid = None;
                for route in before {
                    let saved = (
                        self.game.clone(),
                        self.trace.len(),
                        self.checkpoints.len(),
                        self.knew(),
                    );
                    let tried =
                        self.cast_by(i, cast, card.name(), p, *route, kicked, on_stack, answers);
                    match tried {
                        Ok(how) => {
                            paid = Some(how);
                            break;
                        }
                        Err(Stop::Diverged(Divergence::Unpayable { .. })) => {
                            let (game, trace, checkpoints, knew) = saved;
                            self.game = game;
                            self.trace.truncate(trace);
                            self.checkpoints.truncate(checkpoints);
                            self.restore(knew);
                        }
                        Err(stop) => return Err(stop),
                    }
                }
                let paid = match paid {
                    Some(paid) => paid,
                    None => {
                        self.cast_by(i, cast, card.name(), p, *last, kicked, on_stack, answers)?
                    }
                };
                self.did(i, format!("cast {}, {paid}", card.name()));
            }
            CastsFaceDown { player, .. } => {
                let p = self.seat(*player);
                let did = self.cast_face_down(i, p)?;
                self.did(i, did);
            }
            RevealsHidden {
                object: log::Hidden::FaceDown(n),
                card,
                ..
            } => {
                let did = self.reveal_face_down(*n, card.name())?;
                self.did(i, did);
            }
            TurnsFaceUp {
                player,
                object: log::Hidden::FaceDown(n),
            } => {
                let p = self.seat(*player);
                let did = self.turn_face_up(i, p, *n)?;
                self.did(i, did);
            }
            Discards { player, cards } => {
                let p = self.seat(*player);
                // "discards [X]." then "cycles [X].": the discard is the
                // cost of its cycling (CR 702.29a).
                if let [Object::Card(c)] = cards.as_slice() {
                    let cycles = self.entries[i + 1..]
                        .iter()
                        .position(|e| !matches!(e.event, Chat { .. } | Watching { .. }))
                        .map(|k| i + 1 + k)
                        .filter(|j| {
                            matches!(&self.entries[*j].event,
                                Cycles { player: q, card } if q == player && card.name() == c.name())
                        });
                    if let Some(j) = cycles {
                        // A payment gone back to goes on after both lines.
                        self.cycle(p, c.name(), j)?;
                        for k in i..=j {
                            self.did(k, format!("cycled {} (CR 702.29a)", c.name()));
                        }
                        return Ok(Next::Line(j + 1));
                    }
                    // "discards [Masticore]." then "activates an ability of
                    // [Survival of the Fittest]": the discard is that
                    // ability's cost, which the engine doesn't pay yet, and
                    // never what is resolving.
                    let next = self.entries[i + 1..]
                        .iter()
                        .find(|e| !matches!(e.event, Chat { .. } | Watching { .. }))
                        .and_then(|e| match &e.event {
                            Activates(a) if a.player == *player => match &a.card {
                                Object::Card(x) => Some(x.name().to_string()),
                                _ => None,
                            },
                            _ => None,
                        });
                    let costs_a_discard = |t: &CardText| {
                        matches!(&t.oracle, crate::deck::Oracle::OneFace(_))
                            && t.rules_text().lines().any(|l| {
                                l.split_once(':')
                                    .is_some_and(|(cost, _)| cost.contains("Discard"))
                            })
                    };
                    let paid_by = match next {
                        Some(n) => self.text(&n).is_ok_and(|t| costs_a_discard(&t)),
                        None => false,
                    };
                    if paid_by {
                        return Err(Stop::Unfollowed {
                            event: "Discards as an activated ability's cost".into(),
                            card: Some(c.name().to_string()),
                        });
                    }
                }
                // A discard instead of a permanent entering, read ahead
                // as it entered (`discard_to_enter`).
                // Or paying an "unless" (`discard_unless`), which says so.
                if self.until_consumed(i)? {
                    return Ok(Next::Line(i + 1));
                }
                // A resolving spell or ability's: a card chosen from a
                // revealed hand, or cards the player chooses.
                match self.until_choice()? {
                    Decision::ChooseFromHand { whose, .. } if whose == p => {
                        return self.hand_choice(g, i, p)
                    }
                    Decision::Discard { player, .. } if player == p => {
                        return self.discard(g, i, p)
                    }
                    _ => {}
                }
                // Discarded by an instruction with no choice in it, which
                // the engine must have discarded this turn.
                let turn = self.game().turn();
                let seen = if self.discarded.0 == turn {
                    self.discarded.1
                } else {
                    0
                } + cards.len() as u32;
                if seen <= self.game().discarded() {
                    self.discarded = (turn, seen);
                    self.did(i, "discarded by the instruction (CR 701.9a)");
                    return Ok(Next::Line(i + 1));
                }
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
            Wins(who) | Loses(who) => {
                let won = match &entry.event {
                    Wins(_) => self.seat(*who),
                    _ => PlayerId(1 - self.seat(*who).0),
                };
                // The log stops printing as the game ends, so what was on
                // the stack and ended it is never shown resolving (the
                // last of two Razorkin Needlehead triggers): it resolves
                // now, down to the game's end or an empty stack, and an
                // attack the log shows deals its combat damage.
                let turn = self.game().turn();
                loop {
                    let game = self.game();
                    let fighting =
                        !game.attacks().is_empty() && game.step_now() < Step::EndOfCombat;
                    match game.decision() {
                        Decision::Over(Outcome::Won(p)) if p == won => {
                            self.did(i, "the game ends by the rules (CR 104.2a)");
                            return Ok(Next::Done);
                        }
                        Decision::Over(_) => break,
                        Decision::Priority { .. }
                            if game.stack().is_empty() && !fighting || game.turn() != turn =>
                        {
                            break
                        }
                        _ => {
                            if self.pass().is_err() {
                                break;
                            }
                        }
                    }
                }
                return Err(Stop::Diverged(Divergence::LogOver));
            }
            Activates(a) => {
                let Object::Card(card) = &a.card else {
                    return Err(unfollowed(None));
                };
                let p = self.seat(a.player);
                // An ability targeting a spell, or of a permanent a spell
                // or ability on the stack targets, answers it: activated
                // before it resolves (Mogg Fanatic sacrificed to Seal of
                // Fire's damage).
                let game = self.game.as_ref().expect("set up");
                let at_spells = game.permanents().values().any(|perm| {
                    game.card(perm.card).is_some_and(|t| t.name == card.name())
                        && game.activation_targets_spells(perm.card)
                });
                let on_stack = at_spells && a.targets.iter().any(|t| self.targets_stack(&t.target))
                    || self.targeted_on_stack(&log::Target::Object(a.card.clone()));
                // The ability's text, which the log may print after its
                // last target, where it reads as that target's note.
                let said = std::iter::once(a.ability.as_str())
                    .chain(a.targets.iter().filter_map(|t| t.note.as_deref()))
                    .collect::<Vec<_>>()
                    .join(" ");
                let activation = self.activation(p, card.name(), &said, on_stack)?;
                self.act(Answer::Act(Action::Activate(activation)))?;
                let paid = match self.game().decision() {
                    Decision::Targets { .. } | Decision::Pay { .. } => {
                        format!(", {}", self.aim_and_pay(i, &a.targets, card.name())?)
                    }
                    _ if !a.targets.is_empty() => {
                        return Err(Stop::Diverged(Divergence::Targets {
                            card: card.name().to_string(),
                        }))
                    }
                    _ => String::new(),
                };
                self.did(i, format!("activated {}'s ability{paid}", card.name()));
            }
            // "Search your library for an enchantment card, reveal it":
            // the card the search finds, which the line names. A search
            // resolving before it that reveals nothing (Viridian
            // Emissary's) is answered as the log never shows it.
            Reveals {
                player,
                cards,
                how: log::Reveal::Plain,
            } if cards.len() == 1 => {
                let p = self.seat(*player);
                let turn = self.game().turn();
                let known = loop {
                    // A card looked at and revealed by a resolving
                    // instruction (Delver of Secrets's), named as the
                    // engine asks (`reveal`), which read this line ahead.
                    if self.consumed.contains(&i) {
                        break None;
                    }
                    let game = self.game();
                    match game.decision() {
                        Decision::Search {
                            player,
                            known,
                            reveals: true,
                            ..
                        } if player == p => break Some(known),
                        Decision::May { card, .. } if game.reveals_looked_at(card) => {
                            self.pass()?
                        }
                        Decision::Reveal { .. } if game.looked_at_revealed() => self.pass()?,
                        Decision::Search {
                            player,
                            filter,
                            known,
                            hidden,
                            reveals: false,
                        } => self.search(player, &filter, known, hidden)?,
                        Decision::Priority { .. }
                            if !game.stack().is_empty() && game.turn() == turn =>
                        {
                            self.pass()?
                        }
                        _ => return Err(unfollowed(Some(cards[0].name()))),
                    }
                };
                match known {
                    None => self.did(i, "the card revealed (CR 701.20a)"),
                    Some(known) => {
                        let text = self.text(cards[0].name())?;
                        let game = self.game();
                        let library = game.player(p).library().clone();
                        let found = match known
                            .iter()
                            .find(|c| game.card(**c).is_some_and(|t| t.name == text.name))
                        {
                            Some(c) => Found::Card(*c),
                            None => match library.iter().find(|c| game.card(**c).is_none()) {
                                Some(c) => Found::Hidden(*c, Box::new(text)),
                                None => return Err(Stop::Diverged(Divergence::Unfound)),
                            },
                        };
                        self.act(Answer::Find(found))?;
                        self.did(
                            i,
                            format!("found {}, revealed (CR 701.23a)", cards[0].name()),
                        );
                    }
                }
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
            Attacked {
                defender: log::Defender::Player(d),
                attackers: log::Attackers::Seen(attackers),
            } => {
                let defender = self.seat(*d);
                let mut names = Vec::new();
                for a in attackers {
                    names.push(self.called(a)?);
                }
                let turn = self.game().turn();
                let can = loop {
                    let game = self.game();
                    if game.turn() != turn || game.step_now() > Step::DeclareAttackers {
                        return Err(Stop::Diverged(Divergence::NeverOffered {
                            what: "an attack".into(),
                        }));
                    }
                    match game.decision() {
                        Decision::Attackers { can, .. } => break can,
                        Decision::Priority { .. } => self.act(Answer::Act(Action::Pass))?,
                        _ => self.pass()?,
                    }
                };
                let game = self.game.as_ref().expect("set up");
                let mut declared: Vec<(ObjectId, PlayerId)> = Vec::new();
                for n in &names {
                    let Some(a) = can.iter().find(|id| {
                        !declared.iter().any(|(x, _)| x == *id) && is_called(game, id, n)
                    }) else {
                        return Err(Stop::Diverged(Divergence::NeverOffered {
                            what: format!("attack with {n}"),
                        }));
                    };
                    declared.push((*a, defender));
                }
                self.act(Answer::Attack(declared))?;
                let names: Vec<String> = names.iter().map(ToString::to_string).collect();
                self.did(i, format!("attacked with {}", names.join(", ")));
            }
            Blocks {
                blocker: b @ (Object::Card(_) | Object::Hidden(_)),
                attacker: a @ (Object::Card(_) | Object::Hidden(_)),
            } => {
                let turn = self.game().turn();
                loop {
                    let game = self.game();
                    if let Decision::Blockers { .. } = game.decision() {
                        self.pass()?;
                        break;
                    }
                    // Declared already, by the line before: a second block
                    // line passes no priority, or combat damage would be
                    // dealt before it is checked.
                    if game.turn() != turn || game.step_now() >= Step::DeclareBlockers {
                        break;
                    }
                    self.pass()?;
                }
                let (bn, an) = (self.called(b)?, self.called(a)?);
                let game = self.game();
                if !game
                    .blocks()
                    .iter()
                    .any(|(x, y)| is_called(game, x, &bn) && is_called(game, y, &an))
                {
                    return Err(Stop::Diverged(Divergence::NeverOffered {
                        what: format!("{bn} blocking {an}"),
                    }));
                }
                self.did(i, format!("{bn} blocks {an}"));
            }
            // A creature type chosen as a permanent enters, as the engine
            // asks (`choose_type`), which reads this line ahead.
            Chooses {
                choice: log::Choice::Word(w),
                by: Some(_),
                ..
            } => {
                if !self.until_consumed(i)? {
                    return Err(unfollowed(None));
                }
                self.did(i, format!("chose {w} (CR 614.12a)"));
            }
            // The card named for a resolving instruction (CR 201.4), named
            // as the engine asks (`choose_name`), which reads this line
            // ahead.
            Names { name, .. } => {
                if !self.until_consumed(i)? {
                    return Err(Stop::Unfollowed {
                        event: "Names".into(),
                        card: Some(name.clone()),
                    });
                }
                self.did(i, format!("named {name} (CR 201.4)"));
            }
            // A counter taken off as a mana ability's cost (Gemstone Mine),
            // which the log prints as it is activated.
            RemovesCounters { player, counters } => {
                let p = self.seat(*player);
                if !self.counter_cost_mana(p, counters, true)? {
                    return Err(unfollowed(None));
                }
                self.did(i, "a mana ability's counter removed (CR 602.2b)");
            }
            // Echo paid, as the engine asks (`pay_unless`), which reads
            // this line ahead.
            PaysEcho { card, .. } => {
                if !self.until_consumed(i)? {
                    return Err(unfollowed(Some(card.name())));
                }
                self.did(i, format!("paid {}'s echo (CR 702.30a)", card.name()));
            }
            UsesAbility { card, .. } => {
                // The "may" it answers is answered as the engine asks
                // (`may`), which reads this line ahead.
                if !self.until_consumed(i)? {
                    return Err(unfollowed(Some(card.name())));
                }
                self.did(i, format!("answered {}'s \"may\" (CR 608.2d)", card.name()));
            }
            // A card revealed from a library by a resolving instruction,
            // named as the engine asks (`reveal`), which reads this line
            // ahead.
            Reveals {
                how: log::Reveal::With(log::Source::Card(by)),
                cards,
                ..
            } => {
                if !self.until_consumed(i)? {
                    let card = cards.first().map(|c| c.name().to_string());
                    return Err(Stop::Unfollowed {
                        event: format!("Reveals with {}", by.name()),
                        card,
                    });
                }
                self.did(i, "the card revealed (CR 701.20a)");
            }
            // A library an instruction shuffled ("You may have that player
            // shuffle"), which the engine must have shuffled this turn.
            Shuffles(_) => {
                self.until_choice()?;
                let turn = self.game().turn();
                let seen = if self.shuffled.0 == turn {
                    self.shuffled.1
                } else {
                    0
                } + 1;
                if seen > self.game().shuffled() {
                    return Err(unfollowed(None));
                }
                self.shuffled = (turn, seen);
                self.did(i, "shuffled by the instruction (CR 701.24a)");
            }
            // Sylvan Library's "pay 4 life or put the card on top of your
            // library", one line for each card, read ahead as it is asked.
            KeepsCards { .. }
            | PutsInLibrary {
                cards: log::Moved::Unseen(1),
                end: log::End::Top,
                by: None,
                ..
            } => {
                if !self.until_consumed(i)? {
                    return Err(unfollowed(None));
                }
                self.did(i, "answered a \"pay life or\" (CR 119.4)");
            }
            // Permanents returned to a hand by a cost or an instruction,
            // which the engine must have returned this turn, each in its
            // owner's hand now.
            ReturnsToHand {
                player,
                cards,
                with: log::Source::Card(c),
            } if self
                .entries
                .get(i + 1..)
                .and_then(|rest| {
                    rest.iter()
                        .position(|e| !matches!(e.event, Chat { .. } | Watching { .. }))
                })
                .is_some_and(|k| {
                    matches!(&self.entries[i + 1 + k].event,
                        Casts(cast) if cast.player == *player
                            && cast.via.as_deref().is_none_or(|v| v.starts_with("by return "))
                            && matches!(&cast.card, Object::Card(d) if d.name() == c.name()))
                }) =>
            {
                // The cost of the spell the next line casts, paid as it is
                // cast (Gush).
                let _ = cards;
                self.did(i, "returned to pay for the spell the next line casts");
            }
            // "[Delver of Secrets] transforms into [Insectile
            // Aberration].": what resolves until the engine has transformed
            // one more permanent this turn, which is the one the line
            // names, its other face up (CR 701.27a).
            Transforms { to, .. } => {
                let turn = self.game().turn();
                let seen = if self.transformed.0 == turn {
                    self.transformed.1
                } else {
                    0
                } + 1;
                while self.game().transformed() < seen {
                    let game = self.game();
                    let passing = game.turn() == turn
                        && match game.decision() {
                            Decision::Priority { .. } => !game.stack().is_empty(),
                            Decision::May { .. }
                            | Decision::Reveal { .. }
                            | Decision::Targets { .. }
                            | Decision::Order { .. }
                            | Decision::PayUnless { .. } => true,
                            _ => false,
                        };
                    if !passing {
                        return Err(unfollowed(Some(to.name())));
                    }
                    self.pass()?;
                }
                let game = self.game();
                if !game
                    .permanents()
                    .values()
                    .any(|perm| game.face_name(perm.card) == Some(to.name()))
                {
                    return Err(unfollowed(Some(to.name())));
                }
                self.transformed = (turn, seen);
                self.did(i, format!("transformed into {} (CR 701.27a)", to.name()));
            }
            ReturnsToHand { cards, .. } if !cards.is_empty() => {
                self.until_choice()?;
                let turn = self.game().turn();
                let seen = if self.returned.0 == turn {
                    self.returned.1
                } else {
                    0
                } + cards.len() as u32;
                let game = self.game();
                let in_hands = cards.iter().all(|c| {
                    let wanted = cards.iter().filter(|d| d.name() == c.name()).count();
                    game.players()
                        .iter()
                        .map(|p| {
                            p.hand()
                                .iter()
                                .filter(|h| game.card(**h).is_some_and(|t| t.is_called(c.name())))
                                .count()
                        })
                        .sum::<usize>()
                        >= wanted
                });
                if seen > game.returned() || !in_hands {
                    return Err(unfollowed(cards.first().map(|c| c.name())));
                }
                self.returned = (turn, seen);
                self.did(i, format!("{} returned to a hand", cards.len()));
            }
            // Cards an instruction put on the bottom, which the engine
            // must have put there this turn.
            PutsInLibrary {
                cards: log::Moved::Unseen(n),
                end: log::End::Bottom,
                by: None,
                ..
            } => {
                self.until_choice()?;
                let turn = self.game().turn();
                let seen = if self.bottomed.0 == turn {
                    self.bottomed.1
                } else {
                    0
                } + n;
                if seen > self.game().bottomed() {
                    return Err(unfollowed(None));
                }
                self.bottomed = (turn, seen);
                self.did(
                    i,
                    format!("{n} put on the bottom by the instruction (CR 401.4)"),
                );
            }
            // What a resolving spell or ability did: checked once what is
            // on the stack this turn resolves, against the permanent of
            // that name with the most of those counters.
            PutsCounters { player, counters } => {
                let p = self.seat(*player);
                // A counter put as a mana ability's cost (Wall of Roots's
                // -0/-1), which the log prints as it is activated: the
                // mana floats until what it pays for is cast.
                if !self.counter_mana(p, counters)? {
                    // What resolves on the stack this turn until the
                    // counters are there, and no further: a second trigger
                    // waiting is the next line's.
                    let turn = self.game().turn();
                    while self.counters_missing(counters)?.is_some() {
                        let game = self.game();
                        let passing = game.turn() == turn
                            && match game.decision() {
                                Decision::Priority { .. } => !game.stack().is_empty(),
                                Decision::May { .. }
                                | Decision::ChooseName { .. }
                                | Decision::ChooseCreatureType { .. }
                                | Decision::ChooseMana { .. }
                                | Decision::PayUnless { .. }
                                | Decision::DiscardUnless { .. }
                                | Decision::SacrificeUnless { .. }
                                | Decision::PayLifeOr { .. }
                                | Decision::Reveal { .. }
                                | Decision::Targets { .. }
                                | Decision::Order { .. }
                                | Decision::Search { .. }
                                | Decision::PayOrEnterTapped { .. } => true,
                                _ => false,
                            };
                        if !passing {
                            break;
                        }
                        self.pass()?;
                    }
                    self.counter_mana(p, counters)?;
                }
                if let Some(what) = self.counters_missing(counters)? {
                    return Err(Stop::Diverged(Divergence::NeverOffered { what }));
                }
                // Those the log has shown now, so the next line's are more.
                for counted in counters {
                    if let Some(key) = self.counters_on_named(counted)? {
                        *self.counted.entry(key).or_default() += counted.count;
                    }
                }
                self.did(i, "the counters are on it");
            }
            // CR 608.2b: a spell whose targets are all illegal doesn't
            // resolve; the engine's leaves the stack as it would resolve.
            RemovedForNoTargets { spell, .. } => {
                let name = match spell {
                    log::Source::Card(c) => c.name().to_string(),
                    log::Source::Named(n) => n.clone(),
                };
                let turn = self.game().turn();
                loop {
                    let game = self.game();
                    let there = game.stack().iter().any(|o| match o {
                        StackObject::Spell { card, .. }
                        | StackObject::Trigger { source: card, .. } => {
                            game.card(*card).is_some_and(|t| t.is_called(&name))
                        }
                        StackObject::Ability { .. } => false,
                    });
                    if !there {
                        break;
                    }
                    if game.turn() != turn {
                        return Err(unfollowed(Some(&name)));
                    }
                    self.pass()?;
                }
                if self
                    .game()
                    .last_resolved()
                    .is_none_or(|c| self.name_of(c) != name)
                {
                    return Err(unfollowed(Some(&name)));
                }
                self.did(
                    i,
                    format!("{name} left the stack, its targets illegal (CR 608.2b)"),
                );
            }
            // Tokens a resolving instruction created, which the engine
            // must have created this turn.
            Creates { count, what, .. } => {
                self.until_choice()?;
                let turn = self.game().turn();
                let seen = if self.created.0 == turn {
                    self.created.1
                } else {
                    0
                } + count;
                if seen > self.game().created() {
                    return Err(Stop::Unfollowed {
                        event: "Creates".into(),
                        card: Some(what.clone()),
                    });
                }
                self.created = (turn, seen);
                self.did(i, format!("{count} {what} created (CR 111.1)"));
            }
            TriggerFailed(c) => {
                self.did(
                    i,
                    format!("{}'s trigger has no legal target (CR 603.3d)", c.name()),
                );
            }
            Trigger(t) => {
                let log::Source::Card(c) = &t.source else {
                    let log::Source::Named(n) = &t.source else {
                        unreachable!()
                    };
                    return Err(unfollowed(Some(n)));
                };
                self.trigger(c.name())?;
                self.did(
                    i,
                    format!("put {}'s triggered ability on the stack", c.name()),
                );
            }
            Draws {
                player,
                count,
                with: Some(source),
            } => {
                let p = self.seat(*player);
                let name = match source {
                    log::Source::Card(c) => c.name().to_string(),
                    log::Source::Named(n) => n.clone(),
                };
                self.drawn_by(p, &name, *count)?;
                self.did(i, format!("drew {count} with {name}"));
            }
            Scry {
                player,
                count,
                top,
                bottom,
            } => {
                let p = self.seat(*player);
                let Decision::Scry { player: q, cards } = self.until_choice()? else {
                    return Err(Stop::Diverged(Divergence::NeverOffered {
                        what: "a scry".into(),
                    }));
                };
                if q != p {
                    return Err(Stop::Diverged(Divergence::NeverOffered {
                        what: "a scry".into(),
                    }));
                }
                if cards.len() != *count as usize || top + bottom != *count {
                    return Err(Stop::Diverged(Divergence::Scry {
                        log: *count as usize,
                        engine: cards.len(),
                    }));
                }
                // The cards looked at are hidden alike, so which went
                // where is all the log has to say.
                let (t, b) = cards.split_at(*top as usize);
                self.act(Answer::Scry {
                    top: t.to_vec(),
                    bottom: b.to_vec(),
                })?;
                self.did(
                    i,
                    format!("scried {top} to the top, {bottom} to the bottom"),
                );
            }
            Reveals {
                player,
                how: log::Reveal::HandTo(_) | log::Reveal::Many { .. },
                ..
            } => {
                // Cards a resolving instruction reveals from a library,
                // read ahead as it asked (`reveal`).
                if self.until_consumed(i)? {
                    self.did(i, "the cards revealed (CR 701.20a)");
                    return Ok(Next::Line(i + 1));
                }
                let p = self.seat(*player);
                match self.until_choice()? {
                    Decision::ChooseFromHand { whose, .. } if whose == p => {
                        return self.hand_choice(g, i, p)
                    }
                    _ => {
                        // A hand an instruction revealed with nothing hidden
                        // in it, so nothing to name: the engine's has as
                        // many cards, with those the instruction discarded
                        // since.
                        let Event::Reveals {
                            cards,
                            how: log::Reveal::HandTo(log::Source::Card(by)),
                            ..
                        } = &entry.event
                        else {
                            return Err(unfollowed(None));
                        };
                        let game = self.game.as_ref().expect("set up");
                        let resolved = game
                            .last_resolved()
                            .is_some_and(|c| game.card(c).is_some_and(|t| t.is_called(by.name())));
                        let turn = game.turn();
                        let unmatched = game.discarded()
                            - if self.discarded.0 == turn {
                                self.discarded.1
                            } else {
                                0
                            };
                        let held = game.player(p).hand().len() + unmatched as usize;
                        if !resolved {
                            return Err(unfollowed(None));
                        }
                        let shown =
                            split_halves_once(cards.iter().map(|c| c.name().to_string()).collect())
                                .len();
                        if held != shown {
                            return Err(Stop::Diverged(Divergence::HandSize {
                                log: shown,
                                engine: held,
                            }));
                        }
                        self.did(i, "the hand revealed, nothing in it hidden (CR 701.20a)");
                    }
                }
            }
            // What a resolving instruction put into a hand: checked once
            // what is on the stack this turn resolves.
            PutsInHand { player, cards, .. } => {
                self.until_choice()?;
                let p = self.seat(*player);
                let game = self.game.as_ref().expect("set up");
                let hand = game.player(p).hand();
                let mut left: Vec<CardId> = hand.iter().copied().collect();
                for c in cards {
                    let Some(k) = left
                        .iter()
                        .position(|x| game.card(*x).is_some_and(|t| t.is_called(c.name())))
                    else {
                        return Err(unfollowed(Some(c.name())));
                    };
                    left.remove(k);
                }
                self.did(i, "put into the hand by the instruction");
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

    /// Whether the log's target is a permanent a spell or ability on the
    /// stack targets.
    fn targeted_on_stack(&self, t: &log::Target) -> bool {
        let called = match t {
            log::Target::Object(Object::Card(c)) => Called::Name(c.name().to_string()),
            log::Target::Object(Object::Hidden(log::Hidden::FaceDown(n))) => {
                match self.face_down.get(n) {
                    Some(c) => Called::FaceDown(*c),
                    None => return false,
                }
            }
            _ => return false,
        };
        let game = self.game.as_ref().expect("set up");
        game.stack().iter().any(|o| {
            let (StackObject::Spell { targets, .. }
            | StackObject::Trigger { targets, .. }
            | StackObject::Ability { targets, .. }) = o;
            targets.iter().any(|t| match t {
                Target::Object(id) => is_called(game, id, &called),
                _ => false,
            })
        })
    }

    /// Whether the log's target is a spell on the stack.
    fn targets_stack(&self, t: &log::Target) -> bool {
        let game = self.game.as_ref().expect("set up");
        if let log::Target::Object(Object::Hidden(log::Hidden::FaceDown(n))) = t {
            return self
                .face_down
                .get(n)
                .is_some_and(|c| game.face_down_on_stack(*c));
        }
        let log::Target::Object(Object::Card(c)) = t else {
            return false;
        };
        game.stack().iter().any(|o| match o {
            StackObject::Spell { card, .. }
            | StackObject::Ability { source: card, .. }
            | StackObject::Trigger { source: card, .. } => {
                game.card(*card).is_some_and(|t| t.is_called(c.name()))
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
            Decision::Priority { .. } => {
                // A triggered ability no line has shown isn't resolved:
                // the log shows each as it goes on the stack.
                let top = self.game().stack().back().cloned();
                if let Some(StackObject::Trigger { source, seq, .. }) = top {
                    if !self.matched.contains(&seq) {
                        let card = self.name_of(source);
                        return Err(Stop::Diverged(Divergence::UnloggedTrigger { card }));
                    }
                }
                self.act(Answer::Act(Action::Pass))
            }
            Decision::Targets { card, slots, .. } => {
                let name = self.name_of(card);
                let aims = self
                    .next_trigger_lines()
                    .into_iter()
                    .find(|t| source_name(t) == Some(name.as_str()))
                    .map(|t| t.targets)
                    .ok_or_else(|| {
                        Stop::Diverged(Divergence::Unlogged {
                            what: format!("the targets of {name}'s triggered ability"),
                        })
                    })?;
                let chosen = self.aim(&aims, &slots, &name)?;
                self.act(Answer::Targets(chosen))
            }
            Decision::Order { triggers, .. } => {
                // The order the log shows them go on the stack.
                let names: Vec<String> = triggers.iter().map(|c| self.name_of(*c)).collect();
                let mut order: Vec<usize> = Vec::new();
                for t in self.next_trigger_lines() {
                    let Some(n) = source_name(&t) else { continue };
                    if let Some(k) = (0..names.len()).find(|k| !order.contains(k) && names[*k] == n)
                    {
                        order.push(k);
                    }
                }
                if order.len() != names.len() {
                    return Err(Stop::Diverged(Divergence::Unlogged {
                        what: "the order triggered abilities go on the stack".into(),
                    }));
                }
                self.act(Answer::Order(order))
            }
            // An empty hand revealed is nothing to choose from, and the
            // log prints nothing for it.
            Decision::ChooseFromHand { hand, .. } if hand.is_empty() => {
                self.act(Answer::Pick(None))
            }
            Decision::ChooseFromHand { .. } => Err(Stop::Diverged(Divergence::Unlogged {
                what: "a card chosen from a revealed hand".into(),
            })),
            // No line this turn shows an attack: none is declared.
            Decision::Attackers { .. } => {
                if self.attack_ahead() {
                    return Err(Stop::Diverged(Divergence::Unlogged {
                        what: "a line before the attack that shows it".into(),
                    }));
                }
                self.act(Answer::Attack(Vec::new()))
            }
            Decision::Blockers { can, attackers, .. } => {
                let blocks = self.blocks_ahead(&can, &attackers)?;
                self.act(Answer::Block(blocks))
            }
            // The log never shows how damage was divided: lethal to each
            // blocker in turn, and the rest to the last, or past them to
            // the player with trample (CR 702.19b).
            Decision::Assign {
                power, to, lethal, ..
            } => {
                let mut left = power;
                let mut amounts: Vec<u32> = lethal
                    .iter()
                    .zip(&to)
                    .map(|(l, t)| match t {
                        Target::Player(_) | Target::Spell(_) => 0,
                        Target::Object(_) => {
                            let n = (*l).min(left);
                            left -= n;
                            n
                        }
                    })
                    .collect();
                if let Some(last) = amounts.last_mut() {
                    *last += left;
                }
                self.act(Answer::Assign(amounts))
            }
            Decision::Scry { .. } => Err(Stop::Diverged(Divergence::Unlogged {
                what: "a scry".into(),
            })),
            Decision::Discard { count, .. } => {
                Err(Stop::Diverged(Divergence::UnloggedDiscard { count }))
            }
            // CR 704.5j: the log never shows which was kept, nor can it
            // tell two of one name apart: the first, and the others gone
            // back to if a later line can't be followed.
            Decision::LegendRule { among, .. } => {
                let game = self.game().clone();
                self.checkpoints.push(Checkpoint {
                    game,
                    entry: self.at,
                    trace: self.trace.len(),
                    resume: self.at,
                    others: among[1..]
                        .iter()
                        .map(|o| Retry::Answer(Answer::Keep(*o), "kept another (CR 704.5j)".into()))
                        .collect(),
                    knew: self.knew(),
                    back: Back::Any,
                });
                self.act(Answer::Keep(among[0]))?;
                let at = self.at;
                self.did(at, "kept one of two legendary permanents (CR 704.5j)");
                Ok(())
            }
            Decision::ReturnCommander { .. } => Err(Stop::Diverged(Divergence::Unlogged {
                what: "a commander's return".into(),
            })),
            Decision::Over(_) => Err(Stop::Diverged(Divergence::EngineOver)),
            Decision::Search {
                player,
                filter,
                known,
                hidden,
                ..
            } => self.search(player, &filter, known, hidden),
            Decision::May { card, .. } => self.may(card),
            Decision::ChooseName { player, card, .. } => self.choose_name(player, card),
            Decision::ChooseCreatureType { player, card } => self.choose_type(player, card),
            Decision::PayUnless {
                player, card, owed, ..
            } => self.pay_unless(player, card, owed),
            Decision::PayLifeOr { player, card, .. } => self.pay_life_or(player, card),
            Decision::ChooseMana {
                player, options, ..
            } => self.choose_mana(player, options),
            Decision::Sacrifice { .. } => self.sacrifice(),
            Decision::ReturnToHand { .. } => self.return_to_hand(),
            Decision::Reveal { player, cards } => self.reveal(player, &cards),
            // Cards looked at are hidden alike to the log: which is chosen,
            // and the order they go back in, is nothing it can tell.
            Decision::ChooseAmong { cards, n, .. } => self.choose_among(cards, n),
            Decision::Arrange { cards, .. } => self.act(Answer::Arrange(cards)),
            Decision::PutFromHand { player, .. } => self.put_from_hand(player),
            Decision::DiscardToEnter { player, .. } => self.discard_to_enter(player),
            Decision::DiscardUnless { player, card, .. } => self.discard_unless(player, card),
            // No log shows one paid: Phyrexian Dreadnought's twelve power
            // sacrificed would be its caster's whole board.
            Decision::SacrificeUnless { card, .. } => {
                self.act(Answer::Sacrifice(Vec::new()))?;
                let name = self.name_of(card);
                let at = self.at;
                self.did(at, format!("sacrificed nothing for {name} (CR 118.12a)"));
                Ok(())
            }
            Decision::PayOrEnterTapped { card, life, .. } => self.pay_or_tap(card, life),
            d @ (Decision::Pay { .. }
            | Decision::Kicker { .. }
            | Decision::Mulligan { .. }
            | Decision::Bottom { .. }
            | Decision::OpeningHand { .. }) => {
                unreachable!("the replay answers {d:?} where it arises")
            }
        }
    }

    /// Whether a line from the one being followed to the end of the turn
    /// shows an attack.
    fn attack_ahead(&self) -> bool {
        // Passing to a new turn's line, the lines after it are that turn's.
        if matches!(self.entries[self.at].event, Event::TurnBegins(_)) {
            return false;
        }
        self.entries
            .iter()
            .skip(self.at + 1)
            .take_while(|e| !matches!(e.event, Event::TurnBegins(_)))
            .any(|e| matches!(e.event, Event::Attacked { .. }))
    }

    /// The blocks the log shows from the line being followed to the end of
    /// the turn, as the defending player's declaration: each blocker a
    /// creature of `can` of
    /// that name, each attacker one of `attackers` of its name, the first
    /// when two share it.
    fn blocks_ahead(
        &mut self,
        can: &[ObjectId],
        attackers: &[ObjectId],
    ) -> Result<Vec<(ObjectId, ObjectId)>, Stop> {
        let lines: Vec<(Object, Object)> = self
            .entries
            .iter()
            .skip(self.at)
            .take_while(|e| !matches!(e.event, Event::TurnBegins(_)))
            .filter_map(|e| match &e.event {
                Event::Blocks {
                    blocker: b @ (Object::Card(_) | Object::Hidden(_)),
                    attacker: a @ (Object::Card(_) | Object::Hidden(_)),
                } => Some((b.clone(), a.clone())),
                _ => None,
            })
            .collect();
        let mut out = Vec::new();
        for (b, a) in lines {
            let (b, a) = (self.called(&b)?, self.called(&a)?);
            let game = self.game.as_ref().expect("set up");
            let named = |id: &ObjectId, n: &Called| is_called(game, id, n);
            let Some(blocker) = can
                .iter()
                .find(|id| named(id, &b) && !out.iter().any(|(x, _)| x == *id))
            else {
                return Err(Stop::Diverged(Divergence::NeverOffered {
                    what: format!("{b} blocking {a}"),
                }));
            };
            // Of two attackers of one name, one not yet blocked first.
            let unblocked = attackers
                .iter()
                .find(|id| named(id, &a) && !out.iter().any(|(_, x)| x == *id));
            let Some(attacker) = unblocked.or_else(|| attackers.iter().find(|id| named(id, &a)))
            else {
                return Err(Stop::Diverged(Divergence::NeverOffered {
                    what: format!("{b} blocking {a}"),
                }));
            };
            out.push((*blocker, *attacker));
        }
        Ok(out)
    }

    /// The name a log gives a card: a two-faced card's by the face up.
    fn name_of(&self, c: CardId) -> String {
        let game = self.game.as_ref().expect("set up");
        game.face_name(c).map(str::to_string).unwrap_or_default()
    }

    /// The triggered abilities the log shows from the line being followed
    /// to the end of the turn.
    fn next_trigger_lines(&self) -> Vec<log::Trigger> {
        self.entries
            .iter()
            .skip(self.at)
            .take_while(|e| !matches!(e.event, Event::TurnBegins(_)) || e.turn.is_none())
            .filter_map(|e| match &e.event {
                Event::Trigger(t) => Some(t.clone()),
                _ => None,
            })
            .collect()
    }

    /// The log's targets as the engine's, one per slot: a player by their
    /// seat, a card by a permanent of that name among the slot's legal
    /// targets. Two permanents of one name are told apart by nothing the
    /// log says, so the first is taken.
    fn aim(
        &mut self,
        aims: &[log::Aim],
        slots: &[Vec<Target>],
        card: &str,
    ) -> Result<Vec<Target>, Stop> {
        let wrong = || {
            Stop::Diverged(Divergence::Targets {
                card: card.to_string(),
            })
        };
        if aims.len() != slots.len() {
            return Err(wrong());
        }
        let mut out = Vec::new();
        for (aim, legal) in aims.iter().zip(slots) {
            let t = match &aim.target {
                log::Target::Player(p) => Target::Player(self.seat(*p)),
                log::Target::Object(o @ (Object::Card(_) | Object::Hidden(_))) => {
                    let called = self.called(o)?;
                    let k = match self.aim_pick {
                        Some((at, k)) if at == self.at => k,
                        _ => 0,
                    };
                    let game = self.game.as_ref().expect("set up");
                    // Of permanents of one name, an opponent's first, as
                    // most spells aimed at one are removal; an Aura's
                    // caster's own first.
                    let (caster, aura) = match game.decision() {
                        Decision::Targets { player, card, .. } => (
                            Some(player),
                            game.card(card)
                                .is_some_and(|t| t.type_line.contains("Aura")),
                        ),
                        _ => (None, false),
                    };
                    let chars = game.characteristics();
                    let mut matching: Vec<&Target> = legal
                        .iter()
                        .filter(|t| match (t, &called) {
                            (Target::Object(o), _) => is_called(game, o, &called),
                            (Target::Spell(c), Called::Name(name)) => {
                                !game.face_down_on_stack(*c)
                                    && game.card(*c).is_some_and(|t| t.is_called(name))
                            }
                            (Target::Spell(c), Called::FaceDown(d)) => {
                                c == d && game.face_down_on_stack(*c)
                            }
                            (Target::Player(_), _) => false,
                        })
                        .collect();
                    matching.sort_by_key(|t| match t {
                        Target::Object(o) => {
                            let mine = chars.get(o).map(|c| c.controller) == caster;
                            mine != aura
                        }
                        _ => false,
                    });
                    **matching.get(k).or(matching.first()).ok_or_else(wrong)?
                }
                _ => return Err(wrong()),
            };
            if !legal.contains(&t) {
                return Err(wrong());
            }
            out.push(t);
        }
        Ok(out)
    }

    /// Mox Diamond would enter, and `p` may discard a land card instead:
    /// the log's next "discards [X]" line of theirs this turn says they
    /// did, and which; with none, they didn't, and it went to the
    /// graveyard.
    fn discard_to_enter(&mut self, p: PlayerId) -> Followed {
        let said = self
            .entries
            .iter()
            .enumerate()
            .skip(self.at)
            .take_while(|(k, e)| *k == self.at || !matches!(e.event, Event::TurnBegins(_)))
            .find_map(|(k, e)| match &e.event {
                Event::Discards { player, cards }
                    if self.seat(*player) == p && !self.consumed.contains(&k) =>
                {
                    match cards.as_slice() {
                        [Object::Card(c)] => Some((k, c.name().to_string())),
                        _ => None,
                    }
                }
                _ => None,
            });
        let Some((k, name)) = said else {
            return self.act(Answer::Pick(None));
        };
        let Some(c) = self.in_hand(p, &name)? else {
            return Err(Stop::Diverged(Divergence::NotInHand { card: name }));
        };
        self.consumed.insert(k);
        self.did(k, "discarded instead, so it entered (CR 614.1a)");
        self.act(Answer::Pick(Some(c)))
    }

    /// "Sacrifice this creature unless you discard a card" (CR 118.12a):
    /// the log's next "discards [X]" line of `p`'s this turn says they
    /// paid, and with what; with none, they didn't, and the permanent's
    /// sacrifice prints nothing. Masticore's and Solitary Confinement's
    /// upkeep, where the discard is the line after the trigger's.
    fn discard_unless(&mut self, p: PlayerId, card: CardId) -> Followed {
        let source = self.name_of(card);
        let said = self
            .entries
            .iter()
            .enumerate()
            .skip(self.at)
            .take_while(|(k, e)| *k == self.at || !matches!(e.event, Event::TurnBegins(_)))
            .find_map(|(k, e)| match &e.event {
                Event::Discards { player, cards }
                    if self.seat(*player) == p && !self.consumed.contains(&k) =>
                {
                    match cards.as_slice() {
                        [Object::Card(c)] => Some((k, c.name().to_string())),
                        _ => None,
                    }
                }
                _ => None,
            });
        let Some((k, name)) = said else {
            self.act(Answer::Pick(None))?;
            let at = self.at;
            self.did(at, format!("discarded nothing for {source} (CR 118.12a)"));
            return Ok(());
        };
        let Some(c) = self.in_hand(p, &name)? else {
            return Err(Stop::Diverged(Divergence::NotInHand { card: name }));
        };
        self.consumed.insert(k);
        self.did(
            k,
            format!("discarded for {source}'s \"unless\" (CR 118.12a)"),
        );
        self.act(Answer::Pick(Some(c)))
    }

    /// `p` cycles the card of this name from their hand: offered any time
    /// they have priority, the stack resolving first down to what a later
    /// line shows resolving, as a play's does.
    fn cycle(&mut self, p: PlayerId, name: &str, i: usize) -> Followed {
        self.text(name)?;
        let turn = self.game().turn();
        while !self.game().stack().is_empty() && !self.resolves_later() {
            self.pass()?;
            if self.game().turn() != turn {
                break;
            }
        }
        loop {
            let game = self.game();
            if game.turn() != turn {
                return Err(Stop::Diverged(Divergence::NeverOffered {
                    what: format!("cycle {name}"),
                }));
            }
            if let Decision::Priority { player, .. } = game.decision() {
                if player == p {
                    if let Some(c) = self.in_hand(p, name)? {
                        let Decision::Priority { actions, .. } = self.game().decision() else {
                            unreachable!("naming a card changes no decision")
                        };
                        if actions.contains(&Action::Cycle(c)) {
                            self.act(Answer::Act(Action::Cycle(c)))?;
                            self.aim_and_pay(i, &[], name)?;
                            return Ok(());
                        }
                        if let Some(r) = self.game().refusal(c) {
                            return Err(Stop::Refused {
                                card: name.to_string(),
                                node: r.node,
                                why: r.why,
                            });
                        }
                        return Err(Stop::Unfollowed {
                            event: "Cycles".into(),
                            card: Some(name.to_string()),
                        });
                    }
                }
            }
            self.pass()?;
        }
    }

    /// A spell being cast or an ability activated: the targets the log
    /// shows, chosen before the cost (CR 601.2c, 602.2b), then its mana
    /// paid by the first payment the solver finds, the others kept to go
    /// back to. What paid it, said.
    fn aim_and_pay(&mut self, i: usize, targets: &[log::Aim], name: &str) -> Result<String, Stop> {
        if let Decision::Targets { slots, .. } = self.game().decision() {
            let chosen = self.aim(targets, &slots, name)?;
            self.act(Answer::Targets(chosen))?;
        } else if !targets.is_empty() {
            return Err(Stop::Diverged(Divergence::Targets {
                card: name.to_string(),
            }));
        }
        if !matches!(self.game().decision(), Decision::Pay { .. }) {
            return Ok("no mana to pay".into());
        }
        self.tap_what_returns(i, name)?;
        let payments = self.payments()?;
        let Some((first, rest)) = payments.split_first() else {
            return Err(Stop::Diverged(Divergence::Unpayable {
                card: name.to_string(),
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
                knew: self.knew(),
                back: Back::Unplayable,
            });
        }
        pay_with(self.game(), first)?;
        // The rest of its cost, which the log doesn't show either.
        if let Decision::Sacrifice { .. } = self.game().decision() {
            self.sacrifice()?;
        }
        if let Decision::ReturnToHand { .. } = self.game().decision() {
            self.return_to_hand()?;
        }
        Ok(describe(first))
    }

    /// A spell whose cost returns permanents to hand, Gush's Islands: those
    /// the line beside it names are tapped for mana first, as many as the
    /// cost returns that aren't tapped already, the mana floating (CR
    /// 601.2g–h). The log never shows a mana ability, and a land returned
    /// untapped would have made nothing.
    fn tap_what_returns(&mut self, i: usize, name: &str) -> Followed {
        let Decision::Pay { player, .. } = self.game().decision() else {
            return Ok(());
        };
        let Some(q) = self.player_of(player) else {
            return Ok(());
        };
        let Some(j) = self.returned_for(i, q, name) else {
            return Ok(());
        };
        let Event::ReturnsToHand { cards, .. } = &self.entries[j].event else {
            return Ok(());
        };
        let names: Vec<String> = cards.iter().map(|c| c.name().to_string()).collect();
        let game = self.game();
        let name_of = |o: &ObjectId| {
            game.card(game.permanents()[o].card)
                .map(|t| t.name.clone())
                .unwrap_or_default()
        };
        let tapped = game
            .permanents()
            .iter()
            .filter(|(o, p)| {
                p.tapped
                    && game.characteristics()[*o].controller == player
                    && names.contains(&name_of(o))
            })
            .count();
        let mut wanted = names.len().saturating_sub(tapped);
        while wanted > 0 {
            let Decision::Pay { actions, .. } = self.game().decision() else {
                return Ok(());
            };
            let game = self.game();
            let tap = actions.iter().find_map(|a| match a {
                Payment::Mana(m)
                    if m.pays.is_empty()
                        && m.sacrifices.is_none()
                        && !game.permanents()[&m.source].tapped
                        && names.contains(
                            &game
                                .card(game.permanents()[&m.source].card)
                                .map(|t| t.name.clone())
                                .unwrap_or_default(),
                        ) =>
                {
                    Some(*a)
                }
                _ => None,
            });
            let Some(tap) = tap else {
                return Ok(());
            };
            self.act(Answer::Pay(tap))?;
            wanted -= 1;
        }
        Ok(())
    }

    /// The "returns [Island] and [Island] to its owner's hand with
    /// [Gush]'s ability" line beside line `i`, which casts `spell`: the
    /// log prints the cost's line before the cast, or after it.
    fn returned_for(&self, i: usize, player: log::Player, spell: &str) -> Option<usize> {
        let quiet = |e: &Entry| matches!(e.event, Event::Chat { .. } | Event::Watching { .. });
        let is_it = |j: usize| {
            matches!(&self.entries[j].event, Event::ReturnsToHand {
                player: q,
                with: log::Source::Card(c),
                ..
            } if *q == player && c.name() == spell)
        };
        let before = (0..i).rev().find(|j| !quiet(&self.entries[*j]));
        let after = (i + 1..self.entries.len()).find(|j| !quiet(&self.entries[*j]));
        before.filter(|j| is_it(*j)).or(after.filter(|j| is_it(*j)))
    }

    /// Permanents returned to hand to pay for a spell (CR 601.2h): those
    /// the log's "returns [Island] and [Island] to its owner's hand" line
    /// next names, the tapped first of permanents of one name, since a
    /// land tapped for mana pays twice that way, and the first legal ones
    /// with no such line.
    fn return_to_hand(&mut self) -> Followed {
        let game = self.game.as_ref().expect("set up");
        let Decision::ReturnToHand {
            player,
            count,
            mut legal,
            ..
        } = game.decision()
        else {
            unreachable!("waiting on a return to hand")
        };
        let spell = match game.decision() {
            Decision::ReturnToHand { spell, .. } => game.card(spell).map(|t| t.name.clone()),
            _ => None,
        };
        let named: Vec<String> = spell
            .and_then(|n| {
                let q = self.player_of(player)?;
                self.returned_for(self.at, q, &n)
            })
            .map(|j| match &self.entries[j].event {
                Event::ReturnsToHand { cards, .. } => {
                    cards.iter().map(|c| c.name().to_string()).collect()
                }
                _ => Vec::new(),
            })
            .unwrap_or_default();
        let name = |o: &ObjectId| {
            game.card(game.permanents()[o].card)
                .map(|t| t.name.clone())
                .unwrap_or_default()
        };
        legal.sort_by_key(|o| !game.permanents()[o].tapped);
        let mut chosen = Vec::new();
        for n in &named {
            if let Some(k) = legal
                .iter()
                .position(|o| !chosen.contains(o) && name(o) == *n)
            {
                chosen.push(legal[k]);
            }
        }
        for o in &legal {
            if chosen.len() >= count as usize {
                break;
            }
            if !chosen.contains(o) {
                chosen.push(*o);
            }
        }
        chosen.truncate(count as usize);
        self.act(Answer::ReturnToHand(chosen))
    }

    /// Permanents sacrificed to pay for a spell (CR 601.2h), which the log
    /// never names: first a creature whose dies trigger the lines right
    /// after show, then one no later line of the game names, then the
    /// rest, and another choice gone back to if any later line can't be
    /// followed.
    fn sacrifice(&mut self) -> Followed {
        let game = self.game.as_ref().expect("set up");
        let Decision::Sacrifice {
            player,
            count,
            legal,
            ..
        } = game.decision()
        else {
            unreachable!("waiting on a sacrifice")
        };
        let name = |o: &ObjectId| {
            game.card(game.permanents()[o].card)
                .map(|t| t.name.clone())
                .unwrap_or_default()
        };
        let dying: Vec<String> = self.entries[self.at + 1..]
            .iter()
            .take_while(|e| {
                matches!(
                    e.event,
                    Event::Trigger(_) | Event::Chat { .. } | Event::Watching { .. }
                )
            })
            .filter_map(|e| match &e.event {
                Event::Trigger(t) if self.seat(t.player) == player => {
                    source_name(t).map(str::to_string)
                }
                _ => None,
            })
            .collect();
        let later: BTreeSet<String> = self.entries[self.at + 1..]
            .iter()
            .flat_map(|e| e.event.cards())
            .map(|c| c.name().to_string())
            .collect();
        let mut ranked = legal.clone();
        ranked.sort_by_key(|o| {
            let n = name(o);
            if dying.contains(&n) {
                0
            } else if !later.contains(&n) {
                1
            } else {
                2
            }
        });
        // The first `count` of the ranking, then each other choice of one.
        let count = count as usize;
        let mut choices: Vec<Vec<ObjectId>> = vec![ranked.iter().take(count).copied().collect()];
        if count == 1 {
            choices.extend(ranked.iter().skip(1).map(|o| vec![*o]));
        }
        let described: Vec<String> = choices
            .iter()
            .map(|c| c.iter().map(name).collect::<Vec<_>>().join(" and "))
            .collect();
        let others: Vec<Retry> = choices
            .iter()
            .zip(&described)
            .skip(1)
            .map(|(c, d)| {
                Retry::Answer(
                    Answer::Sacrifice(c.clone()),
                    format!("sacrificed {d} instead"),
                )
            })
            .collect();
        if !others.is_empty() {
            let game = self.game().clone();
            self.checkpoints.push(Checkpoint {
                game,
                entry: self.at,
                trace: self.trace.len(),
                resume: self.at + 1,
                others,
                knew: self.knew(),
                back: Back::Any,
            });
        }
        self.act(Answer::Sacrifice(choices[0].clone()))?;
        let at = self.at;
        self.did(
            at,
            format!("sacrificed {} to pay (CR 701.21a)", described[0]),
        );
        Ok(())
    }

    /// The mana an instruction adds, which the log never says: the colour
    /// the player's next spell this turn asks most first, then their
    /// spells this game, the others gone back to if a later spell can't
    /// be paid for.
    fn choose_mana(&mut self, p: PlayerId, mut options: Vec<ManaPool>) -> Followed {
        self.by_colour(p, &mut options, |o| *o);
        if options.len() > 1 {
            let game = self.game().clone();
            self.checkpoints.push(Checkpoint {
                game,
                entry: self.at,
                trace: self.trace.len(),
                resume: self.at,
                others: options[1..]
                    .iter()
                    .map(|o| Retry::Answer(Answer::Mana(*o), format!("added {o} instead")))
                    .collect(),
                knew: self.knew(),
                back: Back::Unplayable,
            });
        }
        let first = options[0];
        self.act(Answer::Mana(first))?;
        let at = self.at;
        self.did(at, format!("added {first} (CR 106.3)"));
        Ok(())
    }

    /// Sort mana by the colour `p`'s next spell this turn asks most, then
    /// their spells this game.
    fn by_colour<T>(&mut self, p: PlayerId, items: &mut [T], mana: impl Fn(&T) -> ManaPool) {
        let next = self
            .entries
            .iter()
            .skip(self.at)
            .take_while(|e| !matches!(e.event, Event::TurnBegins(_)))
            .find_map(|e| match &e.event {
                Event::Casts(c) if self.seat(c.player) == p => match &c.card {
                    Object::Card(c) => Some(c.name().to_string()),
                    _ => None,
                },
                _ => None,
            });
        let cost = next
            .and_then(|n| self.text(&n).ok())
            .map(|t| t.mana_cost)
            .unwrap_or_default();
        let demand = self.demand[p.0 as usize];
        let colours = [Mana::White, Mana::Blue, Mana::Black, Mana::Red, Mana::Green];
        let letters = ["W", "U", "B", "R", "G"];
        items.sort_by_key(|t| {
            let o = mana(t);
            let (now, game): (usize, u32) = (0..5)
                .filter(|k| o.get(colours[*k]) > 0)
                .map(|k| {
                    (
                        cost.matches(&format!("{}}}", letters[k])).count(),
                        demand[k],
                    )
                })
                .fold((0, 0), |(a, b), (x, y)| (a + x, b + y));
            (std::cmp::Reverse(now), std::cmp::Reverse(game))
        });
    }

    /// A trigger line of a permanent that triggers on becoming tapped
    /// (City of Brass), with nothing on the stack: it was tapped for mana,
    /// which the log never shows, and the mana floats. Whether it was.
    fn tapped_for_mana(&mut self, name: &str) -> Result<bool, Stop> {
        let game = self.game.as_ref().expect("set up");
        let Decision::Priority { player, actions } = game.decision() else {
            return Ok(false);
        };
        if !game.stack().is_empty() {
            return Ok(false);
        }
        let mut mana: Vec<ManaActivation> = actions
            .iter()
            .filter_map(|a| match a {
                Action::Mana(m)
                    if game
                        .card(game.permanents()[&m.source].card)
                        .is_some_and(|t| t.is_called(name))
                        && game.triggers_on_tapping(m.source) =>
                {
                    Some(*m)
                }
                _ => None,
            })
            .collect();
        if mana.is_empty() {
            return Ok(false);
        }
        self.by_colour(player, &mut mana, |m| m.adds);
        self.pass_to_main_for_next_cast(player)?;
        if mana.len() > 1 {
            let game = self.game().clone();
            self.checkpoints.push(Checkpoint {
                game,
                entry: self.at,
                trace: self.trace.len(),
                resume: self.at,
                others: mana[1..]
                    .iter()
                    .map(|m| {
                        Retry::Answer(
                            Answer::Act(Action::Mana(*m)),
                            format!("tapped {name} for {} instead", m.adds),
                        )
                    })
                    .collect(),
                knew: self.knew(),
                back: Back::Unplayable,
            });
        }
        self.act(Answer::Act(Action::Mana(mana[0])))?;
        let at = self.at;
        self.did(
            at,
            format!("tapped {name} for {}, the mana floating", mana[0].adds),
        );
        Ok(true)
    }

    /// An "unless" `p` may pay (CR 118.12a), which the log shows only for
    /// echo: "paid the echo cost for [X]", read ahead to the end of the
    /// turn. Anything else isn't paid, and is paid instead, with the first
    /// payment found, if a later line can't be followed otherwise (Mana
    /// Leak's {3}).
    fn pay_unless(&mut self, p: PlayerId, card: CardId, owed: ManaCost) -> Followed {
        let name = self.name_of(card);
        let said = self
            .entries
            .iter()
            .enumerate()
            .skip(self.at)
            .take_while(|(k, e)| *k == self.at || !matches!(e.event, Event::TurnBegins(_)))
            .find_map(|(k, e)| match &e.event {
                Event::PaysEcho { player, card: c }
                    if self.seat(*player) == p
                        && c.name() == name
                        && !self.consumed.contains(&k) =>
                {
                    Some(k)
                }
                _ => None,
            });
        let game = self.game.as_ref().expect("set up");
        let pool = *game.player(p).pool();
        let payments = solve(&owed, pool, &unlogged_mana(game, p));
        if let Some(k) = said {
            let Some(first) = payments.first() else {
                return Err(Stop::Diverged(Divergence::Unpayable { card: name }));
            };
            self.consumed.insert(k);
            pay_with(self.game(), first)?;
            let at = self.at;
            self.did(at, format!("paid {owed} for {name}'s echo (CR 702.30a)"));
            return Ok(());
        }
        // Echo paid says so, so an echo with no line wasn't.
        let echo = self
            .text(&name)
            .is_ok_and(|t| t.rules_text().lines().any(|l| l.starts_with("Echo")));
        if !echo {
            if let Some(first) = payments.first() {
                let mut paid = self.game().clone();
                pay_with(&mut paid, first)?;
                self.checkpoints.push(Checkpoint {
                    game: paid,
                    entry: self.at,
                    trace: self.trace.len(),
                    resume: self.at,
                    others: vec![Retry::Take(format!("paid {owed} for {name}'s \"unless\""))],
                    knew: self.knew(),
                    back: Back::Any,
                });
            }
        }
        self.act(Answer::Pay(Payment::Cancel))?;
        let at = self.at;
        self.did(at, format!("didn't pay {owed} for {name} (CR 118.12a)"));
        Ok(())
    }

    /// Mana a line shows made in `p`'s own turn, for the spell their next
    /// line casts: if that is a sorcery-speed spell, it was made in a main
    /// phase, where the log never says the turn has got to, so the turn
    /// goes on to the next one first (mana made in the draw step or combat
    /// is gone by then, CR 500.5).
    fn pass_to_main_for_next_cast(&mut self, p: PlayerId) -> Followed {
        let next = self
            .entries
            .iter()
            .skip(self.at + 1)
            .take_while(|e| !matches!(e.event, Event::TurnBegins(_)))
            .find_map(|e| match &e.event {
                Event::Casts(c) if self.seat(c.player) == p => match &c.card {
                    Object::Card(c) => Some(c.name().to_string()),
                    _ => None,
                },
                _ => None,
            });
        // The face the line names: an Adventure's is its own (CR 715.3b).
        let Some(text) = next.and_then(|n| {
            let t = self.text(&n).ok()?;
            Some(t.face(t.side_named(&n)).unwrap_or(t))
        }) else {
            return Ok(());
        };
        let fast = text.type_line.contains("Instant")
            || text.rules_text().lines().any(|l| l.starts_with("Flash"));
        let turn = self.game().turn();
        while !fast
            && self.game().active() == p
            && self.game().turn() == turn
            && self.game().stack().is_empty()
            && !self.game().step_now().is_main()
        {
            self.pass()?;
        }
        Ok(())
    }

    /// The card `player` names for `card`'s instruction (CR 201.4): the
    /// log's "names [X] for [Cabal Therapy]" line, from the line being
    /// followed to the end of the turn.
    /// The creature type chosen as `card` enters: the log's "chooses Beast
    /// for [Engineered Plague]" line, read ahead to the end of the turn.
    fn choose_type(&mut self, player: PlayerId, card: CardId) -> Followed {
        let by = self.name_of(card);
        let said = self
            .entries
            .iter()
            .enumerate()
            .skip(self.at)
            .take_while(|(k, e)| *k == self.at || !matches!(e.event, Event::TurnBegins(_)))
            .find_map(|(k, e)| match &e.event {
                Event::Chooses {
                    player: q,
                    choice: log::Choice::Word(w),
                    by: Some(log::Source::Card(s)),
                } if self.seat(*q) == player && s.name() == by && !self.consumed.contains(&k) => {
                    Some((k, w.clone()))
                }
                _ => None,
            });
        let Some((k, word)) = said else {
            return Err(Stop::Diverged(Divergence::Unlogged {
                what: format!("the creature type chosen for {by}"),
            }));
        };
        self.consumed.insert(k);
        self.act(Answer::CreatureType(word))
    }

    fn choose_name(&mut self, player: PlayerId, card: CardId) -> Followed {
        let by = self.name_of(card);
        let said = self
            .entries
            .iter()
            .enumerate()
            .skip(self.at)
            .take_while(|(k, e)| *k == self.at || !matches!(e.event, Event::TurnBegins(_)))
            .find_map(|(k, e)| match &e.event {
                Event::Names {
                    player: q,
                    name,
                    for_source: log::Source::Card(s),
                } if self.seat(*q) == player && s.name() == by && !self.consumed.contains(&k) => {
                    Some((k, name.clone()))
                }
                _ => None,
            });
        let Some((k, name)) = said else {
            return Err(Stop::Diverged(Divergence::Unlogged {
                what: format!("the name chosen for {by}"),
            }));
        };
        let text = self.text(&name)?;
        self.consumed.insert(k);
        self.act(Answer::Name(Box::new(text)))
    }

    /// Pass priority while something on the stack resolves this turn, and
    /// answer what `pass` answers, until a choice a line has to show.
    fn until_choice(&mut self) -> Result<Decision, Stop> {
        let turn = self.game().turn();
        loop {
            let game = self.game();
            match game.decision() {
                Decision::Priority { .. } if !game.stack().is_empty() && game.turn() == turn => {
                    self.pass()?
                }
                Decision::Search { .. }
                | Decision::PayOrEnterTapped { .. }
                | Decision::May { .. }
                | Decision::ChooseName { .. }
                | Decision::ChooseCreatureType { .. }
                | Decision::ChooseMana { .. }
                | Decision::PayUnless { .. }
                | Decision::DiscardUnless { .. }
                | Decision::SacrificeUnless { .. }
                | Decision::PayLifeOr { .. }
                | Decision::Reveal { .. }
                | Decision::ChooseAmong { .. }
                | Decision::Arrange { .. }
                | Decision::PutFromHand { .. } => self.pass()?,
                d => return Ok(d),
            }
        }
    }

    /// The log shows a triggered ability going on the stack: the engine's
    /// own, from a card of that name, put there as the rules have it,
    /// resolving what is on the stack first if that is what triggers it.
    fn trigger(&mut self, name: &str) -> Followed {
        let text = self.text(name)?;
        let turn = self.game().turn();
        loop {
            let game = self.game.as_ref().expect("set up");
            let found = game.stack().iter().rev().find_map(|o| match o {
                StackObject::Trigger { source, seq, .. }
                    if !self.matched.contains(seq)
                        && game.card(*source).is_some_and(|t| t.name == text.name) =>
                {
                    Some(*seq)
                }
                _ => None,
            });
            if let Some(seq) = found {
                self.matched.insert(seq);
                return Ok(());
            }
            if self.tapped_for_mana(&text.name)? {
                continue;
            }
            let game = self.game.as_ref().expect("set up");
            match game.decision() {
                // What is on the stack resolves, or the turn goes on to
                // what triggers it, combat damage.
                Decision::Priority { .. } if game.turn() == turn => self.pass()?,
                // And what resolving it asks: Goblin Lackey's "may" puts
                // the creature whose trigger this is.
                Decision::Targets { .. }
                | Decision::Order { .. }
                | Decision::Attackers { .. }
                | Decision::Blockers { .. }
                | Decision::Assign { .. }
                | Decision::May { .. }
                | Decision::ChooseName { .. }
                | Decision::ChooseMana { .. }
                | Decision::PayUnless { .. }
                | Decision::DiscardUnless { .. }
                | Decision::SacrificeUnless { .. }
                | Decision::PayLifeOr { .. }
                | Decision::PutFromHand { .. }
                | Decision::Search { .. }
                | Decision::Reveal { .. }
                | Decision::ChooseAmong { .. }
                | Decision::Arrange { .. }
                | Decision::PayOrEnterTapped { .. }
                    if game.turn() == turn =>
                {
                    self.pass()?
                }
                _ => {
                    // A card whose text the engine can't run never
                    // triggers: Bloodghast in a graveyard.
                    let refused = (0..game.card_count())
                        .map(|c| CardId(c as u32))
                        .filter(|c| game.card(*c).is_some_and(|t| t.name == text.name))
                        .find_map(|c| game.refusal(c));
                    return Err(match refused {
                        Some(r) => Stop::Refused {
                            card: name.to_string(),
                            node: r.node,
                            why: r.why,
                        },
                        None => Stop::Diverged(Divergence::Untriggered {
                            card: name.to_string(),
                        }),
                    });
                }
            }
        }
    }

    /// The log shows `p` drawing with a spell or ability of `name`'s: the
    /// engine's resolving it draws, so it resolves, or has already drawn
    /// and waits on a later instruction's choice.
    fn drawn_by(&mut self, p: PlayerId, name: &str, count: u32) -> Followed {
        let text = self.text(name)?;
        let named = |game: &Game, c: &CardId| game.card(*c).is_some_and(|t| t.name == text.name);
        let game = self.game();
        if game.resolving().is_some_and(|c| named(game, &c)) {
            return Ok(());
        }
        let on_stack = |game: &Game| {
            game.stack().iter().any(|o| match o {
                StackObject::Spell { card, .. }
                | StackObject::Ability { source: card, .. }
                | StackObject::Trigger { source: card, .. } => named(game, card),
            })
        };
        if !on_stack(game) {
            // It resolved already, drawing as it went: the line is printed
            // after a choice it asked for first, a scry before the draw.
            if game.last_resolved().is_some_and(|c| named(game, &c)) {
                return Ok(());
            }
            return Err(Stop::Diverged(Divergence::Draw));
        }
        let before = game.player(p).hand().len();
        while on_stack(self.game()) {
            if !matches!(self.game().decision(), Decision::Priority { .. }) {
                break;
            }
            self.pass()?;
        }
        // "You may draw two additional cards": the line is the draw, so
        // the "may" was taken.
        if let Decision::May { card, .. } = self.game().decision() {
            if named(self.game(), &card) {
                self.act(Answer::May(true))?;
                return Ok(());
            }
        }
        let game = self.game();
        let resolving = game.resolving().is_some_and(|c| named(game, &c));
        let drew = game.player(p).hand().len() >= before + count as usize;
        if !resolving && !drew {
            return Err(Stop::Diverged(Divergence::Draw));
        }
        Ok(())
    }

    /// A card chosen from `p`'s revealed hand, and the hand the log shows:
    /// "discards [X]" and "reveals their hand to [Duress], containing …"
    /// (the hand after the discard), or "reveals 5 cards with [Duress]: …"
    /// (all of it), in either order, or the reveal alone when no card
    /// could be chosen.
    /// The hand's hidden cards are named from them, and the engine's hand
    /// must have as many cards as the log's.
    fn hand_choice(&mut self, g: &log::Game, i: usize, p: PlayerId) -> Result<Next, Stop> {
        let mut discarded: Option<String> = None;
        let mut revealed: Option<Vec<String>> = None;
        // Whether the reveal is the whole hand, the discarded card with it.
        let mut whole = false;
        let mut j = i;
        while j < g.entries.len() {
            match &g.entries[j].event {
                Event::Discards { player, cards }
                    if self.seat(*player) == p && discarded.is_none() =>
                {
                    match cards.as_slice() {
                        [Object::Card(c)] => discarded = Some(c.name().to_string()),
                        _ => {
                            return Err(Stop::Unfollowed {
                                event: "Discards".into(),
                                card: None,
                            })
                        }
                    }
                }
                Event::Reveals {
                    player,
                    cards,
                    how: how @ (log::Reveal::HandTo(_) | log::Reveal::Many { .. }),
                } if self.seat(*player) == p && revealed.is_none() => {
                    whole = matches!(how, log::Reveal::Many { .. });
                    revealed = Some(cards.iter().map(|c| c.name().to_string()).collect())
                }
                Event::Chat { .. } | Event::Watching { .. } => {}
                _ => break,
            }
            j += 1;
            if revealed.is_some() && discarded.is_some() {
                break;
            }
        }
        let Some(revealed) = revealed else {
            return Err(Stop::Unfollowed {
                event: "Discards without the hand revealed".into(),
                card: discarded,
            });
        };
        // A player who concedes with the choice to make leaves it unmade.
        let conceded = g.entries[j..]
            .iter()
            .find(|e| !matches!(e.event, Event::Chat { .. } | Event::Watching { .. }))
            .is_some_and(|e| {
                matches!(
                    e.event,
                    Event::Concedes(_) | Event::LosesToInaction(_) | Event::OutOfTime(_)
                )
            });
        let names: Vec<String> = match (&discarded, whole) {
            (Some(d), true) => {
                // The discarded card first, so it is the one picked.
                let Some(at) = revealed.iter().position(|n| n == d) else {
                    return Err(Stop::Diverged(Divergence::NotInHand { card: d.clone() }));
                };
                let mut rest = revealed.clone();
                rest.remove(at);
                std::iter::once(d.clone()).chain(rest).collect()
            }
            _ => discarded.iter().cloned().chain(revealed).collect(),
        };
        let names = split_halves_once(names);
        let hand = self.game().player(p).hand().clone();
        if names.len() != hand.len() {
            return Err(Stop::Diverged(Divergence::HandSize {
                log: names.len(),
                engine: hand.len(),
            }));
        }
        let mut named: Vec<CardId> = Vec::new();
        for n in &names {
            let text = self.text(n)?;
            let game = self.game();
            let known = hand.iter().find(|c| {
                !named.contains(*c) && game.card(**c).is_some_and(|t| t.name == text.name)
            });
            let c = match known {
                Some(c) => *c,
                None => {
                    let Some(c) = hand
                        .iter()
                        .find(|c| !named.contains(*c) && game.card(**c).is_none())
                    else {
                        return Err(Stop::Diverged(Divergence::NotInHand { card: n.clone() }));
                    };
                    game.reveal(*c, text).expect("a hidden card");
                    *c
                }
            };
            named.push(c);
        }
        let pick = discarded.as_ref().map(|_| named[0]);
        if pick.is_some() || !conceded {
            self.act(Answer::Pick(pick))?;
        }
        for k in i..j {
            self.did(
                k,
                "the hand revealed, and the card chosen from it (CR 701.20a)",
            );
        }
        Ok(Next::Line(j))
    }

    /// Line `i` casts a card face down (CR 702.37c): which is the log's to
    /// hide, and a later line names it, as it is revealed leaving the
    /// battlefield or the stack (CR 708.9) or once it is turned face up.
    /// That card of the hand is cast, a hidden one named as it; with no
    /// line naming it, a hidden card stays hidden, the 2/2 being the same
    /// whatever it is.
    fn cast_face_down(&mut self, i: usize, p: PlayerId) -> Result<String, Stop> {
        let n = self.entries[..=i]
            .iter()
            .filter(|e| matches!(e.event, Event::CastsFaceDown { .. }))
            .count() as u32;
        let name = self.face_down_name(i, n)?;
        let card = match &name {
            Some(name) => self.in_hand(p, name)?,
            None => {
                let game = self.game();
                let hand = game.player(p).hand();
                hand.iter().find(|c| game.card(**c).is_none()).copied()
            }
        };
        let Some(card) = card else {
            return Err(Stop::Diverged(Divergence::NotInHand {
                card: name.unwrap_or_else(|| "a card to cast face down".into()),
            }));
        };
        let what = name.clone().unwrap_or_else(|| "a hidden card".into());
        let turn = self.game().turn();
        while !self.game().stack().is_empty() && !self.resolves_later() {
            self.pass()?;
            if self.game().turn() != turn {
                break;
            }
        }
        loop {
            let game = self.game();
            if game.turn() != turn {
                return Err(Stop::Diverged(Divergence::NeverOffered {
                    what: format!("cast {what} face down"),
                }));
            }
            if let Decision::Priority { player, actions } = game.decision() {
                if player == p && actions.contains(&Action::CastFaceDown(card)) {
                    break;
                }
                // One the engine runs no morph of is never offered.
                if player == p && game.card(card).is_some() {
                    if let Some(r) = game.refusal(card) {
                        return Err(Stop::Refused {
                            card: what,
                            node: r.node,
                            why: r.why,
                        });
                    }
                }
            }
            self.pass()?;
        }
        self.act(Answer::Act(Action::CastFaceDown(card)))?;
        let paid = self.aim_and_pay(i, &[], &what)?;
        self.face_down.insert(n, card);
        Ok(format!(
            "cast {what} face down as face-down creature #{n} (CR 702.37c), {paid}"
        ))
    }

    /// The card a later line names "face-down creature #`n`", cast face
    /// down at line `i`: the one it is revealed as, or once a line turns
    /// it face up, the first card with morph a line after that names.
    fn face_down_name(&mut self, i: usize, n: u32) -> Result<Option<String>, Stop> {
        let it = log::Hidden::FaceDown(n);
        let mut up = None;
        for (j, e) in self.entries.iter().enumerate().skip(i + 1) {
            match &e.event {
                Event::RevealsHidden { object, card, .. } if *object == it => {
                    return Ok(Some(card.name().to_string()));
                }
                Event::TurnsFaceUp { object, .. } if *object == it => {
                    up = Some(j);
                    break;
                }
                _ => {}
            }
        }
        let Some(j) = up else {
            return Ok(None);
        };
        let named: Vec<String> = self.entries[j + 1..]
            .iter()
            .flat_map(|e| e.event.cards())
            .filter(|c| !c.is_token())
            .map(|c| c.name().to_string())
            .collect();
        for name in named {
            if self.text(&name).is_ok_and(|t| has_morph(&t)) {
                return Ok(Some(name));
            }
        }
        Ok(None)
    }

    /// The face-down permanent cast as "face-down creature #`n`" is revealed
    /// as `name` (CR 708.9): it is leaving the battlefield, so the stack
    /// resolves until it has.
    fn reveal_face_down(&mut self, n: u32, name: &str) -> Result<String, Stop> {
        let text = self.text(name)?;
        let Some(&card) = self.face_down.get(&n) else {
            return Err(Stop::Unfollowed {
                event: "RevealsHidden of a card not cast face down".into(),
                card: Some(name.to_string()),
            });
        };
        let turn = self.game().turn();
        let face_down = |game: &Game| {
            game.permanents()
                .values()
                .any(|p| p.card == card && p.face_down)
        };
        while face_down(self.game()) && !self.game().stack().is_empty() {
            self.pass()?;
            if self.game().turn() != turn {
                break;
            }
        }
        let game = self.game();
        if face_down(game) {
            return Err(Stop::Unfollowed {
                event: "RevealsHidden of a permanent still face down".into(),
                card: Some(name.to_string()),
            });
        }
        if game.card(card).is_none() {
            game.reveal(card, text.clone())
                .map_err(|_| Stop::Diverged(Divergence::NotInHand { card: name.into() }))?;
        }
        if game.card(card).is_none_or(|t| t.name != text.name) {
            return Err(Stop::Diverged(Divergence::NotInHand {
                card: name.to_string(),
            }));
        }
        Ok(format!(
            "face-down creature #{n} revealed as {name} as it left (CR 708.9)"
        ))
    }

    /// `p` turns "face-down creature #`n`" face up, any time they have
    /// priority (CR 702.37e, 116.2b): in response to what is on the stack,
    /// since the log shows it before what it answers resolves.
    fn turn_face_up(&mut self, i: usize, p: PlayerId, n: u32) -> Result<String, Stop> {
        let Some(&card) = self.face_down.get(&n) else {
            return Err(Stop::Unfollowed {
                event: "TurnsFaceUp of a card not cast face down".into(),
                card: None,
            });
        };
        let name = self.name_of(card);
        if name.is_empty() {
            return Err(Stop::Unfollowed {
                event: "TurnsFaceUp of a card no line names".into(),
                card: None,
            });
        }
        let turn = self.game().turn();
        let id = loop {
            let game = self.game();
            let id = game
                .permanents()
                .iter()
                .find(|(_, perm)| perm.card == card && perm.face_down)
                .map(|(id, _)| *id);
            let Some(id) = id.filter(|_| game.turn() == turn) else {
                return Err(Stop::Diverged(Divergence::NeverOffered {
                    what: format!("turn {name} face up"),
                }));
            };
            if let Decision::Priority { player, actions } = game.decision() {
                if player == p && actions.contains(&Action::TurnFaceUp(id)) {
                    break id;
                }
                if player == p {
                    if let Some(r) = game.refusal(card) {
                        return Err(Stop::Refused {
                            card: name,
                            node: r.node,
                            why: r.why,
                        });
                    }
                }
            }
            self.pass()?;
        };
        self.act(Answer::Act(Action::TurnFaceUp(id)))?;
        let paid = self.aim_and_pay(i, &[], &name)?;
        Ok(format!("turned {name} face up (CR 702.37e), {paid}"))
    }

    /// What a line calls a permanent: a name, or the card cast face down
    /// as a "face-down creature #N".
    fn called(&mut self, o: &Object) -> Result<Called, Stop> {
        match o {
            Object::Card(c) => Ok(Called::Name(self.permanent_name(c)?)),
            Object::Hidden(log::Hidden::FaceDown(n)) => match self.face_down.get(n) {
                Some(c) => Ok(Called::FaceDown(*c)),
                None => Err(Stop::Unfollowed {
                    event: format!("face-down creature #{n} not cast face down"),
                    card: None,
                }),
            },
            _ => Err(Stop::Unfollowed {
                event: short(o),
                card: None,
            }),
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

    /// Whether a later line this turn shows the top of the stack resolving:
    /// a draw or a reveal "with" its card. A play before that line was made
    /// in response to it.
    fn resolves_later(&self) -> bool {
        let game = self.game.as_ref().expect("set up");
        let Some(top) = game.stack().back() else {
            return false;
        };
        let card = match top {
            StackObject::Spell { card, .. }
            | StackObject::Ability { source: card, .. }
            | StackObject::Trigger { source: card, .. } => *card,
        };
        let Some(text) = game.card(card) else {
            return false;
        };
        let is = |s: &log::Source| matches!(s, log::Source::Card(c) if text.is_called(c.name()));
        self.entries
            .iter()
            .skip(self.at + 1)
            .take_while(|e| !matches!(e.event, Event::TurnBegins(_)))
            .any(|e| match &e.event {
                Event::Draws { with: Some(s), .. } => is(s),
                Event::Reveals {
                    how:
                        log::Reveal::With(s)
                        | log::Reveal::HandTo(s)
                        | log::Reveal::Many { with: s, .. },
                    ..
                } => is(s),
                _ => false,
            })
    }

    /// Pass priority until `p` may play or cast `name` from their hand,
    /// or from their graveyard by an alternative cost that allows it,
    /// within this turn, and say how. The stack resolves first, down to
    /// what a later line shows resolving, unless the play targets a spell
    /// on it: the log doesn't say when a spell resolved, and a play that
    /// answers one targets it or comes before its result.
    fn offered(
        &mut self,
        p: PlayerId,
        name: &str,
        kind: Kind,
        respond: bool,
        answers: bool,
    ) -> Result<Action, Stop> {
        // The face the line names, a two-faced card's back by its name.
        let side = self.text(name)?.side_named(name);
        let turn = self.game().turn();
        // A card the engine runs that names a card on the stack targets
        // that spell if it can target spells, and otherwise names the
        // permanent the spell becomes.
        let respond = answers
            || respond
                && match self.find(p, name, kind)? {
                    Some(c) => self.game().refusal(c).is_some() || self.game().targets_spells(c),
                    None => true,
                };
        if !respond {
            while !self.game().stack().is_empty() && !self.resolves_later() {
                self.pass()?;
                if self.game().turn() != turn {
                    break;
                }
            }
        }
        // A spell its caster casts in their own turn's beginning phase with
        // nothing to answer is cast in their main phase, where the log
        // never says the turn has got to: Dark Ritual's mana is gone by
        // then if cast in the draw step. Cast earlier only if what follows
        // needs it.
        let game = self.game.as_ref().expect("set up");
        if matches!(kind, Kind::Cast(_))
            && game.active() == p
            && game.step_now() < Step::PrecombatMain
            && game.stack().is_empty()
            && self.early != Some(self.at)
        {
            let game = self.game().clone();
            self.checkpoints.push(Checkpoint {
                game,
                entry: self.at,
                trace: self.trace.len(),
                resume: self.at,
                others: vec![Retry::Early],
                knew: self.knew(),
                back: Back::Unplayable,
            });
            while self.game().step_now() < Step::PrecombatMain && self.game().turn() == turn {
                self.pass()?;
            }
        }
        // Likewise one cast in combat with nothing to answer may have been
        // cast after it, the log never saying the turn got to its second
        // main phase: Dark Ritual for the Phyrexian Arena cast next.
        let game = self.game.as_ref().expect("set up");
        let fighting = (Step::BeginningOfCombat..=Step::EndOfCombat).contains(&game.step_now());
        if matches!(kind, Kind::Cast(_))
            && game.active() == p
            && fighting
            && game.stack().is_empty()
        {
            if self.late == Some(self.at) {
                while self.game().step_now() < Step::PostcombatMain && self.game().turn() == turn {
                    self.pass()?;
                }
            } else {
                let game = self.game().clone();
                self.checkpoints.push(Checkpoint {
                    game,
                    entry: self.at,
                    trace: self.trace.len(),
                    resume: self.at,
                    others: vec![Retry::Late],
                    knew: self.knew(),
                    back: Back::Unplayable,
                });
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
                if let Some(r) = game.refusal_of(c, side) {
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
                    if let Some(c) = self.find(p, name, kind)? {
                        seen = Some(c);
                        // Asked after naming it: a hidden card isn't offered.
                        let Decision::Priority { actions, .. } = self.game().decision() else {
                            unreachable!("naming a card changes no decision")
                        };
                        let wanted = actions.iter().copied().find(|a| match (kind, side, a) {
                            (Kind::Land, Side::Front, Action::PlayLand(x))
                            | (Kind::Land, Side::Back, Action::PlayBack(x))
                            | (Kind::Cast(Route::ManaCost), Side::Front, Action::Cast(x))
                            | (Kind::Cast(Route::ManaCost), Side::Back, Action::CastBack(x))
                            | (
                                Kind::Cast(Route::Alternative(_)),
                                Side::Front,
                                Action::CastAlternative(x, _),
                            ) => *x == c,
                            _ => false,
                        });
                        if let Some(a) = wanted {
                            return Ok(a);
                        }
                        // One it refuses is never offered: no waiting for
                        // the turn to end.
                        if let Some(r) = self.game().refusal_of(c, side) {
                            return Err(Stop::Refused {
                                card: name.to_string(),
                                node: r.node,
                                why: r.why,
                            });
                        }
                        // Nor one it has no such alternative cost for.
                        if let Kind::Cast(Route::Alternative(from)) = kind {
                            if !self.game().alternatives(c).contains(&from) {
                                return Err(Stop::Unfollowed {
                                    event: "Casts other than from hand for its mana cost".into(),
                                    card: Some(name.to_string()),
                                });
                            }
                        }
                    }
                }
            }
            self.pass()?;
        }
    }

    /// Cast the line's spell by `route`: the card offered, its targets
    /// and its cost paid. What paid it, said.
    #[allow(clippy::too_many_arguments)]
    fn cast_by(
        &mut self,
        i: usize,
        cast: &log::Cast,
        name: &str,
        p: PlayerId,
        route: Route,
        kicked: bool,
        on_stack: bool,
        answers: bool,
    ) -> Result<String, Stop> {
        let cast_it = self.offered(p, name, Kind::Cast(route), on_stack, answers)?;
        self.act(Answer::Act(cast_it))?;
        // CR 601.2b: the line says whether its kicker was paid.
        match self.game().decision() {
            Decision::Kicker { .. } => self.act(Answer::Kick(kicked))?,
            _ if kicked => {
                return Err(Stop::Diverged(Divergence::NeverOffered {
                    what: format!("kick {name}"),
                }))
            }
            _ => {}
        }
        let paid = self.aim_and_pay(i, &cast.targets, name)?;
        Ok(if kicked {
            format!("kicked (CR 702.33d), {paid}")
        } else {
            paid
        })
    }

    /// The card of `p`'s named `name` where `kind` casts or plays it from:
    /// their graveyard for flashback, otherwise their hand, naming a
    /// hidden card of it if none is known to be.
    fn find(&mut self, p: PlayerId, name: &str, kind: Kind) -> Result<Option<CardId>, Stop> {
        // An adventurer card on an adventure is cast from exile before one
        // in hand: the log doesn't say which, and a player who sent it on
        // one cast it from there (CR 715.3d).
        if kind == Kind::Cast(Route::ManaCost) {
            let text = self.text(name)?;
            let game = self.game();
            if text.side_named(name) == Side::Front {
                if let Some(c) = game
                    .adventuring(p)
                    .find(|c| game.card(*c).is_some_and(|t| t.name == text.name))
                {
                    return Ok(Some(c));
                }
            }
        }
        if kind != Kind::Cast(Route::Alternative(CastFrom::Graveyard)) {
            return self.in_hand(p, name);
        }
        let text = self.text(name)?;
        let game = self.game();
        Ok(game
            .player(p)
            .graveyard()
            .iter()
            .copied()
            .find(|c| game.card(*c).is_some_and(|t| t.name == text.name)))
    }

    /// Pass priority until `p` may activate an ability of a permanent named
    /// `name` they control, within this turn, as `offered` does for a
    /// play. Which of its abilities is the one the engine offers: a card
    /// whose abilities it offers more than one of isn't followed yet.
    fn activation(
        &mut self,
        p: PlayerId,
        name: &str,
        said: &str,
        respond: bool,
    ) -> Result<Activation, Stop> {
        let text = self.text(name)?;
        let turn = self.game().turn();
        // An ability of a card in no zone the engine activates from yet: a
        // companion's put into a hand (CR 702.139c), a card's in a
        // graveyard.
        let game = self.game.as_ref().expect("set up");
        if !game
            .permanents()
            .values()
            .any(|perm| game.card(perm.card).is_some_and(|t| t.name == text.name))
            && game.stack().is_empty()
        {
            return Err(Stop::Unfollowed {
                event: "Activates of a card not on the battlefield".into(),
                card: Some(name.to_string()),
            });
        }
        if !respond {
            while !self.game().stack().is_empty() && !self.resolves_later() {
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
                        if offered.iter().all(|a| a.ability == first.ability) {
                            let first = *first;
                            return Ok(self.which_source(&offered, first.ability));
                        }
                        // Which of its abilities: the one whose text has
                        // the most of the words the log prints of it.
                        let score = |a: &Activation| {
                            let Ability::Unread(n) = a.ability else {
                                return 0;
                            };
                            let card = game.permanents()[&a.source].card;
                            game.ability_text(card, n)
                                .map_or(0, |t| shared_words(said, t))
                        };
                        let best = offered.iter().map(score).max().unwrap_or(0);
                        let top: Vec<&Activation> =
                            offered.iter().filter(|a| score(a) == best).collect();
                        if best == 0 || top.iter().any(|a| a.ability != top[0].ability) {
                            return Err(Stop::Unfollowed {
                                event: "Activates one of several abilities".into(),
                                card: Some(name.to_string()),
                            });
                        }
                        let ability = top[0].ability;
                        return Ok(self.which_source(&offered, ability));
                    }
                }
            }
            self.pass()?;
        }
    }

    /// The permanent of the name a counters line names with at least as
    /// many of those counters more than the log has shown on it, and the
    /// key it is counted by.
    fn counters_on_named(
        &mut self,
        counted: &log::Counted,
    ) -> Result<Option<(ObjectId, Counter)>, Stop> {
        let log::Target::Object(Object::Card(c)) = &counted.on else {
            return Err(Stop::Unfollowed {
                event: "PutsCounters".into(),
                card: None,
            });
        };
        let name = self.permanent_name(c)?;
        let kind = counter_kind(&counted.kind);
        let game = self.game.as_ref().expect("set up");
        Ok(game
            .permanents()
            .iter()
            .filter(|(_, p)| game.card(p.card).is_some_and(|t| t.is_called(&name)))
            .map(|(id, _)| (*id, kind.clone()))
            .find(|key| {
                let shown = self.counted.get(key).copied().unwrap_or(0);
                game.counters_on(key.0, &key.1) >= shown + counted.count
            }))
    }

    /// What of a counters line isn't on the battlefield yet, if anything.
    fn counters_missing(&mut self, counters: &[log::Counted]) -> Result<Option<String>, Stop> {
        for counted in counters {
            if self.counters_on_named(counted)?.is_none() {
                let name = match &counted.on {
                    log::Target::Object(Object::Card(c)) => c.name().to_string(),
                    _ => String::new(),
                };
                return Ok(Some(format!(
                    "{} {} counters on {name}",
                    counted.count, counted.kind
                )));
            }
        }
        Ok(None)
    }

    /// A line putting one counter on a permanent of `p`'s whose mana
    /// ability costs that counter: the ability activated now, if `p` has
    /// priority, the first mana it offers taken. Whether it was.
    fn counter_mana(&mut self, p: PlayerId, counters: &[log::Counted]) -> Result<bool, Stop> {
        self.counter_cost_mana(p, counters, false)
    }

    /// As `counter_mana`, for a counter put on (`removes` false) or taken
    /// off (Gemstone Mine's mining counter) as a mana ability's cost.
    fn counter_cost_mana(
        &mut self,
        p: PlayerId,
        counters: &[log::Counted],
        removes: bool,
    ) -> Result<bool, Stop> {
        let [counted] = counters else {
            return Ok(false);
        };
        let log::Target::Object(Object::Card(c)) = &counted.on else {
            return Ok(false);
        };
        if counted.count != 1 {
            return Ok(false);
        }
        let name = self.permanent_name(c)?;
        let kind = counter_kind(&counted.kind);
        let offered = |game: &Game| -> Vec<ManaActivation> {
            let Decision::Priority { player, actions } = game.decision() else {
                return Vec::new();
            };
            if player != p {
                return Vec::new();
            }
            actions
                .into_iter()
                .filter_map(|a| match a {
                    Action::Mana(m) => {
                        let cost = if removes {
                            game.mana_removes(&m)
                        } else {
                            game.mana_counter(&m)
                        };
                        (cost.as_ref() == Some(&kind)
                            && game
                                .card(game.permanents()[&m.source].card)
                                .is_some_and(|t| t.is_called(&name)))
                        .then_some(m)
                    }
                    _ => None,
                })
                .collect()
        };
        // In another's turn, or with them to act, they pass to `p` first:
        // a mana ability is activated with priority (CR 605.3a).
        let game = self.game.as_ref().expect("set up");
        if offered(game).is_empty() {
            let theirs =
                matches!(game.decision(), Decision::Priority { player, .. } if player != p);
            let mut passed = game.clone();
            if !theirs
                || passed.step(Answer::Act(Action::Pass)).is_err()
                || offered(&passed).is_empty()
            {
                return Ok(false);
            }
            self.act(Answer::Act(Action::Pass))?;
        }
        self.pass_to_main_for_next_cast(p)?;
        let mut mana = offered(self.game.as_ref().expect("set up"));
        if mana.is_empty() {
            return Ok(false);
        }
        // Which mana it made the log doesn't say: what the next spell
        // asks first, the rest gone back to.
        self.by_colour(p, &mut mana, |m| m.adds);
        if mana.len() > 1 {
            let game = self.game().clone();
            self.checkpoints.push(Checkpoint {
                game,
                entry: self.at,
                trace: self.trace.len(),
                // A line of a counter put on is followed again, to count
                // it; one taken off is done, or another would be.
                resume: self.at + usize::from(removes),
                others: mana[1..]
                    .iter()
                    .map(|m| {
                        Retry::Answer(
                            Answer::Act(Action::Mana(*m)),
                            format!("made {} instead", m.adds),
                        )
                    })
                    .collect(),
                knew: self.knew(),
                back: Back::Unplayable,
            });
        }
        self.act(Answer::Act(Action::Mana(mana[0])))?;
        Ok(true)
    }

    /// Which of the permanents of one name the line activates `ability`
    /// of, which the log can't say: one not activated this turn first,
    /// the others gone back to when a later play can't be made (a second
    /// Mishra's Factory animated, not the first again).
    fn which_source(&mut self, offered: &[Activation], ability: Ability) -> Activation {
        let turn = self.game().turn();
        let mut sources: Vec<Activation> = offered
            .iter()
            .filter(|a| a.ability == ability)
            .copied()
            .collect();
        let activated = if self.activated.0 == turn {
            self.activated.1.clone()
        } else {
            Vec::new()
        };
        sources.sort_by_key(|a| activated.contains(&a.source));
        let k = match self.pick {
            Some((at, k)) if at == self.at && k < sources.len() => k,
            _ => {
                if sources.len() > 1 {
                    let game = self.game().clone();
                    self.checkpoints.push(Checkpoint {
                        game,
                        entry: self.at,
                        trace: self.trace.len(),
                        resume: self.at,
                        others: (1..sources.len()).map(Retry::Pick).collect(),
                        knew: self.knew(),
                        back: Back::Unplayable,
                    });
                }
                0
            }
        };
        let chosen = sources[k];
        if self.activated.0 != turn {
            self.activated = (turn, Vec::new());
        }
        self.activated.1.push(chosen.source);
        chosen
    }

    /// Answer a search the log doesn't show the result of. The log says
    /// when a search fails (`FailsToFind`), so this one found a card; which
    /// card isn't said, so the replay chooses, as it chooses a payment, and
    /// goes back to choose again if a later spell can't be paid for. The
    /// candidates are the library's known cards, then a hidden card named
    /// as each basic land, the colours the player's spells this game need
    /// most first, then each land the log shows the player playing in the
    /// match, then each card it shows them casting or playing later this
    /// game. Last, as a hypothesis, a real land the log never names that
    /// the filter matches, of those legal in the game's format
    /// (`UnseenLand`), the ones making the colours the player's spells
    /// need most first, at most `UNSEEN_TRIES` of them. The engine checks
    /// each matches what was searched for.
    fn search(
        &mut self,
        p: PlayerId,
        filter: &crate::lower::CardFilter,
        known: Vec<CardId>,
        hidden: usize,
    ) -> Followed {
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
            // Then what the player casts or plays later this game, which
            // came from their library if a tutor found it.
            let later: Vec<String> = self
                .entries
                .iter()
                .skip(self.at)
                .filter_map(|e| match &e.event {
                    Event::Casts(c) if self.seat(c.player) == p => match &c.card {
                        Object::Card(c) => Some(c.name().to_string()),
                        _ => None,
                    },
                    Event::PlaysLand {
                        player,
                        card: Object::Card(c),
                    } if self.seat(*player) == p => Some(c.name().to_string()),
                    _ => None,
                })
                .collect();
            let mut names: Vec<String> = Vec::new();
            // The lands seen played in the match: theirs, then their
            // opponent's.
            let seen = self.lands[p.0 as usize]
                .iter()
                .chain(&self.lands[1 - p.0 as usize])
                .cloned();
            for n in basics
                .into_iter()
                .map(|(_, n)| n.to_string())
                .chain(std::iter::once("Wastes".to_string()))
                .chain(seen)
                .chain(later)
            {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
            let mut unseen: Vec<&UnseenLand> = self
                .unseen_lands
                .iter()
                .filter(|l| !self.mentioned.contains(&l.name) && !names.contains(&l.name))
                .collect();
            unseen.sort_by_key(|l| {
                let need: u32 = (0..5).filter(|k| l.makes[*k]).map(|k| demand[k]).sum();
                (std::cmp::Reverse(need), l.name.clone())
            });
            let mut tried = 0;
            for l in unseen {
                if tried == UNSEEN_TRIES {
                    break;
                }
                let Ok(text) = self.text(&l.name) else {
                    continue;
                };
                // Only one the filter matches is worth asking the engine,
                // which checks it again.
                if crate::lower::printed(&text).is_ok_and(|c| filter.matches(&c)) {
                    names.push(l.name.clone());
                    tried += 1;
                }
            }
            for n in names {
                let Ok(text) = self.text(&n) else { continue };
                let did = if !self.hypothesis(&text.name) {
                    format!("found {n}, from the hidden library")
                } else {
                    format!("found {n}, from the hidden library, a land the log never names")
                };
                tries.push((Answer::Find(Found::Hidden(c, Box::new(text))), did));
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
                knew: self.knew(),
                back: Back::Unplayable,
            });
        }
        self.note_unseen(first);
        self.act(first.clone())?;
        let at = self.at;
        self.did(at, did.clone());
        Ok(())
    }

    /// A choice among cards the log never shows (Impulse's look, Flare of
    /// Cultivation's land put onto the battlefield): the first, and when
    /// one is chosen and the cards' names differ, a checkpoint to go back
    /// and choose each other one if a later line can't be followed. Hidden
    /// cards are alike, so there is nothing to go back to among them.
    fn choose_among(&mut self, cards: Vec<CardId>, n: usize) -> Followed {
        let names: Vec<Option<String>> = {
            let game = self.game();
            cards
                .iter()
                .map(|c| game.card(*c).map(|t| t.name.clone()))
                .collect()
        };
        let others: Vec<Retry> = cards
            .iter()
            .zip(&names)
            .skip(1)
            .filter_map(|(c, name)| match name {
                Some(name) if n == 1 && Some(name) != names[0].as_ref() => Some(Retry::Answer(
                    Answer::Choose(vec![*c]),
                    format!("chose {name}"),
                )),
                _ => None,
            })
            .collect();
        if !others.is_empty() {
            let game = self.game().clone();
            self.checkpoints.push(Checkpoint {
                game,
                entry: self.at,
                trace: self.trace.len(),
                resume: self.at,
                others,
                knew: self.knew(),
                back: Back::Unplayable,
            });
        }
        self.act(Answer::Choose(cards[..n].to_vec()))
    }

    /// A search's card the log never names, found as a hypothesis, is
    /// recorded as one the replay relies on.
    fn note_unseen(&mut self, answer: &Answer) {
        if let Answer::Find(Found::Hidden(_, text)) = answer {
            if self.hypothesis(&text.name) {
                self.unseen.push(text.name.clone());
            }
        }
    }

    /// Whether a card is one of the lands the log never names, found only
    /// as a hypothesis.
    fn hypothesis(&self, name: &str) -> bool {
        !self.mentioned.contains(name) && self.unseen_lands.iter().any(|l| l.name == name)
    }

    /// A shock land's choice, which the log never shows: no line says
    /// life was paid, and life totals aren't printed. The replay doesn't
    /// pay, so the land enters tapped, unless that leaves a later spell
    /// that can't be paid for; then it goes back and pays. Paying is only
    /// ever chosen because something that follows needs it.
    fn pay_or_tap(&mut self, card: CardId, life: u32) -> Followed {
        let name = self
            .game()
            .card(card)
            .map(|t| t.name.clone())
            .unwrap_or_default();
        let game = self.game().clone();
        self.checkpoints.push(Checkpoint {
            game,
            entry: self.at,
            trace: self.trace.len(),
            resume: self.at,
            others: vec![Retry::Answer(
                Answer::PayLife(true),
                format!("paid {life} life for {name} to enter untapped"),
            )],
            knew: self.knew(),
            back: Back::Unplayable,
        });
        self.act(Answer::PayLife(false))?;
        let at = self.at;
        self.did(at, format!("{name} enters tapped, no life paid"));
        Ok(())
    }

    /// "Pay 4 life or put the card on top of your library": the next line
    /// of `p`'s this turn not yet read that answers one, "kept a card."
    /// for the life paid or "put a card on top of …'s library." for not.
    fn pay_life_or(&mut self, p: PlayerId, card: CardId) -> Followed {
        let name = self.name_of(card);
        let said = self
            .entries
            .iter()
            .enumerate()
            .skip(self.at)
            .take_while(|(k, e)| *k == self.at || !matches!(e.event, Event::TurnBegins(_)))
            .find_map(|(k, e)| {
                if self.consumed.contains(&k) {
                    return None;
                }
                match &e.event {
                    Event::KeepsCards { player, count: 1 } if self.seat(*player) == p => {
                        Some((k, true))
                    }
                    Event::PutsInLibrary {
                        player,
                        cards: log::Moved::Unseen(1),
                        end: log::End::Top,
                        by: None,
                        ..
                    } if self.seat(*player) == p => Some((k, false)),
                    _ => None,
                }
            });
        let Some((k, pay)) = said else {
            return Err(Stop::Diverged(Divergence::Unlogged {
                what: format!("whether life was paid for {name}"),
            }));
        };
        self.consumed.insert(k);
        self.act(Answer::PayLife(pay))
    }

    /// A "may" of `card`'s (CR 608.2d, 603.5). A "chooses to use" or
    /// "declines to use" line for it, from the line being followed to the
    /// end of the turn, says which; with none, the replay follows it and
    /// goes back to decline it if a later line can't then be followed, as
    /// it does for a payment: the log shows what the instruction did, or
    /// nothing at all (Goblin Lackey's).
    fn may(&mut self, card: CardId) -> Followed {
        let name = self.name_of(card);
        let said = self
            .entries
            .iter()
            .enumerate()
            .skip(self.at)
            .take_while(|(k, e)| *k == self.at || !matches!(e.event, Event::TurnBegins(_)))
            .find_map(|(k, e)| match &e.event {
                Event::UsesAbility { card: c, uses, .. }
                    if c.name() == name && !self.consumed.contains(&k) =>
                {
                    Some((k, *uses))
                }
                _ => None,
            });
        if let Some((k, uses)) = said {
            self.consumed.insert(k);
            return self.act(Answer::May(uses));
        }
        let game = self.game().clone();
        self.checkpoints.push(Checkpoint {
            game,
            entry: self.at,
            trace: self.trace.len(),
            resume: self.at,
            others: vec![Retry::Answer(
                Answer::May(false),
                format!("declined {name}'s \"may\""),
            )],
            knew: self.knew(),
            back: Back::Any,
        });
        self.act(Answer::May(true))?;
        let at = self.at;
        self.did(
            at,
            format!("followed {name}'s \"may\", which no line declines"),
        );
        Ok(())
    }

    /// Pass, answering what `pass` answers, until line `i` has been read
    /// ahead for a choice, within the turn: whether it was.
    fn until_consumed(&mut self, i: usize) -> Result<bool, Stop> {
        let turn = self.game().turn();
        while !self.consumed.contains(&i) {
            let game = self.game();
            let passing = game.turn() == turn
                && match game.decision() {
                    Decision::Priority { .. } => !game.stack().is_empty(),
                    Decision::May { .. }
                    | Decision::ChooseName { .. }
                    | Decision::ChooseCreatureType { .. }
                    | Decision::ChooseMana { .. }
                    | Decision::PayUnless { .. }
                    | Decision::PayLifeOr { .. }
                    | Decision::Reveal { .. }
                    | Decision::ChooseAmong { .. }
                    | Decision::Arrange { .. }
                    | Decision::Targets { .. }
                    | Decision::Order { .. }
                    | Decision::Search { .. }
                    | Decision::DiscardToEnter { .. }
                    | Decision::DiscardUnless { .. }
                    | Decision::SacrificeUnless { .. }
                    | Decision::PayOrEnterTapped { .. } => true,
                    _ => false,
                };
            if !passing {
                return Ok(false);
            }
            self.pass()?;
        }
        Ok(true)
    }

    /// Cards of `p`'s library a resolving instruction reveals, which no
    /// one had named: the log's "reveals [X] with [Dark Confidant]'s
    /// ability" lines, from the line being followed to the end of the
    /// turn, name them in order.
    fn reveal(&mut self, p: PlayerId, cards: &[CardId]) -> Followed {
        let count = cards.len();
        let game = self.game.as_ref().expect("set up");
        if cards.iter().all(|c| game.player(p).hand().contains(c)) {
            return self.reveal_hand(p);
        }
        let by = game
            .resolving()
            .map(|c| self.name_of(c))
            .unwrap_or_default();
        let looked = game.looked_at_revealed();
        let mut names = Vec::new();
        for (k, e) in self.entries.iter().enumerate().skip(self.at) {
            if names.len() == count || k > self.at && matches!(e.event, Event::TurnBegins(_)) {
                break;
            }
            if let Event::Reveals {
                player,
                cards,
                how:
                    log::Reveal::With(log::Source::Card(s))
                    | log::Reveal::Many {
                        with: log::Source::Card(s),
                        ..
                    },
            } = &e.event
            {
                if self.seat(*player) == p && s.name() == by && !self.consumed.contains(&k) {
                    self.consumed.insert(k);
                    names.extend(cards.iter().map(|c| c.name().to_string()));
                }
            }
            // "reveals [Remand].": a card looked at and then revealed,
            // which the log doesn't say what by (Delver of Secrets's).
            if let Event::Reveals {
                player,
                cards,
                how: log::Reveal::Plain,
            } = &e.event
            {
                if self.seat(*player) == p && looked && !self.consumed.contains(&k) {
                    self.consumed.insert(k);
                    names.extend(cards.iter().map(|c| c.name().to_string()));
                }
            }
        }
        if names.len() != count {
            return Err(Stop::Diverged(Divergence::Unlogged {
                what: format!("the cards {by} reveals"),
            }));
        }
        let texts = names
            .iter()
            .map(|n| self.text(n))
            .collect::<Result<Vec<_>, _>>()?;
        self.act(Answer::Reveal(texts))
    }

    /// `p`'s hand, revealed by a resolving instruction with hidden cards in
    /// it (CR 701.20a): the log's "reveals their hand to [Cabal Therapy],
    /// containing …" line, from the line being followed to the end of the
    /// turn, names all of it, the hidden cards the names no known card
    /// takes, and the engine's hand must have as many cards.
    fn reveal_hand(&mut self, p: PlayerId) -> Followed {
        let game = self.game.as_ref().expect("set up");
        let by = game
            .resolving()
            .map(|c| self.name_of(c))
            .unwrap_or_default();
        let said = self
            .entries
            .iter()
            .enumerate()
            .skip(self.at)
            .take_while(|(k, e)| *k == self.at || !matches!(e.event, Event::TurnBegins(_)))
            .find_map(|(k, e)| match &e.event {
                Event::Reveals {
                    player,
                    cards,
                    how: log::Reveal::HandTo(log::Source::Card(s)),
                } if self.seat(*player) == p && s.name() == by && !self.consumed.contains(&k) => {
                    Some((
                        k,
                        cards
                            .iter()
                            .map(|c| c.name().to_string())
                            .collect::<Vec<_>>(),
                    ))
                }
                _ => None,
            });
        let Some((k, names)) = said else {
            return Err(Stop::Diverged(Divergence::Unlogged {
                what: format!("the hand revealed to {by}"),
            }));
        };
        let names = split_halves_once(names);
        let hand = game.player(p).hand().clone();
        if names.len() != hand.len() {
            return Err(Stop::Diverged(Divergence::HandSize {
                log: names.len(),
                engine: hand.len(),
            }));
        }
        // The known cards take their names first; the rest name the
        // hidden ones, in order.
        let mut left: Vec<CardId> = hand.iter().copied().collect();
        let mut unnamed = Vec::new();
        for n in &names {
            let text = self.text(n)?;
            let game = self.game.as_ref().expect("set up");
            match left
                .iter()
                .position(|c| game.card(*c).is_some_and(|t| t.name == text.name))
            {
                Some(at) => {
                    left.remove(at);
                }
                None => unnamed.push(text),
            }
        }
        let game = self.game.as_ref().expect("set up");
        if left.iter().any(|c| game.card(*c).is_some()) {
            let card = left
                .iter()
                .find_map(|c| game.card(*c))
                .map(|t| t.name.clone())
                .unwrap_or_default();
            return Err(Stop::Diverged(Divergence::NotInHand { card }));
        }
        self.consumed.insert(k);
        self.act(Answer::Reveal(unnamed))
    }

    /// A card `p` puts onto the battlefield from their hand, which the log
    /// never names as it happens: the first card of theirs a later line
    /// shows on the battlefield (a trigger's source, an ability activated,
    /// an attacker or blocker) more times than they have permanents of it
    /// already and lines have cast or played it since: a second Goblin
    /// Warchief attacking beside the one cast after the trigger. None is an
    /// error, so a "may" that put it goes back and declines.
    fn put_from_hand(&mut self, p: PlayerId) -> Followed {
        let game = self.game.as_ref().expect("set up");
        let chars = game.characteristics();
        let on_battlefield = |n: &str| {
            game.permanents()
                .iter()
                .filter(|(id, perm)| {
                    chars[id].controller == p && game.card(perm.card).is_some_and(|t| t.name == n)
                })
                .count()
        };
        let mut explained: BTreeMap<String, usize> = BTreeMap::new();
        let mut candidates: Vec<String> = Vec::new();
        for e in self.entries.iter().skip(self.at) {
            let mine = |q: &log::Player| self.seat(*q) == p;
            let named: Vec<String> = match &e.event {
                Event::Casts(c) if mine(&c.player) => {
                    if let Object::Card(c) = &c.card {
                        *explained.entry(c.name().to_string()).or_default() += 1;
                    }
                    continue;
                }
                Event::PlaysLand {
                    player,
                    card: Object::Card(c),
                } if mine(player) => {
                    *explained.entry(c.name().to_string()).or_default() += 1;
                    continue;
                }
                Event::Trigger(t) if mine(&t.player) => {
                    source_name(t).map(str::to_string).into_iter().collect()
                }
                Event::Activates(a) if mine(&a.player) => match &a.card {
                    Object::Card(c) => vec![c.name().to_string()],
                    _ => vec![],
                },
                Event::Attacked {
                    defender: log::Defender::Player(d),
                    attackers: log::Attackers::Seen(xs),
                } if !mine(d) => xs
                    .iter()
                    .filter_map(|x| match x {
                        Object::Card(c) => Some(c.name().to_string()),
                        _ => None,
                    })
                    .collect(),
                _ => vec![],
            };
            for n in &named {
                let shown = named.iter().filter(|m| *m == n).count();
                let had = on_battlefield(n) + explained.get(n).copied().unwrap_or(0);
                if shown > had && !candidates.contains(n) {
                    candidates.push(n.clone());
                }
            }
        }
        for n in candidates {
            let Ok(text) = self.text(&n) else { continue };
            let mut tried = self.game().clone();
            let hand = tried.player(p).hand().clone();
            let c = match hand
                .iter()
                .find(|c| tried.card(**c).is_some_and(|t| t.name == text.name))
            {
                Some(c) => *c,
                None => {
                    let Some(c) = hand.iter().find(|c| tried.card(**c).is_none()) else {
                        continue;
                    };
                    tried.reveal(*c, text).expect("a hidden card");
                    *c
                }
            };
            if tried.step(Answer::Pick(Some(c))).is_ok() {
                *self.game() = tried;
                let at = self.at;
                self.did(at, format!("put {n} onto the battlefield from the hand"));
                return Ok(());
            }
        }
        Err(Stop::Diverged(Divergence::Unlogged {
            what: "a card put onto the battlefield from a hand".into(),
        }))
    }

    /// The payments the solver finds for the spell being cast, best first.
    fn payments(&mut self) -> Result<Vec<Vec<ManaActivation>>, Stop> {
        let game = self.game();
        let Decision::Pay { player, owed, .. } = game.decision() else {
            unreachable!("a cast waits for its payment")
        };
        let pool = *game.player(player).pool();
        let mut found = solve(&owed, pool, &unlogged_mana(game, player));
        // A permanent a later line this turn attacks, blocks or activates
        // with is tapped for mana last: tapped, it couldn't.
        let used: BTreeSet<String> = self
            .entries
            .iter()
            .skip(self.at + 1)
            .take_while(|e| !matches!(e.event, Event::TurnBegins(_)))
            .flat_map(|e| match &e.event {
                Event::Attacked {
                    attackers: log::Attackers::Seen(xs),
                    ..
                } => xs.iter().collect::<Vec<_>>(),
                Event::Blocks { blocker, .. } => vec![blocker],
                Event::Activates(a) => vec![&a.card],
                _ => Vec::new(),
            })
            .filter_map(|o| match o {
                Object::Card(c) => Some(c.name().to_string()),
                _ => None,
            })
            .collect();
        let game = self.game.as_ref().expect("set up");
        let named = |o: &ObjectId| {
            game.permanents()
                .get(o)
                .and_then(|p| game.card(p.card))
                .is_some_and(|t| used.contains(&t.name))
        };
        // And a permanent sacrificed for mana is the last resort.
        found.sort_by_key(|p| {
            (
                p.iter().filter(|a| a.sacrifices.is_some()).count(),
                p.iter()
                    .filter(|a| named(&a.source) || a.sacrifices.as_ref().is_some_and(named))
                    .count(),
            )
        });
        Ok(found)
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
                Decision::Priority { .. }
                | Decision::Search { .. }
                | Decision::PayOrEnterTapped { .. } => self.pass()?,
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

fn source_name(t: &log::Trigger) -> Option<&str> {
    match &t.source {
        log::Source::Card(c) => Some(c.name()),
        log::Source::Named(_) => None,
    }
}

/// A permanent as a line calls it: by its name, or a face-down one by the
/// card the replay cast face down as it, since it has no name (CR 708.2a).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Called {
    Name(String),
    FaceDown(CardId),
}

impl fmt::Display for Called {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Called::Name(n) => f.write_str(n),
            Called::FaceDown(_) => f.write_str("a face-down creature"),
        }
    }
}

/// Whether the permanent is the one a line calls so.
fn is_called(game: &Game, id: &ObjectId, called: &Called) -> bool {
    let Some(perm) = game.permanents().get(id) else {
        return false;
    };
    match called {
        Called::Name(n) => !perm.face_down && game.card(perm.card).is_some_and(|t| &t.name == n),
        Called::FaceDown(c) => perm.face_down && perm.card == *c,
    }
}

/// "Morph {2}{W}{W}": a card that can be cast face down.
fn has_morph(text: &CardText) -> bool {
    matches!(&text.oracle, crate::deck::Oracle::OneFace(t)
        if t.lines().any(|l| l.starts_with("Morph {") || l.starts_with("Morph\u{2014}")))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Land,
    Cast(Route),
}

/// How a line's spell is cast: for its mana cost, from a hand, or for an
/// alternative cost from the zone it allows (CR 118.9).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Route {
    ManaCost,
    Alternative(CastFrom),
}

impl Kind {
    fn verb(self) -> &'static str {
        match self {
            Kind::Land => "play",
            Kind::Cast(_) => "cast",
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
    let (Decision::Pay { actions, .. } | Decision::PayUnless { actions, .. }) = game.decision()
    else {
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

/// The mana abilities a payment may activate without a line saying so:
/// not one that puts a counter on its source, whose line the log prints as
/// it is activated (Wall of Roots, `counter_mana`).
fn unlogged_mana(game: &Game, p: PlayerId) -> Vec<ManaActivation> {
    game.mana_options(p)
        .into_iter()
        .filter(|a| game.mana_counter(a).is_none() && game.mana_removes(a).is_none())
        .collect()
}

/// A revealed hand as the log lists it, with a split card's halves, which
/// some logs list beside it ("[Ice], [Fire], [Fire // Ice]"), dropped: one
/// card, not three.
fn split_halves_once(names: Vec<String>) -> Vec<String> {
    let mut out = names.clone();
    for n in &names {
        if let Some((a, b)) = n.split_once(" // ") {
            for half in [a, b] {
                if let Some(k) = out.iter().position(|x| x == half) {
                    out.remove(k);
                }
            }
        }
    }
    out
}

/// A counter as the log names its kind: "+1/+1", "-0/-1", or a name.
fn counter_kind(kind: &str) -> Counter {
    let pt = kind.split_once('/').and_then(|(a, b)| {
        let n = |s: &str| s.trim_start_matches('+').parse::<i32>().ok();
        Some((n(a)?, n(b)?))
    });
    match pt {
        Some((1, 1)) => Counter::PlusOne,
        Some((-1, -1)) => Counter::MinusOne,
        Some((a, b)) => Counter::Pt(a, b),
        None => Counter::Named(kind.to_string()),
    }
}

/// How many of the words a log prints of an ability, cut off at "...",
/// its Oracle text has after its cost.
fn shared_words(said: &str, oracle: &str) -> usize {
    let words = |t: &str| -> Vec<String> {
        t.split(|c: char| c.is_whitespace() || matches!(c, '.' | ',' | '(' | ')'))
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    let effect = oracle.split_once(": ").map_or(oracle, |(_, e)| e);
    let theirs = words(effect);
    words(said.trim_end_matches("..."))
        .iter()
        .filter(|w| theirs.contains(w))
        .count()
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
fn solve(owed: &ManaCost, pool: ManaPool, options: &[ManaActivation]) -> Vec<Vec<ManaActivation>> {
    // An ability that sacrifices another permanent can be activated again
    // for another (Skirk Prospector's "Sacrifice a Goblin"): each choice of
    // what it sacrifices is a source of its own.
    let mut sources: BTreeMap<(ObjectId, Option<ObjectId>), Vec<ManaActivation>> = BTreeMap::new();
    for m in options {
        let other = m.sacrifices.filter(|o| *o != m.source);
        sources.entry((m.source, other)).or_default().push(*m);
    }
    // Sources that cost mana last, so the mana they spend is made first.
    let mut sources: Vec<Vec<ManaActivation>> = sources.into_values().collect();
    sources.sort_by_key(|v| v.iter().any(|a| !a.pays.is_empty()));
    let filters = sources
        .iter()
        .filter(|v| v.iter().any(|a| !a.pays.is_empty()))
        .count();
    let first_filter = sources.len() - filters;
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
        first_filter: usize,
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
            if i == self.sources.len() {
                return;
            }
            // Enough sources need no more, though the wrong kind of mana
            // can still be filtered: each source spends at least one mana
            // of a payment, so one needs no more sources than its mana
            // symbols, however much each makes (Wild Growth's Forest).
            if chosen.len() as u32 >= self.most && i < self.first_filter {
                if self.first_filter < self.sources.len() {
                    self.go(self.first_filter, pool, chosen);
                }
                return;
            }
            for a in &self.sources[i] {
                if !pool.contains(&a.pays) {
                    continue;
                }
                let mut more = pool.minus(&a.pays);
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
        first_filter,
        visited: 0,
        out: Vec::new(),
    };
    s.go(0, pool, &mut Vec::new());
    let per_source: HashMap<ObjectId, usize> = sources
        .iter()
        .filter_map(|v| Some((v.first()?.source, v.len())))
        .collect();
    let mut out = s.out;
    out.sort_by_key(|p| {
        (
            p.len(),
            p.iter().map(|a| per_source[&a.source]).sum::<usize>(),
        )
    });
    // Then each with a filter land run on mana it leaves, the more mana
    // floating for a later spell this step (Flooded Grove's {G}{G} from
    // one {G}, paying two Fyndhorn Elves): never the first tried, as
    // nothing so far needs it.
    let filters: Vec<&ManaActivation> = options.iter().filter(|a| !a.pays.is_empty()).collect();
    let mut floated = Vec::new();
    for p in out.iter().take(8) {
        let mut after = pool;
        for a in p {
            after = after.minus(&a.pays);
            after.add_all(&a.adds);
        }
        for f in &filters {
            if p.iter().any(|a| a.source == f.source) || !after.contains(&f.pays) {
                continue;
            }
            let mut more = after.minus(&f.pays);
            more.add_all(&f.adds);
            if more.total() > after.total() && !owed.payments(&more).is_empty() {
                let mut with = p.clone();
                with.push(**f);
                floated.push(with);
            }
        }
    }
    out.extend(floated);
    // Sacrifices last, a source's of itself after any of another, so
    // nothing sacrificed is needed for a later activation.
    for p in &mut out {
        p.sort_by_key(|a| match a.sacrifices {
            None => 0,
            Some(o) if o != a.source => 1,
            Some(_) => 2,
        });
    }
    out
}
