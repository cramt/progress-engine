//! The browser's way into `chip-decklist`.
//!
//! The editor must agree with Ichormoon Gauntlet on what a deck is, so it runs
//! the same parser and the same edits rather than TypeScript copies of them.
//! The deck is its `.deck.toml` text (ADR-0020): JavaScript holds the text, and
//! every edit here takes a text and returns the next one.
//!
//! Which line holds a card is decided here too. An add is handed the names of
//! the printings the file holds, `{ "set/num": name }` from Scryfall, and says
//! which line it put the card on; an edit over a selection is one call that
//! says where every line went. So the page never matches names itself or
//! works out how removing a line moves the ones after it.
//!
//! It also ranks the printings of one card by the repo's `meldweb.toml`
//! (ADR-0026), through `chip-scryfall`, so the order a grid shows is decided by
//! the same query reader Gauntlet uses.
//!
//! What crosses into JavaScript is a set of wire types of its own.
//! `web/src/deck.gen.ts` is generated from them; the test at the bottom fails
//! when it is stale and rewrites it under `UPDATE_TS=1`.

use std::collections::HashMap;

pub mod copy;
mod preference;

use chip_decklist::collection::{self, Collection};
use chip_decklist::collection_import;
use chip_decklist::deck::{self, CategoryType, Deck};
use chip_decklist::diff::{self, Change, Diff};
use chip_decklist::{changelog, edit, export, trade, wanted};
use chip_scryfall::bulk::BulkCard;
use facet::Facet;
use preference::{Pin, Preference, RuleText};
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

/// A line of pasted text or an exported file that an import could not carry
/// over whole.
#[derive(Debug, Facet)]
pub struct Unreadable {
    /// 1-based, counting every line of the text.
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
    deck::export_archidekt(text, &read_names(names)?).map_err(|e| JsError::new(&e.to_string()))
}

/// The deck as `target` imports it, `cockatrice`, `cardmarket` or
/// `tabletop-simulator`, with the
/// printing names [`export_archidekt`] takes.
#[wasm_bindgen]
pub fn export_deck(text: &str, target: &str, names: Option<String>) -> Result<String, JsError> {
    let names = read_names(names)?;
    match target {
        "cockatrice" => export::export_cockatrice(text, &names),
        "cardmarket" => export::export_cardmarket(text, &names),
        "tabletop-simulator" => export::export_tabletop_simulator(text, &names),
        _ => return Err(JsError::new(&format!("no export to {target}"))),
    }
    .map_err(|e| JsError::new(&e.to_string()))
}

fn read_names(names: Option<String>) -> Result<HashMap<String, String>, JsError> {
    match names.as_deref() {
        None | Some("") => Ok(HashMap::new()),
        Some(json) => facet_json::from_str(json)
            .map_err(|e| JsError::new(&format!("names are not {{\"set/num\": name}}: {e}"))),
    }
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

/// Where a card added to a deck goes.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum AddTo {
    /// Archidekt's *Automatic*: the line holding the card, in the deck first,
    /// else a new line in no category.
    Automatic,
    /// The line holding the card in exactly these categories, else a new one.
    Categories { categories: Vec<String> },
}

/// What an add did: the next text, the line holding the card now, and
/// whether that line is new.
#[derive(Debug, Facet)]
pub struct Added {
    pub text: String,
    pub line: u32,
    pub made: bool,
}

fn added_wire(a: edit::Added) -> String {
    facet_json::to_string(&Added {
        text: a.text,
        line: index(a.line),
        made: a.made,
    })
    .expect("Added serialises")
}

/// JSON of [`Added`]: `text` with one more nonfoil `card` (JSON of
/// [`NewCard`]) where `to` (JSON of [`AddTo`]) says, on the line already
/// holding it or a new last line. Which line holds a card is decided here:
/// `names`, `{ "set/num": name }` as [`export_archidekt`] takes, names the
/// printings the file holds, so a name finds the line of its printing and a
/// double-faced card is found by its front face.
#[wasm_bindgen]
pub fn deck_add(
    text: &str,
    card: &str,
    to: &str,
    names: Option<String>,
) -> Result<String, JsError> {
    let (card, comment) = new_card(card)?;
    let to: AddTo =
        facet_json::from_str(to).map_err(|e| refused(format!("to is not an AddTo: {e}")))?;
    let to = match &to {
        AddTo::Automatic => edit::AddTo::Automatic,
        AddTo::Categories { categories } => edit::AddTo::Categories(categories),
    };
    edit::add_card(text, &card, to, comment.as_deref(), &read_names(names)?)
        .map(added_wire)
        .map_err(refused)
}

/// A board a card can be put on at a keystroke or by the drag strip.
#[derive(Debug, Clone, Copy, Facet)]
#[repr(u8)]
#[facet(rename_all = "kebab-case")]
pub enum Board {
    Maybeboard,
    Sideboard,
}

/// Where a moved card goes.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Dest {
    /// That category, declared untyped first when the deck has none by it.
    Category { name: String },
    /// The deck's category of that type, declared when it has none.
    Board { board: Board },
}

