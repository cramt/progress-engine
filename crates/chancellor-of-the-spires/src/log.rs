//! PDBot's Magic Online logs, read into typed events.
//!
//! logs.pennydreadfulmagic.com publishes the client's own log of every game
//! PDBot watched (ADR-0043, "Real games replayed are the acceptance test").
//! A log is a header (format, comment, flags, players), then each game: a
//! `== Game N (id) ==` line and timestamped lines, one event each. This
//! module is text in, events out, with no I/O, so it builds for wasm too;
//! `chancellor-logs` fetches and measures.
//!
//! The log is untrusted text. A line no shape here reads is kept as
//! [`Event::Unrecognised`] rather than dropped, so a game that parsed whole
//! is one with none of them.

use std::fmt;

/// One match: the header and its games.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// `Freeform`, `PennyDreadful`, …: the room it was played in.
    pub format: String,
    /// The comment the players gave the match, often the real format.
    pub comment: String,
    pub flags: Vec<String>,
    /// The players, in the header's order. A [`Player`] indexes this.
    pub players: Vec<String>,
    pub games: Vec<Game>,
    /// `Match Winner: <player>: 2–1`, when the log has it.
    pub winner: Option<(Player, String)>,
}

/// A player of the match, as an index into [`Match::players`]. Every
/// event's actor is one: a line whose subject isn't a player of the match
/// is unrecognised, never a player made up on the spot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Player(pub u8);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    /// The game's number within the match, from 1.
    pub number: u32,
    /// Magic Online's id for the game.
    pub id: u64,
    /// `[Time]`, seconds since the epoch.
    pub started: Option<u64>,
    /// `[Gatherling]` and `[League]` lines: the event it was part of.
    pub notes: Vec<String>,
    pub entries: Vec<Entry>,
    /// `Winner: <player>`.
    pub winner: Option<Player>,
    /// `Game N Completed.`
    pub completed: bool,
}

impl Game {
    /// Lines no shape here reads.
    pub fn unrecognised(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().filter_map(|e| match &e.event {
            Event::Unrecognised(l) => Some(l.as_str()),
            _ => None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The line's timestamp; a line the log printed without one has none.
    pub time: Option<Clock>,
    /// The turn it happened in; `None` before the first turn (the
    /// mulligans, who plays first).
    pub turn: Option<Turn>,
    pub event: Event,
    /// The line as the log has it, after the timestamp.
    pub text: String,
}

/// `[HH:MM:SS]`, the watcher's wall clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Clock {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// `Turn 3: alice`. Magic Online numbers each player's turns, so both
/// players have a turn 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Turn {
    pub number: u32,
    pub active: Player,
    pub extra: bool,
}

/// A card as the log names it: `[Lightning Bolt]`, a token's
/// `[Elemental Token]`, or one face of a double-faced card.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Card(String);

impl Card {
    /// Some logs carry names encoded twice (`JÃ\u{83}Â¶tun Grunt`); the
    /// name is the one the card has ("Jötun Grunt").
    pub fn new(name: &str) -> Card {
        Card(repair_encoding(name))
    }

    pub fn name(&self) -> &str {
        &self.0
    }

    /// The log names tokens "<Subtypes> Token".
    pub fn is_token(&self) -> bool {
        self.0.ends_with(" Token")
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Text that is UTF-8 read as Latin-1 and encoded again, undone as many
/// times as it was done.
fn repair_encoding(s: &str) -> String {
    let mut s = s.to_string();
    for _ in 0..3 {
        if s.is_ascii() || s.chars().any(|c| c as u32 > 0xFF) {
            break;
        }
        let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
        match String::from_utf8(bytes) {
            Ok(t) if t != s => s = t,
            _ => break,
        }
    }
    s
}

/// A permanent whose name the log hides: "face-down creature #1".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hidden {
    FaceDown(u32),
    Manifested(u32),
    Disguised(u32),
}

/// A card on the battlefield or the stack: named, or hidden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Object {
    Card(Card),
    Hidden(Hidden),
    /// A player's name where a card's goes: the log does this for some
    /// cards it can't name (a companion cast from outside the game, a card
    /// played face down), so the card is unknown but whose it is isn't.
    Unnamed(Player),
}

/// What a spell or ability targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Object(Object),
    Player(Player),
    /// Older logs name some targets without brackets: "Expedition Map",
    /// "Triggered ability from Goblin Lackey".
    Named(String),
    /// "targeting Nothing and [X]": a target slot left empty.
    Nothing,
}

/// A target, and what the log says beside it: the mode it is for
/// ("(Kolaghan's Command deals 2 damage to any target.)") or the damage
/// divided to it ("for 2").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aim {
    pub target: Target,
    pub note: Option<String>,
}

/// Where an ability or effect came from: a card ("[Relic of
/// Progenitus]'s ability" too), or a name the log leaves unbracketed
/// ("Teferi's Emblem", "Mishra's Bauble", "Triggered ability from
/// Spellstutter Sprite").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Card(Card),
    Named(String),
}

/// Who is attacked: a player, or a planeswalker or battle someone controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Defender {
    Player(Player),
    Permanent { controller: Player, card: Card },
}

/// The attackers, or only how many when the log says "17 cards".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attackers {
    Seen(Vec<Object>),
    Unseen(u32),
}

/// Where in a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Top,
    Bottom,
    /// "beneath the top 2 cards".
    Beneath(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// A step a player skips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Untap,
    Draw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Battlefield,
    Graveyard,
}

/// Cards moved where the log shows them, or only how many.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Moved {
    Seen(Vec<Card>),
    Unseen(u32),
}

/// How a revealed card was revealed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reveal {
    Plain,
    AtRandom,
    /// By a spell or ability: "with [Thoughtseize]".
    With(Source),
    /// "reveals N cards with [Shardless Agent]: …" (cascade, a look at a
    /// hand), with the count the log gives.
    Many {
        count: u32,
        with: Source,
    },
    /// "reveals their hand to [Agonizing Remorse], containing …".
    HandTo(Source),
    OpeningHand,
    /// "reveals [X] and puts it on the bottom of their library."
    ToBottom,
    /// "reveals [X] and shuffles it into their library."
    ShuffledIn,
}

/// What exiled a card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExiledBy {
    Source(Source),
    /// "with its own ability".
    Itself,
    /// Suspend: "exiles [Rift Bolt] with 1 time counter."
    Suspend(u32),
}

/// What a player chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Cards, perhaps none ("chooses no cards").
    Cards(Vec<Card>),
    /// A colour, a creature type, a land's name, "Face up".
    Word(String),
}

/// Counters on one object or player: "two loyalty counters on [X]".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counted {
    pub count: u32,
    pub kind: String,
    pub on: Target,
}

/// A spell cast, as the log gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cast {
    pub player: Player,
    pub card: Object,
    /// How it was cast when not plainly from hand for its mana cost, in
    /// the log's words: "without paying its mana cost with suspend", "with
    /// Flashback {1B} from the graveyard", "by paying {2}", "with kicker".
    pub via: Option<String>,
    /// The modes chosen, each in quotes in the log.
    pub modes: Vec<String>,
    /// "revealing [Master of Waves]": cards revealed to cast it.
    pub revealing: Vec<Card>,
    pub targets: Vec<Aim>,
    /// "(X is 3.)"
    pub x: Option<u32>,
    /// Older logs print the spell's text after its targets.
    pub text: Option<String>,
}

/// An ability activated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activation {
    pub player: Player,
    pub card: Object,
    /// The ability's text as the log prints it, often cut off with "...".
    pub ability: String,
    pub modes: Vec<String>,
    pub targets: Vec<Aim>,
    pub x: Option<u32>,
}

/// A triggered ability put on the stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    pub player: Player,
    pub source: Source,
    /// The ability's text, which older logs leave out.
    pub ability: Option<String>,
    pub modes: Vec<String>,
    pub targets: Vec<Aim>,
}

