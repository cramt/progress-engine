//! The cards someone wants and does not have: the Magic repo's `wanted.toml`
//! (ADR-0034), and the cards their decks hold that the collection lacks.
//!
//! A want is a card line as the collection writes one, without a place, since
//! it is not anywhere yet. The file also says how the decks count, which is
//! the only thing the derived list needs from the user:
//!
//! ```toml
//! deck_copies = "shared"
//! cards = [
//!   { name = "The One Ring" },
//!   { printing = "ltr/451", finish = "foil" },  # The One Ring
//! ]
//! ```
//!
//! `deck_copies` absent is `"each"`: every deck holds copies of its own.

use std::collections::HashMap;
use std::num::NonZeroU32;

use facet::Facet;
use thiserror::Error;
use toml_edit::{InlineTable, Value};

use crate::changelog::{finish_name, message};
use crate::collection::Collection;
use crate::deck::{line, CardRef, CategoryType, Deck, DeckError, Finish, Printing};
use crate::edit::{self, card_comments, document, finish_with, Added, EditError};
use crate::identity::{holds, name_key, name_of, Names};

/// How many copies of a card the decks need between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeckCopies {
    /// Every deck its own, as when each one stays sleeved: a card in three
    /// decks needs three.
    #[default]
    Each,
    /// One set moved between decks: a card in three decks needs as many as
    /// the deck wanting most of it.
    Shared,
}

impl DeckCopies {
    pub fn as_str(self) -> &'static str {
        match self {
            DeckCopies::Each => "each",
            DeckCopies::Shared => "shared",
        }
    }
}

