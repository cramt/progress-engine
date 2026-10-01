//! The cards someone owns and where each one physically is: the Magic repo's
//! `collection.toml` (ADR-0023).
//!
//! A card line is written as a deck writes one, named once by printing or by
//! name, with `qty` and `finish`, so the two files read alike and share their
//! line edits. What differs is where a card is. A deck's categories are
//! labels, and a card carries as many as it needs; a card in the collection
//! is in one **place** or none, because a physical copy is in one binder, box
//! or deck at a time. A place is declared under `[places]`, and one that
//! stands for a deck names the deck's file:
//!
//! ```toml
//! cards = [
//!   { printing = "moc/94", at = "Lantern" },  # Rashmi and Ragavan
//!   { name = "Sol Ring", qty = 3, at = "Bulk" },
//!   { name = "Island", qty = 40 },
//! ]
//!
//! [places]
//! Bulk = {}
//! "Trade binder" = {}
//! Lantern = { deck = "decks/lantern.deck.toml" }
//! ```
//!
//! A card with no `at` is unsorted: owned, and not put anywhere yet.

use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroU32;

use facet::Facet;
use thiserror::Error;
use toml_edit::{InlineTable, Item, Table, Value};

use crate::changelog::{finish_name, message, reference};
use crate::deck::{line, CardRef, DeckError, Finish, Printing};
use crate::edit::{self, card_comments, card_mut, document, finish_with, put, EditError};

/// Where cards can be. `deck` is the path in the Magic repo of the deck the
/// place is, for the cards sleeved in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    pub deck: Option<String>,
}