/// One line of a game. A variant a player does names them as `player`
/// (or `controller`); the rest happen to cards (blocks, transformations)
/// or to the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    // Around the game.
    /// `[CHAT] name: text`. The speaker is a name, not a [`Player`]: anyone
    /// watching can talk, PDBot included, and PDBot's notes run on over the
    /// untimestamped lines after.
    Chat {
        speaker: String,
        text: String,
    },
    /// "X has started watching." / "X has stopped watching."
    Watching {
        who: String,
        started: bool,
    },
    Joined(Player),
    Left(Player),
    Disconnected(Player),
    /// The client's own complaints: "Illegal attack.", "The vote is in!".
    Notice(String),
    /// A line naming a player and nothing else ("alice.").
    Blank(Player),

    // Before the first turn.
    PlaysFirst {
        player: Player,
        first: bool,
    },
    Mulligan {
        player: Player,
        to: u32,
    },
    /// "begins the game with N cards in hand", having put `bottomed` on
    /// the bottom first.
    Keeps {
        player: Player,
        hand: u32,
        bottomed: u32,
    },

    // Turns.
    TurnBegins(Turn),
    Skips {
        player: Player,
        step: Step,
    },
    TakesExtraTurn(Player),
    /// "It becomes day." (`true`) / "It becomes night."
    Day(bool),

    // Spells and abilities.
    Casts(Cast),
    /// "Cast face-down with Morph."
    CastsFaceDown {
        player: Player,
        how: String,
    },
    Activates(Activation),
    Trigger(Trigger),
    /// "Couldn't put Triggered Ability from [X] on the stack."
    TriggerFailed(Card),
    /// "alice's [X]'s triggered ability does not resolve because the
    /// intervening "if" condition is not met."
    InterveningIfFails {
        controller: Player,
        card: Card,
    },
    /// "Chosen mode: …", printed after the spell.
    ChosenMode(String),
    /// "chooses to use [X]'s ability" / "declines to use".
    UsesAbility {
        player: Player,
        card: Card,
        uses: bool,
    },
    Counters {
        player: Player,
        spell: Source,
        with: Source,
    },
    RemovedForNoTargets {
        player: Player,
        spell: Source,
    },
    ChangesTargets {
        player: Player,
        spell: Card,
        to: Vec<Aim>,
    },
    /// "alice targets bob for Triggered ability from Oath of Druids."
    Targets {
        player: Player,
        target: Target,
        for_source: Source,
    },
    AppliesReplacement {
        player: Player,
        source: Object,
    },
    /// "chooses blue for [Tarnation Vista]", "chooses [Slaughter Pact].",
    /// "chooses [X] with [Y]'s ability".
    Chooses {
        player: Player,
        choice: Choice,
        by: Option<Source>,
    },
    ChoosesDamageSource {
        player: Player,
        card: Card,
    },
    Names {
        player: Player,
        name: String,
        for_source: Source,
    },
    Flips {
        player: Player,
        heads: u32,
        tails: u32,
    },
    /// "The chosen player is bob"
    ChosenPlayer(Player),
    /// "kept a card." (Sylvan Library)
    KeepsCards {
        player: Player,
        count: u32,
    },
    PaysEcho {
        player: Player,
        card: Card,
    },
    PaysLife {
        player: Player,
        amount: u32,
    },
    /// "alice's [Bandit's Talent] becomes level 2."
    Level {
        controller: Player,
        card: Card,
        level: u32,
    },
    /// "won the flip." (`true`) / "lost the flip."
    FlipResult {
        player: Player,
        won: bool,
    },
    /// "is tempted by the Ring. [X] becomes their Ring-bearer."
    Tempted {
        player: Player,
        bearer: Option<Card>,
    },

    // Combat.
    Attacked {
        defender: Defender,
        attackers: Attackers,
    },
    Blocks {
        blocker: Object,
        attacker: Object,
    },
    /// "Blockers for [X] are ordered as follows: [Y], [Z]"
    BlockersOrdered {
        attacker: Object,
        blockers: Vec<Object>,
    },
    StopsBlocking(Card),
    Fight(Card, Card),

    // Cards moving.
    Draws {
        player: Player,
        count: u32,
        with: Option<Source>,
    },
    PlaysLand {
        player: Player,
        card: Object,
    },
    Discards {
        player: Player,
        cards: Vec<Object>,
    },
    /// "discards [Loxodon Smiter] but it enters the battlefield instead."
    DiscardedOntoBattlefield {
        player: Player,
        card: Card,
    },
    Reveals {
        player: Player,
        cards: Vec<Card>,
        how: Reveal,
    },
    /// "reveals face-down creature #1 as Exalted Angel."
    RevealsHidden {
        player: Player,
        object: Hidden,
        card: Card,
    },
    TurnsFaceUp {
        player: Player,
        object: Hidden,
    },
    Mills {
        player: Player,
        cards: Moved,
    },
    Scry {
        player: Player,
        count: u32,
        top: u32,
        bottom: u32,
    },
    PutsInLibrary {
        player: Player,
        cards: Moved,
        owner: Player,
        end: End,
        by: Option<Source>,
    },
    PutsInGraveyard {
        player: Player,
        cards: Vec<Card>,
        then_top: u32,
    },
    PutsInHand {
        player: Player,
        cards: Vec<Card>,
        by: Source,
    },
    PutsOntoBattlefield {
        player: Player,
        card: Card,
        attacking: Option<Target>,
    },
    Exiles {
        player: Player,
        cards: Vec<Card>,
        by: Option<ExiledBy>,
    },
    /// "returns [X] to its owner's hand with [Y]('s ability)"; no cards
    /// when the log says "returns no cards".
    ReturnsToHand {
        player: Player,
        cards: Vec<Card>,
        with: Source,
    },
    ReturnsToBattlefield {
        player: Player,
        card: Card,
        with: Source,
    },
    /// "[X] is returned to alice's hand from the battlefield."
    ReturnedToHand {
        card: Card,
        owner: Player,
        from: Zone,
    },
    /// "returns [Life from the Loam] to their hand with its dredge ability."
    Dredges {
        player: Player,
        card: Card,
    },
    /// "alice's [Greenbelt Rampager] returns to their hand and they get 1
    /// energy."
    ReturnsForEnergy {
        player: Player,
        card: Card,
        energy: u32,
    },
    Sacrifices {
        player: Player,
        card: Card,
    },
    SacrificedAtRandom {
        controller: Player,
        card: Card,
    },
    Cycles {
        player: Player,
        card: Card,
    },
    Transmutes {
        player: Player,
        card: Card,
    },
    GainsControl {
        player: Player,
        card: Card,
        from: Player,
    },
    Creates {
        player: Player,
        source: Card,
        count: u32,
        what: String,
        under: Option<Player>,
    },
    CreatesEmblem {
        player: Player,
        source: Card,
        text: String,
    },
    Transforms {
        from: Card,
        to: Card,
    },
    Investigates(Player),
    Incubates {
        player: Player,
        count: u32,
    },
    Explores(Card),
    Connives(Card),
    Trains {
        controller: Player,
        card: Card,
    },
    Plotted(Card),
    /// "[X] prepares Bind to Life".
    Prepares {
        card: Card,
        spell: String,
    },
    UnlocksDoor {
        player: Player,
        card: Card,
        side: Side,
    },
    Beholds {
        player: Player,
        cards: Vec<Card>,
        with: Source,
    },
    Forages {
        player: Player,
        cards: Vec<Card>,
        with: Source,
    },
    /// "blights [Goblin Matron] 1".
    Blights {
        player: Player,
        card: Card,
        count: u32,
    },
    /// "is protecting bob's [Invasion of Ixalan]": a battle's protector.
    Protects {
        player: Player,
        controller: Player,
        card: Card,
    },

    // Counters, life, damage, the library.
    PutsCounters {
        player: Player,
        counters: Vec<Counted>,
    },
    RemovesCounters {
        player: Player,
        counters: Vec<Counted>,
    },
    GainsLife {
        player: Player,
        amount: u32,
    },
    LosesLife {
        player: Player,
        amount: u32,
    },
    /// "gains 1 energy from [Glint-Sleeve Siphoner]'s ability."
    GainsEnergy {
        player: Player,
        amount: u32,
        from: Option<Source>,
    },
    /// "[X] deals 3 damage to bob"
    Deals {
        source: Card,
        amount: u32,
        to: Target,
    },
    /// "bob is dealt 2 points of damage."
    IsDealt {
        player: Player,
        amount: u32,
    },
    DamagePrevented {
        amount: u32,
        source: Card,
        to: Target,
    },
    DamageRedirected {
        amount: u32,
        source: Card,
        from: Target,
        to: Target,
    },
    SearchPrevented(Player),
    FailsToFind {
        player: Player,
        what: String,
    },
    Shuffles(Player),

    // The end.
    Concedes(Player),
    Wins(Player),
    Loses(Player),
    LosesToInaction(Player),
    /// "has run out of time and has lost the match."
    OutOfTime(Player),

    /// A line no shape here reads, kept whole.
    Unrecognised(String),
}

impl Event {
    /// Every card the event names in brackets, tokens included.
    pub fn cards(&self) -> Vec<&Card> {
        use Event::*;
        let mut out: Vec<&Card> = Vec::new();
        let o = &mut out;
        match self {
            Chat { .. }
            | Watching { .. }
            | Joined(_)
            | Left(_)
            | Disconnected(_)
            | Notice(_)
            | Blank(_)
            | PlaysFirst { .. }
            | ChosenPlayer(_)
            | KeepsCards { .. }
            | PaysLife { .. }
            | Mulligan { .. }
            | Keeps { .. }
            | TurnBegins(_)
            | Skips { .. }
            | TakesExtraTurn(_)
            | Day(_)
            | CastsFaceDown { .. }
            | ChosenMode(_)
            | Flips { .. }
            | FlipResult { .. }
            | TurnsFaceUp { .. }
            | Scry { .. }
            | Investigates(_)
            | Incubates { .. }
            | GainsLife { .. }
            | LosesLife { .. }
            | IsDealt { .. }
            | SearchPrevented(_)
            | FailsToFind { .. }
            | Shuffles(_)
            | Concedes(_)
            | Wins(_)
            | Loses(_)
            | LosesToInaction(_)
            | OutOfTime(_)
            | Unrecognised(_) => {}
            Casts(c) => {
                of_object(&c.card, o);
                o.extend(&c.revealing);
                of_aims(&c.targets, o);
            }
            Activates(a) => {
                of_object(&a.card, o);
                of_aims(&a.targets, o);
            }
            Trigger(t) => {
                of_source(&t.source, o);
                of_aims(&t.targets, o);
            }
            TriggerFailed(c)
            | StopsBlocking(c)
            | Explores(c)
            | Connives(c)
            | Plotted(c)
            | InterveningIfFails { card: c, .. }
            | UsesAbility { card: c, .. }
            | ChoosesDamageSource { card: c, .. }
            | PaysEcho { card: c, .. }
            | Level { card: c, .. }
            | DiscardedOntoBattlefield { card: c, .. }
            | RevealsHidden { card: c, .. }
            | ReturnedToHand { card: c, .. }
            | Dredges { card: c, .. }
            | ReturnsForEnergy { card: c, .. }
            | Sacrifices { card: c, .. }
            | SacrificedAtRandom { card: c, .. }
            | Cycles { card: c, .. }
            | Transmutes { card: c, .. }
            | GainsControl { card: c, .. }
            | Creates { source: c, .. }
            | CreatesEmblem { source: c, .. }
            | Trains { card: c, .. }
            | Prepares { card: c, .. }
            | UnlocksDoor { card: c, .. }
            | Blights { card: c, .. }
            | Protects { card: c, .. } => o.push(c),
            PutsOntoBattlefield {
                card: c, attacking, ..
            } => {
                o.push(c);
                if let Some(t) = attacking {
                    of_target(t, o);
                }
            }
            Counters { spell, with, .. } => {
                of_source(spell, o);
                of_source(with, o);
            }
            RemovedForNoTargets { spell, .. } => of_source(spell, o),
            ChangesTargets { spell, to, .. } => {
                o.push(spell);
                of_aims(to, o);
            }
            Targets {
                target: t,
                for_source,
                ..
            } => {
                of_target(t, o);
                of_source(for_source, o);
            }
            AppliesReplacement { source: s, .. } => of_object(s, o),
            Chooses { choice, by, .. } => {
                if let Choice::Cards(cs) = choice {
                    o.extend(cs);
                }
                if let Some(s) = by {
                    of_source(s, o);
                }
            }
            Names { for_source, .. } => of_source(for_source, o),
            GainsEnergy { from, .. } => {
                if let Some(s) = from {
                    of_source(s, o);
                }
            }
            Tempted { bearer, .. } => o.extend(bearer),
            Attacked {
                defender,
                attackers,
            } => {
                if let Defender::Permanent { card, .. } = defender {
                    o.push(card);
                }
                if let Attackers::Seen(os) = attackers {
                    os.iter().for_each(|x| of_object(x, o));
                }
            }
            Blocks { blocker, attacker } => {
                of_object(blocker, o);
                of_object(attacker, o);
            }
            BlockersOrdered { attacker, blockers } => {
                of_object(attacker, o);
                blockers.iter().for_each(|x| of_object(x, o));
            }
            PlaysLand { card, .. } => of_object(card, o),
            Discards { cards, .. } => cards.iter().for_each(|x| of_object(x, o)),
            Fight(a, b) | Transforms { from: a, to: b } => {
                o.push(a);
                o.push(b);
            }
            Draws { with, .. } => {
                if let Some(s) = with {
                    of_source(s, o);
                }
            }
            PutsInGraveyard { cards, .. } => o.extend(cards),
            Reveals { cards, how, .. } => {
                o.extend(cards);
                if let Reveal::With(s) | Reveal::Many { with: s, .. } | Reveal::HandTo(s) = how {
                    of_source(s, o);
                }
            }
            Mills { cards, .. } => of_moved(cards, o),
            PutsInLibrary { cards, by, .. } => {
                of_moved(cards, o);
                if let Some(s) = by {
                    of_source(s, o);
                }
            }
            PutsInHand { cards, by: s, .. }
            | ReturnsToHand { cards, with: s, .. }
            | Beholds { cards, with: s, .. }
            | Forages { cards, with: s, .. } => {
                o.extend(cards);
                of_source(s, o);
            }
            ReturnsToBattlefield { card, with, .. } => {
                o.push(card);
                of_source(with, o);
            }
            Exiles { cards, by, .. } => {
                o.extend(cards);
                if let Some(ExiledBy::Source(s)) = by {
                    of_source(s, o);
                }
            }
            PutsCounters { counters, .. } | RemovesCounters { counters, .. } => {
                counters.iter().for_each(|c| of_target(&c.on, o))
            }
            Deals { source: c, to, .. } | DamagePrevented { source: c, to, .. } => {
                o.push(c);
                of_target(to, o);
            }
            DamageRedirected {
                source: c,
                from,
                to,
                ..
            } => {
                o.push(c);
                of_target(from, o);
                of_target(to, o);
            }
        }
        out
    }
}

