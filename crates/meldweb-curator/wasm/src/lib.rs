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

use chip_decklist::deck::{self, CategoryType, Deck};
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

/// Archidekt's text export as `.deck.toml`, each printing's name written
/// beside it as a comment from the name Archidekt gave it.
#[wasm_bindgen]
pub fn import_archidekt(text: &str) -> Result<String, JsError> {
    deck::import_archidekt(text).map_err(|e| JsError::new(&e.to_string()))
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
    let card: NewCard =
        facet_json::from_str(card).map_err(|e| refused(format!("card is not a CardRef: {e}")))?;
    let categories: Vec<String> = facet_json::from_str(categories)
        .map_err(|e| refused(format!("categories are not string[]: {e}")))?;
    let (card, comment) = match card {
        NewCard::Name { name } => (deck::CardRef::Name(name), None),
        NewCard::Printing { set, num, name } => (
            deck::CardRef::Printing(deck::Printing {
                set: set.trim().to_ascii_lowercase(),
                num: num.trim().to_string(),
            }),
            name,
        ),
    };
    edit::add_card(text, &card, &categories, comment.as_deref()).map_err(refused)
}

/// The commit message that saves `before` as `after` at `path`, per
/// `chip_decklist::changelog`. An empty `before` is a deck's first save.
#[wasm_bindgen]
pub fn commit_message(before: &str, after: &str, path: &str) -> Result<String, JsError> {
    changelog::commit_message_for_text(before, after, path).map_err(refused)
}

/// `text` with the deck's `name` set, and its `format` unless `format` is
/// empty; a file without them gains them at its top.
#[wasm_bindgen]
pub fn set_deck_meta(text: &str, name: &str, format: &str) -> Result<String, JsError> {
    edit::set_deck_meta(text, name, format).map_err(refused)
}

/// The text of a new, empty deck. An empty `format` is left out.
#[wasm_bindgen]
pub fn new_deck(name: &str, format: &str) -> Result<String, JsError> {
    edit::new_deck(name, format).map_err(refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/src/deck.gen.ts");

    fn typescript() -> String {
        let mut g = facet_typescript::TypeScriptGenerator::new();
        g.add_type::<Parsed>();
        g.add_type::<NewCard>();
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
        let text = import_archidekt("1x Rashmi and Ragavan (moc) 94 [Commander{top}]\n").unwrap();
        assert!(
            text.contains(r#"{ printing = "moc/94", in = ["Commander"] },  # Rashmi and Ragavan"#),
            "{text}"
        );
        assert!(
            text.contains(r#"Commander = { type = "commander" }"#),
            "{text}"
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
    fn absent_fields_are_omitted_not_null() {
        let json = parse_deck(r#"cards = [{ name = "Sol Ring" }]"#);
        assert!(!json.contains("null"), "{json}");
    }
}
