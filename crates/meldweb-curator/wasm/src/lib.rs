//! The browser's way into `chip-dec                kind: c.kind.map(Kind::from),list`.
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
use chip_decklist::edit;
use facet::Facet;
use wasm_bindgen::prelude::{wasm_bindgen, JsError};

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum CardRef {
    Printing { set: String, num: String },
    Name { name: String },
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

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/src/deck.gen.ts");

    fn typescript() -> String {
        let mut g = facet_typescript::TypeScriptGenerator::new();
        g.add_type::<Parsed>();
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
    fn absent_fields_are_omitted_not_null() {
        let json = parse_deck(r#"cards = [{ name = "Sol Ring" }]"#);
        assert!(!json.contains("null"), "{json}");
    }
}