fn of_aims<'a>(a: &'a [Aim], o: &mut Vec<&'a Card>) {
    a.iter().for_each(|a| of_target(&a.target, o));
}

fn of_target<'a>(t: &'a Target, o: &mut Vec<&'a Card>) {
    if let Target::Object(x) = t {
        of_object(x, o);
    }
}

fn of_object<'a>(x: &'a Object, o: &mut Vec<&'a Card>) {
    if let Object::Card(c) = x {
        o.push(c);
    }
}

fn of_source<'a>(s: &'a Source, o: &mut Vec<&'a Card>) {
    if let Source::Card(c) = s {
        o.push(c);
    }
}

fn of_moved<'a>(m: &'a Moved, o: &mut Vec<&'a Card>) {
    if let Moved::Seen(cs) = m {
        o.extend(cs);
    }
}

/// Why a log isn't a match at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Malformed {
    /// The four header lines aren't there.
    NoHeader,
    /// More players than a [`Player`] can index.
    TooManyPlayers(usize),
}

impl fmt::Display for Malformed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Malformed::NoHeader => f.write_str("no format, comment, flags and players header"),
            Malformed::TooManyPlayers(n) => write!(f, "{n} players"),
        }
    }
}

/// A whole export: the header, then each game.
pub fn parse(text: &str) -> Result<Match, Malformed> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    if lines.len() < 4 {
        return Err(Malformed::NoHeader);
    }
    let players: Vec<String> = lines[3]
        .split(',')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    if players.len() > u8::MAX as usize {
        return Err(Malformed::TooManyPlayers(players.len()));
    }
    let mut m = Match {
        format: lines[0].to_string(),
        comment: lines[1].to_string(),
        flags: lines[2]
            .split(',')
            .filter(|f| !f.is_empty())
            .map(str::to_string)
            .collect(),
        players,
        games: Vec::new(),
        winner: None,
    };
    let reader = Reader::new(&m.players);
    let mut turn = None;
    for (time, body) in join_continued(&lines[4..]) {
        if let Some(time) = time {
            let Some(game) = m.games.last_mut() else {
                continue;
            };
            let event = reader.event(&body);
            if let Event::TurnBegins(t) = event {
                turn = Some(t);
            }
            game.entries.push(Entry {
                time: Some(time),
                turn,
                event,
                text: body,
            });
            continue;
        }
        let body = body.trim_end();
        if body.is_empty() {
            continue;
        }
        if let Some((number, id)) = game_header(body) {
            turn = None;
            m.games.push(Game {
                number,
                id,
                started: None,
                notes: Vec::new(),
                entries: Vec::new(),
                winner: None,
                completed: false,
            });
            continue;
        }
        if let Some((p, score)) = body
            .strip_prefix("Match Winner: ")
            .and_then(|r| reader.player(r))
            .and_then(|(p, r)| Some((p, r.strip_prefix(": ")?)))
        {
            m.winner = Some((p, score.to_string()));
            continue;
        }
        // `Game N Completed.` can follow the next game's header.
        if let Some(n) = body
            .strip_prefix("Game ")
            .and_then(|r| r.strip_suffix(" Completed."))
            .and_then(|n| n.parse::<u32>().ok())
        {
            if let Some(g) = m.games.iter_mut().rev().find(|g| g.number == n) {
                g.completed = true;
                continue;
            }
        }
        let Some(game) = m.games.last_mut() else {
            // Header lines past the fourth: nothing a game holds.
            continue;
        };
        if let Some(t) = body
            .strip_prefix("[Time] ")
            .and_then(|t| t.trim().parse().ok())
        {
            game.started = Some(t);
            continue;
        }
        if body.starts_with("[Gatherling] ") || body.starts_with("[League] ") {
            game.notes.push(body.to_string());
            continue;
        }
        if let Some(p) = body
            .strip_prefix("Winner: ")
            .and_then(|w| reader.whole_player(w))
        {
            game.winner = Some(p);
            continue;
        }
        // Now and then an event is printed without its timestamp.
        if let Some(event) = reader.read(body) {
            if let Event::TurnBegins(t) = event {
                turn = Some(t);
            }
            game.entries.push(Entry {
                time: None,
                turn,
                event,
                text: body.to_string(),
            });
            continue;
        }
        // PDBot's notes on a bugged card run on below its chat line.
        if let Some(Entry {
            event: Event::Chat { text, .. },
            ..
        }) = game.entries.last_mut()
        {
            text.push('\n');
            text.push_str(body);
            continue;
        }
        game.entries.push(Entry {
            time: None,
            turn,
            event: Event::Unrecognised(body.to_string()),
            text: body.to_string(),
        });
    }
    Ok(m)
}

/// `== Game 2 (956025478) ==`
fn game_header(line: &str) -> Option<(u32, u64)> {
    let rest = line.strip_prefix("== Game ")?.strip_suffix(" ==")?;
    let (n, id) = rest.split_once(" (")?;
    Some((n.parse().ok()?, id.strip_suffix(')')?.parse().ok()?))
}

fn clock(line: &str) -> Option<(Clock, &str)> {
    let b = line.as_bytes();
    if b.len() < 10 || b[0] != b'[' || b[3] != b':' || b[6] != b':' || b[9] != b']' {
        return None;
    }
    let two = |i: usize| line.get(i..i + 2)?.parse::<u8>().ok();
    let time = Clock {
        hour: two(1)?,
        minute: two(4)?,
        second: two(7)?,
    };
    let body = &line[10..];
    Some((time, body.strip_prefix(' ').unwrap_or(body)))
}

/// The lines with their timestamps split off, a line the client broke
/// put back together: a long one ("reveals 17 cards with …") is broken
/// after a space and the rest stamped with the same time, and a modal
/// ability's bullets go on untimestamped lines starting " ** ".
fn join_continued(lines: &[&str]) -> Vec<(Option<Clock>, String)> {
    let mut out: Vec<(Option<Clock>, String)> = Vec::new();
    for line in lines {
        let last = out.last_mut();
        match clock(line) {
            Some((time, body)) => {
                if let Some((Some(prev), text)) = last {
                    if *prev == time && text.ends_with(' ') && body.starts_with('[') {
                        text.push_str(body);
                        continue;
                    }
                }
                out.push((Some(time), body.to_string()));
            }
            None => {
                if let Some((Some(_), text)) = last {
                    if line.starts_with(" ** ") {
                        text.push('\n');
                        text.push_str(line);
                        continue;
                    }
                }
                out.push((None, line.to_string()));
            }
        }
    }
    out
}

/// "a", "two", "17": how many.
fn count(word: &str) -> Option<u32> {
    const WORDS: [&str; 21] = [
        "no",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
    ];
    match word {
        "a" | "an" => Some(1),
        "zero" => Some(0),
        w => WORDS
            .iter()
            .position(|x| *x == w)
            .map(|n| n as u32)
            .or_else(|| w.parse().ok()),
    }
}

/// "N card" or "N cards" before `rest`.
fn n_cards(s: &str) -> Option<(u32, &str)> {
    let (n, r) = s.split_once(' ')?;
    let r = r.strip_prefix("cards").or_else(|| r.strip_prefix("card"))?;
    Some((count(n)?, r))
}

/// `[Name]` at the start of `s`.
fn card(s: &str) -> Option<(Card, &str)> {
    let rest = s.strip_prefix('[')?;
    let (name, rest) = rest.split_once(']')?;
    Some((Card::new(name), rest))
}

/// "face-down creature #1" at the start of `s`.
fn hidden(s: &str) -> Option<(Hidden, &str)> {
    let (make, r): (fn(u32) -> Hidden, &str) =
        if let Some(r) = s.strip_prefix("face-down creature #") {
            (Hidden::FaceDown, r)
        } else if let Some(r) = s.strip_prefix("manifested creature #") {
            (Hidden::Manifested, r)
        } else {
            (Hidden::Disguised, s.strip_prefix("disguised creature #")?)
        };
    let end = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
    Some((make(r[..end].parse().ok()?), &r[end..]))
}

fn object(s: &str) -> Option<(Object, &str)> {
    if let Some((c, r)) = card(s) {
        return Some((Object::Card(c), r));
    }
    hidden(s).map(|(h, r)| (Object::Hidden(h), r))
}

/// A source taking the whole of `s`: "[X]", "[X]'s ability", or an
/// unbracketed name.
fn source_whole(s: &str) -> Option<Source> {
    // Magic Online writes "with with" for some sources.
    let s = s.strip_prefix("with ").unwrap_or(s);
    match card(s) {
        Some((c, "" | "'s ability")) => Some(Source::Card(c)),
        Some(_) => None,
        None if !s.is_empty() && !s.contains('[') => Some(Source::Named(s.to_string())),
        None => None,
    }
}

/// The text inside the parentheses `s` starts with, and what follows. The
/// log cuts long ability text off with "...", sometimes inside a
/// parenthesis of its own, so an unclosed one ends at the last `)` before
/// " targeting " or at the line's last `)`.
fn parenthesised(s: &str) -> Option<(&str, &str)> {
    let inner = s.strip_prefix('(')?;
    let mut depth = 0usize;
    for (i, c) in inner.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some((&inner[..i], &inner[i + 1..])),
            ')' => depth -= 1,
            _ => {}
        }
    }
    let end = inner.find(") targeting ").or_else(|| inner.rfind(')'))?;
    Some((&inner[..end], &inner[end + 1..]))
}

/// A trailing "(X is 3.)", taken off.
fn x_value(s: &str) -> (&str, Option<u32>) {
    if let Some(start) = s.rfind("(X is ") {
        if let Some(n) = s[start + 6..]
            .strip_suffix(".)")
            .and_then(|n| n.parse().ok())
        {
            return (&s[..start], Some(n));
        }
    }
    (s, None)
}

/// List separators: "[A] and [B]", "[A], [B], and [C]".
fn separator(s: &str) -> Option<&str> {
    s.strip_prefix(", and ")
        .or_else(|| s.strip_prefix(" and "))
        .or_else(|| s.strip_prefix(", "))
}

/// `[A]`, `[A] and [B]`, `[A], [B], and [C]`.
fn cards(s: &str) -> Option<(Vec<Card>, &str)> {
    let (first, mut rest) = card(s)?;
    let mut out = vec![first];
    while let Some((c, r)) = separator(rest).and_then(card) {
        out.push(c);
        rest = r;
    }
    Some((out, rest))
}

/// Quoted modes: `"A" and "B"`, `"A", "B", and "C"`.
fn quoted(s: &str) -> Option<(Vec<String>, &str)> {
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        let inner = rest.strip_prefix('"')?;
        let (q, r) = inner.split_once('"')?;
        out.push(q.to_string());
        rest = r;
        match separator(rest) {
            Some(r) if r.starts_with('"') => rest = r,
            _ => return Some((out, rest)),
        }
    }
}

