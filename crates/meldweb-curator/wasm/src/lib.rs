//! The browser's way into `chip-decklist`.
//!
//! The editor must agree with Ichormoon Gauntlet on what a deck is, so it runs
//! the same parser and the same edits rather than TypeScript copies of them.
//! The deck is its `.deck.toml` text (ADR-0020): JavaScript holds the text, and
//! every edit here takes a text and returns the next one.
//!
//! What crosses into JavaScript is a set of wire types of its own.
//! `web/src/deck.gen.ts` is generated from them; the test at the bottom fails
//! when it is stale and rewrites it under `UPDATE_TS=1`.

use std::collections::HashMap;

use chip_decklist::collection::{self, Collection};
use chip_decklist::deck::{self, CategoryType, Deck};
use chip_decklist::diff::{self, Change, Diff};
use chip_decklist::{changelog, edit};
use facet::Facet;
use wasm_bindgen::prelude::{wasm_bindgen, JsError};

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum CardRef {
    Printing { set: String, num: String },
    Name { name: String },
}

/// A card to add: a [`CardRef`], and for a printing optionally the card's
/// name, written beside the new line as its comment the way an import does.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum NewCard {
    Printing {
        set: String,
        num: String,
        #[facet(default, skip_serializing_if = Option::is_none)]
        name: Option<String>,
    },
    Name {
        name: String,
    },
}

/// A printing, `set/num` in the file.
#[derive(Debug, Facet)]
pub struct Printing {
    pub set: String,
    pub num: String,
}

/// The category type tree (ADR-0020), as the strings the file uses.
#[derive(Debug, Clone, Copy, Facet)]
#[repr(u8)]
#[facet(rename_all = "kebab-case")]
pub enum Kind {
    InDeck,
    Commander,
    NotInDeck,
    Sideboard,
    Companion,
    Maybeboard,
    Attractions,
    StickerSheet,
}

impl From<CategoryType> for Kind {
    fn from(t: CategoryType) -> Self {
        match t {
            CategoryType::InDeck => Kind::InDeck,
            CategoryType::Commander => Kind::Commander,
            CategoryType::NotInDeck => Kind::NotInDeck,
            CategoryType::Sideboard => Kind::Sideboard,
            CategoryType::Companion => Kind::Companion,
            CategoryType::Maybeboard => Kind::Maybeboard,
            CategoryType::Attractions => Kind::Attractions,
            CategoryType::StickerSheet => Kind::StickerSheet,
        }
    }
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(rename_all = "kebab-case")]
pub enum Finish {
    Nonfoil,
    Foil,
    Etched,
}

#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct Card {
    /// Position in the file's `cards` list, 0-based: how an edit finds it.
    pub index: usize,
    pub card: CardRef,
    pub qty: u32,
    pub finish: Finish,
    pub categories: Vec<String>,
    /// The deepest type among its categories, `"in-deck"` when none is typed.
    pub place: Kind,
    /// Counted toward the deck: `place` is within in-deck.
    pub in_deck: bool,
}

