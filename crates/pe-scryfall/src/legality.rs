//! Legality rules that a single card decides on its own.
//!
//! Everything here is a fact printed on the card or a judgment that follows
//! from one card alone. How many copies a decklist actually has, which cards
//! were nominated as commanders and how big the deck is are all decklist data,
//! so they stay with the caller that joins the two — this crate never learns
//! what a decklist is.
//!
//! The governing rule is the same one the query parser follows: silence is
//! never evidence. A card the index knows nothing about must come back
//! *unknown*, because a legality report that cries "illegal" over a missing
//! field is the confidently wrong number this project exists to prevent.

use facet::Facet;

use crate::zone::has_word;
use crate::Colors;

/// What the index says about a card's legality in Commander.
///
/// `Unknown` is an answer rather than a failure. The index is a cache built by
/// an external tool and the test fixtures are hand-written subsets of it, so
/// the field is routinely absent; a word Scryfall adds after this was written
/// lands here for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommanderLegality {
    Legal,
    NotLegal,
    Banned,
    Unknown,
}

impl CommanderLegality {
    /// Read Scryfall's legality word.
    ///
    /// `legal`, `not_legal` and `banned` are the only three the bulk data uses
    /// for Commander today; `restricted` and anything newer are deliberately
    /// left as `Unknown` rather than guessed at.
    pub fn from_word(word: &str) -> Self {
        match word {
            "legal" => Self::Legal,
            "not_legal" => Self::NotLegal,
            "banned" => Self::Banned,
            _ => Self::Unknown,
        }
    }

    /// Whether a Commander deck may contain the card at all, or `None` when
    /// the index never said.
    ///
    /// An `Option` so that a caller cannot fall into `!is_legal()` and report a
    /// violation it has no evidence for.
    pub fn permits_play(self) -> Option<bool> {
        match self {
            Self::Legal => Some(true),
            Self::NotLegal | Self::Banned => Some(false),
            Self::Unknown => None,
        }
    }
}

/// Scryfall's legality word for one card, as written in the index.
///
/// Stored as text and interpreted on read, so that a word this crate has never
/// seen degrades to [`CommanderLegality::Unknown`]. Parsing straight into the
/// enum would instead reject a 25MB index over one unfamiliar string. It is a
/// newtype rather than a `String` so that nobody can write
/// `card.commander_legal == "legal"` and quietly treat an empty field as
/// illegal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
#[facet(transparent)]
pub struct LegalityWord(String);

impl LegalityWord {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn commander(&self) -> CommanderLegality {
        CommanderLegality::from_word(&self.0)
    }
}

impl From<&str> for LegalityWord {
    fn from(word: &str) -> Self {
        LegalityWord(word.to_string())
    }
}

/// How a card qualifies for the command zone, when it does.
///
/// A named route rather than a bool because the three are not interchangeable:
/// a Background only commands alongside a partner that chose it, so a caller
/// that can see the rest of the deck has something better to say than
/// "illegal".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommanderRoute {
    /// Rule 903.3: a legendary creature, the ordinary route.
    LegendaryCreature,
    /// The card grants itself the role in its own text — the planeswalker
    /// commanders and the spell commanders.
    SaysSo,
    /// Rule 903.3d: a Background, and only beside a "Choose a Background"
    /// commander. Whether such a partner exists is decklist data.
    Background,
}

/// Which route, if any, puts this card in the command zone.
///
/// Deliberately the printed routes only. A card that becomes a creature
/// somewhere other than the battlefield (Grist) is a rules-committee ruling
/// rather than anything the type line or oracle text says, so it is not
/// modelled here.
pub fn commander_route(name: &str, type_line: &str, oracle: &str) -> Option<CommanderRoute> {
    if is_legendary_creature(type_line) {
        Some(CommanderRoute::LegendaryCreature)
    } else if says_it_can_be_your_commander(name, oracle) {
        Some(CommanderRoute::SaysSo)
    } else if is_background(type_line) {
        Some(CommanderRoute::Background)
    } else {
        None
    }
}

/// Whether the front face is a legendary creature.
///
/// Only the front face counts: a modal card whose *back* is a legendary
/// creature cannot be your commander.
pub fn is_legendary_creature(type_line: &str) -> bool {
    let types = front_face_types(type_line);
    has_word(types, "legendary") && has_word(types, "creature")
}

/// Whether the card carries the `Basic` supertype.
///
/// Rule 100.2a lifts the copy limit for the *supertype*, not for lands — which
/// is why `Basic Snow Land` counts and why the one printed `Basic Creature`
/// does too. Matched as a whole word left of the em dash, following `zone.rs`:
/// substring matching on type lines has already produced one real bug in this
/// crate, `Plane` inside `Planeswalker`.
pub fn has_basic_supertype(type_line: &str) -> bool {
    has_word(front_face_types(type_line), "basic")
}

/// Whether the card is a Background, which is a subtype and so sits right of
/// the em dash.
pub fn is_background(type_line: &str) -> bool {
    type_line.split("//").any(|face| {
        face.split_once('—')
            .is_some_and(|(_, subtypes)| has_word(subtypes, "background"))
    })
}

/// Whether the card's own text says it can be your commander.
///
/// The sentence must be about *this* card, which is why the name is needed.
/// A bare substring search for "can be your commander" also matches the
/// Background reminder "It becomes legendary and can be your commander", which
/// is a promise about somebody else's creature; the subject is checked instead.
/// Cards name themselves in full ("Aminatou, the Fateshifter"), by the short
/// name printed before the comma ("Svega"), or as "This card" inside a reminder.
pub fn says_it_can_be_your_commander(name: &str, oracle: &str) -> bool {
    const GRANT: &str = "can be your commander.";

    let oracle = oracle.to_ascii_lowercase();
    let mut rest = oracle.as_str();
    while let Some(at) = rest.find(GRANT) {
        if names_itself(rest[..at].trim_end(), name) {
            return true;
        }
        rest = &rest[at + GRANT.len()..];
    }
    false
}

/// Whether a card's colour identity fits inside the identity a deck allows.
pub fn identity_fits_within(ci: &[String], allowed: Colors) -> bool {
    Colors::from_identity(ci).is_subset_of(allowed)
}

/// The types and supertypes of the front face: left of the em dash, and left
/// of the `//` that joins the faces of a double-faced card.
fn front_face_types(type_line: &str) -> &str {
    let front = type_line.split("//").next().unwrap_or(type_line);
    front.split_once('—').map_or(front, |(types, _)| types)
}

/// Whether `head`, the text leading up to "can be your commander", ends by
/// naming the card itself.
fn names_itself(head: &str, name: &str) -> bool {
    self_references(name).any(|subject| {
        head.strip_suffix(&subject.to_ascii_lowercase())
            // A boundary check, so a card named "Cid" is not found inside
            // "Lucid"; reminder text opens with "(" instead of a space.
            .is_some_and(|before| {
                before
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_alphanumeric())
            })
    })
}

fn self_references(name: &str) -> impl Iterator<Item = String> + '_ {
    let faces = name.split("//").flat_map(|face| {
        let face = face.trim();
        let short = face.split(',').next().unwrap_or(face).trim();
        [face.to_string(), short.to_string()]
    });
    std::iter::once("this card".to_string()).chain(faces)
}
