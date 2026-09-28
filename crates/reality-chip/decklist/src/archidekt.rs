//! Archidekt's text, into a [`Deck`] and back out, read and written the way
//! Archidekt itself reads it.
//!
//! `docs/research/archidekt-import-shapes.md` is the spec. What it found, and
//! what this module does with it:
//!
//! - Archidekt places a card by its **first** category alone. The rest are
//!   labels that stay on the card.
//! - `{top}` on that first category makes the card a commander; a category
//!   named `Commander` without it is an ordinary group.
//! - `Sideboard` and `Maybeboard`, in exactly that case, are its only boards.
//!   `{noDeck}` takes a card out of the deck under any other name.
//!
//! A `.deck.toml` types a *category*, and a card is where the most specific
//! type among its categories says (ADR-0020). So a category is typed only when
//! it is some card's first, and a card that lists a typed category later on its
//! line, where Archidekt ignores it, loses that category rather than moving.
//! The import says so, as it says of every line it could not read.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use thiserror::Error;

use crate::deck::{
    card, CardRef, Category, CategoryType, Deck, DeckError, Finish, Printing, RawCard,
};
use crate::ParseError;

/// A line of Archidekt text the import could not carry over whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// 1-based, counting every line of the text.
    pub line: usize,
    /// The line, trimmed.
    pub text: String,
    pub reason: String,
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}: {:?}", self.line, self.reason, self.text)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ImportError {
    /// The predecessor dropped lines it could not match, so a typo'd line
    /// vanished and surfaced only as a wrong total much later.
    #[error("{0}")]
    Unreadable(Unreadable),
    #[error(transparent)]
    Deck(#[from] DeckError),
}

/// What an import made of Archidekt text: the deck, and every line it could not
/// carry over whole, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub deck: Deck,
    pub unreadable: Vec<Unreadable>,
    /// The name each printing's line gave it.
    pub names: HashMap<Printing, String>,
}

impl Imported {
    /// The deck as `.deck.toml`, each printing's name written beside it as a
    /// comment. The name is the one the line gave, and a comment is never read
    /// back, so it cannot disagree with the printing it sits beside.
    pub fn to_toml(&self) -> String {
        self.deck.to_toml(|p| self.names.get(p).cloned())
    }
}

fn reason(e: &ParseError) -> String {
    match e {
        ParseError::Malformed { .. } => {
            "not a card, which reads like `1x Card Name (set) 123 [Category]`".into()
        }
        ParseError::ZeroQuantity { .. } => "a quantity of 0 is no card".into(),
        ParseError::EmptyName { .. } => "the card has no name".into(),
        ParseError::UnknownMarker { marker, .. } => format!(
            "{marker} is no finish this reads, which are *F* (foil) and *E* (etched), \
             so the card is left out rather than imported in the wrong one"
        ),
    }
}

/// The type Archidekt gives a category that is some card's first, from its
/// exact name and its flags.
fn archidekt_type(name: &str, flags: &[String]) -> Option<CategoryType> {
    let flag = |f: &str| flags.iter().any(|x| x == f);
    if flag("top") {
        Some(CategoryType::Commander)
    } else if name == "Sideboard" {
        Some(CategoryType::Sideboard)
    } else if name == "Maybeboard" {
        Some(CategoryType::Maybeboard)
    } else if flag("nodeck") {
        // Archidekt has no board for these; `{noDeck}` is all it knows. The
        // names are the ones the export writes, so they read back as written.
        Some(match name {
            "Companion" => CategoryType::Companion,
            "Attractions" => CategoryType::Attractions,
            "Sticker Sheet" => CategoryType::StickerSheet,
            _ => CategoryType::NotInDeck,
        })
    } else {
        None
    }
}

