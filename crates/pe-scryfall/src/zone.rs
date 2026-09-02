//! Card types that are never in your library.
//!
//! Stickers, attractions, planes and the rest are shuffled into a deck of their
//! own or into no deck at all. Counting them as library cards inflates the
//! library size and so moves every probability in the report: a 99-card list
//! with ten attractions answers questions about a 109-card library that does
//! not exist.

/// Which of those types a card is, when it is one.
///
/// A named type rather than a bool, because these are three or four different
/// zones wearing one hat — the attraction deck really is drawn from, via Open
/// an Attraction, and a later feature that models it needs to tell an
/// attraction from an emblem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutsideLibrary {
    Stickers,
    Attraction,
    Plane,
    Phenomenon,
    Scheme,
    Vanguard,
    Conspiracy,
    Dungeon,
    Emblem,
}

impl OutsideLibrary {
    pub fn as_str(self) -> &'static str {
        match self {
            OutsideLibrary::Stickers => "Stickers",
            OutsideLibrary::Attraction => "Attraction",
            OutsideLibrary::Plane => "Plane",
            OutsideLibrary::Phenomenon => "Phenomenon",
            OutsideLibrary::Scheme => "Scheme",
            OutsideLibrary::Vanguard => "Vanguard",
            OutsideLibrary::Conspiracy => "Conspiracy",
            OutsideLibrary::Dungeon => "Dungeon",
            OutsideLibrary::Emblem => "Emblem",
        }
    }
}

impl std::fmt::Display for OutsideLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Card types, which sit left of the em dash alongside the supertypes.
const CARD_TYPES: [(&str, OutsideLibrary); 8] = [
    ("stickers", OutsideLibrary::Stickers),
    ("plane", OutsideLibrary::Plane),
    ("phenomenon", OutsideLibrary::Phenomenon),
    ("scheme", OutsideLibrary::Scheme),
    ("vanguard", OutsideLibrary::Vanguard),
    ("conspiracy", OutsideLibrary::Conspiracy),
    ("dungeon", OutsideLibrary::Dungeon),
    ("emblem", OutsideLibrary::Emblem),
];

/// Classify a type line, if it names a type that is never in the library.
///
/// The type line is parsed rather than searched. `type_line.contains("Plane")`
/// matches every planeswalker, and there are two in the fixture deck alone; the
/// card types are whole words taken from the left of the em dash. Attraction is
/// the one that lives on the right, as a subtype of `Artifact — Attraction`.
///
/// Categories cannot do this job at all: "Sticker Package" is a legitimate
/// category for the real cards that apply stickers, and matching it once
/// dropped five of them from a 100-card list.
pub fn outside_library(type_line: &str) -> Option<OutsideLibrary> {
    type_line.split("//").find_map(face_outside_library)
}

fn face_outside_library(face: &str) -> Option<OutsideLibrary> {
    let (types, subtypes) = face.split_once('—').unwrap_or((face, ""));

    CARD_TYPES
        .iter()
        .find(|(word, _)| has_word(types, word))
        .map(|&(_, found)| found)
        .or_else(|| has_word(subtypes, "attraction").then_some(OutsideLibrary::Attraction))
}

/// Shared with `legality.rs`, which avoids substrings for the same reason.
pub(crate) fn has_word(haystack: &str, word: &str) -> bool {
    haystack
        .split_whitespace()
        .any(|w| w.eq_ignore_ascii_case(word))
}