/// Counters, one kind or several: "a +1/+1 counter on [X], [Y]", "two
/// poison counters on bob", "a +1/+1 counter on [X], a loyalty counter
/// on [Y], and a loyalty counter on [Z]" (proliferate). `prep` is "on"
/// or "from".
fn counted(reader: &Reader, s: &str, prep: &str) -> Option<Vec<Counted>> {
    let s = s.strip_suffix('.')?;
    let mut out = Vec::new();
    let mut rest = s;
    loop {
        let (n, r) = rest.split_once(' ')?;
        let n = count(n)?;
        let (kind, r) = r
            .split_once(&format!(" counters {prep} "))
            .or_else(|| r.split_once(&format!(" counter {prep} ")))?;
        if kind.contains('[') {
            return None;
        }
        let (aims, r) = reader.aims(r)?;
        out.extend(aims.into_iter().map(|a| Counted {
            count: n,
            kind: kind.to_string(),
            on: a.target,
        }));
        match separator(r) {
            None if r.is_empty() => return Some(out),
            Some(r) => rest = r,
            None => return None,
        }
    }
}

/// Reads a line's body against the match's players.
struct Reader {
    /// Each name with its index, longest first, so "bob" never matches the
    /// front of "bob2".
    players: Vec<(String, Player)>,
}

