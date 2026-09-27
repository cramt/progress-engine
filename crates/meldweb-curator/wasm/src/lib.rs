//! The browser's way into `chip-decklist`.
//!
//! The editor must agree with Ichormoon Gauntlet on what a decklist is, so it
//! runs the same parser rather than a TypeScript copy of it. What crosses into
//! JavaScript is a wire type of its own, not `chip_decklist::Entry`: that one's
//! JSON is `gauntlet parse`'s output, and the scripts reading it should not
//! change shape because a web page wanted `null`s dropped.
//!
//! `web/src/decklist.gen.ts` is generated from these types; the test at the
//! bottom fails when it is stale and rewrites it under `UPDATE_TS=1`.

use chip_decklist::{Category, ParseError};
use facet::Facet;
use wasm_bindgen::prelude::{wasm_bindgen, JsError};

#[derive(Debug, Facet)]
pub struct Entry {
    /// 1-based line in the text it was parsed from: how an edit finds it.
    pub line: usize,
    pub qty: u32,
    pub name: String,
    #[facet(skip_serializing_if = Option::is_none)]
    pub set: Option<String>,
    #[facet(skip_serializing_if = Option::is_none)]
    pub num: Option<String>,
    pub foil: bool,
    pub categories: Vec<Category>,
    /// Decided here rather than in TypeScript, for the same reason the parser is.
    pub commander: bool,
    /// Sideboard, maybeboard, companion or `{noDeck}`: listed, not among the deck.
    pub outside: bool,
}

impl Entry {
    fn new(line: usize, e: chip_decklist::Entry) -> Self {
        let commander = e.is_commander();
        let outside = e.is_outside();
        Entry {
            line,
            qty: e.qty.get(),
            name: e.name,
            set: e.set,
            num: e.num,
            foil: e.foil,
            categories: e.categories,
            commander,
            outside,
        }
    }
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Parsed {
    Deck {
        entries: Vec<Entry>,
        /// Physical cards in the deck, so excluding anything `outside`.
        total: u32,
    },
    /// The parser refuses a line rather than dropping it, and names the line.
    Refused {
        line: usize,
        text: String,
        message: String,
    },
}

pub fn parse_decklist(text: &str) -> Parsed {
    // Line by line rather than chip_decklist::parse, to keep each line number.
    let entries: Result<Vec<Entry>, ParseError> = text
        .lines()
        .enumerate()
        .filter_map(|(i, l)| {
            chip_decklist::parse_line(l, i + 1)
                .map(|e| e.map(|e| Entry::new(i + 1, e)))
                .transpose()
        })
        .collect();
    match entries {
        Ok(entries) => {
            let total = entries.iter().filter(|e| !e.outside).map(|e| e.qty).sum();
            Parsed::Deck { entries, total }
        }
        Err(e) => {
            let message = e.to_string();
            let (ParseError::Malformed { line, text }
            | ParseError::ZeroQuantity { line, text }
            | ParseError::EmptyName { line, text }) = e;
            Parsed::Refused {
                line,
                text,
                message,
            }
        }
    }
}

/// `text` with line `line`'s categories replaced by `categories` (JSON of
/// `Category[]`), and every other byte as it was.
#[wasm_bindgen]
pub fn set_categories(text: &str, line: usize, categories: &str) -> Result<String, JsError> {
    let categories: Vec<Category> = facet_json::from_str(categories)
        .map_err(|e| JsError::new(&format!("categories are not Category[]: {e}")))?;
    let mut out = String::with_capacity(text.len() + 32);
    let mut found = false;
    for (i, raw) in text.split_inclusive('\n').enumerate() {
        let body = raw.trim_end_matches(['\n', '\r']);
        if i + 1 == line {
            let edited = chip_decklist::with_categories(body, &categories)
                .ok_or_else(|| JsError::new(&format!("line {line} is not a card")))?;
            out.push_str(&edited);
            out.push_str(&raw[body.len()..]);
            found = true;
        } else {
            out.push_str(raw);
        }
    }
    if !found {
        return Err(JsError::new(&format!("there is no line {line}")));
    }
    Ok(out)
}

/// JSON of [`Parsed`]; `web/src/decklist.ts` is the typed side of it.
#[wasm_bindgen]
pub fn parse(text: &str) -> String {
    facet_json::to_string(&parse_decklist(text)).expect("Parsed serialises")
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../web/src/decklist.gen.ts");

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
    fn lantern_is_a_hundred_cards_with_one_commander() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../decks/lantern.txt"
        ))
        .unwrap();
        let Parsed::Deck { entries, total } = parse_decklist(&text) else {
            panic!("lantern.txt refused");
        };
        assert_eq!(total, 100);
        let commanders: Vec<_> = entries.iter().filter(|e| e.commander).collect();
        assert_eq!(commanders.len(), 1);
        assert_eq!(commanders[0].name, "Rashmi and Ragavan");
    }

    #[test]
    fn a_bad_line_is_refused_by_number() {
        let json = parse("1 Sol Ring\n0 Island\n");
        assert_eq!(
            json,
            r#"{"kind":"refused","line":2,"text":"0 Island","message":"line 2: quantity must be greater than zero: \"0 Island\""}"#
        );
    }

    #[test]
    fn moving_a_card_rewrites_only_its_line() {
        let text = "// lantern\r\n1 Sol Ring [Ramp]\r\n1 Island [Land]\r\n";
        let edited = set_categories(text, 2, r#"[{"name":"Draw","flags":["top"]}]"#).unwrap();
        assert_eq!(
            edited,
            "// lantern\r\n1 Sol Ring [Draw{top}]\r\n1 Island [Land]\r\n"
        );
    }

    #[test]
    fn absent_printing_is_omitted_not_null() {
        let json = parse("1 Sol Ring\n");
        assert!(!json.contains("null"), "{json}");
    }
}