/// One line of the collection: copies of a card, alike in finish and place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owned {
    pub card: CardRef,
    pub qty: NonZeroU32,
    pub finish: Finish,
    /// The place it is in, `None` for unsorted.
    pub at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Collection {
    /// Sorted by name, as a TOML table is.
    pub places: Vec<Place>,
    /// In file order.
    pub cards: Vec<Owned>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CollectionError {
    #[error("not a collection file: {0}")]
    Toml(String),
    /// A line that is not a card, the way it would not be one in a deck.
    #[error(transparent)]
    Card(#[from] DeckError),
    #[error("card {index} ({card}): place {place:?} is not declared under [places]")]
    Undeclared {
        index: usize,
        card: CardRef,
        place: String,
    },
    #[error("place {place:?}: deck must be the deck's path, like \"decks/lantern.deck.toml\"")]
    EmptyDeck { place: String },
    #[error("places {a:?} and {b:?} are both the deck {deck}; a deck is one place")]
    SameDeck { deck: String, a: String, b: String },
    #[error("place {name:?} is already declared{}", existing.as_ref().map_or_else(|| " with no deck".to_string(), |d| format!(" as the deck {d}")))]
    DeclaredDifferently {
        name: String,
        existing: Option<String>,
    },
    #[error("place {name:?} still holds {qty} cards; move them out first")]
    NotEmpty { name: String, qty: u32 },
    #[error("there is no place {0:?}")]
    NoPlace(String),
    #[error("card {index} has {have}, so {qty} of it cannot move")]
    TooMany { index: usize, have: u32, qty: u32 },
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct RawCollection {
    #[facet(default)]
    cards: Vec<RawOwned>,
    #[facet(default)]
    places: BTreeMap<String, RawPlace>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct RawOwned {
    name: Option<String>,
    printing: Option<String>,
    qty: Option<u32>,
    finish: Option<String>,
    at: Option<String>,
}

#[derive(Facet)]
#[facet(deny_unknown_fields)]
struct RawPlace {
    deck: Option<String>,
}

impl Collection {
    /// Reads a `collection.toml`. The empty text is the empty collection, so a
    /// Magic repo without the file owns nothing yet.
    pub fn parse(text: &str) -> Result<Collection, CollectionError> {
        let raw: RawCollection =
            facet_toml::from_str(text).map_err(|e| CollectionError::Toml(e.to_string()))?;

        let places: Vec<Place> = raw
            .places
            .into_iter()
            .map(|(name, p)| match p.deck {
                Some(d) if d.trim().is_empty() => Err(CollectionError::EmptyDeck { place: name }),
                deck => Ok(Place { name, deck }),
            })
            .collect::<Result<_, _>>()?;
        let mut decks: HashMap<&str, &str> = HashMap::new();
        for p in &places {
            if let Some(deck) = &p.deck {
                if let Some(other) = decks.insert(deck, &p.name) {
                    return Err(CollectionError::SameDeck {
                        deck: deck.clone(),
                        a: other.to_string(),
                        b: p.name.clone(),
                    });
                }
            }
        }

        let cards = raw
            .cards
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let index = i + 1;
                let (card, qty, finish) =
                    line(index, c.name, c.printing, c.qty, c.finish.as_deref())?;
                if let Some(place) = &c.at {
                    if !places.iter().any(|p| &p.name == place) {
                        return Err(CollectionError::Undeclared {
                            index,
                            card,
                            place: place.clone(),
                        });
                    }
                }
                Ok(Owned {
                    card,
                    qty,
                    finish,
                    at: c.at,
                })
            })
            .collect::<Result<_, CollectionError>>()?;

        Ok(Collection { places, cards })
    }

    pub fn place(&self, name: &str) -> Option<&Place> {
        self.places.iter().find(|p| p.name == name)
    }

    /// Copies in `place`, `None` counting the unsorted ones.
    pub fn qty_at(&self, place: Option<&str>) -> u32 {
        self.cards
            .iter()
            .filter(|c| c.at.as_deref() == place)
            .map(|c| c.qty.get())
            .sum()
    }
}

fn check(text: &str) -> Result<(), EditError> {
    Collection::parse(text)?;
    Ok(())
}

fn parse(text: &str) -> Result<Collection, EditError> {
    Ok(Collection::parse(text)?)
}

fn no_card(c: &Collection, index: usize) -> Result<&Owned, EditError> {
    c.cards.get(index).ok_or(EditError::NoCard(index))
}

/// Card `index` at `qty` copies; zero removes its line.
pub fn set_qty(text: &str, index: usize, qty: u32) -> Result<String, EditError> {
    edit::set_qty(text, index, qty, check)
}

/// Drops card `index`'s line, its comment with it.
pub fn remove(text: &str, index: usize) -> Result<String, EditError> {
    edit::remove_line(text, index, check)
}

/// Card `index`'s finish. Nonfoil is the absent key.
pub fn set_finish(text: &str, index: usize, finish: Finish) -> Result<String, EditError> {
    edit::set_finish(text, index, finish, check)
}

/// Card `index` named by the printing `set/num`; a line that named the card
/// by name keeps that name as its comment.
pub fn set_printing(text: &str, index: usize, set: &str, num: &str) -> Result<String, EditError> {
    edit::set_printing(text, index, set, num, check)
}

fn new_line(card: &CardRef, qty: u32, finish: Finish, at: Option<&str>) -> InlineTable {
    let mut line = InlineTable::new();
    match card {
        CardRef::Name(name) => line.insert("name", name.as_str().into()),
        CardRef::Printing(p) => line.insert("printing", p.to_string().into()),
    };
    if qty != 1 {
        line.insert("qty", i64::from(qty).into());
    }
    match finish {
        Finish::Nonfoil => {}
        Finish::Foil => {
            line.insert("finish", "foil".into());
        }
        Finish::Etched => {
            line.insert("finish", "etched".into());
        }
    }
    if let Some(at) = at {
        line.insert("at", at.into());
    }
    line.fmt();
    line
}

/// The line other than `skip` holding `card` in `finish` at `at`, which more
/// copies of it join rather than starting a line of their own.
fn line_for(
    c: &Collection,
    card: &CardRef,
    finish: Finish,
    at: Option<&str>,
    skip: Option<usize>,
) -> Option<usize> {
    (0..c.cards.len()).find(|&i| {
        let o = &c.cards[i];
        Some(i) != skip && &o.card == card && o.finish == finish && o.at.as_deref() == at
    })
}

/// `qty` more of `card` in `finish`, at the place `at` or unsorted: more on
/// the line that already holds it so, or else a new last line with `comment`
/// (for a printing, its name) beside it.
pub fn add(
    text: &str,
    card: &CardRef,
    qty: u32,
    finish: Finish,
    at: Option<&str>,
    comment: Option<&str>,
) -> Result<String, EditError> {
    let c = parse(text)?;
    if qty == 0 {
        return Ok(text.to_string());
    }
    if let Some(at) = at {
        if c.place(at).is_none() {
            return Err(CollectionError::NoPlace(at.to_string()).into());
        }
    }
    if let Some(i) = line_for(&c, card, finish, at, None) {
        return set_qty(text, i, c.cards[i].qty.get() + qty);
    }
    let mut doc = document(text)?;
    edit::push_line(&mut doc, new_line(card, qty, finish, at), comment)?;
    finish_with(doc, check)
}

/// Moves `qty` of card `index` to the place `to`, or to unsorted. All of a
/// line moves in place, a one-line diff; part of one leaves the rest where it
/// was. Either way the copies join a line already holding the card alike
/// there, rather than making a second.
pub fn move_cards(
    text: &str,
    index: usize,
    qty: u32,
    to: Option<&str>,
) -> Result<String, EditError> {
    let c = parse(text)?;
    let from = no_card(&c, index)?;
    let have = from.qty.get();
    if qty == 0 || qty > have {
        return Err(CollectionError::TooMany { index, have, qty }.into());
    }
    if let Some(to) = to {
        if c.place(to).is_none() {
            return Err(CollectionError::NoPlace(to.to_string()).into());
        }
    }
    if from.at.as_deref() == to {
        return Ok(text.to_string());
    }

    if let Some(j) = line_for(&c, &from.card, from.finish, to, Some(index)) {
        // The joined line first: dropping the source line would move it.
        let text = set_qty(text, j, c.cards[j].qty.get() + qty)?;
        return set_qty(&text, index, have - qty);
    }
    if qty == have {
        let mut doc = document(text)?;
        let card = card_mut(&mut doc, index)?;
        match to {
            Some(to) => put(card, "at", to),
            None => {
                card.remove("at");
                card.fmt();
            }
        }
        return finish_with(doc, check);
    }
    let comment = card_comments(text).into_iter().nth(index).flatten();
    let text = set_qty(text, index, have - qty)?;
    let mut doc = document(&text)?;
    edit::push_line(
        &mut doc,
        new_line(&from.card, qty, from.finish, to),
        comment.as_deref(),
    )?;
    finish_with(doc, check)
}

/// Moves all of each line in `indices` to the place `to`, or to unsorted, as
/// one edit: what sorting a pile of unsorted cards takes. Each joins a line
/// already holding the card alike there, as [`move_cards`] does.
pub fn move_lines(text: &str, indices: &[usize], to: Option<&str>) -> Result<String, EditError> {
    let c = parse(text)?;
    let mut indices = indices.to_vec();
    indices.sort_unstable();
    indices.dedup();
    // Last first: a move that joins another line drops its own, which shifts
    // only the lines after it, and those have already moved.
    let mut text = text.to_string();
    for &index in indices.iter().rev() {
        text = move_cards(&text, index, no_card(&c, index)?.qty.get(), to)?;
    }
    Ok(text)
}

/// `qty` of card `index`'s copies made the printing `printing` (or left the
/// card they are, for `None`) in `finish`, where they are: a scanned guess
/// corrected, or the foil found in a stack. All of a line changes in place;
/// part of one leaves the rest as it was. Either way the copies join a line
/// already holding them alike in that place, rather than making a second.
pub fn reprint(
    text: &str,
    index: usize,
    qty: u32,
    printing: Option<&Printing>,
    finish: Finish,
) -> Result<String, EditError> {
    let c = parse(text)?;
    let from = no_card(&c, index)?;
    let have = from.qty.get();
    if qty == 0 || qty > have {
        return Err(CollectionError::TooMany { index, have, qty }.into());
    }
    let printing = printing.map(|p| Printing {
        set: p.set.trim().to_ascii_lowercase(),
        num: p.num.trim().to_string(),
    });
    let card = printing
        .clone()
        .map_or_else(|| from.card.clone(), CardRef::Printing);
    if card == from.card && finish == from.finish {
        return Ok(text.to_string());
    }
    let at = from.at.as_deref();

    if let Some(j) = line_for(&c, &card, finish, at, Some(index)) {
        let text = set_qty(text, j, c.cards[j].qty.get() + qty)?;
        return set_qty(&text, index, have - qty);
    }
    if qty == have {
        let text = match &printing {
            Some(p) if card != from.card => set_printing(text, index, &p.set, &p.num)?,
            _ => text.to_string(),
        };
        return set_finish(&text, index, finish);
    }
    // A printing's line is commented with the card's name, as an import
    // writes one: the old line's name, or its comment when it was a printing.
    let comment = match (&card, &from.card) {
        (CardRef::Name(_), _) => None,
        (CardRef::Printing(_), CardRef::Name(name)) => Some(name.clone()),
        (CardRef::Printing(_), CardRef::Printing(_)) => {
            card_comments(text).into_iter().nth(index).flatten()
        }
    };
    let text = set_qty(text, index, have - qty)?;
    let mut doc = document(&text)?;
    edit::push_line(
        &mut doc,
        new_line(&card, qty, finish, at),
        comment.as_deref(),
    )?;
    finish_with(doc, check)
}

/// Declares the place `name` under `[places]`, standing for the deck at
/// `deck` when one is given. Declaring it again the same way changes nothing.
pub fn declare_place(text: &str, name: &str, deck: Option<&str>) -> Result<String, EditError> {
    let c = parse(text)?;
    if let Some(existing) = c.place(name) {
        if existing.deck.as_deref() == deck {
            return Ok(text.to_string());
        }
        return Err(CollectionError::DeclaredDifferently {
            name: name.to_string(),
            existing: existing.deck.clone(),
        }
        .into());
    }
    let mut doc = document(text)?;
    if !doc.contains_key("places") {
        doc.insert("places", Item::Table(Table::new()));
    }
    let table = doc["places"]
        .as_table_like_mut()
        .ok_or_else(|| EditError::Toml("places is not a table".into()))?;
    let mut value = InlineTable::new();
    if let Some(deck) = deck {
        value.insert("deck", deck.into());
        value.fmt();
    }
    table.insert(name, Item::Value(Value::InlineTable(value)));
    finish_with(doc, check)
}

/// Drops the place `name`, which must hold no cards.
pub fn undeclare_place(text: &str, name: &str) -> Result<String, EditError> {
    let c = parse(text)?;
    if c.place(name).is_none() {
        return Err(CollectionError::NoPlace(name.to_string()).into());
    }
    let qty = c.qty_at(Some(name));
    if qty > 0 {
        return Err(CollectionError::NotEmpty {
            name: name.to_string(),
            qty,
        }
        .into());
    }
    let mut doc = document(text)?;
    if let Some(table) = doc.get_mut("places").and_then(Item::as_table_like_mut) {
        table.remove(name);
        if table.is_empty() {
            doc.remove("places");
        }
    }
    finish_with(doc, check)
}

/// What the changelog tells apart about a copy.
#[derive(Clone, PartialEq, Eq)]
struct Key {
    card: CardRef,
    finish: Finish,
    at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Add,
    Remove,
    Move,
    Printing,
    Finish,
    Declare,
    Undeclare,
    Redeck,
}

fn place_name(at: &Option<String>) -> String {
    at.as_deref()
        .map_or_else(|| "unsorted".to_string(), str::to_lowercase)
}

fn copies(c: &Collection) -> Vec<(Key, u32)> {
    let mut out: Vec<(Key, u32)> = Vec::new();
    for o in &c.cards {
        let key = Key {
            card: o.card.clone(),
            finish: o.finish,
            at: o.at.clone(),
        };
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += o.qty.get(),
            None => out.push((key, o.qty.get())),
        }
    }
    out
}

/// The commit message for saving the collection `before` as `after` at
/// `path`, a line of the collection's changelog: `collection: +1 Sol Ring to
/// bulk, 2 Lightning Bolt: bulk → trade binder`. It is about copies, not
/// lines: a card that went from one place to another moved, whatever lines
/// that took, and one bought is an add. `name_of` labels a printing.
pub fn commit_message(
    before: &Collection,
    after: &Collection,
    path: &str,
    name_of: impl Fn(&Printing) -> Option<String>,
) -> String {
    let label = |r: &CardRef| match r {
        CardRef::Name(n) => n.clone(),
        CardRef::Printing(p) => name_of(p).unwrap_or_else(|| p.to_string()),
    };
    let old = copies(before);
    let new = copies(after);
    let count =
        |list: &[(Key, u32)], key: &Key| list.iter().find(|(k, _)| k == key).map_or(0, |(_, n)| *n);
    // Copies gone from where they were, and copies come to where they are.
    let mut gone: Vec<(Key, u32)> = old
        .iter()
        .filter_map(|(k, n)| {
            n.checked_sub(count(&new, k))
                .filter(|d| *d > 0)
                .map(|d| (k.clone(), d))
        })
        .collect();
    let mut came: Vec<(Key, u32)> = new
        .iter()
        .filter_map(|(k, n)| {
            n.checked_sub(count(&old, k))
                .filter(|d| *d > 0)
                .map(|d| (k.clone(), d))
        })
        .collect();

    let mut changes: Vec<(Kind, String, String)> = Vec::new();
    // Most alike first: the same card somewhere else, then the same card in
    // another finish, then another printing of it.
    type Alike = fn(&Key, &Key) -> Option<Kind>;
    let passes: [Alike; 3] = [
        |a, b| (a.card == b.card && a.finish == b.finish).then_some(Kind::Move),
        |a, b| (a.card == b.card && a.at == b.at).then_some(Kind::Finish),
        |a, b| (a.finish == b.finish && a.at == b.at).then_some(Kind::Printing),
    ];
    for alike in passes {
        for (a, left) in gone.iter_mut() {
            for (b, right) in came.iter_mut() {
                if *left == 0 || *right == 0 {
                    continue;
                }
                let Some(kind) = alike(a, b) else { continue };
                if kind == Kind::Printing && label(&a.card) != label(&b.card) {
                    continue;
                }
                let n = (*left).min(*right);
                *left -= n;
                *right -= n;
                let name = label(&b.card);
                let text = match kind {
                    Kind::Move => {
                        format!("{n} {name}: {} → {}", place_name(&a.at), place_name(&b.at))
                    }
                    Kind::Finish => format!(
                        "{n} {name}: {} → {}",
                        finish_name(a.finish),
                        finish_name(b.finish)
                    ),
                    _ => format!(
                        "{n} {name}: {} → {}",
                        reference(&a.card),
                        reference(&b.card)
                    ),
                };
                changes.push((kind, name, text));
            }
        }
    }
    for (k, n) in gone.iter().filter(|(_, n)| *n > 0) {
        let name = label(&k.card);
        let text = match &k.at {
            Some(at) => format!("-{n} {name} from {}", at.to_lowercase()),
            None => format!("-{n} {name}"),
        };
        changes.push((Kind::Remove, name, text));
    }
    for (k, n) in came.iter().filter(|(_, n)| *n > 0) {
        let name = label(&k.card);
        let text = match &k.at {
            Some(at) => format!("+{n} {name} to {}", at.to_lowercase()),
            None => format!("+{n} {name}"),
        };
        changes.push((Kind::Add, name, text));
    }

    for p in &after.places {
        let name = p.name.to_lowercase();
        match before.place(&p.name) {
            None => changes.push((
                Kind::Declare,
                name.clone(),
                match &p.deck {
                    None => format!("+place {name}"),
                    Some(d) => format!("+place {name} ({d})"),
                },
            )),
            Some(old) if old.deck != p.deck => {
                let d = |d: &Option<String>| d.clone().unwrap_or_else(|| "no deck".into());
                changes.push((
                    Kind::Redeck,
                    name.clone(),
                    format!("place {name}: {} → {}", d(&old.deck), d(&p.deck)),
                ));
            }
            Some(_) => {}
        }
    }
    for p in &before.places {
        if after.place(&p.name).is_none() {
            let name = p.name.to_lowercase();
            changes.push((Kind::Undeclare, name.clone(), format!("-place {name}")));
        }
    }

    changes.sort_by(|x, y| {
        (&x.0, x.1.to_lowercase(), &x.1, &x.2).cmp(&(&y.0, y.1.to_lowercase(), &y.1, &y.2))
    });
    let lines: Vec<&str> = changes.iter().map(|c| c.2.as_str()).collect();
    message(path, &lines)
}

/// [`commit_message`] between two collection texts, labelling each printing
/// with the name commented beside it in either, the newer text's first. The
/// empty text is the empty collection, for the file's first save.
pub fn commit_message_for_text(
    before: &str,
    after: &str,
    path: &str,
) -> Result<String, CollectionError> {
    let old = Collection::parse(before)?;
    let new = Collection::parse(after)?;
    let mut names: HashMap<Printing, String> = HashMap::new();
    for (text, c) in [(before, &old), (after, &new)] {
        for (o, comment) in c.cards.iter().zip(card_comments(text)) {
            if let (CardRef::Printing(p), Some(name)) = (&o.card, comment) {
                names.insert(p.clone(), name);
            }
        }
    }
    Ok(commit_message(&old, &new, path, |p| names.get(p).cloned()))
}
