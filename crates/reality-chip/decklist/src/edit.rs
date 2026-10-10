//! Edits to a `.deck.toml` that leave the rest of the file as it was written:
//! comments, spacing and order survive, so an edit is the diff it looks like.
//!
//! Every edit re-reads the result with [`Deck::parse`] and refuses what that
//! refuses, so no edit can leave behind a deck the format does not allow.
//!
//! A card's line is `  { ... },  # Name`. TOML hangs the comment after the
//! comma on whatever follows it, the next card or the array's end, so the
//! edits that add, drop or rename a line move that text with it: a line and
//! its comment come and go together.

use std::num::NonZeroU32;

use thiserror::Error;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Key, Table, Value};

use crate::collection::CollectionError;
use crate::deck::{quote_multiline, CardRef, CategoryType, Deck, DeckError, Finish};
use crate::identity::{holds, Names};
use crate::wanted::WantedError;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EditError {
    #[error("not a TOML file: {0}")]
    Toml(String),
    #[error("there is no card {0}")]
    NoCard(usize),
    #[error("category {name:?} is already declared as {existing}")]
    DeclaredDifferently { name: String, existing: String },
    #[error("there is no change {0} between those two decks")]
    NoChange(usize),
    #[error("category {0:?} cannot go while a card is still in it")]
    StillUsed(String),
    #[error(transparent)]
    Invalid(#[from] DeckError),
    #[error(transparent)]
    Collection(#[from] CollectionError),
    #[error(transparent)]
    Wanted(#[from] WantedError),
}

/// What an edited file must still be once edited: [`check_deck`] for a deck,
/// and the collection's own for a collection. The line edits here work on
/// any file whose `cards` are one inline table per line.
pub(crate) type Check = fn(&str) -> Result<(), EditError>;

fn check_deck(text: &str) -> Result<(), EditError> {
    Deck::parse(text)?;
    Ok(())
}

pub(crate) fn document(text: &str) -> Result<DocumentMut, EditError> {
    text.parse::<DocumentMut>()
        .map_err(|e| EditError::Toml(e.to_string()))
}

fn finish(doc: DocumentMut) -> Result<String, EditError> {
    finish_with(doc, check_deck)
}

pub(crate) fn finish_with(doc: DocumentMut, check: Check) -> Result<String, EditError> {
    let text = doc.to_string();
    check(&text)?;
    Ok(text)
}

fn cards_mut(doc: &mut DocumentMut, index: usize) -> Result<&mut Array, EditError> {
    doc.get_mut("cards")
        .and_then(Item::as_array_mut)
        .filter(|cards| index < cards.len())
        .ok_or(EditError::NoCard(index))
}

pub(crate) fn card_mut(doc: &mut DocumentMut, index: usize) -> Result<&mut InlineTable, EditError> {
    cards_mut(doc, index)?
        .get_mut(index)
        .and_then(Value::as_inline_table_mut)
        .ok_or(EditError::NoCard(index))
}

/// Where a key sits on a card's line, in the order [`Deck::to_toml`] writes.
fn rank(key: &str) -> usize {
    ["name", "printing", "qty", "finish", "in", "at"]
        .iter()
        .position(|k| *k == key)
        .unwrap_or(usize::MAX)
}

/// Sets `key` on a card. An existing key is replaced where it stands, keeping
/// its spacing; a new one goes where the writer would put it.
pub(crate) fn put(card: &mut InlineTable, key: &str, value: impl Into<Value>) {
    let value = value.into();
    match card.get_mut(key) {
        Some(existing) => {
            let decor = existing.decor().clone();
            *existing = value;
            *existing.decor_mut() = decor;
        }
        None => {
            card.insert(key, value);
            card.sort_values_by(|a: &Key, _, b: &Key, _| rank(a.get()).cmp(&rank(b.get())));
            card.fmt();
        }
    }
}

fn raw(s: Option<&toml_edit::RawString>) -> String {
    s.and_then(|s| s.as_str()).unwrap_or("").to_string()
}

/// The text after card `index`'s comma: the start of the next card's line, or
/// the end of the array. Its first line ends card `index`'s line.
pub(crate) fn follower(cards: &Array, index: usize) -> String {
    match cards.get(index + 1) {
        Some(next) => raw(next.decor().prefix()),
        None => raw(Some(cards.trailing())),
    }
}

fn set_follower(cards: &mut Array, index: usize, text: String) {
    match cards.get_mut(index + 1) {
        Some(next) => next.decor_mut().set_prefix(text),
        None => cards.set_trailing(text),
    }
}

/// The comment on a card's line, from the text that follows its comma.
pub(crate) fn comment(follower: &str) -> Option<String> {
    let (line, _) = follower.split_once('\n')?;
    let text = line.trim().strip_prefix('#')?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Each card's line comment, by card index: for a card named by printing,
/// the name the writer left beside it. Empty when the text has no `cards`.
pub fn card_comments(text: &str) -> Vec<Option<String>> {
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return Vec::new();
    };
    let Some(cards) = doc.get("cards").and_then(Item::as_array) else {
        return Vec::new();
    };
    (0..cards.len())
        .map(|i| comment(&follower(cards, i)))
        .collect()
}

/// Replaces card `index`'s categories (0-based, in file order). No categories
/// drops its `in` key.
pub fn set_categories(
    text: &str,
    index: usize,
    categories: &[String],
) -> Result<String, EditError> {
    set_categories_with(text, index, categories, check_deck)
}

pub(crate) fn set_categories_with(
    text: &str,
    index: usize,
    categories: &[String],
    check: Check,
) -> Result<String, EditError> {
    let mut doc = document(text)?;
    let card = card_mut(&mut doc, index)?;
    if categories.is_empty() {
        // The key before `in` kept no trailing space, so `fmt` puts it back.
        if card.remove("in").is_some() {
            card.fmt();
        }
    } else {
        let list: Array = categories.iter().map(String::as_str).collect();
        match card.get_mut("in") {
            // In place, so the key keeps its position and spacing.
            Some(existing) => {
                let decor = existing.decor().clone();
                *existing = Value::Array(list);
                *existing.decor_mut() = decor;
            }
            None => {
                card.insert("in", Value::Array(list));
            }
        }
        card.fmt();
    }
    finish_with(doc, check)
}

/// Declares `name` under `[categories]`, typed or not. Declaring it again the
/// same way changes nothing.
pub fn declare_category(
    text: &str,
    name: &str,
    kind: Option<CategoryType>,
) -> Result<String, EditError> {
    declare_category_with(text, name, kind, check_deck)
}

pub(crate) fn declare_category_with(
    text: &str,
    name: &str,
    kind: Option<CategoryType>,
    check: Check,
) -> Result<String, EditError> {
    let mut doc = document(text)?;
    if !doc.contains_key("categories") {
        doc.insert("categories", Item::Table(Table::new()));
    }
    let table = doc["categories"]
        .as_table_like_mut()
        .ok_or_else(|| EditError::Toml("categories is not a table".into()))?;
    if let Some(existing) = table.get(name) {
        let existing_kind = existing
            .as_table_like()
            .and_then(|t| t.get("type"))
            .and_then(Item::as_str)
            .map(str::to_string);
        if existing_kind.as_deref() == kind.map(CategoryType::as_str) {
            return Ok(text.to_string());
        }
        return Err(EditError::DeclaredDifferently {
            name: name.to_string(),
            existing: existing_kind.unwrap_or_else(|| "untyped".into()),
        });
    }
    let mut value = InlineTable::new();
    if let Some(kind) = kind {
        value.insert("type", kind.as_str().into());
        value.fmt();
    }
    table.insert(name, Item::Value(Value::InlineTable(value)));
    finish_with(doc, check)
}

/// Sets declared category `name`'s type, `None` making it a label.
pub(crate) fn retype_category(
    text: &str,
    name: &str,
    kind: Option<CategoryType>,
    check: Check,
) -> Result<String, EditError> {
    let mut doc = document(text)?;
    let mut value = InlineTable::new();
    if let Some(kind) = kind {
        value.insert("type", kind.as_str().into());
        value.fmt();
    }
    let existing = doc
        .get_mut("categories")
        .and_then(Item::as_table_like_mut)
        .and_then(|t| t.get_mut(name))
        .ok_or_else(|| EditError::Toml(format!("category {name:?} is not declared")))?;
    match existing.as_value_mut() {
        Some(v) => {
            let decor = v.decor().clone();
            *v = Value::InlineTable(value);
            *v.decor_mut() = decor;
        }
        None => *existing = Item::Value(Value::InlineTable(value)),
    }
    finish_with(doc, check)
}

/// Drops category `name` from `[categories]`, unchecked: whether a card is
/// still in it is for the caller to check once every edit is made.
pub(crate) fn undeclare_category(text: &str, name: &str) -> Result<String, EditError> {
    let mut doc = document(text)?;
    if let Some(table) = doc.get_mut("categories").and_then(Item::as_table_like_mut) {
        table.remove(name);
    }
    Ok(doc.to_string())
}

/// Drops card `index`'s line, its comment with it.
pub fn remove_card(text: &str, index: usize) -> Result<String, EditError> {
    remove_line(text, index, check_deck)
}

pub(crate) fn remove_line(text: &str, index: usize, check: Check) -> Result<String, EditError> {
    let mut doc = document(text)?;
    drop_line(&mut doc, index)?;
    finish_with(doc, check)
}

/// Drops card `index`'s line from `doc`, its comment with it, leaving the
/// check to whoever finishes the document.
pub(crate) fn drop_line(doc: &mut DocumentMut, index: usize) -> Result<(), EditError> {
    let cards = cards_mut(doc, index)?;
    let own = raw(cards.get(index).and_then(|c| c.decor().prefix()));
    let after = follower(cards, index);
    let last = index + 1 == cards.len();
    // The line runs from just after the last newline before the card to the
    // first newline after its comma. A one-line array has no lines to drop,
    // so the next card takes this one's spacing instead.
    let joined = match (own.rfind('\n'), after.find('\n')) {
        (Some(start), Some(end)) => Some(format!("{}{}", &own[..=start], &after[end + 1..])),
        _ if last => None,
        _ => Some(own),
    };
    cards.remove(index);
    if let Some(joined) = joined {
        match cards.get_mut(index) {
            Some(next) => next.decor_mut().set_prefix(joined),
            None => cards.set_trailing(joined),
        }
    }
    Ok(())
}

/// Sets card `index`'s quantity. Zero removes the card, and one drops the
/// `qty` key, as the format writes it.
pub fn set_card_qty(text: &str, index: usize, qty: u32) -> Result<String, EditError> {
    set_qty(text, index, qty, check_deck)
}

pub(crate) fn set_qty(
    text: &str,
    index: usize,
    qty: u32,
    check: Check,
) -> Result<String, EditError> {
    let Some(qty) = NonZeroU32::new(qty) else {
        return remove_line(text, index, check);
    };
    let mut doc = document(text)?;
    let card = card_mut(&mut doc, index)?;
    if qty == NonZeroU32::MIN {
        if card.remove("qty").is_none() {
            return Ok(text.to_string());
        }
        card.fmt();
    } else {
        put(card, "qty", i64::from(qty.get()));
    }
    finish_with(doc, check)
}

/// Sets card `index`'s finish. Nonfoil is the absent key.
pub fn set_card_finish(text: &str, index: usize, finish_: Finish) -> Result<String, EditError> {
    set_finish(text, index, finish_, check_deck)
}

pub(crate) fn set_finish(
    text: &str,
    index: usize,
    finish_: Finish,
    check: Check,
) -> Result<String, EditError> {
    let mut doc = document(text)?;
    let card = card_mut(&mut doc, index)?;
    match finish_ {
        Finish::Nonfoil => {
            if card.remove("finish").is_none() {
                return Ok(text.to_string());
            }
            card.fmt();
        }
        Finish::Foil => put(card, "finish", "foil"),
        Finish::Etched => put(card, "finish", "etched"),
    }
    finish_with(doc, check)
}

/// Names card `index` by the printing `set/num`, keeping its quantity,
/// finish and categories. A card that was named by name keeps that name as
/// the line's comment, the way an import writes a printing.
pub fn set_card_printing(
    text: &str,
    index: usize,
    set: &str,
    num: &str,
) -> Result<String, EditError> {
    set_printing(text, index, set, num, check_deck)
}

pub(crate) fn set_printing(
    text: &str,
    index: usize,
    set: &str,
    num: &str,
    check: Check,
) -> Result<String, EditError> {
    let printing = format!("{}/{}", set.trim().to_ascii_lowercase(), num.trim());
    let mut doc = document(text)?;
    let cards = cards_mut(&mut doc, index)?;
    let card = cards
        .get_mut(index)
        .and_then(Value::as_inline_table_mut)
        .ok_or(EditError::NoCard(index))?;
    let name = card
        .remove("name")
        .and_then(|v| v.as_str().map(str::to_string));
    put(card, "printing", printing);
    if let Some(name) = name {
        let after = follower(cards, index);
        if comment(&after).is_none() {
            if let Some((_, rest)) = after.split_once('\n') {
                set_follower(cards, index, format!("  # {name}\n{rest}"));
            }
        }
    }
    finish_with(doc, check)
}

/// Names card `index` by `name`, any printing of it, keeping its quantity,
/// finish and categories. The comment that named its printing goes with the
/// printing, since the line now says the name itself.
pub(crate) fn set_name(
    text: &str,
    index: usize,
    name: &str,
    check: Check,
) -> Result<String, EditError> {
    let mut doc = document(text)?;
    let cards = cards_mut(&mut doc, index)?;
    let card = cards
        .get_mut(index)
        .and_then(Value::as_inline_table_mut)
        .ok_or(EditError::NoCard(index))?;
    let had_printing = card.remove("printing").is_some();
    put(card, "name", name);
    if had_printing {
        let after = follower(cards, index);
        if comment(&after).is_some() {
            if let Some((_, rest)) = after.split_once('\n') {
                set_follower(cards, index, format!("\n{rest}"));
            }
        }
    }
    finish_with(doc, check)
}

/// Appends `card` as a new line, every field as the format writes it and
/// `comment` beside it, unchecked: its categories may not be declared yet.
pub(crate) fn push_card(
    text: &str,
    card: &crate::deck::Card,
    comment: Option<&str>,
) -> Result<String, EditError> {
    let mut line = InlineTable::new();
    match &card.card {
        CardRef::Name(name) => line.insert("name", name.as_str().into()),
        CardRef::Printing(p) => line.insert("printing", p.to_string().into()),
    };
    if card.qty != NonZeroU32::MIN {
        line.insert("qty", i64::from(card.qty.get()).into());
    }
    match card.finish {
        Finish::Nonfoil => {}
        Finish::Foil => {
            line.insert("finish", "foil".into());
        }
        Finish::Etched => {
            line.insert("finish", "etched".into());
        }
    }
    if !card.categories.is_empty() {
        let list: Array = card.categories.iter().map(String::as_str).collect();
        line.insert("in", Value::Array(list));
    }
    line.fmt();
    let mut doc = document(text)?;
    push_line(&mut doc, line, comment)?;
    Ok(doc.to_string())
}

/// The deck's own keys, in the order [`Deck::to_toml`] writes them above its
/// cards.
const META: [&str; 5] = ["name", "format", "variant_of", "cover", "description"];

fn meta_rank(key: &Key) -> usize {
    META.iter()
        .position(|k| *k == key.get())
        .unwrap_or(META.len())
}

/// Sets one of the deck's own keys in place, or adds it where [`META`] puts
/// it; `None` drops it.
pub(crate) fn set_meta(doc: &mut DocumentMut, key: &str, value: Option<Value>) {
    match value {
        None => {
            doc.remove(key);
        }
        Some(value) => match doc.get_mut(key).and_then(Item::as_value_mut) {
            Some(existing) => {
                let decor = existing.decor().clone();
                *existing = value;
                *existing.decor_mut() = decor;
            }
            None => {
                doc.insert(key, Item::Value(value));
                doc.sort_values_by(|a, _, b, _| meta_rank(a).cmp(&meta_rank(b)));
            }
        },
    }
}

/// Says the deck is a variant of the deck at `parent`, or, with `None`, that
/// it stands on its own.
pub fn set_variant_of(text: &str, parent: Option<&str>) -> Result<String, EditError> {
    let mut doc = document(text)?;
    set_meta(&mut doc, "variant_of", parent.map(Value::from));
    finish(doc)
}

/// Makes card `index` a commander: it joins the deck's commander-typed
/// category, first among its categories because Archidekt reads the first
/// one, and leaves any category that would put it somewhere else. A deck
/// with no commander category gets `Commander`, declared as one.
pub fn set_commander(text: &str, index: usize) -> Result<String, EditError> {
    let deck = Deck::parse(text)?;
    let card = deck.cards.get(index).ok_or(EditError::NoCard(index))?;
    if card.is_commander() {
        return Ok(text.to_string());
    }
    let (text, commander) = match deck
        .categories
        .iter()
        .find(|c| c.kind == Some(CategoryType::Commander))
    {
        Some(c) => (text.to_string(), c.name.clone()),
        None => (
            declare_category(text, "Commander", Some(CategoryType::Commander))?,
            "Commander".to_string(),
        ),
    };
    let fits = |name: &String| {
        deck.category(name)
            .and_then(|c| c.kind)
            .is_none_or(|k| CategoryType::Commander.is_within(k))
    };
    let categories: Vec<String> = std::iter::once(commander.clone())
        .chain(
            card.categories
                .iter()
                .filter(|n| **n != commander && fits(n))
                .cloned(),
        )
        .collect();
    set_categories(&text, index, &categories)
}

fn same_categories(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.contains(x))
}

/// A card's categories once dropped on `to`, dragged out of `from` (`None`
/// when it was in no category). A plain drop moves it: `to` takes `from`'s
/// place in the list. A secondary drop (Ctrl, as in Archidekt) keeps every
/// category and adds `to` behind them.
pub fn drop_onto(
    categories: &[String],
    from: Option<&str>,
    to: &str,
    secondary: bool,
) -> Vec<String> {
    if categories.iter().any(|c| c == to) {
        return if secondary || from.is_none() || from == Some(to) {
            categories.to_vec()
        } else {
            categories
                .iter()
                .filter(|c| Some(c.as_str()) != from)
                .cloned()
                .collect()
        };
    }
    match from {
        Some(from) if !secondary => categories
            .iter()
            .map(|c| if c == from { to.to_string() } else { c.clone() })
            .collect(),
        _ => categories
            .iter()
            .cloned()
            .chain(std::iter::once(to.to_string()))
            .collect(),
    }
}

/// A board a card can be put on at a keystroke or by the drag strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    Maybeboard,
    Sideboard,
}

impl Board {
    fn kind(self) -> CategoryType {
        match self {
            Board::Maybeboard => CategoryType::Maybeboard,
            Board::Sideboard => CategoryType::Sideboard,
        }
    }

    /// What a deck without one gets its board declared as.
    fn name(self) -> &'static str {
        match self {
            Board::Maybeboard => "Maybeboard",
            Board::Sideboard => "Sideboard",
        }
    }
}