/// One action on every card it is applied to.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum DeckEdit {
    Increase,
    /// One fewer; the last one removes the line.
    Decrease,
    Remove,
    /// Archidekt's *Automatic*: in no category.
    Automatic,
    Commander,
    /// To `to`, out of the category the card was reached from; with
    /// `secondary`, into `to` as well.
    Move {
        to: Dest,
        secondary: bool,
    },
}

/// A card as the user reached it: its line, and the category of the stack it
/// was reached in, absent for no category.
#[derive(Debug, Facet)]
pub struct Target {
    pub index: u32,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub from: Option<String>,
}

/// Where one card of the text an edit started from is in the text it made.
//
// An enum where `Option<u32>` would do: facet-typescript 0.46 writes
// `Vec<Option<u32>>` as `number | null[]`, an unparenthesised union
// (`type_for_shape`, facet-rs/facet-format; no upstream issue filed yet).
// Once it writes `(number | null)[]` this can be `Option<u32>`.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Line {
    At { index: u32 },
    Gone,
}

/// An edit's next text, and where each card of the old text is in it:
/// `lines[i]` is where old card `i` went.
#[derive(Debug, Facet)]
pub struct Edited {
    pub text: String,
    pub lines: Vec<Line>,
}

pub fn edit_deck_cards_text(text: &str, edit: &str, targets: &str) -> Result<Edited, String> {
    let edit: DeckEdit =
        facet_json::from_str(edit).map_err(|e| format!("edit is not a DeckEdit: {e}"))?;
    let targets: Vec<Target> =
        facet_json::from_str(targets).map_err(|e| format!("targets are not Target[]: {e}"))?;
    let edit = match edit {
        DeckEdit::Increase => edit::CardEdit::Increase,
        DeckEdit::Decrease => edit::CardEdit::Decrease,
        DeckEdit::Remove => edit::CardEdit::Remove,
        DeckEdit::Automatic => edit::CardEdit::Automatic,
        DeckEdit::Commander => edit::CardEdit::Commander,
        DeckEdit::Move { to, secondary } => edit::CardEdit::Move {
            to: match to {
                Dest::Category { name } => edit::Dest::Category(name),
                Dest::Board { board } => edit::Dest::Board(match board {
                    Board::Maybeboard => edit::Board::Maybeboard,
                    Board::Sideboard => edit::Board::Sideboard,
                }),
            },
            secondary,
        },
    };
    let targets: Vec<edit::Target> = targets
        .into_iter()
        .map(|t| edit::Target {
            index: t.index as usize,
            from: t.from,
        })
        .collect();
    let edited = edit::edit_cards(text, &edit, &targets).map_err(|e| e.to_string())?;
    Ok(Edited {
        text: edited.text,
        lines: edited
            .lines
            .into_iter()
            .map(|l| l.map_or(Line::Gone, |i| Line::At { index: index(i) }))
            .collect(),
    })
}