impl Deck {
    /// Reads Archidekt text as Archidekt does, keeping every card it can and
    /// naming every line it could not carry over whole. Each card is named once
    /// (ADR-0020): by printing when the line has a set and number, by name
    /// otherwise.
    pub fn read_archidekt(text: &str) -> Imported {
        let source: Vec<&str> = text.lines().collect();
        let whole = |line: usize| source[line - 1].trim().to_string();
        let mut unreadable = Vec::new();

        let mut entries = Vec::new();
        for (line, entry) in crate::lines(text) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    unreadable.push(Unreadable {
                        line,
                        text: whole(line),
                        reason: reason(&e),
                    });
                    continue;
                }
            };
            let printing = match (&entry.set, &entry.num) {
                (Some(set), Some(num)) => match Printing::parse(&format!("{set}/{num}")) {
                    Some(p) => Some(p),
                    None => {
                        unreadable.push(Unreadable {
                            line,
                            text: whole(line),
                            reason: format!("({set}) {num} is not a set and collector number"),
                        });
                        continue;
                    }
                },
                _ => None,
            };
            entries.push((line, entry, printing));
        }

        // A flag belongs to the category, and Archidekt repeats it on each
        // line; a line that leaves it off does not clear it.
        let mut flags: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        let mut first: BTreeSet<&str> = BTreeSet::new();
        for (_, e, _) in &entries {
            if let Some(p) = e.primary() {
                first.insert(&p.name);
            }
            for c in &e.categories {
                let have = flags.entry(&c.name).or_default();
                for f in &c.flags {
                    if !have.contains(f) {
                        have.push(f.clone());
                    }
                }
            }
        }
        let categories: Vec<Category> = flags
            .iter()
            .map(|(&name, flags)| Category {
                name: name.to_string(),
                kind: first
                    .contains(name)
                    .then(|| archidekt_type(name, flags))
                    .flatten(),
            })
            .collect();
        let kind = |name: &str| {
            categories
                .iter()
                .find(|c| c.name == name)
                .and_then(|c| c.kind)
        };

        let mut cards = Vec::new();
        let mut names = HashMap::new();
        for (line, e, printing) in &entries {
            let place = e
                .primary()
                .and_then(|p| kind(&p.name))
                .unwrap_or(CategoryType::InDeck);
            let mut at = place;
            let mut kept = Vec::new();
            for (n, c) in e.categories.iter().enumerate() {
                if let (true, Some(t)) = (n > 0, kind(&c.name)) {
                    let deeper = if t.is_within(at) {
                        Some(t)
                    } else if at.is_within(t) {
                        Some(at)
                    } else {
                        None
                    };
                    // Keep it when the card stays where Archidekt put it, or,
                    // outside the deck, becomes more specific about where.
                    match deeper {
                        Some(d) if d == place || place.is_within(CategoryType::NotInDeck) => at = d,
                        _ => {
                            unreadable.push(Unreadable {
                                line: *line,
                                text: whole(*line),
                                reason: format!(
                                    "the card is kept without {:?}: Archidekt places a card by its \
                                     first category alone, which puts this one in {place}, and \
                                     {:?} is {t} where it comes first",
                                    c.name, c.name
                                ),
                            });
                            continue;
                        }
                    }
                }
                kept.push(c.name.clone());
            }
            let raw = RawCard {
                name: printing.is_none().then(|| e.name.clone()),
                printing: printing.as_ref().map(Printing::to_string),
                qty: Some(e.qty.get()),
                finish: match e.finish {
                    Finish::Nonfoil => None,
                    Finish::Foil => Some("foil".to_string()),
                    Finish::Etched => Some("etched".to_string()),
                },
                categories: kept,
            };
            match card(cards.len() + 1, raw, &categories) {
                Ok(c) => {
                    if let Some(p) = printing {
                        names.insert(p.clone(), e.name.clone());
                    }
                    cards.push(c);
                }
                Err(err) => unreadable.push(Unreadable {
                    line: *line,
                    text: whole(*line),
                    reason: err.to_string(),
                }),
            }
        }

        Imported {
            deck: Deck {
                name: None,
                format: None,
                categories,
                cards,
            },
            unreadable,
            names,
        }
    }

    /// [`Deck::read_archidekt`], refusing the text at its first line that
    /// could not be carried over whole. This is Gauntlet's reading: a number
    /// from a deck missing a line is a wrong number.
    pub fn from_archidekt(text: &str) -> Result<Deck, ImportError> {
        let imported = Deck::read_archidekt(text);
        match imported.unreadable.into_iter().next() {
            Some(u) => Err(ImportError::Unreadable(u)),
            None => Ok(imported.deck),
        }
    }
}

/// Archidekt's text export as `.deck.toml` text, refusing any line it could
/// not carry over whole. [`Deck::read_archidekt`] is the reading that keeps
/// going and lists them instead.
pub fn import_archidekt(text: &str) -> Result<String, ImportError> {
    let imported = Deck::read_archidekt(text);
    if let Some(u) = imported.unreadable.first() {
        return Err(ImportError::Unreadable(u.clone()));
    }
    Ok(imported.to_toml())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExportError {
    #[error(transparent)]
    Deck(#[from] DeckError),
    #[error(
        "no name for {}: Archidekt text names every card, and the deck file names a printing only by set and number",
        .0.iter().map(Printing::to_string).collect::<Vec<_>>().join(", ")
    )]
    Unnamed(Vec<Printing>),
    #[error("category {0:?} cannot be written as Archidekt text, which has no way to escape `,`, `[`, `]`, `{{` or `}}` in a category name")]
    Unwritable(String),
    #[error(
        "categories {first:?} and {second:?} would both be written as Archidekt's {word:?}, \
         which reads back as one category, so a card in one of them would move"
    )]
    SameWord {
        word: String,
        first: String,
        second: String,
    },
    #[error("card {index} ({card}): its only categories are named like Archidekt's boards ({category:?}), so Archidekt would take it out of the deck")]
    Misread {
        index: usize,
        card: CardRef,
        category: String,
    },
}

/// The word Archidekt reads as a category of this type
/// (`archidekt-import-shapes.md`, "What that means for Copy as Archidekt").
/// A typed category's own name gives way to it: Archidekt knows its boards
/// only by these exact words, and a commander only by `{top}`.
fn archidekt_word(c: &Category) -> String {
    use CategoryType::*;
    match c.kind {
        Some(Commander) => "Commander{top}".into(),
        Some(Sideboard) => "Sideboard".into(),
        Some(Maybeboard) => "Maybeboard".into(),
        Some(Companion) => "Companion{noDeck}".into(),
        Some(Attractions) => "Attractions{noDeck}".into(),
        Some(StickerSheet) => "Sticker Sheet{noDeck}".into(),
        Some(NotInDeck) => format!("{}{{noDeck}}", c.name),
        Some(InDeck) | None => c.name.clone(),
    }
}