impl Reader {
    fn new(players: &[String]) -> Reader {
        let mut ps: Vec<(String, Player)> = players
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), Player(i as u8)))
            .collect();
        ps.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
        Reader { players: ps }
    }

    /// A player's name at the start of `s`.
    fn player<'a>(&self, s: &'a str) -> Option<(Player, &'a str)> {
        self.players
            .iter()
            .find_map(|(name, p)| s.strip_prefix(name.as_str()).map(|r| (*p, r)))
    }

    fn whole_player(&self, s: &str) -> Option<Player> {
        self.players
            .iter()
            .find_map(|(name, p)| (name == s).then_some(*p))
    }

    /// "alice's ": a player's possessive.
    fn possessive<'a>(&self, s: &'a str) -> Option<(Player, &'a str)> {
        let (p, r) = self.player(s)?;
        Some((p, r.strip_prefix("'s ")?))
    }

    /// A card, a hidden permanent, or a player's name standing for an
    /// unnamed card.
    fn object<'a>(&self, s: &'a str) -> Option<(Object, &'a str)> {
        object(s).or_else(|| self.player(s).map(|(p, r)| (Object::Unnamed(p), r)))
    }

    /// `[A]`, `face-down creature #1 and [B]`, `[A], [B], and [C]`.
    fn objects<'a>(&self, s: &'a str) -> Option<(Vec<Object>, &'a str)> {
        let (first, mut rest) = self.object(s)?;
        let mut out = vec![first];
        while let Some((x, r)) = separator(rest).and_then(|r| self.object(r)) {
            out.push(x);
            rest = r;
        }
        Some((out, rest))
    }

    /// "[A], [B], and face-down creature #1", or "17 cards".
    fn attackers(&self, s: &str) -> Option<Attackers> {
        if let Some(n) = s.strip_suffix(" cards") {
            return Some(Attackers::Unseen(n.parse().ok()?));
        }
        let (os, r) = self.objects(s)?;
        r.is_empty().then_some(Attackers::Seen(os))
    }

    fn target<'a>(&self, s: &'a str) -> Option<(Target, &'a str)> {
        if let Some((x, r)) = object(s) {
            return Some((Target::Object(x), r));
        }
        if let Some(r) = s.strip_prefix("Nothing") {
            return Some((Target::Nothing, r));
        }
        if let Some((p, r)) = self.player(s) {
            return Some((Target::Player(p), r));
        }
        // An unbracketed name runs to the next bracketed item, note or
        // the end.
        let end = [", [", " and [", " (", ", and "]
            .iter()
            .filter_map(|m| s.find(m))
            .min()
            .unwrap_or(s.len());
        let name = s[..end].trim_end_matches('.');
        (!name.is_empty() && !name.contains('['))
            .then(|| (Target::Named(name.to_string()), &s[name.len()..]))
    }

    /// A list of targets, each perhaps with "(mode text)" or "for N".
    fn aims<'a>(&self, s: &'a str) -> Option<(Vec<Aim>, &'a str)> {
        let mut out = Vec::new();
        let mut rest = s;
        loop {
            let (target, r) = self.target(rest)?;
            rest = r;
            let mut note = None;
            if let Some((n, r)) = rest.strip_prefix(' ').and_then(parenthesised) {
                note = Some(n.to_string());
                rest = r;
            } else if let Some(r) = rest.strip_prefix(" for ") {
                let end = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
                if end > 0 {
                    note = Some(format!("for {}", &r[..end]));
                    rest = &r[end..];
                }
            }
            out.push(Aim { target, note });
            match separator(rest) {
                Some(r) if self.target(r).is_some() => rest = r,
                _ => return Some((out, rest)),
            }
        }
    }

    /// Targets, then perhaps older logs' "(text)" groups, to the end:
    /// "bob. (Target player draws a card.)." The text, if any, is returned.
    fn aims_and_text(&self, s: &str) -> Option<(Vec<Aim>, Option<String>)> {
        let (aims, rest) = self.aims(s)?;
        Some((aims, trailing_text(rest)?))
    }

    fn event(&self, body: &str) -> Event {
        self.read(body.trim_end())
            .unwrap_or_else(|| Event::Unrecognised(body.trim_end().to_string()))
    }

    fn read(&self, body: &str) -> Option<Event> {
        if let Some(chat) = body.strip_prefix("[CHAT] ") {
            let (speaker, text) = chat.split_once(": ").unwrap_or((chat, ""));
            return Some(Event::Chat {
                speaker: speaker.to_string(),
                text: text.to_string(),
            });
        }
        if let Some(who) = body.strip_suffix(" has started watching.") {
            return Some(Event::Watching {
                who: who.to_string(),
                started: true,
            });
        }
        if let Some(who) = body.strip_suffix(" has stopped watching.") {
            return Some(Event::Watching {
                who: who.to_string(),
                started: false,
            });
        }
        if let Some(rest) = body.strip_prefix("Turn ") {
            let (n, rest) = rest.split_once(": ")?;
            let (name, extra) = match rest.strip_suffix(" - Extra Turn") {
                Some(name) => (name, true),
                None => (rest, false),
            };
            return Some(Event::TurnBegins(Turn {
                number: n.parse().ok()?,
                active: self.whole_player(name)?,
                extra,
            }));
        }
        if let Some(rest) = body
            .strip_prefix("Couldn't put Triggered Ability from ")
            .or_else(|| body.strip_prefix("Couldn't put triggered ability from "))
        {
            let (c, rest) = card(rest)?;
            return (rest == " on the stack.").then_some(Event::TriggerFailed(c));
        }
        if let Some(mode) = body.strip_prefix("Chosen mode: ") {
            return Some(Event::ChosenMode(mode.to_string()));
        }
        match body {
            "It becomes day." => return Some(Event::Day(true)),
            "It becomes night." => return Some(Event::Day(false)),
            "The vote is in!" | "Log is not visible at this time.  Please contact Silasary." => {
                return Some(Event::Notice(body.to_string()))
            }
            _ => {}
        }
        if let Some(r) = body.strip_prefix("The chosen player is ") {
            return Some(Event::ChosenPlayer(self.whole_player(r)?));
        }
        if let Some(r) = body.strip_prefix("Blockers for ") {
            let (attacker, r) = self.object(r)?;
            let (blockers, r) = self.objects(r.strip_prefix(" are ordered as follows: ")?)?;
            return r
                .is_empty()
                .then_some(Event::BlockersOrdered { attacker, blockers });
        }
        if body.starts_with("Illegal ") {
            return Some(Event::Notice(body.to_string()));
        }
        if let Some(e) = self.object_first(body) {
            return Some(e);
        }
        if let Some(e) = self.damage(body) {
            return Some(e);
        }
        if let Some((p, rest)) = self.possessive(body) {
            if let Some(e) = self.possessed(p, rest) {
                return Some(e);
            }
        }
        let (p, rest) = self.player(body)?;
        if rest == "." {
            return Some(Event::Blank(p));
        }
        // An unnamed card blocking: "alice blocks [X]."
        if let Some(r) = rest.strip_prefix(" blocks ") {
            let (attacker, r) = self.object(r)?;
            return (r == ".").then_some(Event::Blocks {
                blocker: Object::Unnamed(p),
                attacker,
            });
        }
        self.acts(p, rest.strip_prefix(' ')?)
    }

    /// Lines that open with a card or a hidden permanent: "[X] blocks
    /// [Y].", "[X] transforms into [Y].", "[X] explores.".
    fn object_first(&self, body: &str) -> Option<Event> {
        let (x, rest) = object(body)?;
        if let Some(r) = rest.strip_prefix(" blocks ") {
            let (a, r) = self.object(r)?;
            return (r == ".").then_some(Event::Blocks {
                blocker: x,
                attacker: a,
            });
        }
        let Object::Card(c) = x else {
            return None;
        };
        if let Some(r) = rest.strip_prefix(" transforms into ") {
            let (to, r) = card(r)?;
            return (r == ".").then_some(Event::Transforms { from: c, to });
        }
        if let Some(r) = rest.strip_prefix(" and ") {
            let (b, r) = card(r)?;
            return (r == " fight.").then_some(Event::Fight(c, b));
        }
        if let Some(r) = rest.strip_prefix(" is returned to ") {
            let (owner, r) = self.possessive(r)?;
            let from = match r {
                "hand from the battlefield." => Zone::Battlefield,
                "hand from the graveyard." => Zone::Graveyard,
                _ => return None,
            };
            return Some(Event::ReturnedToHand {
                card: c,
                owner,
                from,
            });
        }
        if let Some(r) = rest.strip_prefix(" deals ") {
            let (n, r) = r.split_once(" damage to ")?;
            let (to, r) = self.target(r)?;
            return matches!(r, "" | ".").then_some(Event::Deals {
                source: c,
                amount: n.parse().ok()?,
                to,
            });
        }
        if let Some(spell) = rest.strip_prefix(" prepares ") {
            return Some(Event::Prepares {
                card: c,
                spell: spell.trim_end_matches('.').to_string(),
            });
        }
        match rest {
            " stops blocking." => Some(Event::StopsBlocking(c)),
            " explores." => Some(Event::Explores(c)),
            " connives." => Some(Event::Connives(c)),
            " becomes plotted." => Some(Event::Plotted(c)),
            _ => None,
        }
    }

    /// "2 damage caused by [X] to bob is prevented." / "… is redirected
    /// from bob to [Y]."
    fn damage(&self, body: &str) -> Option<Event> {
        let (n, rest) = body.split_once(" damage caused by ")?;
        let amount = n.parse().ok()?;
        let (source, rest) = card(rest)?;
        if let Some(r) = rest.strip_prefix(" to ") {
            let (to, r) = self.target(r)?;
            return (r == " is prevented.").then_some(Event::DamagePrevented {
                amount,
                source,
                to,
            });
        }
        let r = rest.strip_prefix(" is redirected from ")?;
        let (from, r) = self.target(r)?;
        let (to, r) = self.target(r.strip_prefix(" to ")?)?;
        (r == ".").then_some(Event::DamageRedirected {
            amount,
            source,
            from,
            to,
        })
    }

    /// Lines about a player's card: "alice's [X] creates …", "alice's [X]
    /// is being attacked by …".
    fn possessed(&self, p: Player, rest: &str) -> Option<Event> {
        let Some((c, rest)) = card(rest) else {
            // Older logs leave the spell unbracketed: "alice's Seal of
            // Cleansing is removed from the stack …".
            let spell =
                rest.strip_suffix(" is removed from the stack because it has no legal targets.")?;
            return Some(Event::RemovedForNoTargets {
                player: p,
                spell: Source::Named(spell.to_string()),
            });
        };
        if let Some(r) = rest.strip_prefix(" is being attacked by ") {
            return Some(Event::Attacked {
                defender: Defender::Permanent {
                    controller: p,
                    card: c,
                },
                attackers: self.attackers(r)?,
            });
        }
        if let Some(text) = rest.strip_prefix(" creates an emblem. ") {
            let text = parenthesised(text).map_or(text, |(t, _)| t);
            return Some(Event::CreatesEmblem {
                player: p,
                source: c,
                text: text.trim_matches('"').to_string(),
            });
        }
        if let Some(r) = rest.strip_prefix(" creates ") {
            let r = r.strip_suffix('.')?;
            let (r, under) = match r.split_once(" under ") {
                Some((what, owner)) => {
                    let (o, tail) = self.possessive(owner)?;
                    (tail == "control").then_some(())?;
                    (what, Some(o))
                }
                None => (r, None),
            };
            let (n, what) = r.split_once(' ')?;
            return Some(Event::Creates {
                player: p,
                source: c,
                count: count(n)?,
                what: what.to_string(),
                under,
            });
        }
        if let Some(n) = rest
            .strip_prefix(" returns to their hand and they get ")
            .and_then(|r| r.strip_suffix(" energy."))
        {
            return Some(Event::ReturnsForEnergy {
                player: p,
                card: c,
                energy: n.parse().ok()?,
            });
        }
        if let Some(n) = rest
            .strip_prefix(" becomes level ")
            .and_then(|r| r.strip_suffix('.'))
        {
            return Some(Event::Level {
                controller: p,
                card: c,
                level: n.parse().ok()?,
            });
        }
        if rest.starts_with("'s triggered ability does not resolve because the intervening") {
            return Some(Event::InterveningIfFails {
                controller: p,
                card: c,
            });
        }
        match rest {
            " is sacrificed at random." => Some(Event::SacrificedAtRandom {
                controller: p,
                card: c,
            }),
            " trains." => Some(Event::Trains {
                controller: p,
                card: c,
            }),
            " is removed from the stack because it has no legal targets." => {
                Some(Event::RemovedForNoTargets {
                    player: p,
                    spell: Source::Card(c),
                })
            }
            _ => None,
        }
    }

    /// "alice <does something>": the line with its subject taken off.
    fn acts(&self, p: Player, s: &str) -> Option<Event> {
        use Event::*;
        let fixed = match s {
            "joined the game." => Some(Joined(p)),
            "has left the game." => Some(Left(p)),
            "has lost connection to the game." => Some(Disconnected(p)),
            "chooses to play first." => Some(PlaysFirst {
                player: p,
                first: true,
            }),
            "chooses to not play first." => Some(PlaysFirst {
                player: p,
                first: false,
            }),
            "skips their draw step." => Some(Skips {
                player: p,
                step: Step::Draw,
            }),
            "skips their untap step." => Some(Skips {
                player: p,
                step: Step::Untap,
            }),
            "takes an extra turn after this one." => Some(TakesExtraTurn(p)),
            "draws a card." | "draws their next card." => Some(Draws {
                player: p,
                count: 1,
                with: None,
            }),
            "investigates." => Some(Investigates(p)),
            "is prevented from searching their library." => Some(SearchPrevented(p)),
            "shuffles their library." => Some(Shuffles(p)),
            "won the flip." => Some(FlipResult {
                player: p,
                won: true,
            }),
            "lost the flip." => Some(FlipResult {
                player: p,
                won: false,
            }),
            "is tempted by the Ring." => Some(Tempted {
                player: p,
                bearer: None,
            }),
            "has conceded from the game." => Some(Concedes(p)),
            "wins the game." => Some(Wins(p)),
            "loses the game." => Some(Loses(p)),
            "has lost the game due to inaction." => Some(LosesToInaction(p)),
            "has run out of time and has lost the match." => Some(OutOfTime(p)),
            _ => None,
        };
        if fixed.is_some() {
            return fixed;
        }
        if let Some(r) = s.strip_prefix("reveals: ") {
            let (cards, r) = cards(r)?;
            return (r == ".").then_some(Reveals {
                player: p,
                cards,
                how: Reveal::Plain,
            });
        }
        let (verb, rest) = s.split_once(' ')?;
        match verb {
            "plays" => {
                let (card, r) = self.object(rest)?;
                (r == ".").then_some(PlaysLand { player: p, card })
            }
            "casts" if rest == "a card face down." => Some(CastsFaceDown {
                player: p,
                how: rest.trim_end_matches('.').to_string(),
            }),
            "casts" if rest.starts_with("ability from ") => {
                // A companion put into hand: "casts ability from [X] ( Put
                // X into your hand.)."
                let (card, r) = card(rest.strip_prefix("ability from ")?)?;
                let (ability, r) = parenthesised(r.strip_prefix(' ')?)?;
                matches!(r, "" | ".").then(|| {
                    Activates(Activation {
                        player: p,
                        card: Object::Card(card),
                        ability: ability.trim().to_string(),
                        modes: vec![],
                        targets: vec![],
                        x: None,
                    })
                })
            }
            "casts" => self.cast(p, rest).map(Casts),
            "kept" => {
                let (n, r) = n_cards(rest)?;
                (r == ".").then_some(KeepsCards {
                    player: p,
                    count: n,
                })
            }
            "paid" => {
                if let Some(r) = rest.strip_prefix("the echo cost for ") {
                    let (card, r) = card(r)?;
                    return (r == ".").then_some(PaysEcho { player: p, card });
                }
                Some(PaysLife {
                    player: p,
                    amount: rest.strip_suffix(" life.")?.parse().ok()?,
                })
            }
            "Cast" => Some(CastsFaceDown {
                player: p,
                how: rest.trim_end_matches('.').to_string(),
            }),
            "activates" => {
                if let Some(r) = rest
                    .split_once(" ability of ")
                    .filter(|(k, _)| !k.contains(' '))
                {
                    // "activates Ninjutsu ability of [X]."
                    if r.0 != "an" {
                        let (card, tail) = card(r.1)?;
                        return (tail == ".").then(|| {
                            Activates(Activation {
                                player: p,
                                card: Object::Card(card),
                                ability: r.0.to_string(),
                                modes: vec![],
                                targets: vec![],
                                x: None,
                            })
                        });
                    }
                }
                self.activation(p, rest.strip_prefix("an ability of ")?)
                    .map(Activates)
            }
            "puts" | "put" => self.puts(p, rest),
            "removes" => Some(RemovesCounters {
                player: p,
                counters: counted(self, rest, "from")?,
            }),
            "draws" => {
                let (n, r) = n_cards(rest)?;
                if r == "." {
                    return Some(Draws {
                        player: p,
                        count: n,
                        with: None,
                    });
                }
                let with = source_whole(r.strip_prefix(" with ")?.strip_suffix('.')?)?;
                Some(Draws {
                    player: p,
                    count: n,
                    with: Some(with),
                })
            }
            "gains" if rest.contains(" energy") => {
                let (n, r) = rest.split_once(" energy")?;
                let from = match r {
                    "." => None,
                    r => Some(source_whole(r.strip_prefix(" from ")?.strip_suffix('.')?)?),
                };
                Some(GainsEnergy {
                    player: p,
                    amount: n.parse().ok()?,
                    from,
                })
            }
            "gains" if !rest.starts_with("control ") => {
                let n = rest.strip_suffix(" life.")?;
                Some(GainsLife {
                    player: p,
                    amount: n.parse().ok()?,
                })
            }
            "loses" => {
                let n = rest.strip_suffix(" life.")?;
                Some(LosesLife {
                    player: p,
                    amount: n.parse().ok()?,
                })
            }
            "mulligans" => {
                let n = rest.strip_prefix("to ")?.strip_suffix(" cards.")?;
                Some(Mulligan {
                    player: p,
                    to: count(n)?,
                })
            }
            "begins" => {
                let n = rest
                    .strip_prefix("the game with ")?
                    .strip_suffix(" cards in hand.")?;
                Some(Keeps {
                    player: p,
                    hand: count(n)?,
                    bottomed: 0,
                })
            }
            "discards" => {
                let (cards, r) = self.objects(rest)?;
                match r {
                    "." => Some(Discards { player: p, cards }),
                    " but it enters the battlefield instead." => Some(DiscardedOntoBattlefield {
                        player: p,
                        card: match cards.into_iter().next()? {
                            Object::Card(c) => c,
                            _ => return None,
                        },
                    }),
                    _ => None,
                }
            }
            "mills" => {
                if let Some(n) = rest.strip_suffix(" cards.") {
                    return Some(Mills {
                        player: p,
                        cards: Moved::Unseen(count(n)?),
                    });
                }
                let (cards, r) = cards(rest)?;
                (r == ".").then_some(Mills {
                    player: p,
                    cards: Moved::Seen(cards),
                })
            }
            "scrys" => {
                let (n, r) = rest.split_once(" (")?;
                let r = r.strip_suffix(" bottom).")?;
                let (top, bottom) = r.split_once(" top, ")?;
                Some(Scry {
                    player: p,
                    count: n.parse().ok()?,
                    top: top.parse().ok()?,
                    bottom: bottom.parse().ok()?,
                })
            }
            "flipped" => {
                let r = rest.strip_suffix('.')?;
                let (mut heads, mut tails) = (0, 0);
                let mut r = r;
                loop {
                    let (n, tail) = r.split_once(' ')?;
                    let n = count(n)?;
                    let (side, tail) = tail.split_once(" and ").unwrap_or((tail, ""));
                    match side {
                        "heads" => heads += n,
                        "tails" => tails += n,
                        _ => return None,
                    }
                    if tail.is_empty() {
                        break;
                    }
                    r = tail;
                }
                Some(Flips {
                    player: p,
                    heads,
                    tails,
                })
            }
            "reveals" => self.reveals(p, rest),
            "chooses" | "chose" => self.chooses(p, rest),
            "declines" => {
                let (card, r) = card(rest.strip_prefix("to use ")?)?;
                (r == "'s ability.").then_some(UsesAbility {
                    player: p,
                    card,
                    uses: false,
                })
            }
            "names" => {
                let (name, r) = rest.rsplit_once(" for ")?;
                Some(Names {
                    player: p,
                    name: name.to_string(),
                    for_source: source_whole(r.strip_suffix('.')?)?,
                })
            }
            "targets" => {
                let (target, r) = self.target(rest)?;
                Some(Targets {
                    player: p,
                    target,
                    for_source: source_whole(r.strip_prefix(" for ")?.strip_suffix('.')?)?,
                })
            }
            "counters" => {
                let r = rest.strip_suffix('.')?;
                let (spell, with) = match card(r) {
                    Some((c, tail)) => (Source::Card(c), tail.strip_prefix(" with ")?),
                    None => {
                        let (s, w) = r.split_once(" with ")?;
                        (Source::Named(s.to_string()), w)
                    }
                };
                Some(Counters {
                    player: p,
                    spell,
                    with: source_whole(with)?,
                })
            }
            "exiles" => {
                let (cards, r) = cards(rest)?;
                let r = r.strip_suffix('.')?;
                if r.is_empty() {
                    return Some(Exiles {
                        player: p,
                        cards,
                        by: None,
                    });
                }
                let r = r.strip_prefix(" with ")?;
                let by = if r == "its own ability" {
                    ExiledBy::Itself
                } else if let Some(n) = r
                    .strip_suffix(" time counters")
                    .or_else(|| r.strip_suffix(" time counter"))
                {
                    ExiledBy::Suspend(n.parse().ok()?)
                } else {
                    ExiledBy::Source(source_whole(r)?)
                };
                Some(Exiles {
                    player: p,
                    cards,
                    by: Some(by),
                })
            }
            "returns" => {
                if let Some((card, r)) = card(rest) {
                    if r == " to their hand with its dredge ability." {
                        return Some(Dredges { player: p, card });
                    }
                    if let Some(w) = r.strip_prefix(" to the battlefield with ") {
                        return Some(ReturnsToBattlefield {
                            player: p,
                            card,
                            with: source_whole(w.trim_end_matches('.'))?,
                        });
                    }
                }
                let (cards, r) = match rest.strip_prefix("no cards") {
                    Some(r) => (Vec::new(), r),
                    None => cards(rest)?,
                };
                let r = r
                    .strip_prefix(" to its owner's hand with ")
                    .or_else(|| r.strip_prefix(" to their owner's hand with "))?;
                Some(ReturnsToHand {
                    player: p,
                    cards,
                    with: source_whole(r.strip_suffix('.')?)?,
                })
            }
            "sacrifices" => {
                let (card, r) = card(rest)?;
                (r == ".").then_some(Sacrifices { player: p, card })
            }
            "cycles" => {
                let (card, r) = card(rest)?;
                (r == ".").then_some(Cycles { player: p, card })
            }
            "transmutes" => {
                let (card, r) = card(rest)?;
                (r == ".").then_some(Transmutes { player: p, card })
            }
            "incubates" => Some(Incubates {
                player: p,
                count: rest.trim_end_matches('.').parse().ok()?,
            }),
            "blights" => {
                let (card, r) = card(rest)?;
                Some(Blights {
                    player: p,
                    card,
                    count: r.trim_start().trim_end_matches('.').parse().ok()?,
                })
            }
            "applies" => {
                let r = rest.strip_prefix("replacement effect from ")?;
                let r = r
                    .strip_prefix("triggered ability from ")
                    .or_else(|| r.strip_prefix("Triggered Ability from "))
                    .unwrap_or(r);
                let (source, r) = object(r)?;
                (r == ".").then_some(AppliesReplacement { player: p, source })
            }
            "changes" => {
                let (spell, r) = card(rest.strip_prefix("targets of ")?)?;
                let (to, r) = self.aims(r.strip_prefix(" to ")?)?;
                (r == ".").then_some(ChangesTargets {
                    player: p,
                    spell,
                    to,
                })
            }
            "gains" => {
                let (card, r) = card(rest.strip_prefix("control of ")?)?;
                let from = self.whole_player(r.strip_prefix(" from ")?.trim_end_matches('.'))?;
                Some(GainsControl {
                    player: p,
                    card,
                    from,
                })
            }
            "is" => {
                if let Some(r) = rest.strip_prefix("being attacked by ") {
                    return Some(Attacked {
                        defender: Defender::Player(p),
                        attackers: self.attackers(r)?,
                    });
                }
                if let Some(r) = rest.strip_prefix("tempted by the Ring. ") {
                    let (bearer, r) = card(r)?;
                    return (r == " becomes their Ring-bearer.").then_some(Tempted {
                        player: p,
                        bearer: Some(bearer),
                    });
                }
                if let Some(r) = rest.strip_prefix("dealt ") {
                    let (n, _) = r.split_once(" point")?;
                    return Some(IsDealt {
                        player: p,
                        amount: n.parse().ok()?,
                    });
                }
                let (controller, r) = self.possessive(rest.strip_prefix("protecting ")?)?;
                let (card, r) = card(r)?;
                (r == ".").then_some(Protects {
                    player: p,
                    controller,
                    card,
                })
            }
            "fails" => Some(FailsToFind {
                player: p,
                what: rest
                    .strip_prefix("to find ")?
                    .trim_end_matches('.')
                    .to_string(),
            }),
            "turns" => {
                let (object, r) = hidden(rest)?;
                (r == " face up.").then_some(TurnsFaceUp { player: p, object })
            }
            "unlocks" => {
                let (card, r) = card(rest.strip_prefix("the door to the ")?)?;
                let side = match r {
                    " on the left." => Side::Left,
                    " on the right." => Side::Right,
                    _ => return None,
                };
                Some(UnlocksDoor {
                    player: p,
                    card,
                    side,
                })
            }
            "Beholds" | "Forages" => {
                let (cards, r) = cards(rest)?;
                let with = source_whole(r.strip_prefix(" with ")?.strip_suffix('.')?)?;
                Some(if verb == "Beholds" {
                    Beholds {
                        player: p,
                        cards,
                        with,
                    }
                } else {
                    Forages {
                        player: p,
                        cards,
                        with,
                    }
                })
            }
            _ => None,
        }
    }

    /// After "casts ".
    fn cast(&self, player: Player, s: &str) -> Option<Cast> {
        let (card, rest) = self.object(s)?;
        let (rest, x) = x_value(rest);
        let rest = rest.strip_suffix('.').unwrap_or(rest);
        let (how, targets, text) = match rest.split_once(" targeting ") {
            Some((how, t)) => {
                let (aims, text) = self.aims_and_text(t)?;
                (how, aims, text)
            }
            None => (rest, Vec::new(), None),
        };
        let mut via = Vec::new();
        let mut modes = Vec::new();
        let mut revealing = Vec::new();
        let mut rest = how.trim_start();
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix("choosing ") {
                let (q, r) = quoted(r)?;
                modes = q;
                // A mode's own sentence ends the cast's: `"Draw a card.".`
                rest = r.trim_start_matches('.').trim_start();
            } else if let Some(r) = rest.strip_prefix("revealing ") {
                let (cs, r) = cards(r)?;
                revealing = cs;
                rest = r.trim_start();
            } else {
                // Everything up to the next mode or reveal is the route.
                let end = ["choosing \"", "revealing ["]
                    .iter()
                    .filter_map(|m| rest.find(m))
                    .min()
                    .unwrap_or(rest.len());
                if end == 0 {
                    return None;
                }
                via.push(rest[..end].trim());
                rest = &rest[end..];
            }
        }
        let via = via.join(" ");
        Some(Cast {
            player,
            card,
            via: (!via.is_empty()).then_some(via),
            modes,
            revealing,
            targets,
            x,
            text,
        })
    }

    /// After "activates an ability of ". Newer logs give the text before
    /// the targets, `[X] (text) targeting [Y].`; older ones after,
    /// `[X] targeting [Y]. (text) (text).`
    fn activation(&self, player: Player, s: &str) -> Option<Activation> {
        let (card, rest) = self.object(s)?;
        let (rest, x) = x_value(rest);
        let mut modes = Vec::new();
        let (ability, targets) = if let Some(r) = rest.strip_prefix(" targeting ") {
            let (aims, text) = self.aims_and_text(r.strip_suffix('.').unwrap_or(r))?;
            (text.unwrap_or_default(), aims)
        } else if let Some(r) = rest.strip_prefix(". ") {
            (
                trailing_text(r.strip_suffix('.').unwrap_or(r))??,
                Vec::new(),
            )
        } else {
            let (ability, rest) = parenthesised(rest.strip_prefix(' ')?)?;
            let mut rest = rest.strip_suffix('.').unwrap_or(rest);
            if let Some(r) = rest.strip_prefix(" choosing ") {
                let (q, r) = quoted(r)?;
                modes = q;
                rest = r.trim_start_matches('.');
            }
            let targets = match rest.strip_prefix(" targeting ") {
                Some(t) => {
                    let (aims, r) = self.aims(t)?;
                    if !r.is_empty() {
                        return None;
                    }
                    aims
                }
                None if rest.is_empty() => Vec::new(),
                None => return None,
            };
            (ability.to_string(), targets)
        };
        Some(Activation {
            player,
            card,
            ability,
            modes,
            targets,
            x,
        })
    }

    /// After "puts " (or "put "): triggers, counters and cards moved.
    fn puts(&self, p: Player, s: &str) -> Option<Event> {
        if let Some(r) = s
            .strip_prefix("a triggered ability from ")
            .or_else(|| s.strip_prefix("triggered ability from "))
        {
            return self.trigger(p, r).map(Event::Trigger);
        }
        // "puts N cards on the bottom of their library and begins the game
        // with six cards in hand."
        if let Some((bottom, hand)) =
            s.split_once(" on the bottom of their library and begins the game with ")
        {
            let (n, r) = n_cards(bottom)?;
            let hand = hand.strip_suffix(" cards in hand.")?;
            return r.is_empty().then_some(Event::Keeps {
                player: p,
                hand: count(hand)?,
                bottomed: n,
            });
        }
        if let Some(counters) = counted(self, s, "on") {
            return Some(Event::PutsCounters {
                player: p,
                counters,
            });
        }
        if let Some((cards, r)) = cards(s) {
            if let Some(r) = r.strip_prefix(" onto the battlefield") {
                let card = cards.into_iter().next()?;
                if r == "." {
                    return Some(Event::PutsOntoBattlefield {
                        player: p,
                        card,
                        attacking: None,
                    });
                }
                let (t, r) = self.target(r.strip_prefix(" attacking ")?)?;
                return matches!(r, "" | ".").then_some(Event::PutsOntoBattlefield {
                    player: p,
                    card,
                    attacking: Some(t),
                });
            }
            if let Some(r) = r.strip_prefix(" into their graveyard") {
                let then_top = match r {
                    "" | "." => 0,
                    r => {
                        let (n, r) = n_cards(r.strip_prefix(" and ")?)?;
                        (r == " on top of their library.").then_some(n)?
                    }
                };
                return Some(Event::PutsInGraveyard {
                    player: p,
                    cards,
                    then_top,
                });
            }
            if let Some(w) = r.strip_prefix(" into their hand with ") {
                return Some(Event::PutsInHand {
                    player: p,
                    cards,
                    by: source_whole(w.strip_suffix('.')?)?,
                });
            }
            let (end, owner, by) = self.library_end(p, r.strip_prefix(' ')?)?;
            return Some(Event::PutsInLibrary {
                player: p,
                cards: Moved::Seen(cards),
                owner,
                end,
                by,
            });
        }
        // "a card on the bottom of the library.", "1 card on top of their
        // library.", "a card on top of bob's library."
        let (n, r) = n_cards(s)?;
        let (end, owner, by) = self.library_end(p, r.strip_prefix(' ')?)?;
        Some(Event::PutsInLibrary {
            player: p,
            cards: Moved::Unseen(n),
            owner,
            end,
            by,
        })
    }

    /// After "triggered ability from ": `[X] onto the stack (text)
    /// targeting …`, or older logs' `[X] onto the stack targeting …
    /// (text).`
    fn trigger(&self, player: Player, r: &str) -> Option<Trigger> {
        let (source, r) = match card(r) {
            Some((c, r)) => (Source::Card(c), r.strip_prefix(" onto the stack")?),
            None => {
                let (name, r) = r.split_once(" onto the stack")?;
                (Source::Named(name.to_string()), r)
            }
        };
        let r = r.strip_suffix('.').unwrap_or(r);
        let mut modes = Vec::new();
        if let Some(t) = r.strip_prefix(" targeting ") {
            let (targets, ability) = self.aims_and_text(t)?;
            return Some(Trigger {
                player,
                source,
                ability,
                modes,
                targets,
            });
        }
        let (ability, mut r) = match r.strip_prefix(' ').and_then(parenthesised) {
            Some((a, r)) => (Some(a.to_string()), r.strip_suffix('.').unwrap_or(r)),
            None => (None, r),
        };
        if let Some(c) = r.strip_prefix(" choosing ") {
            let (q, tail) = quoted(c)?;
            modes = q;
            r = tail.trim_start_matches('.');
        }
        let targets = match r.strip_prefix(" targeting ") {
            Some(t) => {
                let (aims, r) = self.aims(t)?;
                if !r.is_empty() {
                    return None;
                }
                aims
            }
            None if r.is_empty() => Vec::new(),
            None => return None,
        };
        Some(Trigger {
            player,
            source,
            ability,
            modes,
            targets,
        })
    }

    /// "on top of their library.", "on the bottom of bob's library with
    /// [X].", "beneath the top 2 cards of their library."
    fn library_end(&self, p: Player, s: &str) -> Option<(End, Player, Option<Source>)> {
        let (end, r) = if let Some(r) = s.strip_prefix("on top of ") {
            (End::Top, r)
        } else if let Some(r) = s
            .strip_prefix("on the bottom of ")
            .or_else(|| s.strip_prefix("on bottom of "))
        {
            (End::Bottom, r)
        } else {
            let (n, r) = n_cards(s.strip_prefix("beneath the top ")?)?;
            (End::Beneath(n), r.strip_prefix(" of ")?)
        };
        let (owner, r) = match r
            .strip_prefix("their library")
            .or_else(|| r.strip_prefix("the library"))
        {
            Some(r) => (p, r),
            None => {
                let (o, r) = self.possessive(r)?;
                (o, r.strip_prefix("library")?)
            }
        };
        let by = match r {
            "." => None,
            r => Some(source_whole(r.strip_prefix(" with ")?.strip_suffix('.')?)?),
        };
        Some((end, owner, by))
    }

    /// After "reveals ".
    fn reveals(&self, p: Player, s: &str) -> Option<Event> {
        let reveal = |cards, how| Event::Reveals {
            player: p,
            cards,
            how,
        };
        if let Some((object, r)) = hidden(s) {
            let name = r.strip_prefix(" as ")?.strip_suffix('.')?;
            return Some(Event::RevealsHidden {
                player: p,
                object,
                card: Card::new(name),
            });
        }
        if let Some(r) = s.strip_prefix("their hand to ") {
            let (to, r) = r.split_once(", containing ")?;
            let (cards, r) = match r.strip_prefix("no cards") {
                Some(r) => (Vec::new(), r),
                None => cards(r)?,
            };
            let to = source_whole(to)?;
            return (r == ".").then(|| reveal(cards, Reveal::HandTo(to)));
        }
        if let Some(r) = s.strip_prefix(": ") {
            let (cards, r) = cards(r)?;
            return (r == ".").then(|| reveal(cards, Reveal::Plain));
        }
        if let Some((n, r)) = s.split_once(" cards with ") {
            let (with, r) = r.split_once(": ")?;
            let (cards, r) = cards(r)?;
            return (r == ".").then(|| {
                Some(reveal(
                    cards,
                    Reveal::Many {
                        count: n.parse().ok()?,
                        with: source_whole(with)?,
                    },
                ))
            })?;
        }
        let (cards, r) = cards(s)?;
        let how = match r {
            "." => Reveal::Plain,
            " at random." => Reveal::AtRandom,
            " from their opening hand." => Reveal::OpeningHand,
            " and puts it on the bottom of their library." => Reveal::ToBottom,
            " and shuffles it into their library." => Reveal::ShuffledIn,
            r => Reveal::With(source_whole(r.strip_prefix(" with ")?.strip_suffix('.')?)?),
        };
        Some(reveal(cards, how))
    }

    /// After "chooses ".
    fn chooses(&self, p: Player, s: &str) -> Option<Event> {
        if let Some(r) = s.strip_prefix("not to use ") {
            let (card, r) = card(r)?;
            return (r == "'s ability.").then_some(Event::UsesAbility {
                player: p,
                card,
                uses: false,
            });
        }
        if let Some(r) = s.strip_prefix("to use ") {
            let (card, r) = card(r)?;
            return (r == "'s ability.").then_some(Event::UsesAbility {
                player: p,
                card,
                uses: true,
            });
        }
        let s = s.strip_suffix('.')?;
        if let Some(c) = s.strip_suffix(" as a damage source") {
            let (card, r) = card(c)?;
            return r
                .is_empty()
                .then_some(Event::ChoosesDamageSource { player: p, card });
        }
        // "[X] and [Y] for [Z]'s ability", "no cards with [Z]'s ability",
        // "blue for [Tarnation Vista]", "Forest".
        let (what, by) = match s.rsplit_once(" for ").or_else(|| s.rsplit_once(" with ")) {
            Some((what, by)) if by.starts_with('[') => (what, Some(source_whole(by)?)),
            _ => (s, None),
        };
        let choice = if what == "no cards" {
            Choice::Cards(Vec::new())
        } else if let Some((cs, r)) = cards(what) {
            r.is_empty().then_some(Choice::Cards(cs))?
        } else if what.contains('[') {
            return None;
        } else {
            Choice::Word(what.to_string())
        };
        Some(Event::Chooses {
            player: p,
            choice,
            by,
        })
    }
}

