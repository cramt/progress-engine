//! The canonical definition of "what a decklist is".
//!
//! This exists as one implementation on purpose. The jq parser it replaces
//! carried the warning that motivated this crate: *"One copy of this regex, not
//! two — the two callers must agree on what a decklist is, or a deck could
//! validate at 100 cards and then deal a different 100."* `scryfall check` and
//! `scryfall play` now both shell out to `progress-engine parse`.

use std::num::NonZeroU32;
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
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
}

/// One `[Category{flag}]` element. Archidekt allows several per line,
/// comma-separated: `[Big Colorless,Test]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
        let (name, flags) = match raw.split_once('{') {
            Some((name, rest)) => {
                let inner = rest.strip_suffix('}').unwrap_or(rest);
                let flags = inner
                    .split(',')
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

    fn name_starts_with(&self, prefix: &str) -> bool {
        self.name.to_ascii_lowercase().starts_with(prefix)
    }

    fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub qty: NonZeroU32,
    pub name: String,
    pub set: Option<String>,
    pub num: Option<String>,
    pub foil: bool,
    /// Raw bracket contents exactly as written, `""` when absent. Preserved so
    /// nothing downstream that matched on the old opaque string breaks silently.
    pub category: String,
    pub categories: Vec<Category>,
}

impl Entry {
    /// A commander line, `[Commander{top}]`.
    ///
    /// Tested per category rather than against the whole bracket: with multiple
    /// categories `[Commander{top},Ramp]` no longer starts with "commander" as a
    /// single string, and anchoring on the raw text would silently lose the
    /// commander.
    pub fn is_commander(&self) -> bool {
        self.categories.iter().any(|c| c.name_starts_with("commander"))
    }

    /// In the list but not among the 100 — a companion is a 101st card (CR 903.11).
    ///
    /// Deliberately does NOT match `sticker`: sticker *sheets* sit outside the
    /// deck, but "Sticker Package" is a normal category for the real cards that
    /// apply them (Park Bleater, Ticketomaton). Matching that prefix once dropped
    /// five cards from a 100-card list and reported them as companions.
    pub fn is_outside(&self) -> bool {
        self.categories.iter().any(|c| {
            c.has_flag("nodeck")
                || c.name_starts_with("companion")
                || c.name_starts_with("sideboard")
                || c.name_starts_with("maybe")
        })
    }
}

fn line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // "2x Card Name (set) 123 *F* [Category{flags},Other]".
        // Only quantity and name are mandatory. The name is lazy so the trailing
        // optional groups win the ambiguity, which keeps multi-word names intact.
        Regex::new(
            r"(?x)
            ^\s*(?P<qty>[0-9]+)\s*[xX]?\s+(?P<name>.*?)
            (?:\s+\((?P<set>[^)]+)\)(?:\s+(?P<num>[^\s\[]+))?)?
            (?:\s+\*[Ff]\*)?
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

    let caps = line_re().captures(line).ok_or_else(|| ParseError::Malformed {
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

    let category = caps.name("cat").map(|m| m.as_str()).unwrap_or("").to_string();
    let categories = category.split(',').filter_map(Category::parse).collect();

    Ok(Some(Entry {
        qty,
        name,
        set: caps.name("set").map(|m| m.as_str().trim().to_string()),
        num: caps.name("num").map(|m| m.as_str().trim().to_string()),
        foil: line.contains("*F*") || line.contains("*f*"),
        category,
        categories,
    }))
}

/// Parse a whole decklist. Errors name the offending line rather than skipping it.
pub fn parse(text: &str) -> Result<Vec<Entry>, ParseError> {
    text.lines()
        .enumerate()
        .filter_map(|(i, l)| parse_line(l, i + 1).transpose())
        .collect()
}

/// Total physical cards, which is not the line count the moment a list has `3x Plains`.
pub fn total(entries: &[Entry]) -> u32 {
    entries.iter().map(|e| e.qty.get()).sum()
}
