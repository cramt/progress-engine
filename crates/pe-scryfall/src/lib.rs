//! A rudimentary parser for Scryfall search syntax.
//!
//! Deliberately a subset. The governing rule is that anything unsupported is a
//! *parse error naming the offending term*, never a silent no-match — a query
//! that quietly matches nothing produces a confidently wrong probability, which
//! is the exact failure this crate exists to prevent.

pub mod index;
mod parse;
mod zone;

pub use parse::{parse, ParseError};
pub use zone::{outside_library, OutsideLibrary};

/// A card as the matcher sees it: Scryfall's data plus the categories the
/// decklist assigned it.
#[derive(Debug, Clone)]
pub struct CardView<'a> {
    pub name: &'a str,
    pub type_line: &'a str,
    pub oracle: &'a str,
    pub cmc: f64,
    /// Colour identity letters, e.g. `["W","U"]`.
    pub color_identity: &'a [String],
    pub categories: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Lt,
    Le,
    Eq,
    Ne,
    Ge,
    Gt,
}

impl Cmp {
    fn test_ord(self, ordering: std::cmp::Ordering) -> bool {
        use std::cmp::Ordering::*;
        matches!(
            (self, ordering),
            (Cmp::Lt, Less)
                | (Cmp::Le, Less | Equal)
                | (Cmp::Eq, Equal)
                | (Cmp::Ne, Less | Greater)
                | (Cmp::Ge, Greater | Equal)
                | (Cmp::Gt, Greater)
        )
    }
}

/// Colour identity as a 6-bit set over WUBRG plus colourless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Colors(u8);

impl Colors {
    pub const LETTERS: [(char, u8); 5] = [('w', 1), ('u', 2), ('b', 4), ('r', 8), ('g', 16)];

    pub fn from_letters(s: &str) -> Option<Self> {
        let mut bits = 0u8;
        for ch in s.to_ascii_lowercase().chars() {
            if ch == 'c' {
                continue; // colourless contributes no bits
            }
            let (_, bit) = Self::LETTERS.iter().find(|(l, _)| *l == ch)?;
            bits |= bit;
        }
        Some(Colors(bits))
    }

    pub fn from_identity(ci: &[String]) -> Self {
        let joined: String = ci.concat();
        Colors::from_letters(&joined).unwrap_or_default()
    }

    fn is_subset_of(self, other: Colors) -> bool {
        self.0 & !other.0 == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsProperty {
    Permanent,
    Spell,
    Historic,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    /// `t:land` — substring of the type line, case-insensitive.
    Type(String),
    /// `o:"Add {W}"` — substring of oracle text, case-insensitive.
    Oracle(String),
    /// `name:"Rogue's Passage"`, or a bare word.
    Name(String),
    /// `cat:"Exile Outlet"` — the one non-Scryfall addition, matching the
    /// decklist's Archidekt categories.
    Category(String),
    /// `mv<=2`, `cmc=3`.
    ManaValue(Cmp, f64),
    /// `id<=W`. `id:` is a synonym for `id<=`, as on Scryfall.
    Identity(Cmp, Colors),
    /// `is:permanent`.
    Is(IsProperty),
    Not(Box<Query>),
    And(Vec<Query>),
    Or(Vec<Query>),
}

impl Query {
    pub fn matches(&self, card: &CardView<'_>) -> bool {
        match self {
            Query::Type(s) => contains_ci(card.type_line, s),
            Query::Oracle(s) => contains_ci(card.oracle, s),
            Query::Name(s) => contains_ci(card.name, s),
            Query::Category(s) => card.categories.iter().any(|c| c.eq_ignore_ascii_case(s)),
            Query::ManaValue(cmp, v) => {
                card.cmc.partial_cmp(v).is_some_and(|ord| cmp.test_ord(ord))
            }
            Query::Identity(cmp, want) => {
                let have = Colors::from_identity(card.color_identity);
                match cmp {
                    // Subset/superset rather than a numeric ordering: `id<=W`
                    // asks whether the card fits inside a white deck.
                    Cmp::Le => have.is_subset_of(*want),
                    Cmp::Lt => have.is_subset_of(*want) && have != *want,
                    Cmp::Ge => want.is_subset_of(have),
                    Cmp::Gt => want.is_subset_of(have) && have != *want,
                    Cmp::Eq => have == *want,
                    Cmp::Ne => have != *want,
                }
            }
            Query::Is(p) => match p {
                IsProperty::Permanent => [
                    "artifact",
                    "creature",
                    "enchantment",
                    "land",
                    "planeswalker",
                    "battle",
                ]
                .iter()
                .any(|t| contains_ci(card.type_line, t)),
                IsProperty::Spell => !contains_ci(card.type_line, "land"),
                // Historic is artifact, legendary or saga (the Teshar test).
                IsProperty::Historic => ["artifact", "legendary", "saga"]
                    .iter()
                    .any(|t| contains_ci(card.type_line, t)),
            },
            Query::Not(inner) => !inner.matches(card),
            Query::And(parts) => parts.iter().all(|p| p.matches(card)),
            Query::Or(parts) => parts.iter().any(|p| p.matches(card)),
        }
    }
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}