/// What follows a list of targets in older logs: nothing, or "." and
/// parenthesised text groups ("bob. (Target player draws a card.).").
/// `None` when it's something else.
fn trailing_text(s: &str) -> Option<Option<String>> {
    let mut rest = s.trim_end_matches('.');
    if rest.is_empty() {
        return Some(None);
    }
    let mut texts = Vec::new();
    while !rest.is_empty() {
        let r = rest
            .strip_prefix(". ")
            .or_else(|| rest.strip_prefix(' '))
            .unwrap_or(rest);
        let (t, r) = parenthesised(r)?;
        texts.push(t.trim());
        rest = r.trim_end_matches('.');
    }
    Some(Some(texts.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str =
        "Freeform\r\n2015 vs bob\r\nBuggedCardObserver,Tourney\r\nalice,bob\r\n\r\n";

    fn game(lines: &str) -> Game {
        let m = parse(&format!(
            "{HEADER}== Game 1 (956024812) ==\r\n[Time] 1783789475\r\n{lines}"
        ))
        .unwrap();
        m.games.into_iter().next().unwrap()
    }

    fn one(line: &str) -> Event {
        let g = game(&format!("[20:04:37] {line}\r\n"));
        assert_eq!(g.entries.len(), 1);
        g.entries.into_iter().next().unwrap().event
    }

    fn cast(line: &str) -> Cast {
        match one(line) {
            Event::Casts(c) => c,
            e => panic!("{e:?}"),
        }
    }

    const ALICE: Player = Player(0);
    const BOB: Player = Player(1);

    fn c(n: &str) -> Card {
        Card::new(n)
    }

    fn on(n: &str) -> Target {
        Target::Object(Object::Card(c(n)))
    }

    fn aim(t: Target) -> Aim {
        Aim {
            target: t,
            note: None,
        }
    }

    #[test]
    fn the_header_and_a_game() {
        let m = parse(&format!(
            "{HEADER}== Game 1 (956024812) ==\r\n[Time] 1783789475\r\n\
             [Gatherling] Event=2015 Modern Biweekly 3.14\r\n\
             [20:04:37] PDBot has started watching.\r\n\
             [20:04:46] [CHAT] bob: gl hf\r\n\
             [20:04:50] alice chooses to play first.\r\n\
             [20:04:51] bob mulligans to six cards.\r\n\
             [20:04:59] bob puts a card on the bottom of their library and begins the game with six cards in hand.\r\n\
             [20:05:00] Turn 1: alice\r\n\
             [20:05:02] alice plays [Polluted Delta].\r\n\
             [20:26:08] bob has conceded from the game.\r\n\
             [20:26:08] alice wins the game.\r\n\
             Winner: alice\r\nGame 1 Completed.\r\nMatch Winner: alice: 2–0\r\n"
        ))
        .unwrap();
        assert_eq!(m.format, "Freeform");
        assert_eq!(m.comment, "2015 vs bob");
        assert_eq!(m.players, ["alice", "bob"]);
        assert_eq!(m.winner, Some((ALICE, "2–0".into())));
        let g = &m.games[0];
        assert_eq!(
            (g.number, g.id, g.started),
            (1, 956024812, Some(1783789475))
        );
        assert_eq!(g.notes, ["[Gatherling] Event=2015 Modern Biweekly 3.14"]);
        assert_eq!(g.winner, Some(ALICE));
        assert!(g.completed);
        assert_eq!(g.unrecognised().count(), 0);
        assert_eq!(
            g.entries[4],
            Entry {
                time: Some(Clock {
                    hour: 20,
                    minute: 4,
                    second: 59
                }),
                turn: None,
                event: Event::Keeps {
                    player: BOB,
                    hand: 6,
                    bottomed: 1
                },
                text: "bob puts a card on the bottom of their library and begins the game with six cards in hand.".into(),
            }
        );
        assert_eq!(
            g.entries[6].turn,
            Some(Turn {
                number: 1,
                active: ALICE,
                extra: false
            })
        );
        assert_eq!(
            g.entries[6].event,
            Event::PlaysLand {
                player: ALICE,
                card: Object::Card(c("Polluted Delta"))
            }
        );
    }

    #[test]
    fn casts_with_targets_modes_and_routes() {
        assert_eq!(
            cast("alice casts [Lightning Bolt] targeting [Kitchen Finks]."),
            Cast {
                player: ALICE,
                card: Object::Card(c("Lightning Bolt")),
                via: None,
                modes: vec![],
                revealing: vec![],
                targets: vec![aim(on("Kitchen Finks"))],
                x: None,
                text: None,
            }
        );
        let rift = cast(
            "bob casts [Rift Bolt] without paying its mana cost with suspend targeting alice.",
        );
        assert_eq!(
            rift.via.as_deref(),
            Some("without paying its mana cost with suspend")
        );
        assert_eq!(rift.targets, [aim(Target::Player(ALICE))]);
        let chord = cast("alice casts [Chord of Calling].(X is 3.)");
        assert_eq!((chord.x, chord.via), (Some(3), None));
        let kolaghan = cast(
            "bob casts [Kolaghan's Command] choosing \"Target player discards a card.\" and \
             \"Kolaghan's Command deals 2 damage to any target.\" targeting alice and \
             [Birds of Paradise] (Kolaghan's Command deals 2 damage to any target.).",
        );
        assert_eq!(kolaghan.modes.len(), 2);
        assert_eq!(kolaghan.targets[1].target, on("Birds of Paradise"));
        assert_eq!(
            kolaghan.targets[1].note.as_deref(),
            Some("Kolaghan's Command deals 2 damage to any target.")
        );
        let electrolyze = cast("alice casts [Electrolyze] targeting [Kitchen Finks] for 2.");
        assert_eq!(electrolyze.targets[0].note.as_deref(), Some("for 2"));
        let adept = cast("alice casts [Silvergill Adept] revealing [Master of Waves].");
        assert_eq!(adept.revealing, [c("Master of Waves")]);
        let morph = cast("alice casts [Path to Exile] targeting face-down creature #1.");
        assert_eq!(
            morph.targets,
            [aim(Target::Object(Object::Hidden(Hidden::FaceDown(1))))]
        );
        let old = cast("bob casts [Brainstorm] targeting bob. (Draw three cards.).");
        assert_eq!(old.text.as_deref(), Some("Draw three cards."));
    }

    #[test]
    fn abilities_and_triggers() {
        assert_eq!(
            one(
                "bob activates an ability of [Ghost Quarter] (Destroy target land. Its \
                 controller may search ...) targeting [Plains]."
            ),
            Event::Activates(Activation {
                player: BOB,
                card: Object::Card(c("Ghost Quarter")),
                ability: "Destroy target land. Its controller may search ...".into(),
                modes: vec![],
                targets: vec![aim(on("Plains"))],
                x: None,
            })
        );
        let Event::Activates(a) = one(
            "alice activates an ability of [Polukranos, World Eater] (Monstrosity X.)(X is 4.)",
        ) else {
            panic!()
        };
        assert_eq!((a.ability.as_str(), a.x), ("Monstrosity X.", Some(4)));
        assert_eq!(
            one(
                "alice puts a triggered ability from [Aether Vial] onto the stack (At the \
                 beginning of your upkeep, you may put a charge counter on this artifact.)."
            ),
            Event::Trigger(Trigger {
                player: ALICE,
                source: Source::Card(c("Aether Vial")),
                ability: Some(
                    "At the beginning of your upkeep, you may put a charge counter on this \
                     artifact."
                        .into()
                ),
                modes: vec![],
                targets: vec![],
            })
        );
        let Event::Trigger(t) = one(
            "bob puts a triggered ability from Teferi's Emblem onto the stack (Whenever you \
             draw a card, exile target permanent an opponent controls.) targeting [Island].",
        ) else {
            panic!()
        };
        assert_eq!(t.source, Source::Named("Teferi's Emblem".into()));
        assert_eq!(t.targets, [aim(on("Island"))]);
        assert_eq!(
            one("Couldn't put Triggered Ability from [Fervent Champion] on the stack."),
            Event::TriggerFailed(c("Fervent Champion"))
        );
    }

    #[test]
    fn a_modal_abilitys_bullets_are_part_of_its_line() {
        let g = game(
            "[05:44:09] alice puts a triggered ability from [Tranquil Frillback] onto the stack (When you pay {G}, choose up to that many  \r\n \
             ** Destroy target artifact or enchantment. \r\n \
             ** Exile ...) choosing \"Destroy target artifact or enchantment.\" targeting [Sythis, Harvest's Hand].\r\n",
        );
        let [Entry {
            event: Event::Trigger(t),
            ..
        }] = &g.entries[..]
        else {
            panic!("{:?}", g.entries)
        };
        assert_eq!(t.modes, ["Destroy target artifact or enchantment."]);
        assert_eq!(t.targets, [aim(on("Sythis, Harvest's Hand"))]);
    }

    #[test]
    fn combat() {
        assert_eq!(
            one("bob is being attacked by [Kitchen Finks], [Elemental Token], and face-down creature #2"),
            Event::Attacked {
                defender: Defender::Player(BOB),
                attackers: Attackers::Seen(vec![
                    Object::Card(c("Kitchen Finks")),
                    Object::Card(c("Elemental Token")),
                    Object::Hidden(Hidden::FaceDown(2)),
                ]),
            }
        );
        assert_eq!(
            one("alice's [Liliana of the Veil] is being attacked by [Scavenging Ooze]"),
            Event::Attacked {
                defender: Defender::Permanent {
                    controller: ALICE,
                    card: c("Liliana of the Veil")
                },
                attackers: Attackers::Seen(vec![Object::Card(c("Scavenging Ooze"))]),
            }
        );
        assert_eq!(
            one("[Snapcaster Mage] blocks [Kitchen Finks]."),
            Event::Blocks {
                blocker: Object::Card(c("Snapcaster Mage")),
                attacker: Object::Card(c("Kitchen Finks"))
            }
        );
    }

    #[test]
    fn zones_and_counters() {
        assert_eq!(
            one("alice reveals 3 cards with [Inquisition of Kozilek]: [Plains], [Path to Exile], and [Detention Sphere]."),
            Event::Reveals {
                player: ALICE,
                cards: vec![c("Plains"), c("Path to Exile"), c("Detention Sphere")],
                how: Reveal::Many {
                    count: 3,
                    with: Source::Card(c("Inquisition of Kozilek"))
                },
            }
        );
        assert_eq!(
            one("bob draws two cards with [Spymaster's Vault]'s ability."),
            Event::Draws {
                player: BOB,
                count: 2,
                with: Some(Source::Card(c("Spymaster's Vault")))
            }
        );
        assert_eq!(
            one("bob mills 14 cards."),
            Event::Mills {
                player: BOB,
                cards: Moved::Unseen(14)
            }
        );
        assert_eq!(
            one("alice scrys 1 (0 top, 1 bottom)."),
            Event::Scry {
                player: ALICE,
                count: 1,
                top: 0,
                bottom: 1
            }
        );
        assert_eq!(
            one("bob put a card on top of alice's library."),
            Event::PutsInLibrary {
                player: BOB,
                cards: Moved::Unseen(1),
                owner: ALICE,
                end: End::Top,
                by: None
            }
        );
        assert_eq!(
            one("bob puts [Hazoret the Fervent] beneath the top 2 cards of their library."),
            Event::PutsInLibrary {
                player: BOB,
                cards: Moved::Seen(vec![c("Hazoret the Fervent")]),
                owner: BOB,
                end: End::Beneath(2),
                by: None
            }
        );
        assert_eq!(
            one("alice puts two poison counters on bob."),
            Event::PutsCounters {
                player: ALICE,
                counters: vec![Counted {
                    count: 2,
                    kind: "poison".into(),
                    on: Target::Player(BOB)
                }]
            }
        );
        let Event::PutsCounters { counters, .. } = one(
            "alice puts a +1/+1 counter on [Court Homunculus], a loyalty counter on [Karn Liberated], and a loyalty counter on [Ugin, the Spirit Dragon].",
        ) else {
            panic!()
        };
        assert_eq!(
            counters.iter().map(|c| c.kind.as_str()).collect::<Vec<_>>(),
            ["+1/+1", "loyalty", "loyalty"]
        );
        assert_eq!(
            one("bob removes a +1/+1 counter from [Walking Ballista]."),
            Event::RemovesCounters {
                player: BOB,
                counters: vec![Counted {
                    count: 1,
                    kind: "+1/+1".into(),
                    on: on("Walking Ballista")
                }]
            }
        );
        assert_eq!(
            one("alice exiles [Wooded Foothills] and [Lightning Bolt] with with [Grim Lavamancer]'s ability."),
            Event::Exiles {
                player: ALICE,
                cards: vec![c("Wooded Foothills"), c("Lightning Bolt")],
                by: Some(ExiledBy::Source(Source::Card(c("Grim Lavamancer")))),
            }
        );
        assert_eq!(
            one("bob's [Lingering Souls] creates two Spirit Tokens."),
            Event::Creates {
                player: BOB,
                source: c("Lingering Souls"),
                count: 2,
                what: "Spirit Tokens".into(),
                under: None
            }
        );
        assert_eq!(
            one("2 damage caused by [Goblin Guide] to bob is prevented."),
            Event::DamagePrevented {
                amount: 2,
                source: c("Goblin Guide"),
                to: Target::Player(BOB)
            }
        );
        assert_eq!(
            one("alice chooses [Vapor Snag] with [Wasteland Strangler]'s ability."),
            Event::Chooses {
                player: ALICE,
                choice: Choice::Cards(vec![c("Vapor Snag")]),
                by: Some(Source::Card(c("Wasteland Strangler")))
            }
        );
        assert_eq!(
            one("alice chooses blue for [Tarnation Vista]."),
            Event::Chooses {
                player: ALICE,
                choice: Choice::Word("blue".into()),
                by: Some(Source::Card(c("Tarnation Vista")))
            }
        );
    }

    #[test]
    fn a_long_line_broken_in_two_is_one_event() {
        let g = game(
            "[04:12:44] alice reveals 3 cards with [Shardless Agent]: [Mirrex], [Island], and \r\n\
             [04:12:44] [Hypergenesis].\r\n",
        );
        assert_eq!(g.entries.len(), 1);
        assert_eq!(
            g.entries[0].event.cards(),
            [
                &c("Mirrex"),
                &c("Island"),
                &c("Hypergenesis"),
                &c("Shardless Agent")
            ]
        );
    }

    #[test]
    fn names_are_matched_whole_and_longest_first() {
        let m = parse(
            "Freeform\n\nx\nbob,bob jr\n\n== Game 1 (1) ==\n\
             [00:00:01] bob jr draws a card.\n[00:00:02] bobby draws a card.\n",
        )
        .unwrap();
        let g = &m.games[0];
        assert_eq!(
            g.entries[0].event,
            Event::Draws {
                player: Player(1),
                count: 1,
                with: None
            }
        );
        assert_eq!(
            g.unrecognised().collect::<Vec<_>>(),
            ["bobby draws a card."]
        );
    }

    #[test]
    fn pdbots_notes_run_on_and_unknown_lines_are_kept() {
        let g = game(
            "[20:37:46] [CHAT] PDBot: [sU]Leonin Arbiter[sU] has a graphical bug.\r\n\
             Our data about this bug is out of date.\r\n\
             [20:37:50] alice does a thing never seen.\r\n",
        );
        assert!(
            matches!(&g.entries[0].event, Event::Chat { text, .. } if text.ends_with("out of date."))
        );
        assert_eq!(
            g.unrecognised().collect::<Vec<_>>(),
            ["alice does a thing never seen."]
        );
    }

    #[test]
    fn an_event_without_its_timestamp_is_still_an_event() {
        let g = game("[20:37:46] alice draws a card.\r\nbob's [Kaya, Ghost Assassin] is being attacked by [Flourishing Fox]\r\n");
        assert_eq!(g.entries[1].time, None);
        assert!(matches!(g.entries[1].event, Event::Attacked { .. }));
    }

    #[test]
    fn a_twice_encoded_name_is_repaired() {
        assert_eq!(
            c("J\u{c3}\u{83}\u{c2}\u{b6}tun Grunt").name(),
            "Jötun Grunt"
        );
        assert_eq!(c("Lightning Bolt").name(), "Lightning Bolt");
    }
}

#[cfg(test)]
mod unnamed {
    use super::*;

    #[test]
    fn a_players_name_where_a_card_goes_is_an_unnamed_card() {
        let m = parse(
            "Freeform\n\nx\nalice,bob\n\n== Game 1 (1) ==\n\
             [00:00:01] alice plays bob.\n\
             [00:00:02] alice is being attacked by bob and [Spirit Token]\n\
             [00:00:03] alice blocks [Spirit Token].\n",
        )
        .unwrap();
        let events: Vec<_> = m.games[0].entries.iter().map(|e| &e.event).collect();
        assert_eq!(
            events,
            [
                &Event::PlaysLand {
                    player: Player(0),
                    card: Object::Unnamed(Player(1))
                },
                &Event::Attacked {
                    defender: Defender::Player(Player(0)),
                    attackers: Attackers::Seen(vec![
                        Object::Unnamed(Player(1)),
                        Object::Card(Card::new("Spirit Token"))
                    ])
                },
                &Event::Blocks {
                    blocker: Object::Unnamed(Player(0)),
                    attacker: Object::Card(Card::new("Spirit Token"))
                },
            ]
        );
    }
}
