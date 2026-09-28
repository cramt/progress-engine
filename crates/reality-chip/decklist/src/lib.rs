//! The canonical definition of "what a decklist is".
//!
//! This exists as one implementation on purpose. The jq parser it replaces
//! carried the warning that motivated this crate: *"One copy of this regex, not
//! two — the two callers must agree on what a decklist is, or a deck could
//! validate at 100 cards and then deal a different 100."* `scryfall check` and
//! `scryfall play` now both shell out to `gauntlet parse`.

pub mod archidekt;
pub mod changelog;
pub mod deck;
pub mod edit;

use std::num::NonZeroU32;
use std::sync::OnceLock;

use facet::Facet;

use crate::deck::{CategoryType, Finish};
use regex::Regex;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    /// The predecessor silently dropped lines it could not match, so a typo'd
    /// line vanished and only surfaced as a wrong total much later. Refuse instead.
    #[error("line {line}: not a decklist entry: {text:?}")]
    Malformed { line: usize, text: String },
    #[error("line {line}: quantity must be greater than zero: {text:?}")]
    ZeroQuantity { line: usize, text: String },
    #[error("line {line}: card name is empty: {text:?}")]
    EmptyName { line: usize, text: String },
    /// Read as part of the name, an unknown marker made a card no index has,
    /// and its meaning (a finish, most likely) was lost without a word.
    #[error("line {line}: {marker} is not a marker this reads, which are *F* (foil) and *E* (etched): {text:?}")]
    UnknownMarker {
        line: usize,
        text: String,
        marker: String,
    },
}

/// One `[Category{flag}]` element. Archidekt allows several per line,
/// comma-separated: `[Big Colorless,Test]`.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct Category {
    /// Category text with any `{flags}` stripped, e.g. `Commander`.
    pub name: String,
    /// Flags inside braces, lowercased, e.g. `top`, `nodeck`.
    pub flags: Vec<String>,
}

