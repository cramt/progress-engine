//! The canonical definition of "what a decklist is".
//!
//! This exists as one implementation on purpose. The jq parser it replaces
//! carried the warning that motivated this crate: *"One copy of this regex, not
//! two — the two callers must agree on what a decklist is, or a deck could
//! validate at 100 cards and then deal a different 100."* `scryfall check` and
//! `scryfall play` now both shell out to `gauntlet parse`.

pub mod archidekt;
pub mod deck;
pub mod edit;

use std::num::NonZeroU32;
use std::sync::OnceLock;

use facet::Facet;
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
}

#[derive(Debug, Clone, PartialEq, Eq, Facet)]
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
    /// The first category, the only one Archidekt places a card by
    /// (`docs/research/archidekt-import-shapes.md`). The rest are labels that
    /// stay on the card.
    pub fn primary(&self) -> Option<&Category> {
        self.categories.first()
    }

    /// A commander, the way Archidekt reads one: the first category carries
    /// `{top}`. `[Commander]` without it is an ordinary group, and
    /// `[Ramp,Commander{top}]` shows under Ramp with no crown.
    pub fn is_commander(&self) -> bool {
        self.primary().is_some_and(Category::is_premier)
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
        self.primary()
            .is_some_and(|c| c.is_no_deck() || c.name == "Sideboard" || c.name == "Maybeboard")
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
    let categories = merge(category.split(',').filter_map(Category::parse));

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

/// Parse a whole decklist. Errors name the offending line rather than skipping it.
pub fn parse(text: &str) -> Result<Vec<Entry>, ParseError> {
    lines(text).map(|(_, e)| e).collect()
}

/// Total physical cards, which is not the line count the moment a list has `3x Plains`.
pub fn total(entries: &[Entry]) -> u32 {
    entries.iter().map(|e| e.qty.get()).sum()
}

impl std::fmt::Display for Category {
    /// `Name{flag,flag}`, the form [`Category::parse`] reads. Flags come out
    /// lowercased, as parsing left them: `{noDeck}` is written back `{nodeck}`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)?;
        if !self.flags.is_empty() {
            write!(f, "{{{}}}", self.flags.join(","))?;
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
