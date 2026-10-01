//! The family's own deck format, `.deck.toml`.
//!
//! [ADR-0020](../../../../docs/adr/0020-decks-are-toml-with-typed-categories.md)
//! is the authority. In short: a card is named exactly once, by printing or by
//! name; categories are declared, optionally typed from a fixed tree; and where
//! a card is follows from the most specific type among its categories.
//!
//! The file is read into `Raw*` types that mirror it, then checked into the
//! domain types, which cannot hold a card named twice, an undeclared category
//! or a card in two places at once.

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU32;

use facet::Facet;
use thiserror::Error;

pub use crate::archidekt::{
    export_archidekt, import_archidekt, ExportError, ImportError, Imported, SetOnly, Unreadable,
};

/// Where a category says its cards are. The tree is the tool's, not the deck's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CategoryType {
    InDeck,
    Commander,
    NotInDeck,
    Sideboard,
    Companion,
    Maybeboard,
    Attractions,
    StickerSheet,
}

impl CategoryType {
    pub const ALL: [CategoryType; 8] = [
        CategoryType::InDeck,
        CategoryType::Commander,
        CategoryType::NotInDeck,
        CategoryType::Sideboard,
        CategoryType::Companion,
        CategoryType::Maybeboard,
        CategoryType::Attractions,
        CategoryType::StickerSheet,
    ];

    pub fn parent(self) -> Option<CategoryType> {
        use CategoryType::*;
        match self {
            InDeck | NotInDeck => None,
            Commander => Some(InDeck),
            Sideboard | Maybeboard | Attractions | StickerSheet => Some(NotInDeck),
            Companion => Some(Sideboard),
        }
    }

    pub fn as_str(self) -> &'static str {
        use CategoryType::*;
        match self {
            InDeck => "in-deck",
            Commander => "commander",
            NotInDeck => "not-in-deck",
            Sideboard => "sideboard",
            Companion => "companion",
            Maybeboard => "maybeboard",
            Attractions => "attractions",
            StickerSheet => "sticker-sheet",
        }
    }

    fn parse(text: &str) -> Option<CategoryType> {
        CategoryType::ALL.into_iter().find(|t| t.as_str() == text)
    }

    /// `self` is `other` or lies below it: a companion is within sideboard,
    /// which is within not-in-deck.
    pub fn is_within(self, other: CategoryType) -> bool {
        std::iter::successors(Some(self), |t| t.parent()).any(|t| t == other)
    }

    fn depth(self) -> usize {
        std::iter::successors(self.parent(), |t| t.parent()).count()
    }
}

impl fmt::Display for CategoryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub name: String,
    /// `None` is a label: it says what a card does, not where it is.
    pub kind: Option<CategoryType>,
}

/// A printing is its set and collector number, `msc/183` in the file. The set
/// is lowercased, as Scryfall writes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Printing {
    pub set: String,
    pub num: String,
}

impl Printing {
    pub(crate) fn parse(text: &str) -> Option<Printing> {
        let (set, num) = text.split_once('/')?;
        let (set, num) = (set.trim(), num.trim());
        if set.is_empty() || num.is_empty() || num.contains('/') {
            return None;
        }
        Some(Printing {
            set: set.to_ascii_lowercase(),
            num: num.to_string(),
        })
    }
}

impl fmt::Display for Printing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.set, self.num)
    }
}

/// How a card is named in the file: once, one way or the other.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CardRef {
    /// This exact printing. Its name comes from printing data, not the file.
    Printing(Printing),
    /// Any printing of the card with this name.
    Name(String),
}

impl fmt::Display for CardRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CardRef::Printing(p) => p.fmt(f),
            CardRef::Name(n) => f.write_str(n),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Facet)]
#[repr(u8)]
#[facet(rename_all = "lowercase")]
pub enum Finish {
    #[default]
    Nonfoil,
    Foil,
    Etched,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub card: CardRef,
    pub qty: NonZeroU32,
    pub finish: Finish,
    /// Declared category names, in the order the file lists them.
    pub categories: Vec<String>,
    /// Where the card is: the most specific type among its categories, or
    /// `InDeck` when none is typed.
    pub place: CategoryType,
}

impl Card {
    /// In the library, counted toward the deck.
    pub fn in_deck(&self) -> bool {
        self.place.is_within(CategoryType::InDeck)
    }