impl Category {
    fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        // Flags are every `{...}` group after the name, and a group may hold
        // several: Archidekt writes its maybeboard `Maybeboard{noDeck}{noPrice}`.
        let (name, flags) = match raw.split_once('{') {
            Some((name, rest)) => {
                let flags = rest
                    .split(['{', '}', ','])
                    .map(|f| f.trim().to_ascii_lowercase())
                    .filter(|f| !f.is_empty())
                    .collect();
                (name.trim(), flags)
            }
            None => (raw, Vec::new()),
        };
        if name.is_empty() && flags.is_empty() {
            return None;
        }
        Some(Category {
            name: name.to_string(),
            flags,
        })
    }

    /// `{top}`, which Archidekt calls Premier: "this category marks cards as
    /// being a Commander".
    pub fn is_premier(&self) -> bool {
        self.has_flag("top")
    }

    /// `{noDeck}`: the category's In Deck box is cleared.
    pub fn is_no_deck(&self) -> bool {
        self.has_flag("nodeck")
    }

    fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    /// The type Archidekt gives this category when it is a card's first, from
    /// its exact name and its flags. The one place a category is typed: both a
    /// parsed [`Entry`] and an imported deck ask it.
    pub fn archidekt_type(&self) -> Option<CategoryType> {
        if self.is_premier() {
            Some(CategoryType::Commander)
        } else if self.name == "Sideboard" {
            Some(CategoryType::Sideboard)
        } else if self.name == "Maybeboard" {
            Some(CategoryType::Maybeboard)
        } else if self.is_no_deck() {
            // Archidekt has no board for these; `{noDeck}` is all it knows. The
            // names are the ones the export writes, so they read back as written.
            Some(match self.name.as_str() {
                "Companion" => CategoryType::Companion,
                "Attractions" => CategoryType::Attractions,
                "Sticker Sheet" => CategoryType::StickerSheet,
                _ => CategoryType::NotInDeck,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct Entry {
    pub qty: NonZeroU32,
    pub name: String,
    pub set: Option<String>,
    pub num: Option<String>,
    /// From the marker after the printing: `*F*` foil, `*E*` etched, none
    /// nonfoil.
    pub finish: Finish,
    /// Raw bracket contents exactly as written, `""` when absent. Preserved so
    /// nothing downstream that matched on the old opaque string breaks silently.
    pub category: String,
    pub categories: Vec<Category>,
}

impl Entry {
    /// The first category, the only one Archidekt places a card by
    /// (`docs/research/archidekt-import-shapes.md`). The rest are labels that
    /// stay on the card.
    pub fn primary(&self) -> Option<&Category> {
        self.categories.first()
    }

    /// Where Archidekt puts the card: its first category's type
    /// ([`Category::archidekt_type`]), or in the deck. The flags are the ones
    /// this entry carries, so an entry from [`parse`] is placed by the whole
    /// list's flags, as [`deck::Deck::read_archidekt`] places it.
    pub fn place(&self) -> CategoryType {
        self.primary()
            .and_then(Category::archidekt_type)
            .unwrap_or(CategoryType::InDeck)
    }

    /// A commander, the way Archidekt reads one: the first category carries
    /// `{top}`. `[Commander]` without it is an ordinary group, and
    /// `[Ramp,Commander{top}]` shows under Ramp with no crown.
    pub fn is_commander(&self) -> bool {
        self.place() == CategoryType::Commander
    }

    /// In the list but not among the 100, the way Archidekt reads it: the first
    /// category is `{noDeck}`, or is exactly `Sideboard` or `Maybeboard`, its
    /// only two boards. `sideboard`, `Sideboard Lessons`, `[Companion]` and
    /// `[Removal,Sideboard]` all count toward the deck in Archidekt, and so they
    /// do here.
    ///
    /// Nothing is read from a name prefix. Matching `sticker` once dropped five
    /// real cards in "Sticker Package" from a 100-card list; a sticker sheet is
    /// out of the deck by its `{noDeck}` alone.
    pub fn is_outside(&self) -> bool {
        !self.place().is_within(CategoryType::InDeck)
    }
}

fn line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // "2x Card Name (set) 123 *F* [Category{flags},Other]".
        // Only quantity and name are mandatory. The name is lazy so the trailing
        // optional groups win the ambiguity, which keeps multi-word names intact.
        // A printing needs its number: names end in parentheticals too
        // (`Erase (Not the Urza's Legacy One)`), and one with no number after
        // it stays in the name.
        Regex::new(
            r"(?x)
            ^\s*(?P<qty>[0-9]+)\s*[xX]?\s+(?P<name>.*?)
            (?:\s+\((?P<set>[^)]+)\)\s+(?P<num>[^\s\[*(][^\s\[]*))?
            (?:\s+(?P<marker>\*[^*\s\[\]]+\*))?
            (?:\s+\[(?P<cat>[^\]]*)\])?\s*$",
        )
        .expect("decklist line regex is valid")
    })
}

/// Parse one line. `None` for blanks and comments.
///
/// `//` opens a comment only at the start of a line: card names contain it
/// (`Unstable Glyphbridge // Sandswirl Wanderglyph`) and treating it as an
/// inline comment marker would truncate every double-faced card.
pub fn parse_line(line: &str, number: usize) -> Result<Option<Entry>, ParseError> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") {
        return Ok(None);
    }

    let caps = line_re()
        .captures(line)
        .ok_or_else(|| ParseError::Malformed {
            line: number,
            text: trimmed.to_string(),
        })?;

    let qty: u32 = caps["qty"].parse().map_err(|_| ParseError::Malformed {
        line: number,
        text: trimmed.to_string(),
    })?;
    let qty = NonZeroU32::new(qty).ok_or_else(|| ParseError::ZeroQuantity {
        line: number,
        text: trimmed.to_string(),
    })?;

    let name = caps["name"].trim().to_string();
    if name.is_empty() {
        return Err(ParseError::EmptyName {
            line: number,
            text: trimmed.to_string(),
        });
    }

    let category = caps
        .name("cat")
        .map(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let categories = merge(split_categories(&category).filter_map(Category::parse));

    // `*E*` is as a user-posted Archidekt export writes an etched card; the
    // sandbox probes in archidekt-import-shapes.md never had one.
    let finish = match caps.name("marker").map(|m| m.as_str()) {
        None => Finish::Nonfoil,
        Some("*F*" | "*f*") => Finish::Foil,
        Some("*E*") => Finish::Etched,
        Some(other) => {
            return Err(ParseError::UnknownMarker {
                line: number,
                text: trimmed.to_string(),
                marker: other.to_string(),
            })
        }
    };

    Ok(Some(Entry {
        qty,
        name,
        set: caps.name("set").map(|m| m.as_str().trim().to_string()),
        num: caps.name("num").map(|m| m.as_str().trim().to_string()),
        finish,
        category,
        categories,
    }))
}

/// The bracket's categories, split at each `,` outside braces: inside them a
/// comma separates flags, as in `Maybeboard{noDeck,noPrice}`.
fn split_categories(bracket: &str) -> impl Iterator<Item = &str> {
    let mut depth = 0usize;
    bracket.split(move |c| {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        c == ',' && depth == 0
    })
}

/// One category per name: a second mention of a name adds its flags to the
/// first rather than listing the card in it twice.
fn merge(categories: impl IntoIterator<Item = Category>) -> Vec<Category> {
    let mut out: Vec<Category> = Vec::new();
    for c in categories {
        match out.iter_mut().find(|o| o.name == c.name) {
            Some(o) => {
                for f in c.flags {
                    if !o.flags.contains(&f) {
                        o.flags.push(f);
                    }
                }
            }
            None => out.push(c),
        }
    }
    out
}

/// A `# Heading` line: the category every following card gets as its first,
/// until the next heading. `# Commander` sets Premier by itself, as it does in
/// Archidekt; the bracket form `[Commander]` does not.
fn heading(line: &str) -> Option<Option<Category>> {
    let rest = line.trim().strip_prefix('#')?;
    Some(Category::parse(rest).map(|mut c| {
        if c.name == "Commander" && !c.is_premier() {
            c.flags.push("top".to_string());
        }
        c
    }))
}

/// Every card line of a decklist with its 1-based line number, each read or
/// refused on its own, and `# Heading` lines applied to the cards below them.
/// Blanks, `//` comments and headings yield nothing.
pub fn lines(text: &str) -> impl Iterator<Item = (usize, Result<Entry, ParseError>)> + '_ {
    let mut current: Option<Category> = None;
    text.lines().enumerate().filter_map(move |(i, l)| {
        if let Some(h) = heading(l) {
            current = h;
            return None;
        }
        let entry = parse_line(l, i + 1).transpose()?;
        Some((
            i + 1,
            entry.map(|mut e| {
                if let Some(h) = &current {
                    e.categories = merge(std::iter::once(h.clone()).chain(e.categories));
                }
                e
            }),
        ))
    })
}