/// Where a moved card goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dest {
    /// The category of that name, declared untyped first when the deck has
    /// none by it.
    Category(String),
    /// The deck's category of the board's type, declared as [`Board::name`]
    /// when it has none.
    Board(Board),
}

/// What one keystroke, menu pick or drop does to every card it is applied to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardEdit {
    Increase,
    /// One fewer; the last one removes the line.
    Decrease,
    Remove,
    /// Archidekt's *Automatic*: in no category.
    Automatic,
    Commander,
    /// To `to`, out of the category the card was reached from; with
    /// `secondary`, into `to` as well (see [`drop_onto`]).
    Move {
        to: Dest,
        secondary: bool,
    },
}

/// A card as the user reached it: its line, and the category of the stack it
/// was reached in, `None` for no category. A move replaces `from`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub index: usize,
    pub from: Option<String>,
}

/// An edit's next text, and where each line of the old text is in it:
/// `lines[i]` is old card `i`'s index now, `None` once its line is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edited {
    pub text: String,
    pub lines: Vec<Option<usize>>,
}

/// `edit` on every one of `targets`, as one edit. Either every card takes it
/// or, refused on any, none does and the error says why. A target named
/// twice is edited once.
pub fn edit_cards(text: &str, edit: &CardEdit, targets: &[Target]) -> Result<Edited, EditError> {
    let deck = Deck::parse(text)?;
    let mut text = text.to_string();
    let to = match edit {
        CardEdit::Move { to, .. } => Some(match to {
            Dest::Category(name) => {
                if deck.category(name).is_none() {
                    text = declare_category(&text, name, None)?;
                }
                name.clone()
            }
            Dest::Board(board) => {
                match deck
                    .categories
                    .iter()
                    .find(|c| c.kind == Some(board.kind()))
                {
                    Some(c) => c.name.clone(),
                    None => {
                        text = declare_category(&text, board.name(), Some(board.kind()))?;
                        board.name().to_string()
                    }
                }
            }
        }),
        _ => None,
    };

    let targets = targets_by_index(targets);
    let mut removed = vec![false; deck.cards.len()];
    // Last line first: dropping a line moves only the lines after it, and
    // those are done.
    for target in targets.iter().rev() {
        let card = deck
            .cards
            .get(target.index)
            .ok_or(EditError::NoCard(target.index))?;
        let i = target.index;
        text = match edit {
            CardEdit::Increase => set_card_qty(&text, i, card.qty.get() + 1)?,
            CardEdit::Decrease => {
                removed[i] = card.qty.get() == 1;
                set_card_qty(&text, i, card.qty.get() - 1)?
            }
            CardEdit::Remove => {
                removed[i] = true;
                remove_card(&text, i)?
            }
            CardEdit::Automatic => set_categories(&text, i, &[])?,
            CardEdit::Commander => set_commander(&text, i)?,
            CardEdit::Move { secondary, .. } => {
                let to = to.as_deref().expect("a move has somewhere to go");
                let categories =
                    drop_onto(&card.categories, target.from.as_deref(), to, *secondary);
                set_categories(&text, i, &categories)?
            }
        };
    }

    let mut gone = 0;
    let lines = removed
        .iter()
        .enumerate()
        .map(|(i, &r)| {
            if r {
                gone += 1;
                None
            } else {
                Some(i - gone)
            }
        })
        .collect();
    Ok(Edited { text, lines })
}