/// Written without a flag, and so a label to Archidekt unless it is one of
/// the two names it treats as boards.
fn is_board_name(word: &str) -> bool {
    word == "Sideboard" || word == "Maybeboard"
}

impl Deck {
    /// The deck as Archidekt text, one `1x Card Name [Category,Category]` line
    /// per card in file order: names only, with no set, collector number or
    /// finish. The category that places the card comes first, because that is
    /// the only one Archidekt places it by.
    ///
    /// `name_of` names a card the file names by printing; the file itself holds
    /// only the set and number. Refuses, listing them, when a printing has no name.
    ///
    /// What does not survive a trip through Archidekt: printings and finishes;
    /// a typed category's own name, which becomes Archidekt's word for the type;
    /// the in-deck type, which Archidekt reads as a plain label; and a card with
    /// no categories, which Archidekt gives one of its own choosing.
    pub fn to_archidekt(
        &self,
        name_of: impl Fn(&Printing) -> Option<String>,
    ) -> Result<String, ExportError> {
        let unnamed: Vec<Printing> = self
            .cards
            .iter()
            .filter_map(|c| match &c.card {
                CardRef::Printing(p) if name_of(p).is_none() => Some(p.clone()),
                _ => None,
            })
            .collect();
        if !unnamed.is_empty() {
            return Err(ExportError::Unnamed(unnamed));
        }

        // Archidekt merges a category's flags across lines, so a word names
        // one category however it is flagged. Two of the same type merging
        // loses only a name; of different types, it moves a card.
        let mut by_word: BTreeMap<String, &Category> = BTreeMap::new();
        let used: BTreeSet<&str> = self
            .cards
            .iter()
            .flat_map(|c| c.categories.iter().map(String::as_str))
            .collect();
        let placing_kind = |c: &Category| c.kind.filter(|k| *k != CategoryType::InDeck);
        for cat in used.iter().filter_map(|n| self.category(n)) {
            let word = archidekt_word(cat);
            let name = word.split('{').next().unwrap_or(&word).to_string();
            match by_word.get(&name) {
                Some(other) if placing_kind(other) != placing_kind(cat) => {
                    return Err(ExportError::SameWord {
                        word: name,
                        first: other.name.clone(),
                        second: cat.name.clone(),
                    });
                }
                Some(_) => {}
                None => {
                    by_word.insert(name, cat);
                }
            }
        }

        let mut out = String::new();
        for (index, c) in self.cards.iter().enumerate() {
            let declared: Vec<&Category> = c
                .categories
                .iter()
                .filter_map(|n| self.category(n))
                .collect();
            let placing =
                |cat: &&Category| cat.kind == Some(c.place) && c.place != CategoryType::InDeck;
            let flagged = |cat: &&Category| !matches!(cat.kind, None | Some(CategoryType::InDeck));

            let mut words: Vec<String> = Vec::new();
            let plain: Vec<&Category> = declared.iter().copied().filter(|x| !flagged(x)).collect();
            let ordered = declared
                .iter()
                .copied()
                .filter(placing)
                .chain(
                    declared
                        .iter()
                        .copied()
                        .filter(|x| flagged(x) && !placing(x)),
                )
                .chain(plain.iter().copied().filter(|x| !is_board_name(&x.name)))
                .chain(plain.iter().copied().filter(|x| is_board_name(&x.name)));
            for cat in ordered {
                let own_name = matches!(
                    cat.kind,
                    None | Some(CategoryType::InDeck | CategoryType::NotInDeck)
                );
                if own_name && cat.name.contains([',', '[', ']', '{', '}']) {
                    return Err(ExportError::Unwritable(cat.name.clone()));
                }
                let word = archidekt_word(cat);
                if !words.contains(&word) {
                    words.push(word);
                }
            }
            if let Some(first) = words.first() {
                if is_board_name(first) && c.place.is_within(CategoryType::InDeck) {
                    return Err(ExportError::Misread {
                        index,
                        card: c.card.clone(),
                        category: first.clone(),
                    });
                }
            }

            let name = match &c.card {
                CardRef::Name(n) => n.clone(),
                CardRef::Printing(p) => name_of(p).expect("checked above"),
            };
            out += &format!("{}x {name}", c.qty);
            if !words.is_empty() {
                out += &format!(" [{}]", words.join(","));
            }
            out.push('\n');
        }
        Ok(out)
    }
}

/// `.deck.toml` text as Archidekt text. `names` maps a printing as the file
/// writes it (`"moc/94"`) to its card's name, which the file does not hold.
pub fn export_archidekt(
    text: &str,
    names: &HashMap<String, String>,
) -> Result<String, ExportError> {
    Deck::parse(text)?.to_archidekt(|p| names.get(&p.to_string()).cloned())
}