/// Parse a whole decklist. Errors name the offending line rather than skipping
/// it. Every mention of a category carries the flags the list gives it
/// ([`share_flags`]), so each entry is placed as the whole list places it.
pub fn parse(text: &str) -> Result<Vec<Entry>, ParseError> {
    let mut entries = lines(text).map(|(_, e)| e).collect::<Result<Vec<_>, _>>()?;
    share_flags(&mut entries);
    Ok(entries)
}

/// A flag belongs to the category, not the line: Archidekt repeats it on each
/// line, and a line that leaves it off does not clear it. Gives every mention
/// of a category the flags any mention gave it, in first-seen order.
pub fn share_flags(entries: &mut [Entry]) {
    let mut flags: Vec<(String, Vec<String>)> = Vec::new();
    for c in entries.iter().flat_map(|e| &e.categories) {
        let have = match flags.iter_mut().find(|(n, _)| *n == c.name) {
            Some((_, have)) => have,
            None => {
                flags.push((c.name.clone(), Vec::new()));
                &mut flags.last_mut().expect("just pushed").1
            }
        };
        for f in &c.flags {
            if !have.contains(f) {
                have.push(f.clone());
            }
        }
    }
    for c in entries.iter_mut().flat_map(|e| &mut e.categories) {
        if let Some((_, all)) = flags.iter().find(|(n, _)| *n == c.name) {
            c.flags.clone_from(all);
        }
    }
}

/// Total physical cards, which is not the line count the moment a list has `3x Plains`.
pub fn total(entries: &[Entry]) -> u32 {
    entries.iter().map(|e| e.qty.get()).sum()
}

impl std::fmt::Display for Category {
    /// `Name{flag}{flag}`, the form Archidekt exports and [`Category::parse`]
    /// reads. Flags come out lowercased, as parsing left them: `{noDeck}` is
    /// written back `{nodeck}`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)?;
        for flag in &self.flags {
            write!(f, "{{{flag}}}")?;
        }
        Ok(())
    }
}

/// `line` with its categories replaced and every other byte left alone, so an
/// edit to a card's categories is a one-line diff that keeps its quantity,
/// printing and spelling exactly as written. No categories drops the bracket.
///
/// `None` when `line` is not an entry: a comment or blank has no categories to
/// replace, and a malformed line should be refused where it was parsed.
pub fn with_categories(line: &str, categories: &[Category]) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("//") {
        return None;
    }
    let caps = line_re().captures(line)?;
    let bracket = categories
        .iter()
        .map(Category::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let (head, tail) = match caps.name("cat") {
        // The span inside the brackets, widened to the brackets and the
        // whitespace before them.
        Some(cat) => {
            let open = line[..cat.start() - 1].trim_end().len();
            (&line[..open], &line[cat.end() + 1..])
        }
        None => {
            let end = line.trim_end().len();
            (&line[..end], &line[end..])
        }
    };
    Some(if categories.is_empty() {
        format!("{head}{tail}")
    } else {
        format!("{head} [{bracket}]{tail}")
    })
}