/// One line of the wanted list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    pub card: CardRef,
    pub qty: NonZeroU32,
    pub finish: Finish,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Wanted {
    pub deck_copies: DeckCopies,
    /// In file order.
    pub cards: Vec<Want>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WantedError {
    #[error("not a wanted file: {0}")]
    Toml(String),
    #[error(transparent)]
    Card(#[from] DeckError),
    #[error("deck_copies {0:?} is not \"each\" or \"shared\"")]
    DeckCopies(String),
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct RawWanted {
    deck_copies: Option<String>,
    #[facet(default)]
    cards: Vec<RawWant>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct RawWant {
    name: Option<String>,
    printing: Option<String>,
    qty: Option<u32>,
    finish: Option<String>,
}

impl Wanted {
    /// Reads a `wanted.toml`. The empty text wants nothing, so a Magic repo
    /// without the file has an empty list.
    pub fn parse(text: &str) -> Result<Wanted, WantedError> {
        let raw: RawWanted =
            facet_toml::from_str(text).map_err(|e| WantedError::Toml(e.to_string()))?;
        let deck_copies = match raw.deck_copies.as_deref() {
            None | Some("each") => DeckCopies::Each,
            Some("shared") => DeckCopies::Shared,
            Some(other) => return Err(WantedError::DeckCopies(other.to_string())),
        };
        let cards = raw
            .cards
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let (card, qty, finish) =
                    line(i + 1, c.name, c.printing, c.qty, c.finish.as_deref())?;
                Ok(Want { card, qty, finish })
            })
            .collect::<Result<_, WantedError>>()?;
        Ok(Wanted { deck_copies, cards })
    }
}

fn check(text: &str) -> Result<(), EditError> {
    Wanted::parse(text)?;
    Ok(())
}

/// Want `index` at `qty` copies; zero drops its line.
pub fn set_qty(text: &str, index: usize, qty: u32) -> Result<String, EditError> {
    edit::set_qty(text, index, qty, check)
}

/// Drops want `index`'s line, its comment with it.
pub fn remove(text: &str, index: usize) -> Result<String, EditError> {
    edit::remove_line(text, index, check)
}

/// Want `index`'s finish. Nonfoil is the absent key.
pub fn set_finish(text: &str, index: usize, finish: Finish) -> Result<String, EditError> {
    edit::set_finish(text, index, finish, check)
}

/// `qty` more of `card` in `finish`: more on the line already wanting it so,
/// found as the collection finds one, or else a new last line with `comment`
/// (for a printing, its name) beside it.
pub fn add(
    text: &str,
    card: &CardRef,
    qty: NonZeroU32,
    finish: Finish,
    comment: Option<&str>,
    names: &Names,
) -> Result<Added, EditError> {
    let w = Wanted::parse(text)?;
    let names = edit::names_with_comments(text, w.cards.iter().map(|c| &c.card), names);
    if let Some(i) = w
        .cards
        .iter()
        .position(|c| c.finish == finish && holds(&c.card, card, &names))
    {
        return Ok(Added {
            text: set_qty(text, i, w.cards[i].qty.get() + qty.get())?,
            line: i,
            made: false,
        });
    }
    let mut line = InlineTable::new();
    match card {
        CardRef::Name(name) => line.insert("name", name.as_str().into()),
        CardRef::Printing(p) => line.insert("printing", p.to_string().into()),
    };
    if qty.get() != 1 {
        line.insert("qty", i64::from(qty.get()).into());
    }
    if finish != Finish::Nonfoil {
        line.insert("finish", finish_name(finish).into());
    }
    line.fmt();
    let mut doc = document(text)?;
    edit::push_line(&mut doc, line, comment)?;
    Ok(Added {
        text: finish_with(doc, check)?,
        line: w.cards.len(),
        made: true,
    })
}

/// How the decks count, written above the cards; `each` is the absent key.
pub fn set_deck_copies(text: &str, copies: DeckCopies) -> Result<String, EditError> {
    let mut doc = document(text)?;
    match copies {
        DeckCopies::Each => {
            doc.remove("deck_copies");
        }
        DeckCopies::Shared => match doc.get_mut("deck_copies").and_then(|i| i.as_value_mut()) {
            Some(existing) => {
                let decor = existing.decor().clone();
                *existing = Value::from(copies.as_str());
                *existing.decor_mut() = decor;
            }
            None => {
                doc.insert("deck_copies", Value::from(copies.as_str()).into());
                doc.sort_values_by(|a, _, b, _| {
                    (a.get() != "deck_copies").cmp(&(b.get() != "deck_copies"))
                });
            }
        },
    }
    finish_with(doc, check)
}

/// The commit message for saving the wanted list `before` as `after` at
/// `path`: `wanted: +1 The One Ring, -2 Sol Ring, deck copies: each →
/// shared`. A want's finish is named when it is not nonfoil.
pub fn commit_message_for_text(
    before: &str,
    after: &str,
    path: &str,
) -> Result<String, WantedError> {
    let old = Wanted::parse(before)?;
    let new = Wanted::parse(after)?;
    let mut names: HashMap<Printing, String> = HashMap::new();
    for (text, w) in [(before, &old), (after, &new)] {
        for (c, comment) in w.cards.iter().zip(card_comments(text)) {
            if let (CardRef::Printing(p), Some(name)) = (&c.card, comment) {
                names.insert(p.clone(), name);
            }
        }
    }
    let label = |c: &CardRef, f: Finish| {
        let name = match c {
            CardRef::Name(n) => n.clone(),
            CardRef::Printing(p) => names.get(p).cloned().unwrap_or_else(|| p.to_string()),
        };
        match f {
            Finish::Nonfoil => name,
            f => format!("{name} ({})", finish_name(f)),
        }
    };
    let totals = |w: &Wanted| {
        let mut out: Vec<((CardRef, Finish), u32)> = Vec::new();
        for c in &w.cards {
            let key = (c.card.clone(), c.finish);
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some((_, n)) => *n += c.qty.get(),
                None => out.push((key, c.qty.get())),
            }
        }
        out
    };
    let (was, now) = (totals(&old), totals(&new));
    let count = |list: &[((CardRef, Finish), u32)], key: &(CardRef, Finish)| {
        list.iter().find(|(k, _)| k == key).map_or(0, |(_, n)| *n)
    };
    let mut changes: Vec<(u8, String, String)> = Vec::new();
    for (key, n) in &was {
        let d = n.saturating_sub(count(&now, key));
        if d > 0 {
            let name = label(&key.0, key.1);
            changes.push((1, name.clone(), format!("-{d} {name}")));
        }
    }
    for (key, n) in &now {
        let d = n.saturating_sub(count(&was, key));
        if d > 0 {
            let name = label(&key.0, key.1);
            changes.push((0, name.clone(), format!("+{d} {name}")));
        }
    }
    if old.deck_copies != new.deck_copies {
        changes.push((
            2,
            String::new(),
            format!(
                "deck copies: {} → {}",
                old.deck_copies.as_str(),
                new.deck_copies.as_str()
            ),
        ));
    }
    changes.sort_by_key(|c| (c.0, c.1.to_lowercase()));
    let lines: Vec<&str> = changes.iter().map(|c| c.2.as_str()).collect();
    Ok(message(path, &lines))
}

/// A card the decks hold more copies of than the collection does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Missing {
    /// The card's name, or `set/num` for a printing nobody named.
    pub name: String,
    /// What the decks need less what is owned: more than none.
    pub missing: u32,
    pub owned: u32,
    /// Each deck that holds it, by path, and how many.
    pub decks: Vec<(String, u32)>,
}

/// The basic lands, which a deck holds as many of as it likes and which few
/// people log in a collection, so missing them is not news.
const BASICS: [&str; 6] = ["plains", "island", "swamp", "mountain", "forest", "wastes"];