    pub fn is_commander(&self) -> bool {
        self.place == CategoryType::Commander
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deck {
    pub name: Option<String>,
    /// Metadata. Nothing in the family reads it to decide what is legal (ADR-0005).
    pub format: Option<String>,
    /// The printing whose art stands for the deck in Curator's deck list. It
    /// need not be in the deck, and nothing in the family reads it otherwise.
    pub cover: Option<Printing>,
    /// Sorted by name, as a TOML table is.
    pub categories: Vec<Category>,
    /// In file order.
    pub cards: Vec<Card>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DeckError {
    #[error("not a deck file: {0}")]
    Toml(String),
    #[error("card {index}: named twice, by `name` and by `printing`; a printing already says which card it is, so keep one")]
    NamedTwice { index: usize },
    #[error("card {index}: has neither `name` nor `printing`")]
    Unnamed { index: usize },
    #[error("card {index}: printing {text:?} is not set/number, like \"msc/183\"")]
    BadPrinting { index: usize, text: String },
    #[error("cover {text:?} is not set/number, like \"msc/183\"")]
    BadCover { text: String },
    #[error("card {index} ({card}): qty must be at least 1")]
    ZeroQty { index: usize, card: CardRef },
    #[error("card {index} ({card}): finish {text:?} is not \"foil\" or \"etched\"")]
    BadFinish {
        index: usize,
        card: CardRef,
        text: String,
    },
    #[error("category {category:?}: type {text:?} is not one of {}", type_names())]
    UnknownType { category: String, text: String },
    #[error("card {index} ({card}): category {category:?} is not declared under [categories]")]
    Undeclared {
        index: usize,
        card: CardRef,
        category: String,
    },
    #[error("card {index} ({card}): lists category {category:?} twice")]
    ListedTwice {
        index: usize,
        card: CardRef,
        category: String,
    },
    #[error(
        "card {index} ({card}): {a:?} ({a_type}) and {b:?} ({b_type}) put it in two places at once"
    )]
    TwoPlaces {
        index: usize,
        card: CardRef,
        a: String,
        a_type: CategoryType,
        b: String,
        b_type: CategoryType,
    },
}

fn type_names() -> String {
    CategoryType::ALL.map(CategoryType::as_str).join(", ")
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct RawDeck {
    name: Option<String>,
    format: Option<String>,
    cover: Option<String>,
    #[facet(default)]
    cards: Vec<RawCard>,
    #[facet(default)]
    categories: BTreeMap<String, RawCategory>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
pub(crate) struct RawCard {
    pub(crate) name: Option<String>,
    pub(crate) printing: Option<String>,
    pub(crate) qty: Option<u32>,
    pub(crate) finish: Option<String>,
    #[facet(rename = "in", default)]
    pub(crate) categories: Vec<String>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct RawCategory {
    #[facet(rename = "type")]
    kind: Option<String>,
}

impl Deck {
    pub fn parse(text: &str) -> Result<Deck, DeckError> {
        let raw: RawDeck =
            facet_toml::from_str(text).map_err(|e| DeckError::Toml(e.to_string()))?;

        let categories =
            raw.categories
                .into_iter()
                .map(|(name, c)| {
                    let kind = match c.kind {
                        None => None,
                        Some(text) => Some(CategoryType::parse(&text).ok_or_else(|| {
                            DeckError::UnknownType {
                                category: name.clone(),
                                text,
                            }
                        })?),
                    };
                    Ok(Category { name, kind })
                })
                .collect::<Result<Vec<_>, DeckError>>()?;

        let cards = raw
            .cards
            .into_iter()
            .enumerate()
            .map(|(i, c)| card(i + 1, c, &categories))
            .collect::<Result<Vec<_>, DeckError>>()?;

        let cover = raw
            .cover
            .map(|text| Printing::parse(&text).ok_or(DeckError::BadCover { text }))
            .transpose()?;

        Ok(Deck {
            name: raw.name,
            format: raw.format,
            cover,
            categories,
            cards,
        })
    }

    pub fn category(&self, name: &str) -> Option<&Category> {
        self.categories.iter().find(|c| c.name == name)
    }
}

/// What names a card line and how many of it there are, as every file in the
/// family that lists cards writes them: named once, `qty` absent for one,
/// `finish` absent for nonfoil. `index` is 1-based, for the error.
pub(crate) fn line(
    index: usize,
    name: Option<String>,
    printing: Option<String>,
    qty: Option<u32>,
    finish: Option<&str>,
) -> Result<(CardRef, NonZeroU32, Finish), DeckError> {
    let card = match (name, printing) {
        (Some(_), Some(_)) => return Err(DeckError::NamedTwice { index }),
        (None, None) => return Err(DeckError::Unnamed { index }),
        (Some(name), None) => CardRef::Name(name),
        (None, Some(text)) => {
            CardRef::Printing(Printing::parse(&text).ok_or(DeckError::BadPrinting { index, text })?)
        }
    };

    let qty = match qty {
        None => NonZeroU32::MIN,
        Some(n) => NonZeroU32::new(n).ok_or_else(|| DeckError::ZeroQty {
            index,
            card: card.clone(),
        })?,
    };

    let finish = match finish {
        None => Finish::Nonfoil,
        Some("foil") => Finish::Foil,
        Some("etched") => Finish::Etched,
        Some(other) => {
            return Err(DeckError::BadFinish {
                index,
                card,
                text: other.to_string(),
            })
        }
    };
    Ok((card, qty, finish))
}

pub(crate) fn card(index: usize, raw: RawCard, declared: &[Category]) -> Result<Card, DeckError> {
    let (card, qty, finish) = line(
        index,
        raw.name,
        raw.printing,
        raw.qty,
        raw.finish.as_deref(),
    )?;

    let mut typed: Vec<(&str, CategoryType)> = Vec::new();
    for (n, name) in raw.categories.iter().enumerate() {
        if raw.categories[..n].contains(name) {
            return Err(DeckError::ListedTwice {
                index,
                card,
                category: name.clone(),
            });
        }
        let category =
            declared
                .iter()
                .find(|c| &c.name == name)
                .ok_or_else(|| DeckError::Undeclared {
                    index,
                    card: card.clone(),
                    category: name.clone(),
                })?;
        if let Some(kind) = category.kind {
            typed.push((&category.name, kind));
        }
    }

    // The deepest type is where the card is, and every other typed category
    // must be on the way up to it.
    let place = match typed.iter().max_by_key(|(_, t)| t.depth()) {
        None => CategoryType::InDeck,
        Some(&(deepest, place)) => {
            if let Some(&(other, kind)) = typed.iter().find(|(_, t)| !place.is_within(*t)) {
                return Err(DeckError::TwoPlaces {
                    index,
                    card,
                    a: deepest.to_string(),
                    a_type: place,
                    b: other.to_string(),
                    b_type: kind,
                });
            }
            place
        }
    };

    Ok(Card {
        card,
        qty,
        finish,
        categories: raw.categories,
        place,
    })
}

impl Deck {
    /// The deck as `.deck.toml`, one card per line. `name_of` gives the name a
    /// printing is written with as a comment; the comment is never read back.
    pub fn to_toml(&self, name_of: impl Fn(&Printing) -> Option<String>) -> String {
        let mut out = String::new();
        if let Some(name) = &self.name {
            out += &format!("name = {}\n", quote(name));
        }
        if let Some(format) = &self.format {
            out += &format!("format = {}\n", quote(format));
        }
        if let Some(cover) = &self.cover {
            out += &format!("cover = {}\n", quote(&cover.to_string()));
        }
        if !out.is_empty() {
            out.push('\n');
        }

        out += "cards = [\n";
        for c in &self.cards {
            let mut fields = vec![match &c.card {
                CardRef::Printing(p) => format!("printing = {}", quote(&p.to_string())),
                CardRef::Name(n) => format!("name = {}", quote(n)),
            }];
            if c.qty.get() != 1 {
                fields.push(format!("qty = {}", c.qty));
            }
            match c.finish {
                Finish::Nonfoil => {}
                Finish::Foil => fields.push("finish = \"foil\"".into()),
                Finish::Etched => fields.push("finish = \"etched\"".into()),
            }
            if !c.categories.is_empty() {
                let list: Vec<String> = c.categories.iter().map(|n| quote(n)).collect();
                fields.push(format!("in = [{}]", list.join(", ")));
            }
            out += &format!("  {{ {} }},", fields.join(", "));
            if let CardRef::Printing(p) = &c.card {
                if let Some(name) = name_of(p) {
                    out += &format!("  # {name}");
                }
            }
            out.push('\n');
        }
        out += "]\n";

        if !self.categories.is_empty() {
            out += "\n[categories]\n";
            for c in &self.categories {
                let value = match c.kind {
                    None => "{}".to_string(),
                    Some(t) => format!("{{ type = \"{t}\" }}"),
                };
                out += &format!("{} = {value}\n", key(&c.name));
            }
        }
        out
    }
}

/// A TOML basic string.
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            '\t' => out += "\\t",
            c if c.is_control() => out += &format!("\\u{:04X}", c as u32),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A TOML key: bare when it can be, quoted when not.
fn key(s: &str) -> String {
    let bare = !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if bare {
        s.to_string()
    } else {
        quote(s)
    }
}