/// `targets` in line order, each line once: the first that names it.
fn targets_by_index(targets: &[Target]) -> Vec<&Target> {
    let mut sorted: Vec<&Target> = Vec::with_capacity(targets.len());
    for t in targets {
        if !sorted.iter().any(|s| s.index == t.index) {
            sorted.push(t);
        }
    }
    sorted.sort_by_key(|t| t.index);
    sorted
}

/// Where a card added to a deck goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddTo<'a> {
    /// Archidekt's *Automatic*: a copy more on a line already holding the
    /// card, one in the deck before one outside it, else a new line in no
    /// category.
    Automatic,
    /// A copy more on the line holding the card in exactly these categories,
    /// else a new line in them.
    Categories(&'a [String]),
}

/// What an add did: the next text, the line holding the card now, and
/// whether that line is new rather than one the file had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub text: String,
    pub line: usize,
    pub made: bool,
}

/// Every printing's name: `names` first, and for a printing they leave out,
/// the comment the file writes beside its line.
pub(crate) fn names_with_comments<'a>(
    text: &str,
    cards: impl Iterator<Item = &'a CardRef>,
    names: &Names,
) -> Names {
    let mut all = names.clone();
    for (card, comment) in cards.zip(card_comments(text)) {
        if let (CardRef::Printing(p), Some(comment)) = (card, comment) {
            all.entry(p.to_string()).or_insert(comment);
        }
    }
    all
}