fn is_basic(key: &str) -> bool {
    let key = key.strip_prefix("snow-covered ").unwrap_or(key);
    BASICS.contains(&key)
}

/// A card's key across files: its front face in lower case, or `set/num`
/// for a printing nobody has named.
fn key_of(card: &CardRef, names: &Names) -> (String, String) {
    match name_of(card, names) {
        Some(name) => (name_key(name), name.to_string()),
        None => (card.to_string(), card.to_string()),
    }
}

/// Copies held in the deck as a physical card: everything but the maybeboard
/// and what is set aside, which nobody needs to own to play it.
fn needs_copy(place: CategoryType) -> bool {
    !matches!(place, CategoryType::Maybeboard | CategoryType::NotInDeck)
}

/// The deck a variant is built from, following `variant_of` up through the
/// decks given, so a deck and its variants are one family. A parent that is
/// not among them, or a loop, ends the walk where it stands.
fn family<'a>(decks: &'a [(String, Deck, String)], path: &'a str) -> &'a str {
    let mut at = path;
    let mut seen = vec![path];
    while let Some(parent) = decks
        .iter()
        .find(|(p, _, _)| p == at)
        .and_then(|(_, d, _)| d.variant_of.as_deref())
        .filter(|parent| decks.iter().any(|(p, _, _)| p == parent))
    {
        if seen.contains(&parent) {
            break;
        }
        seen.push(parent);
        at = parent;
    }
    at
}

/// Every card `decks` (each its path, the deck and its text) hold more copies of than `collection` does,
/// by name: an owned copy of any printing or finish, anywhere, counts. A deck
/// and its variants are one build and need as many as the one wanting most;
/// between builds, `copies` says whether each needs its own. Basic lands are
/// left out. `names` names printings neither file comments.
pub fn missing(
    collection: &Collection,
    collection_text: &str,
    decks: &[(String, Deck, String)],
    names: &Names,
    copies: DeckCopies,
) -> Vec<Missing> {
    let mut all = edit::names_with_comments(
        collection_text,
        collection.cards.iter().map(|o| &o.card),
        names,
    );
    for (_, deck, text) in decks {
        all = edit::names_with_comments(text, deck.cards.iter().map(|c| &c.card), &all);
    }
    struct Need {
        name: String,
        /// Per deck, in the order the decks came.
        decks: Vec<(String, u32)>,
    }
    let mut needs: Vec<(String, Need)> = Vec::new();
    for (path, deck, _) in decks {
        for card in deck.cards.iter().filter(|c| needs_copy(c.place)) {
            let (key, name) = key_of(&card.card, &all);
            if is_basic(&key) {
                continue;
            }
            let at = match needs.iter().position(|(k, _)| *k == key) {
                Some(at) => at,
                None => {
                    needs.push((
                        key,
                        Need {
                            name,
                            decks: Vec::new(),
                        },
                    ));
                    needs.len() - 1
                }
            };
            let decks = &mut needs[at].1.decks;
            match decks.iter_mut().find(|(p, _)| p == path) {
                Some((_, n)) => *n += card.qty.get(),
                None => decks.push((path.clone(), card.qty.get())),
            }
        }
    }

    let mut owned: HashMap<String, u32> = HashMap::new();
    for o in &collection.cards {
        *owned.entry(key_of(&o.card, &all).0).or_default() += o.qty.get();
    }

    let mut out: Vec<Missing> = needs
        .into_iter()
        .filter_map(|(key, need)| {
            let mut builds: Vec<(&str, u32)> = Vec::new();
            for (path, n) in &need.decks {
                let root = family(decks, path);
                match builds.iter_mut().find(|(r, _)| *r == root) {
                    Some((_, most)) => *most = (*most).max(*n),
                    None => builds.push((root, *n)),
                }
            }
            let wanted = match copies {
                DeckCopies::Each => builds.iter().map(|(_, n)| n).sum(),
                DeckCopies::Shared => builds.iter().map(|(_, n)| *n).max().unwrap_or(0),
            };
            let have = owned.get(&key).copied().unwrap_or(0);
            (wanted > have).then(|| Missing {
                name: need.name,
                missing: wanted - have,
                owned: have,
                decks: need.decks,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// Copies of `card` the collection holds, by name as [`missing`] counts
/// them: any printing, finish or place.
pub fn owned(collection: &Collection, collection_text: &str, card: &CardRef, names: &Names) -> u32 {
    let all = edit::names_with_comments(
        collection_text,
        collection.cards.iter().map(|o| &o.card),
        names,
    );
    let key = key_of(card, &all).0;
    collection
        .cards
        .iter()
        .filter(|o| key_of(&o.card, &all).0 == key)
        .map(|o| o.qty.get())
        .sum()
}