#[derive(Debug, Facet)]
pub struct Category {
    pub name: String,
    /// Absent for a label that says nothing about where a card is.
    #[facet(skip_serializing_if = Option::is_none)]
    pub kind: Option<Kind>,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Parsed {
    Deck {
        #[facet(skip_serializing_if = Option::is_none)]
        name: Option<String>,
        #[facet(skip_serializing_if = Option::is_none)]
        format: Option<String>,
        /// The path of the deck this one is a variant of.
        #[facet(rename = "variantOf", skip_serializing_if = Option::is_none)]
        variant_of: Option<String>,
        /// The printing whose art stands for the deck in the deck list.
        #[facet(skip_serializing_if = Option::is_none)]
        cover: Option<Printing>,
        /// Markdown about the deck, for a person to read.
        #[facet(skip_serializing_if = Option::is_none)]
        description: Option<String>,
        categories: Vec<Category>,
        cards: Vec<Card>,
        /// Physical cards in the deck, so counting nothing outside it.
        total: u32,
    },
    /// The deck file is not one the format allows, and this says why.
    Refused { message: String },
}

fn wire(d: Deck) -> Parsed {
    let total = d
        .cards
        .iter()
        .filter(|c| c.in_deck())
        .map(|c| c.qty.get())
        .sum();
    let cards = d
        .cards
        .into_iter()
        .enumerate()
        .map(|(index, c)| Card {
            index,
            in_deck: c.in_deck(),
            card: match c.card {
                deck::CardRef::Printing(p) => CardRef::Printing {
                    set: p.set,
                    num: p.num,
                },
                deck::CardRef::Name(name) => CardRef::Name { name },
            },
            qty: c.qty.get(),
            finish: match c.finish {
                deck::Finish::Nonfoil => Finish::Nonfoil,
                deck::Finish::Foil => Finish::Foil,
                deck::Finish::Etched => Finish::Etched,
            },
            categories: c.categories,
            place: c.place.into(),
        })
        .collect();
    Parsed::Deck {
        name: d.name,
        format: d.format,
        variant_of: d.variant_of,
        cover: d.cover.map(|p| Printing {
            set: p.set,
            num: p.num,
        }),
        description: d.description,
        categories: d
            .categories
            .into_iter()
            .map(|c| Category {
                name: c.name,
                kind: c.kind.map(Kind::from),
            })
            .collect(),
        cards,
        total,
    }
}

pub fn parse_deck_text(text: &str) -> Parsed {
    match Deck::parse(text) {
        Ok(d) => wire(d),
        Err(e) => Parsed::Refused {
            message: e.to_string(),
        },
    }
}

/// JSON of [`Parsed`]; `web/src/deck.ts` is the typed side of it.
#[wasm_bindgen]
pub fn parse_deck(text: &str) -> String {
    facet_json::to_string(&parse_deck_text(text)).expect("Parsed serialises")
}

/// A line of pasted Archidekt text the import could not carry over whole.
#[derive(Debug, Facet)]
pub struct Unreadable {
    /// 1-based, counting every line of the pasted text.
    pub line: u32,
    pub text: String,
    pub reason: String,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Imported {
    /// The deck as `.deck.toml`, and every line that did not make it in whole.
    Imported {
        toml: String,
        unreadable: Vec<Unreadable>,
        /// Cards the line gave a set but no number, named by name until the
        /// browser pins the printing that set has.
        #[facet(rename = "setOnly")]
        set_only: Vec<SetOnly>,
    },
    /// Nothing usable came out: no line was a card.
    Refused { message: String },
}

/// A card named by name whose Archidekt line also gave a set.
#[derive(Debug, Facet)]
pub struct SetOnly {
    /// Into the deck's cards, as the edits index them.
    pub index: u32,
    pub line: u32,
    pub text: String,
    pub name: String,
    pub set: String,
}

pub fn import_archidekt_text(text: &str) -> Imported {
    let imported = Deck::read_archidekt(text);
    let unreadable: Vec<Unreadable> = imported
        .unreadable
        .iter()
        .map(|u| Unreadable {
            line: u32::try_from(u.line).unwrap_or(u32::MAX),
            text: u.text.clone(),
            reason: u.reason.clone(),
        })
        .collect();
    if imported.deck.cards.is_empty() {
        return Imported::Refused {
            message: match imported.unreadable.first() {
                None => "there are no cards in the text".into(),
                Some(u) => format!("no line was a card, starting with {u}"),
            },
        };
    }
    Imported::Imported {
        toml: imported.to_toml(),
        unreadable,
        set_only: imported
            .set_only
            .iter()
            .map(|s| SetOnly {
                index: u32::try_from(s.index).unwrap_or(u32::MAX),
                line: u32::try_from(s.line).unwrap_or(u32::MAX),
                text: s.text.clone(),
                name: s.name.clone(),
                set: s.set.clone(),
            })
            .collect(),
    }
}

/// JSON of [`Imported`]: pasted Archidekt text as `.deck.toml`, read the way
/// Archidekt reads it, each printing's name written beside it as a comment,
/// and every line that could not be carried over with the reason.
#[wasm_bindgen]
pub fn import_archidekt(text: &str) -> String {
    facet_json::to_string(&import_archidekt_text(text)).expect("Imported serialises")
}

/// The deck as Archidekt text, names only. `names` is JSON of
/// `{ "set/num": "Card Name" }` for the cards the file names by printing, which
/// the file itself does not name; the browser has them from Scryfall. Throws,
/// listing them, when a printing has no name.
#[wasm_bindgen]
pub fn export_archidekt(text: &str, names: Option<String>) -> Result<String, JsError> {
    let names: HashMap<String, String> = match names.as_deref() {
        None | Some("") => HashMap::new(),
        Some(json) => facet_json::from_str(json)
            .map_err(|e| JsError::new(&format!("names are not {{\"set/num\": name}}: {e}")))?,
    };
    deck::export_archidekt(text, &names).map_err(|e| JsError::new(&e.to_string()))
}

/// `text` with card `index`'s categories replaced by `categories` (JSON of
/// `string[]`). Refuses an edit that leaves a deck the format does not allow.
#[wasm_bindgen]
pub fn set_card_categories(text: &str, index: usize, categories: &str) -> Result<String, JsError> {
    let categories: Vec<String> = facet_json::from_str(categories)
        .map_err(|e| JsError::new(&format!("categories are not string[]: {e}")))?;
    edit::set_categories(text, index, &categories).map_err(|e| JsError::new(&e.to_string()))
}

/// `text` with `name` declared under `[categories]`, of type `kind` (a type
/// name from the tree) or untyped when `kind` is empty.
#[wasm_bindgen]
pub fn declare_category(text: &str, name: &str, kind: &str) -> Result<String, JsError> {
    let kind = match kind {
        "" => None,
        k => Some(
            CategoryType::ALL
                .into_iter()
                .find(|t| t.as_str() == k)
                .ok_or_else(|| JsError::new(&format!("{k:?} is not a category type")))?,
        ),
    };
    edit::declare_category(text, name, kind).map_err(|e| JsError::new(&e.to_string()))
}

fn refused(e: impl ToString) -> JsError {
    JsError::new(&e.to_string())
}

/// `text` with card `index` at `qty` copies; zero removes it.
#[wasm_bindgen]
pub fn set_card_qty(text: &str, index: usize, qty: u32) -> Result<String, JsError> {
    edit::set_card_qty(text, index, qty).map_err(refused)
}

/// `text` without card `index`'s line.
#[wasm_bindgen]
pub fn remove_card(text: &str, index: usize) -> Result<String, JsError> {
    edit::remove_card(text, index).map_err(refused)
}

/// `text` with card `index` a commander, declaring a `Commander` category
/// when the deck has no commander-typed one.
#[wasm_bindgen]
pub fn set_commander(text: &str, index: usize) -> Result<String, JsError> {
    edit::set_commander(text, index).map_err(refused)
}

/// `text` with card `index` named by the printing `set/num`.
#[wasm_bindgen]
pub fn set_card_printing(
    text: &str,
    index: usize,
    set: &str,
    num: &str,
) -> Result<String, JsError> {
    edit::set_card_printing(text, index, set, num).map_err(refused)
}

/// `text` with card `index`'s finish `"nonfoil"`, `"foil"` or `"etched"`.
#[wasm_bindgen]
pub fn set_card_finish(text: &str, index: usize, finish: &str) -> Result<String, JsError> {
    let finish = match finish {
        "nonfoil" => deck::Finish::Nonfoil,
        "foil" => deck::Finish::Foil,
        "etched" => deck::Finish::Etched,
        other => return Err(refused(format!("{other:?} is not a finish"))),
    };
    edit::set_card_finish(text, index, finish).map_err(refused)
}

/// `text` with one more `card` (JSON of [`NewCard`]) in `categories` (JSON
/// of `string[]`): a new last line, or one more of a card already in exactly
/// those categories.
#[wasm_bindgen]
pub fn add_card(text: &str, card: &str, categories: &str) -> Result<String, JsError> {
    let (card, comment) = new_card(card)?;
    let categories: Vec<String> = facet_json::from_str(categories)
        .map_err(|e| refused(format!("categories are not string[]: {e}")))?;
    edit::add_card(text, &card, &categories, comment.as_deref()).map_err(refused)
}

/// A card to add, from JSON of [`NewCard`], and the name to comment it with.
fn new_card(json: &str) -> Result<(deck::CardRef, Option<String>), JsError> {
    let card: NewCard =
        facet_json::from_str(json).map_err(|e| refused(format!("card is not a CardRef: {e}")))?;
    Ok(match card {
        NewCard::Name { name } => (deck::CardRef::Name(name), None),
        NewCard::Printing { set, num, name } => (
            deck::CardRef::Printing(deck::Printing {
                set: set.trim().to_ascii_lowercase(),
                num: num.trim().to_string(),
            }),
            name,
        ),
    })
}

/// The commit message that saves `before` as `after` at `path`, per
/// `chip_decklist::changelog`. An empty `before` is a deck's first save.
#[wasm_bindgen]
pub fn commit_message(before: &str, after: &str, path: &str) -> Result<String, JsError> {
    changelog::commit_message_for_text(before, after, path).map_err(refused)
}

/// One change between two decks, as `chip_decklist::diff` finds it. A card
/// line is named by its index in the `before` deck, the `after` deck, or
/// both; `text` is the line a commit message says the change with.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum DeckChange {
    Add {
        after: u32,
        text: String,
    },
    Remove {
        before: u32,
        text: String,
    },
    Qty {
        before: u32,
        after: u32,
        text: String,
    },
    /// The line's categories.
    Move {
        before: u32,
        after: u32,
        text: String,
    },
    Printing {
        before: u32,
        after: u32,
        text: String,
    },
    Finish {
        before: u32,
        after: u32,
        text: String,
    },
    Declare {
        category: String,
        text: String,
    },
    Undeclare {
        category: String,
        text: String,
    },
    Retype {
        category: String,
        text: String,
    },
    Rename {
        text: String,
    },
    Format {
        text: String,
    },
    VariantOf {
        text: String,
    },
    Cover {
        text: String,
    },
    Description {
        text: String,
    },
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Compared {
    /// Every change, in commit-message order. A change's position here is
    /// how `apply_changes` takes it.
    Diff { changes: Vec<DeckChange> },
    /// One of the two texts is not a deck, and this says why.
    Refused { message: String },
}

fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

pub fn compare_texts(before: &str, after: &str) -> Compared {
    let (old, new, names) = match diff::parse_pair(before, after) {
        Ok(pair) => pair,
        Err(e) => {
            return Compared::Refused {
                message: e.to_string(),
            }
        }
    };
    let changes = Diff::new(&old, &new, |p| names.get(p).cloned())
        .changes
        .into_iter()
        .map(|(change, text)| match change {
            Change::Add { after } => DeckChange::Add {
                after: index(after),
                text,
            },
            Change::Remove { before } => DeckChange::Remove {
                before: index(before),
                text,
            },
            Change::Qty { before, after } => DeckChange::Qty {
                before: index(before),
                after: index(after),
                text,
            },
            Change::Move { before, after } => DeckChange::Move {
                before: index(before),
                after: index(after),
                text,
            },
            Change::Printing { before, after } => DeckChange::Printing {
                before: index(before),
                after: index(after),
                text,
            },
            Change::Finish { before, after } => DeckChange::Finish {
                before: index(before),
                after: index(after),
                text,
            },
            Change::Declare(category) => DeckChange::Declare { category, text },
            Change::Undeclare(category) => DeckChange::Undeclare { category, text },
            Change::Retype(category) => DeckChange::Retype { category, text },
            Change::Rename => DeckChange::Rename { text },
            Change::Format => DeckChange::Format { text },
            Change::VariantOf => DeckChange::VariantOf { text },
            Change::Cover => DeckChange::Cover { text },
            Change::Description => DeckChange::Description { text },
        })
        .collect();
    Compared::Diff { changes }
}

/// JSON of [`Compared`]: every change from `before` to `after`.
#[wasm_bindgen]
pub fn compare_decks(before: &str, after: &str) -> String {
    facet_json::to_string(&compare_texts(before, after)).expect("Compared serialises")
}

/// `before` with the changes at `take`, positions in [`compare_decks`]'s
/// list, taken from `after` and nothing else touched.
#[wasm_bindgen]
pub fn apply_changes(before: &str, after: &str, take: &[u32]) -> Result<String, JsError> {
    let take: Vec<usize> = take.iter().map(|&i| i as usize).collect();
    diff::apply(before, after, &take).map_err(refused)
}

/// `text` as a variant of the deck at `parent`, or standing alone when
/// `parent` is absent or empty.
#[wasm_bindgen]
pub fn set_variant_of(text: &str, parent: Option<String>) -> Result<String, JsError> {
    let parent = parent.filter(|p| !p.is_empty());
    edit::set_variant_of(text, parent.as_deref()).map_err(refused)
}

/// `text` with the deck's `name` set, and its `format` unless `format` is
/// empty; a file without them gains them at its top.
#[wasm_bindgen]
pub fn set_deck_meta(text: &str, name: &str, format: &str) -> Result<String, JsError> {
    edit::set_deck_meta(text, name, format).map_err(refused)
}

/// `text` with the deck's `cover` set to the printing `set/num`, or dropped
/// when `cover` is absent.
#[wasm_bindgen]
pub fn set_deck_cover(text: &str, cover: Option<String>) -> Result<String, JsError> {
    edit::set_deck_cover(text, cover.as_deref()).map_err(refused)
}

/// `text` with the deck's Markdown `description` set, or dropped when it is
/// absent or only whitespace.
#[wasm_bindgen]
pub fn set_deck_description(text: &str, description: Option<String>) -> Result<String, JsError> {
    edit::set_deck_description(text, description.as_deref()).map_err(refused)
}

/// The text of a new, empty deck. An empty `format` is left out.
#[wasm_bindgen]
pub fn new_deck(name: &str, format: &str) -> Result<String, JsError> {
    edit::new_deck(name, format).map_err(refused)
}

/// One line of the collection (ADR-0023).
#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct OwnedCard {
    /// Position in the file's `cards` list, 0-based: how an edit finds it.
    pub index: usize,
    pub card: CardRef,
    pub qty: u32,
    pub finish: Finish,
    /// The place the copies are in; absent for unsorted.
    #[facet(skip_serializing_if = Option::is_none)]
    pub at: Option<String>,
}

#[derive(Debug, Facet)]
pub struct Place {
    pub name: String,
    /// The path of the deck this place is, when it is one.
    #[facet(skip_serializing_if = Option::is_none)]
    pub deck: Option<String>,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum ParsedCollection {
    Collection {
        places: Vec<Place>,
        cards: Vec<OwnedCard>,
        /// Physical cards owned, wherever they are.
        total: u32,
    },
    /// The file is not one the format allows, and this says why.
    Refused { message: String },
}

fn finish_wire(f: deck::Finish) -> Finish {
    match f {
        deck::Finish::Nonfoil => Finish::Nonfoil,
        deck::Finish::Foil => Finish::Foil,
        deck::Finish::Etched => Finish::Etched,
    }
}

fn card_wire(c: deck::CardRef) -> CardRef {
    match c {
        deck::CardRef::Printing(p) => CardRef::Printing {
            set: p.set,
            num: p.num,
        },
        deck::CardRef::Name(name) => CardRef::Name { name },
    }
}

pub fn parse_collection_text(text: &str) -> ParsedCollection {
    match Collection::parse(text) {
        Ok(c) => ParsedCollection::Collection {
            total: c.cards.iter().map(|o| o.qty.get()).sum(),
            places: c
                .places
                .into_iter()
                .map(|p| Place {
                    name: p.name,
                    deck: p.deck,
                })
                .collect(),
            cards: c
                .cards
                .into_iter()
                .enumerate()
                .map(|(index, o)| OwnedCard {
                    index,
                    card: card_wire(o.card),
                    qty: o.qty.get(),
                    finish: finish_wire(o.finish),
                    at: o.at,
                })
                .collect(),
        },
        Err(e) => ParsedCollection::Refused {
            message: e.to_string(),
        },
    }
}

/// JSON of [`ParsedCollection`]; `web/src/collection.ts` is the typed side.
#[wasm_bindgen]
pub fn parse_collection(text: &str) -> String {
    facet_json::to_string(&parse_collection_text(text)).expect("ParsedCollection serialises")
}

fn finish_arg(finish: &str) -> Result<deck::Finish, JsError> {
    match finish {
        "nonfoil" => Ok(deck::Finish::Nonfoil),
        "foil" => Ok(deck::Finish::Foil),
        "etched" => Ok(deck::Finish::Etched),
        other => Err(refused(format!("{other:?} is not a finish"))),
    }
}

/// A place argument: the empty string is unsorted.
fn place_arg(place: &str) -> Option<&str> {
    (!place.is_empty()).then_some(place)
}

/// `text` with `qty` more of `card` (JSON of [`NewCard`]) in `finish`, at the
/// place `at` or unsorted when it is empty.
#[wasm_bindgen]
pub fn collection_add(
    text: &str,
    card: &str,
    qty: u32,
    finish: &str,
    at: &str,
) -> Result<String, JsError> {
    let (card, comment) = new_card(card)?;
    collection::add(
        text,
        &card,
        qty,
        finish_arg(finish)?,
        place_arg(at),
        comment.as_deref(),
    )
    .map_err(refused)
}

/// `text` with `qty` of card `index` moved to the place `to`, or to unsorted
/// when it is empty.
#[wasm_bindgen]
pub fn collection_move(text: &str, index: usize, qty: u32, to: &str) -> Result<String, JsError> {
    collection::move_cards(text, index, qty, place_arg(to)).map_err(refused)
}

/// `text` with all of each line in `indices` moved to the place `to`, or to
/// unsorted when it is empty.
#[wasm_bindgen]
pub fn collection_move_lines(text: &str, indices: &[u32], to: &str) -> Result<String, JsError> {
    let indices: Vec<usize> = indices.iter().map(|&i| i as usize).collect();
    collection::move_lines(text, &indices, place_arg(to)).map_err(refused)
}

/// `text` with `qty` of card `index` made the printing `set/num`, or left the
/// card it is when `set` is empty, in `finish`.
#[wasm_bindgen]
pub fn collection_reprint(
    text: &str,
    index: usize,
    qty: u32,
    set: &str,
    num: &str,
    finish: &str,
) -> Result<String, JsError> {
    let printing = (!set.is_empty()).then(|| deck::Printing {
        set: set.to_string(),
        num: num.to_string(),
    });
    collection::reprint(text, index, qty, printing.as_ref(), finish_arg(finish)?).map_err(refused)
}

/// `text` with card `index` at `qty` copies; zero removes it.
#[wasm_bindgen]
pub fn collection_set_qty(text: &str, index: usize, qty: u32) -> Result<String, JsError> {
    collection::set_qty(text, index, qty).map_err(refused)
}

/// `text` with the place `name` declared, standing for the deck at `deck`
/// unless it is empty.
#[wasm_bindgen]
pub fn declare_place(text: &str, name: &str, deck: &str) -> Result<String, JsError> {
    collection::declare_place(text, name, place_arg(deck)).map_err(refused)
}

/// `text` without the place `name`, which must hold nothing.
#[wasm_bindgen]
pub fn undeclare_place(text: &str, name: &str) -> Result<String, JsError> {
    collection::undeclare_place(text, name).map_err(refused)
}

/// `text` with the place `from` called `to`, its cards with it.
#[wasm_bindgen]
pub fn rename_place(text: &str, from: &str, to: &str) -> Result<String, JsError> {
    collection::rename_place(text, from, to).map_err(refused)
}

/// The commit message that saves the collection `before` as `after` at
/// `path`. An empty `before` is the file's first save.
#[wasm_bindgen]
pub fn collection_commit_message(before: &str, after: &str, path: &str) -> Result<String, JsError> {
    collection::commit_message_for_text(before, after, path).map_err(refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/src/deck.gen.ts");

    fn typescript() -> String {
        let mut g = facet_typescript::TypeScriptGenerator::new();
        g.add_type::<Parsed>();
        g.add_type::<NewCard>();
        g.add_type::<Imported>();
        g.add_type::<ParsedCollection>();
        g.add_type::<Compared>();
        format!(
            "// Generated from crates/meldweb-curator/wasm/src/lib.rs. Do not edit:\n\
             // UPDATE_TS=1 cargo test -p meldweb-wasm rewrites it.\n\n{}",
            g.finish().trim_end()
        ) + "\n"
    }

    #[test]
    fn generated_typescript_is_current() {
        let want = typescript();
        if std::env::var_os("UPDATE_TS").is_some() {
            std::fs::write(GENERATED, &want).unwrap();
        }
        let have = std::fs::read_to_string(GENERATED).unwrap_or_default();
        assert!(
            have == want,
            "{GENERATED} is stale; run UPDATE_TS=1 cargo test -p meldweb-wasm"
        );
    }

    #[test]
    fn an_archidekt_import_names_each_printing_in_a_comment() {
        let Imported::Imported {
            toml,
            unreadable,
            set_only,
        } = import_archidekt_text("1x Rashmi and Ragavan (moc) 94 [Commander{top}]\n")
        else {
            panic!("refused");
        };
        assert!(unreadable.is_empty() && set_only.is_empty());
        assert!(
            toml.contains(r#"{ printing = "moc/94", in = ["Commander"] },  # Rashmi and Ragavan"#),
            "{toml}"
        );
        assert!(
            toml.contains(r#"Commander = { type = "commander" }"#),
            "{toml}"
        );
    }

    #[test]
    fn an_archidekt_import_lists_every_line_it_could_not_read() {
        let json = import_archidekt("1x Sol Ring [Ramp]\nnot a card\n");
        assert!(json.starts_with(r#"{"kind":"imported","#), "{json}");
        assert!(
            json.contains(r#""unreadable":[{"line":2,"text":"not a card","reason":"#),
            "{json}"
        );
    }

    #[test]
    fn an_archidekt_import_with_no_card_is_refused() {
        let Imported::Refused { message } = import_archidekt_text("nothing\n") else {
            panic!("imported nothing");
        };
        assert!(message.contains("line 1"), "{message}");
        assert!(matches!(
            import_archidekt_text(""),
            Imported::Refused { .. }
        ));
    }

    #[test]
    fn an_archidekt_export_names_printings_from_the_map_it_is_given() {
        let text = "cards = [{ printing = \"moc/94\", in = [\"Commander\"] }]\n[categories]\nCommander = { type = \"commander\" }\n";
        let out = export_archidekt(text, Some(r#"{"moc/94":"Rashmi and Ragavan"}"#.into()));
        assert_eq!(
            out.ok().as_deref(),
            Some("1x Rashmi and Ragavan [Commander{top}]\n")
        );
        let named = "cards = [{ name = \"Sol Ring\" }]\n";
        assert_eq!(
            export_archidekt(named, None).ok().as_deref(),
            Some("1x Sol Ring\n")
        );
    }

    #[test]
    fn lantern_is_a_hundred_cards_with_one_commander() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../decks/lantern.deck.toml"
        ))
        .unwrap();
        let Parsed::Deck { cards, total, .. } = parse_deck_text(&text) else {
            panic!("lantern refused");
        };
        assert_eq!(total, 100);
        let commanders: Vec<_> = cards
            .iter()
            .filter(|c| matches!(c.place, Kind::Commander))
            .collect();
        assert_eq!(commanders.len(), 1);
    }

    #[test]
    fn a_refused_deck_says_why() {
        let Parsed::Refused { message } = parse_deck_text(r#"cards = [{ qty = 1 }]"#) else {
            panic!("a card with no name was accepted");
        };
        assert!(message.contains("neither"), "{message}");
    }

    #[test]
    fn a_card_to_add_is_a_card_ref_with_an_optional_name() {
        let text = "cards = [\n]\n";
        let text = add_card(text, r#"{"kind":"name","name":"Sol Ring"}"#, "[]").unwrap();
        let text = add_card(
            &text,
            r#"{"kind":"printing","set":"MOC","num":"94","name":"Rashmi and Ragavan"}"#,
            "[]",
        )
        .unwrap();
        let text = add_card(&text, r#"{"kind":"printing","set":"moc","num":"94"}"#, "[]").unwrap();
        assert_eq!(
            text,
            "cards = [\n  { name = \"Sol Ring\" },\n  { printing = \"moc/94\", qty = 2 },  # Rashmi and Ragavan\n]\n"
        );
        assert_eq!(
            commit_message("", &text, "decks/new.deck.toml").unwrap(),
            "new: +2 Rashmi and Ragavan, +1 Sol Ring"
        );
    }

    #[test]
    fn a_collection_names_each_card_once_and_its_place_when_it_has_one() {
        let text = declare_place("", "Bulk", "").unwrap();
        let text = collection_add(
            &text,
            r#"{"kind":"name","name":"Sol Ring"}"#,
            2,
            "foil",
            "Bulk",
        )
        .unwrap();
        let text = collection_add(
            &text,
            r#"{"kind":"printing","set":"MOC","num":"94","name":"Rashmi and Ragavan"}"#,
            1,
            "nonfoil",
            "",
        )
        .unwrap();
        let json = parse_collection(&text);
        assert!(!json.contains("null"), "{json}");
        assert_eq!(
            json,
            r#"{"kind":"collection","places":[{"name":"Bulk"}],"cards":[{"index":0,"card":{"kind":"name","name":"Sol Ring"},"qty":2,"finish":"foil","at":"Bulk"},{"index":1,"card":{"kind":"printing","set":"moc","num":"94"},"qty":1,"finish":"nonfoil"}],"total":3}"#
        );
        assert_eq!(
            collection_commit_message("", &text, "collection.toml").unwrap(),
            "collection: +1 Rashmi and Ragavan, +2 Sol Ring to bulk, +place bulk"
        );
    }

    #[test]
    fn absent_fields_are_omitted_not_null() {
        let json = parse_deck(r#"cards = [{ name = "Sol Ring" }]"#);
        assert!(!json.contains("null"), "{json}");
    }
}