/// Adds one nonfoil `card` where `to` says: a copy more on a line already
/// holding it, by [`holds`] with `names` naming the file's printings, or else
/// a new last line in the file's style with `comment` beside it (the card's
/// name, for a printing). A foil or etched line of the card is another card.
pub fn add_card(
    text: &str,
    card: &CardRef,
    to: AddTo<'_>,
    comment: Option<&str>,
    names: &Names,
) -> Result<Added, EditError> {
    let deck = Deck::parse(text)?;
    let names = names_with_comments(text, deck.cards.iter().map(|c| &c.card), names);
    let held = || {
        deck.cards
            .iter()
            .enumerate()
            .filter(|(_, c)| c.finish == Finish::Nonfoil && holds(&c.card, card, &names))
    };
    let line = match to {
        AddTo::Automatic => held().find(|(_, c)| c.in_deck()).or_else(|| held().next()),
        AddTo::Categories(categories) => {
            held().find(|(_, c)| same_categories(&c.categories, categories))
        }
    };
    if let Some((i, c)) = line {
        return Ok(Added {
            text: set_card_qty(text, i, c.qty.get() + 1)?,
            line: i,
            made: false,
        });
    }

    let mut line = InlineTable::new();
    match card {
        CardRef::Name(name) => line.insert("name", name.as_str().into()),
        CardRef::Printing(p) => line.insert("printing", p.to_string().into()),
    };
    if let AddTo::Categories(categories) = to {
        if !categories.is_empty() {
            let list: Array = categories.iter().map(String::as_str).collect();
            line.insert("in", Value::Array(list));
        }
    }
    line.fmt();

    let mut doc = document(text)?;
    push_line(&mut doc, line, comment)?;
    Ok(Added {
        text: finish(doc)?,
        line: deck.cards.len(),
        made: true,
    })
}