/// JSON of [`Edited`]: `edit` (JSON of [`DeckEdit`]) on every one of
/// `targets` (JSON of [`Target`]`[]`) as one edit, refused whole when any
/// card refuses it. The line remap says where each card went, so the page
/// never works out how a removal shifts the lines after it.
#[wasm_bindgen]
pub fn edit_deck_cards(text: &str, edit: &str, targets: &str) -> Result<String, JsError> {
    let edited = edit_deck_cards_text(text, edit, targets).map_err(refused)?;
    Ok(facet_json::to_string(&edited).expect("Edited serialises"))
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

fn qty_arg(qty: u32) -> Result<std::num::NonZeroU32, JsError> {
    std::num::NonZeroU32::new(qty).ok_or_else(|| refused("a quantity must be at least one"))
}

/// JSON of [`Added`]: `text` with `qty` more of `card` (JSON of [`NewCard`])
/// in `finish`, at the place `at` or unsorted when it is empty, on the line
/// holding it there as [`deck_add`] finds one, `names` naming printings.
#[wasm_bindgen]
pub fn collection_add(
    text: &str,
    card: &str,
    qty: u32,
    finish: &str,
    at: &str,
    names: Option<String>,
) -> Result<String, JsError> {
    let (card, comment) = new_card(card)?;
    collection::add(
        text,
        &card,
        qty_arg(qty)?,
        finish_arg(finish)?,
        place_arg(at),
        comment.as_deref(),
        &read_names(names)?,
    )
    .map(added_wire)
    .map_err(refused)
}

/// `text` with `qty` fewer of `card` (JSON of [`NewCard`]) in `finish` at
/// `at`, off the line [`collection_add`] would put them on. Refused when no
/// line holds them there any more.
#[wasm_bindgen]
pub fn collection_take(
    text: &str,
    card: &str,
    qty: u32,
    finish: &str,
    at: &str,
    names: Option<String>,
) -> Result<String, JsError> {
    let (card, _) = new_card(card)?;
    collection::take(
        text,
        &card,
        qty_arg(qty)?,
        finish_arg(finish)?,
        place_arg(at),
        &read_names(names)?,
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

/// One printing in [`Ranked`]'s order: its index in what was ranked, and
/// which rules it matched.
#[derive(Debug, Facet)]
pub struct RankedPrinting {
    pub index: u32,
    pub matched: Vec<u32>,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Ranked {
    Ranked {
        rules: Vec<RuleText>,
        /// Whether the rules came from the repo's `meldweb.toml`; when not,
        /// they are [`preference::DEFAULT`]'s.
        declared: bool,
        /// Which of `rules` are pins, by index: what ranked a pinned printing
        /// first is shown as the pin, not as its query.
        pins: Vec<u32>,
        order: Vec<RankedPrinting>,
    },
    /// `meldweb.toml` is not one the format allows, or a printing is not a
    /// Scryfall card, and this says why.
    Refused { message: String },
}

pub fn rank_printings_text(settings: Option<&str>, printings: &str) -> Ranked {
    let preference = match Preference::parse(settings) {
        Ok(p) => p,
        Err(message) => return Ranked::Refused { message },
    };
    let cards: Vec<BulkCard> = match facet_json::from_str(printings) {
        Ok(c) => c,
        Err(e) => {
            return Ranked::Refused {
                message: format!("a printing is not a Scryfall card: {e}"),
            }
        }
    };
    let order = preference
        .rank(&cards)
        .into_iter()
        .map(|(i, matched)| RankedPrinting {
            index: index(i),
            matched,
        })
        .collect();
    Ranked::Ranked {
        pins: preference
            .rules
            .iter()
            .enumerate()
            .filter(|(_, r)| Pin::of(&r.text).is_some())
            .map(|(i, _)| index(i))
            .collect(),
        rules: preference.rules.into_iter().map(|r| r.text).collect(),
        declared: settings.is_some(),
        order,
    }
}

/// JSON of [`Ranked`]: `printings`, a JSON array of Scryfall card objects,
/// ranked by `settings`, the repo's `meldweb.toml`, or by the default rules
/// when it has none.
#[wasm_bindgen]
pub fn rank_printings(settings: Option<String>, printings: &str) -> String {
    facet_json::to_string(&rank_printings_text(settings.as_deref(), printings))
        .expect("Ranked serialises")
}

/// The `meldweb.toml` a repo without one ranks printings by.
#[wasm_bindgen]
pub fn default_settings() -> String {
    preference::DEFAULT.to_string()
}

/// `meldweb.toml` as the settings page edits it: its rules, each query unread
/// so a bad one can be fixed in place, and the default rules beside them.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum SettingsRules {
    Read {
        /// The cards given a printing of their own, wherever the file has them.
        pins: Vec<Pin>,
        /// Every other rule, in order.
        rules: Vec<RuleText>,
        /// The deck list's order, by path; a deck not in it goes after, by name.
        decks: Vec<String>,
        /// Whether the rules came from the repo's `meldweb.toml`.
        declared: bool,
        defaults: Vec<RuleText>,
    },
    /// The file is not one the format allows, and this says why.
    Refused {
        message: String,
        defaults: Vec<RuleText>,
    },
}

pub fn read_settings_text(text: Option<&str>) -> SettingsRules {
    let defaults = preference::read_rules(None).expect("the default rules read");
    match preference::read(text) {
        Ok(preference::Settings { pins, rules, decks }) => SettingsRules::Read {
            pins,
            rules,
            decks,
            declared: text.is_some(),
            defaults,
        },
        Err(message) => SettingsRules::Refused { message, defaults },
    }
}

/// JSON of [`SettingsRules`] for the repo's `meldweb.toml`, or for none.
#[wasm_bindgen]
pub fn read_settings(text: Option<String>) -> String {
    facet_json::to_string(&read_settings_text(text.as_deref())).expect("SettingsRules serialises")
}

/// Why `query` is not a printing rule, or nothing when it is one.
#[wasm_bindgen]
pub fn check_rule(query: &str) -> Option<String> {
    preference::check(query)
}

/// `pins`, `rules` and `decks`, JSON arrays of `Pin`, `RuleText` and paths,
/// as a `meldweb.toml`, the pins first; refused when a rule does not parse.
#[wasm_bindgen]
pub fn write_settings(pins: &str, rules: &str, decks: &str) -> Result<String, JsError> {
    let settings = preference::Settings {
        pins: facet_json::from_str(pins).map_err(refused)?,
        rules: facet_json::from_str(rules).map_err(refused)?,
        decks: facet_json::from_str(decks).map_err(refused)?,
    };
    preference::write(&settings).map_err(refused)
}

/// `settings`, the repo's `meldweb.toml` or none, with the deck list in
/// `order`, a JSON array of deck paths.
#[wasm_bindgen]
pub fn order_decks(settings: Option<String>, order: &str) -> Result<String, JsError> {
    let order: Vec<String> = facet_json::from_str(order).map_err(refused)?;
    preference::order_decks(settings.as_deref(), order).map_err(refused)
}

/// `settings`, the repo's `meldweb.toml` or none, with `set/num` as the
/// printing of the card called `name`, in place of any it had.
#[wasm_bindgen]
pub fn pin_printing(
    settings: Option<String>,
    name: &str,
    set: &str,
    num: &str,
) -> Result<String, JsError> {
    let pin = Pin {
        name: name.to_string(),
        set: set.to_string(),
        num: num.to_string(),
    };
    preference::pin(settings.as_deref(), pin).map_err(refused)
}

/// JSON of the [`Pin`] of the card called `name` in `settings`, matched
/// whole or by its front face, or nothing when it has none.
#[wasm_bindgen]
pub fn pinned_printing(settings: Option<String>, name: &str) -> Option<String> {
    preference::pinned(settings.as_deref(), name)
        .map(|pin| facet_json::to_string(&pin).expect("Pin serialises"))
}

/// `settings` without a printing of its own for the card called `name`.
#[wasm_bindgen]
pub fn unpin_printing(settings: Option<String>, name: &str) -> Result<String, JsError> {
    preference::unpin(settings.as_deref(), name).map_err(refused)
}

/// The commit for one save of `meldweb.toml`; `before` is `""` for the first.
#[wasm_bindgen]
pub fn settings_commit_message(before: &str, after: &str) -> String {
    preference::commit_message(before, after)
}

/// What Scryfall is asked about an export's rows: a printing by its id, or
/// by set and collector number.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Ask {
    Id { id: String },
    Printing { set: String, num: String },
}

/// A column an import leaves behind, and how many rows gave it a value.
#[derive(Debug, Facet)]
pub struct Dropped {
    pub column: String,
    pub rows: u32,
}

#[derive(Debug, Facet)]
pub struct Skipped {
    pub rows: u32,
    pub reason: String,
}

/// Another app's export, read but not yet resolved against Scryfall.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum CollectionExport {
    Export {
        /// Whose export the header says it is: `ManaBox`, `text list`.
        source: String,
        rows: u32,
        copies: u32,
        /// The binders or folders its rows name, in the order they come.
        places: Vec<String>,
        /// Whether any row has no place, and goes where the import says.
        unplaced: bool,
        /// Each question for Scryfall once.
        asks: Vec<Ask>,
        unreadable: Vec<Unreadable>,
        dropped: Vec<Dropped>,
        skipped: Vec<Skipped>,
    },
    /// No row was a card.
    Refused { message: String },
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn unreadable_wire(u: &chip_decklist::archidekt::Unreadable) -> Unreadable {
    Unreadable {
        line: count(u.line),
        text: u.text.clone(),
        reason: u.reason.clone(),
    }
}

pub fn read_collection_export_text(text: &str) -> CollectionExport {
    let read = collection_import::read(text);
    if read.rows.is_empty() {
        return CollectionExport::Refused {
            message: match read.unreadable.first() {
                None => "there are no cards in it".into(),
                Some(u) => format!("no row was a card, starting with {u}"),
            },
        };
    }
    let mut places: Vec<String> = Vec::new();
    let mut asks: Vec<collection_import::Ask> = Vec::new();
    for row in &read.rows {
        if let Some(p) = &row.place {
            if !places.contains(p) {
                places.push(p.clone());
            }
        }
        if let Some(a) = row.ask() {
            asks.push(a);
        }
    }
    let mut seen = std::collections::HashSet::new();
    asks.retain(|a| seen.insert(a.clone()));
    CollectionExport::Export {
        source: read.source.label().into(),
        rows: count(read.rows.len()),
        copies: read.rows.iter().map(|r| r.qty.get()).sum(),
        places,
        unplaced: read.rows.iter().any(|r| r.place.is_none()),
        asks: asks
            .into_iter()
            .map(|a| match a {
                collection_import::Ask::Id(id) => Ask::Id { id },
                collection_import::Ask::Printing(p) => Ask::Printing {
                    set: p.set,
                    num: p.num,
                },
            })
            .collect(),
        unreadable: read.unreadable.iter().map(unreadable_wire).collect(),
        dropped: read
            .dropped
            .into_iter()
            .map(|d| Dropped {
                column: d.column,
                rows: count(d.rows),
            })
            .collect(),
        skipped: read
            .skipped
            .into_iter()
            .map(|s| Skipped {
                rows: count(s.rows),
                reason: s.reason,
            })
            .collect(),
    }
}

/// JSON of [`CollectionExport`]: what an export holds and what Scryfall must
/// be asked before it can go in.
#[wasm_bindgen]
pub fn read_collection_export(text: &str) -> String {
    facet_json::to_string(&read_collection_export_text(text)).expect("CollectionExport serialises")
}

/// A card Scryfall answered an [`Ask`] with.
#[derive(Debug, Facet)]
pub struct ScryfallCard {
    pub id: String,
    pub set: String,
    pub num: String,
    pub name: String,
}

/// A row that went in, but not as its file named it.
#[derive(Debug, Facet)]
pub struct ImportNote {
    pub line: u32,
    pub reason: String,
}

#[derive(Debug, Facet)]
pub struct CollectionImported {
    pub text: String,
    /// Rows kept by name where the file named a printing, and why.
    pub notes: Vec<ImportNote>,
    /// Rows that did not go in: the file's, and those Scryfall named nothing.
    pub unreadable: Vec<Unreadable>,
}

pub fn import_collection_text(
    text: &str,
    export: &str,
    cards: &str,
    replace: bool,
    default_place: &str,
) -> Result<CollectionImported, String> {
    let cards: Vec<ScryfallCard> = facet_json::from_str(cards)
        .map_err(|e| format!("cards are not Scryfall's answers: {e}"))?;
    let mut found = HashMap::new();
    for c in cards {
        let answer = collection_import::Found {
            printing: deck::Printing {
                set: c.set.to_ascii_lowercase(),
                num: c.num.clone(),
            },
            name: c.name,
        };
        found.insert(
            collection_import::Ask::Id(c.id.to_lowercase()),
            answer.clone(),
        );
        found.insert(
            collection_import::Ask::Printing(answer.printing.clone()),
            answer,
        );
    }
    let read = collection_import::read(export);
    let (incoming, notes, unresolved) =
        collection_import::resolve(&read, &found, place_arg(default_place));
    let merge = if replace {
        collection::Merge::Replace
    } else {
        collection::Merge::Add
    };
    let text =
        collection::import(text, &incoming, merge, &HashMap::new()).map_err(|e| e.to_string())?;
    let mut unreadable: Vec<Unreadable> = read
        .unreadable
        .iter()
        .chain(&unresolved)
        .map(unreadable_wire)
        .collect();
    unreadable.sort_by_key(|u| u.line);
    Ok(CollectionImported {
        text,
        notes: notes
            .into_iter()
            .map(|n| ImportNote {
                line: count(n.line),
                reason: n.reason,
            })
            .collect(),
        unreadable,
    })
}

/// JSON of [`CollectionImported`]: `text` with every copy in `export` added,
/// or, with `replace`, each place it fills holding what it brings. `cards`
/// is JSON of [`ScryfallCard`]`[]`, Scryfall's answers to the export's asks.
/// Rows with no place go to `default_place`, unsorted when it is empty.
#[wasm_bindgen]
pub fn import_collection(
    text: &str,
    export: &str,
    cards: &str,
    replace: bool,
    default_place: &str,
) -> Result<String, JsError> {
    let imported =
        import_collection_text(text, export, cards, replace, default_place).map_err(refused)?;
    Ok(facet_json::to_string(&imported).expect("CollectionImported serialises"))
}

/// How the decks count toward the wanted list (ADR-0034).
#[derive(Debug, Clone, Copy, Facet)]
#[repr(u8)]
#[facet(rename_all = "lowercase")]
pub enum DeckCopies {
    Each,
    Shared,
}

/// One line of the wanted list, and how many of it the collection holds.
#[derive(Debug, Facet)]
pub struct WantedCard {
    /// Position in the file's `cards` list, 0-based: how an edit finds it.
    pub index: usize,
    pub card: CardRef,
    pub qty: u32,
    pub finish: Finish,
    /// Copies owned of any printing of the card.
    pub owned: u32,
}

/// A deck holding a card the collection is short of.
#[derive(Debug, Facet)]
pub struct DeckNeed {
    pub path: String,
    /// The deck's `name`, or its file's stem when it has none.
    pub name: String,
    pub qty: u32,
}

/// A card the decks hold more copies of than the collection does.
#[derive(Debug, Facet)]
pub struct MissingCard {
    pub name: String,
    pub missing: u32,
    pub owned: u32,
    pub decks: Vec<DeckNeed>,
}

/// A deck file the derived list could not read, and why.
#[derive(Debug, Facet)]
pub struct Unread {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum ParsedWanted {
    Wanted {
        #[facet(rename = "deckCopies")]
        deck_copies: DeckCopies,
        cards: Vec<WantedCard>,
        /// Empty while the collection cannot be read, which `collection`
        /// then says.
        missing: Vec<MissingCard>,
        #[facet(skip_serializing_if = Option::is_none)]
        collection: Option<String>,
        /// Decks left out of `missing`.
        unread: Vec<Unread>,
    },
    Refused {
        message: String,
    },
}

/// A deck file as the page loaded it.
#[derive(Debug, Facet)]
pub struct DeckFile {
    pub path: String,
    pub text: String,
}

pub fn read_wanted_text(
    text: &str,
    collection_text: &str,
    decks: &[DeckFile],
    names: &HashMap<String, String>,
) -> ParsedWanted {
    let w = match wanted::Wanted::parse(text) {
        Ok(w) => w,
        Err(e) => {
            return ParsedWanted::Refused {
                message: e.to_string(),
            }
        }
    };
    let deck_copies = match w.deck_copies {
        wanted::DeckCopies::Each => DeckCopies::Each,
        wanted::DeckCopies::Shared => DeckCopies::Shared,
    };
    let mut unread = Vec::new();
    let read: Vec<(String, Deck, String)> = decks
        .iter()
        .filter_map(|d| match Deck::parse(&d.text) {
            Ok(deck) => Some((d.path.clone(), deck, d.text.clone())),
            Err(e) => {
                unread.push(Unread {
                    path: d.path.clone(),
                    message: e.to_string(),
                });
                None
            }
        })
        .collect();
    let (collection, refusal) = match Collection::parse(collection_text) {
        Ok(c) => (c, None),
        Err(e) => (Collection::default(), Some(e.to_string())),
    };
    let deck_name = |path: &str| {
        read.iter()
            .find(|(p, _, _)| p == path)
            .and_then(|(_, d, _)| d.name.clone())
            .unwrap_or_else(|| {
                // As the deck list names it: the file's stem.
                let file = path.rsplit('/').next().unwrap_or(path);
                file.strip_suffix(".deck.toml").unwrap_or(file).to_string()
            })
    };
    let missing = if refusal.is_some() {
        Vec::new()
    } else {
        wanted::missing(&collection, collection_text, &read, names, w.deck_copies)
            .into_iter()
            .map(|m| MissingCard {
                name: m.name,
                missing: m.missing,
                owned: m.owned,
                decks: m
                    .decks
                    .into_iter()
                    .map(|(path, qty)| DeckNeed {
                        name: deck_name(&path),
                        path,
                        qty,
                    })
                    .collect(),
            })
            .collect()
    };
    let cards = w
        .cards
        .into_iter()
        .enumerate()
        .map(|(index, c)| WantedCard {
            index,
            owned: wanted::owned(&collection, collection_text, &c.card, names),
            card: card_wire(c.card),
            qty: c.qty.get(),
            finish: finish_wire(c.finish),
        })
        .collect();
    ParsedWanted::Wanted {
        deck_copies,
        cards,
        missing,
        collection: refusal,
        unread,
    }
}

/// JSON of [`ParsedWanted`]: the wanted list `text`, each want with what
/// `collection` holds of it, and the cards the decks (JSON of `DeckFile[]`)
/// hold that it is short of, `names` naming printings as for an add.
#[wasm_bindgen]
pub fn read_wanted(
    text: &str,
    collection: &str,
    decks: &str,
    names: Option<String>,
) -> Result<String, JsError> {
    let decks: Vec<DeckFile> = facet_json::from_str(decks)
        .map_err(|e| refused(format!("decks are not DeckFile[]: {e}")))?;
    let parsed = read_wanted_text(text, collection, &decks, &read_names(names)?);
    Ok(facet_json::to_string(&parsed).expect("ParsedWanted serialises"))
}

/// JSON of [`Added`]: `text` with `qty` more of `card` (JSON of [`NewCard`])
/// in `finish`, on the line already wanting it so or a new last one.
#[wasm_bindgen]
pub fn wanted_add(
    text: &str,
    card: &str,
    qty: u32,
    finish: &str,
    names: Option<String>,
) -> Result<String, JsError> {
    let (card, comment) = new_card(card)?;
    wanted::add(
        text,
        &card,
        qty_arg(qty)?,
        finish_arg(finish)?,
        comment.as_deref(),
        &read_names(names)?,
    )
    .map(added_wire)
    .map_err(refused)
}

/// `text` with want `index` at `qty` copies; zero removes it.
#[wasm_bindgen]
pub fn wanted_set_qty(text: &str, index: usize, qty: u32) -> Result<String, JsError> {
    wanted::set_qty(text, index, qty).map_err(refused)
}

/// `text` with want `index` in `finish`.
#[wasm_bindgen]
pub fn wanted_set_finish(text: &str, index: usize, finish: &str) -> Result<String, JsError> {
    wanted::set_finish(text, index, finish_arg(finish)?).map_err(refused)
}

/// `text` counting the decks by `copies`, `"each"` or `"shared"`.
#[wasm_bindgen]
pub fn wanted_set_deck_copies(text: &str, copies: &str) -> Result<String, JsError> {
    let copies = match copies {
        "each" => wanted::DeckCopies::Each,
        "shared" => wanted::DeckCopies::Shared,
        other => return Err(refused(format!("{other:?} is not each or shared"))),
    };
    wanted::set_deck_copies(text, copies).map_err(refused)
}

/// The commit message that saves the wanted list `before` as `after` at
/// `path`. An empty `before` is the file's first save.
#[wasm_bindgen]
pub fn wanted_commit_message(before: &str, after: &str, path: &str) -> Result<String, JsError> {
    wanted::commit_message_for_text(before, after, path).map_err(refused)
}

/// Copies of one of their collection's lines to bring to a trade.
#[derive(Debug, Facet)]
pub struct TradePull {
    /// Their place the copies are in; absent for unsorted.
    #[facet(skip_serializing_if = Option::is_none)]
    pub at: Option<String>,
    /// The place is one of their decks.
    #[facet(rename = "inDeck")]
    pub in_deck: bool,
    pub card: CardRef,
    pub finish: Finish,
    pub qty: u32,
}

/// A card the wanted list is short of that their collection holds.
#[derive(Debug, Facet)]
pub struct TradeOffer {
    pub name: String,
    /// Copies still wanted, by hand and by the decks.
    pub short: u32,
    pub pulls: Vec<TradePull>,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum ParsedTrade {
    Trade {
        offers: Vec<TradeOffer>,
        /// Decks left out of what the decks lack.
        unread: Vec<Unread>,
    },
    Refused {
        message: String,
    },
}

pub fn read_trade_text(
    wanted_text: &str,
    collection: &str,
    decks: &[DeckFile],
    theirs: &str,
    deck: Option<&str>,
    names: &HashMap<String, String>,
) -> ParsedTrade {
    let refuse = |whose: &str, e: &dyn std::fmt::Display| ParsedTrade::Refused {
        message: format!("{whose}: {e}"),
    };
    // One deck is wanted for itself: what it lacks against the whole
    // collection, as if no other deck or hand want asked for a copy. A deck
    // not built yet is the deck it would be.
    let (wanted_text, decks) = match deck {
        None => (wanted_text, decks),
        Some(path) => match decks.iter().find(|d| d.path == path) {
            Some(d) => match Deck::parse(&d.text) {
                Ok(_) => ("", std::slice::from_ref(d)),
                Err(e) => return refuse(path, &e),
            },
            None => return refuse(path, &"there is no such deck"),
        },
    };
    let w = match wanted::Wanted::parse(wanted_text) {
        Ok(w) => w,
        Err(e) => return refuse("your wanted list", &e),
    };
    let mine = match Collection::parse(collection) {
        Ok(c) => c,
        Err(e) => return refuse("your collection", &e),
    };
    let their = match Collection::parse(theirs) {
        Ok(c) => c,
        Err(e) => return refuse("their collection", &e),
    };
    let mut unread = Vec::new();
    let read: Vec<(String, Deck, String)> = decks
        .iter()
        .filter_map(|d| match Deck::parse(&d.text) {
            Ok(deck) => Some((d.path.clone(), deck, d.text.clone())),
            Err(e) => {
                unread.push(Unread {
                    path: d.path.clone(),
                    message: e.to_string(),
                });
                None
            }
        })
        .collect();
    let offers = trade::offers(
        (&w, wanted_text),
        (&mine, collection),
        &read,
        (&their, theirs),
        names,
    )
    .into_iter()
    .map(|o| TradeOffer {
        name: o.name,
        short: o.short,
        pulls: o
            .pulls
            .into_iter()
            .map(|p| TradePull {
                at: p.place,
                in_deck: p.in_deck,
                card: card_wire(p.card),
                finish: finish_wire(p.finish),
                qty: p.qty,
            })
            .collect(),
    })
    .collect();
    ParsedTrade::Trade { offers, unread }
}

/// JSON of [`ParsedTrade`]: what the collection `theirs` holds of the wanted
/// list `wanted`, read with `collection` and the decks (JSON of `DeckFile[]`)
/// as [`read_wanted`] reads them. With `deck`, a path among `decks`, only
/// what that deck lacks is wanted.
#[wasm_bindgen]
pub fn read_trade(
    wanted: &str,
    collection: &str,
    decks: &str,
    theirs: &str,
    deck: Option<String>,
    names: Option<String>,
) -> Result<String, JsError> {
    let decks: Vec<DeckFile> = facet_json::from_str(decks)
        .map_err(|e| refused(format!("decks are not DeckFile[]: {e}")))?;
    let parsed = read_trade_text(
        wanted,
        collection,
        &decks,
        theirs,
        deck.as_deref(),
        &read_names(names)?,
    );
    Ok(facet_json::to_string(&parsed).expect("ParsedTrade serialises"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/src/deck.gen.ts");

    fn typescript() -> String {
        let mut g = facet_typescript::TypeScriptGenerator::new();
        g.add_type::<Parsed>();
        g.add_type::<NewCard>();
        g.add_type::<AddTo>();
        g.add_type::<Added>();
        g.add_type::<DeckEdit>();
        g.add_type::<Target>();
        g.add_type::<Edited>();
        g.add_type::<Imported>();
        g.add_type::<ParsedCollection>();
        g.add_type::<CollectionExport>();
        g.add_type::<ScryfallCard>();
        g.add_type::<CollectionImported>();
        g.add_type::<ParsedWanted>();
        g.add_type::<DeckFile>();
        g.add_type::<ParsedTrade>();
        g.add_type::<Compared>();
        g.add_type::<Ranked>();
        g.add_type::<SettingsRules>();
        g.add_type::<chip_scryfall::copy::Wanted>();
        g.add_type::<chip_scryfall::copy::Found>();
        g.add_type::<chip_scryfall::copy::PrintingFacts>();
        g.add_type::<chip_scryfall::copy::SearchAnswer>();
        g.add_type::<copy::StoreStep>();
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
    fn a_trade_for_one_deck_wants_only_what_that_deck_lacks() {
        let decks = [
            DeckFile {
                path: "decks/a.deck.toml".into(),
                text: "cards = [{ name = \"Sol Ring\" }, { name = \"Mind Stone\" }]\n".into(),
            },
            DeckFile {
                path: "decks/b.deck.toml".into(),
                text: "cards = [{ name = \"Arcane Signet\" }]\n".into(),
            },
        ];
        let wanted = "cards = [{ name = \"The One Ring\" }]\n";
        let mine = "cards = [{ name = \"Mind Stone\" }]\n";
        let theirs = "cards = [\n  { name = \"Sol Ring\" },\n  { name = \"Mind Stone\" },\n  \
                      { name = \"Arcane Signet\" },\n  { name = \"The One Ring\" },\n]\n";
        let names =
            |deck| match read_trade_text(wanted, mine, &decks, theirs, deck, &HashMap::new()) {
                ParsedTrade::Trade { offers, .. } => {
                    offers.into_iter().map(|o| o.name).collect::<Vec<_>>()
                }
                ParsedTrade::Refused { message } => vec![message],
            };
        assert_eq!(names(None), ["Arcane Signet", "Sol Ring", "The One Ring"]);
        assert_eq!(names(Some("decks/a.deck.toml")), ["Sol Ring"]);
        assert_eq!(
            names(Some("decks/c.deck.toml")),
            ["decks/c.deck.toml: there is no such deck"]
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
        let add = |text: &str, card: &str| -> Added {
            let json = deck_add(text, card, r#"{"kind":"automatic"}"#, None).unwrap();
            facet_json::from_str(&json).unwrap()
        };
        let sol = add("cards = [\n]\n", r#"{"kind":"name","name":"Sol Ring"}"#);
        assert_eq!((sol.line, sol.made), (0, true));
        let rashmi = add(
            &sol.text,
            r#"{"kind":"printing","set":"MOC","num":"94","name":"Rashmi and Ragavan"}"#,
        );
        assert_eq!((rashmi.line, rashmi.made), (1, true));
        let again = add(
            &rashmi.text,
            r#"{"kind":"printing","set":"moc","num":"94"}"#,
        );
        assert_eq!((again.line, again.made), (1, false));
        let text = again.text;
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
        let add = |text: &str, card: &str, qty, finish, at| -> Added {
            let json = collection_add(text, card, qty, finish, at, None).unwrap();
            facet_json::from_str(&json).unwrap()
        };
        let text = declare_place("", "Bulk", "").unwrap();
        let sol = add(
            &text,
            r#"{"kind":"name","name":"Sol Ring"}"#,
            2,
            "foil",
            "Bulk",
        );
        assert_eq!((sol.line, sol.made), (0, true));
        let rashmi = add(
            &sol.text,
            r#"{"kind":"printing","set":"MOC","num":"94","name":"Rashmi and Ragavan"}"#,
            1,
            "nonfoil",
            "",
        );
        assert_eq!((rashmi.line, rashmi.made), (1, true));
        let text = rashmi.text;
        let taken = collection_take(
            &text,
            r#"{"kind":"name","name":"Rashmi and Ragavan"}"#,
            1,
            "nonfoil",
            "",
            None,
        )
        .unwrap();
        assert_eq!(taken, sol.text, "the comment names the printing");
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

    #[test]
    fn a_card_edit_says_where_every_line_went_and_a_gone_one_is_null() {
        let text = "cards = [\n  { name = \"A\" },\n  { name = \"B\" },\n]\n";
        let edited =
            edit_deck_cards_text(text, r#"{"kind":"remove"}"#, r#"[{"index":0}]"#).unwrap();
        assert_eq!(
            facet_json::to_string(&edited).unwrap(),
            r#"{"text":"cards = [\n  { name = \"B\" },\n]\n","lines":[{"kind":"gone"},{"kind":"at","index":0}]}"#
        );
        let moved = edit_deck_cards_text(
            "cards = [\n  { name = \"A\" },\n]\n",
            r#"{"kind":"move","to":{"kind":"board","board":"sideboard"},"secondary":false}"#,
            r#"[{"index":0,"from":"Nope"}]"#,
        )
        .unwrap();
        assert!(
            moved.text.contains(r#"Sideboard = { type = "sideboard" }"#),
            "{}",
            moved.text
        );
        assert!(
            edit_deck_cards_text(text, r#"{"kind":"remove"}"#, r#"[{"index":9}]"#)
                .unwrap_err()
                .contains("no card 9")
        );
    }

    const MANABOX: &str = "Binder Name,Binder Type,Name,Set code,Set name,Collector number,Foil,Rarity,Quantity,ManaBox ID,Scryfall ID,Purchase price,Misprint,Altered,Condition,Language,Purchase price currency\n\
Trade binder,binder,Sol Ring,CMM,Commander Masters,400,foil,uncommon,2,1,AAA-1,0.0,false,false,near_mint,en,USD\n\
Trade binder,binder,Sol Ring,CMM,Commander Masters,400,foil,uncommon,1,1,AAA-1,0.0,false,false,near_mint,en,USD\n\
,binder,Island,UNF,Unfinity,235,normal,common,4,2,BBB-2,0.0,false,false,near_mint,en,USD\n";

    #[test]
    fn an_export_asks_scryfall_each_thing_once_and_goes_in_as_one_edit() {
        let CollectionExport::Export {
            source,
            copies,
            places,
            unplaced,
            asks,
            ..
        } = read_collection_export_text(MANABOX)
        else {
            panic!("refused");
        };
        assert_eq!(source, "ManaBox");
        assert_eq!(copies, 7);
        assert_eq!(places, ["Trade binder"]);
        assert!(unplaced);
        assert_eq!(asks.len(), 2, "{asks:?}");

        let cards = r#"[{"id":"aaa-1","set":"cmm","num":"400","name":"Sol Ring"}]"#;
        let imported = import_collection_text("", MANABOX, cards, false, "Bulk").unwrap();
        assert_eq!(
            imported.text,
            "cards = [\n  { printing = \"cmm/400\", qty = 3, finish = \"foil\", at = \"Trade binder\" },  # Sol Ring\n  { name = \"Island\", qty = 4, at = \"Bulk\" },\n]\n\n[places]\n\"Trade binder\" = {}\nBulk = {}\n"
        );
        assert_eq!(imported.notes.len(), 1);
        assert_eq!(imported.notes[0].line, 4);
    }

    #[test]
    fn an_export_with_no_card_in_it_is_refused() {
        assert!(matches!(
            read_collection_export_text("Name,Quantity\n,3\n"),
            CollectionExport::Refused { .. }
        ));
    }
}