/// Appends `line` as the last of `cards`, in the file's style, with `comment`
/// beside it; a file with no `cards` gains them.
pub(crate) fn push_line(
    doc: &mut DocumentMut,
    line: InlineTable,
    comment: Option<&str>,
) -> Result<(), EditError> {
    let mut line = Value::InlineTable(line);
    if !doc.contains_key("cards") {
        doc.insert("cards", Item::Value(Value::Array(Array::new())));
    }
    let cards = doc["cards"]
        .as_array_mut()
        .ok_or_else(|| EditError::Toml("cards is not an inline array".into()))?;
    let trailing = raw(Some(cards.trailing()));
    let split = match trailing.split_once('\n') {
        Some((head, rest)) => Some((format!("{head}\n"), rest.to_string())),
        // `cards = []` becomes a list with a line per card.
        None if cards.is_empty() => Some(("\n".to_string(), trailing.clone())),
        None => None,
    };
    match split {
        Some((head, rest)) => {
            let indent = cards
                .get(cards.len().wrapping_sub(1))
                .map(|c| raw(c.decor().prefix()))
                .and_then(|p| p.rsplit_once('\n').map(|(_, i)| i.to_string()))
                .unwrap_or_else(|| "  ".to_string());
            line.decor_mut().set_prefix(format!("{head}{indent}"));
            line.decor_mut().set_suffix("");
            cards.push_formatted(line);
            let comment = comment.map(|c| format!("  # {c}")).unwrap_or_default();
            cards.set_trailing(format!("{comment}\n{rest}"));
            cards.set_trailing_comma(true);
        }
        None => {
            line.decor_mut()
                .set_prefix(if cards.is_empty() { "" } else { " " });
            line.decor_mut().set_suffix("");
            cards.push_formatted(line);
        }
    }
    Ok(())
}

/// Sets the deck's `name`, and its `format` unless `format` is empty, each in
/// place when the file has it. A file without either gains them at its top,
/// a blank line above what was first, as [`Deck::to_toml`] writes them.
pub fn set_deck_meta(text: &str, name: &str, format: &str) -> Result<String, EditError> {
    let mut doc = document(text)?;
    let had_meta = doc.contains_key("name") || doc.contains_key("format");
    let first = doc
        .iter()
        .find(|(_, item)| item.is_value())
        .or_else(|| doc.iter().next())
        .map(|(key, _)| key.to_string());

    let mut inserted = false;
    let mut set = |key: &str, value: &str| match doc.get_mut(key).and_then(Item::as_value_mut) {
        Some(existing) => {
            let decor = existing.decor().clone();
            *existing = value.into();
            *existing.decor_mut() = decor;
        }
        None => {
            doc.insert(key, toml_edit::value(value));
            inserted = true;
        }
    };
    set("name", name);
    let format = format.trim();
    if !format.is_empty() {
        set("format", format);
    }

    if inserted {
        doc.sort_values_by(|a, _, b, _| meta_rank(a).cmp(&meta_rank(b)));
        if let (false, Some(first)) = (had_meta, first) {
            let blank = |d: &toml_edit::Decor| format!("\n{}", raw(d.prefix()));
            if doc.get(&first).is_some_and(Item::is_value) {
                let mut key = doc.key_mut(&first).expect("the key was read");
                let prefix = blank(key.leaf_decor());
                key.leaf_decor_mut().set_prefix(prefix);
            } else if let Some(table) = doc.get_mut(&first).and_then(Item::as_table_mut) {
                let prefix = blank(table.decor());
                table.decor_mut().set_prefix(prefix);
            }
        }
    }
    finish(doc)
}

/// Sets the deck's `cover` to the printing `set/num` in place, or adds it
/// below `name` and `format`; `None` drops it.
pub fn set_deck_cover(text: &str, cover: Option<&str>) -> Result<String, EditError> {
    let mut doc = document(text)?;
    match cover {
        None => {
            doc.remove("cover");
        }
        Some(cover) => match doc.get_mut("cover").and_then(Item::as_value_mut) {
            Some(existing) => {
                let decor = existing.decor().clone();
                *existing = cover.into();
                *existing.decor_mut() = decor;
            }
            None => {
                doc.insert("cover", toml_edit::value(cover));
                doc.sort_values_by(|a, _, b, _| meta_rank(a).cmp(&meta_rank(b)));
            }
        },
    }
    finish(doc)
}

/// Sets the deck's Markdown `description`, written as a multi-line string
/// below `name`, `format` and `cover`. `None`, or only whitespace, drops it.
pub fn set_deck_description(text: &str, description: Option<&str>) -> Result<String, EditError> {
    let mut doc = document(text)?;
    match description.filter(|d| !d.trim().is_empty()) {
        None => {
            doc.remove("description");
        }
        Some(description) => {
            let value: Value = quote_multiline(description)
                .parse()
                .map_err(|e: toml_edit::TomlError| EditError::Toml(e.to_string()))?;
            match doc.get_mut("description").and_then(Item::as_value_mut) {
                Some(existing) => {
                    let decor = existing.decor().clone();
                    *existing = value;
                    *existing.decor_mut() = decor;
                }
                None => {
                    doc.insert("description", Item::Value(value));
                    doc.sort_values_by(|a, _, b, _| meta_rank(a).cmp(&meta_rank(b)));
                    // A block of prose wants a blank line before what follows it.
                    let next = doc
                        .iter()
                        .map(|(k, _)| k.to_string())
                        .skip_while(|k| k != "description")
                        .nth(1);
                    if let Some(next) = next {
                        let blank = |d: &toml_edit::Decor| {
                            let prefix = raw(d.prefix());
                            if prefix.starts_with('\n') {
                                prefix
                            } else {
                                format!("\n{prefix}")
                            }
                        };
                        if doc.get(&next).is_some_and(Item::is_value) {
                            let mut key = doc.key_mut(&next).expect("the key was read");
                            let prefix = blank(key.leaf_decor());
                            key.leaf_decor_mut().set_prefix(prefix);
                        } else if let Some(table) = doc.get_mut(&next).and_then(Item::as_table_mut)
                        {
                            let prefix = blank(table.decor());
                            table.decor_mut().set_prefix(prefix);
                        }
                    }
                }
            }
        }
    }
    finish(doc)
}

/// The text of a new deck with nothing in it. An empty `format` is left out.
pub fn new_deck(name: &str, format: &str) -> Result<String, EditError> {
    let deck = Deck {
        name: Some(name.to_string()),
        format: (!format.trim().is_empty()).then(|| format.trim().to_string()),
        variant_of: None,
        cover: None,
        description: None,
        categories: Vec::new(),
        cards: Vec::new(),
    };
    let text = deck.to_toml(|_| None);
    Deck::parse(&text)?;
    Ok(text)
}
