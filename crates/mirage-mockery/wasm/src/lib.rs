//! Mirage Mockery in the browser: Curator's custom cards page reads, checks
//! and renders a `.card.toml` with the same `mockery-core` the CLI runs.
//!
//! The page does the fetching: the card's art from the repo, and the real
//! cards to compare it with from Scryfall. Nothing here touches the network.
//! `meldweb-curator/web/src/mockery.gen.ts` is generated from the wire types
//! below; the test at the bottom fails when it is stale.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::anyhow;
use facet::Facet;
use mockery_core::card::Card;
use mockery_core::read::read;
use mockery_core::real::{cost_of, real_card, telling_word};
use mockery_core::render::{self, Layout, Paper, Sheet};
use wasm_bindgen::prelude::{wasm_bindgen, JsError};

/// A line of rules text Insight could not read, and the word it stopped at.
#[derive(Debug, Facet)]
pub struct Unread {
    pub line: String,
    pub read: String,
    pub at: String,
}

/// A `.card.toml`, read.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Checked {
    Card {
        name: String,
        type_line: String,
        /// `{2}{U}`, as Oracle text writes it; empty for no cost.
        cost: String,
        /// A word any real card with this text has too, to narrow the search
        /// for one; absent for a card with no rules text, which is never
        /// refused for it.
        #[facet(default, skip_serializing_if = Option::is_none)]
        word: Option<String>,
        /// The art's path, relative to the card file.
        #[facet(default, skip_serializing_if = Option::is_none)]
        art: Option<String>,
        lines: u32,
        unread: Vec<Unread>,
    },
    Refused {
        message: String,
    },
}

/// A real card to compare with.
#[derive(Debug, Facet)]
pub struct Real {
    pub name: String,
    pub oracle: String,
}

/// Portrait, as Phyrexian Metamorph's paper.
#[derive(Debug, Clone, Copy, Facet)]
#[repr(u8)]
#[facet(rename_all = "kebab-case")]
pub enum SheetPaper {
    A4,
    A3,
    Letter,
    Legal,
}

/// The same parameters as a proxy sheet, so the two print alike.
#[derive(Debug, Clone, Copy, Facet)]
#[facet(rename_all = "camelCase")]
pub struct CardSheet {
    pub paper: SheetPaper,
    pub gap_mm: f32,
    pub margin_mm: f32,
    pub cut_marks: bool,
}

/// One card on a sheet: its text, and its art's bytes when it names some.
#[derive(Debug, Facet)]
pub struct SheetCard {
    pub text: String,
    pub qty: u32,
}

fn parse(text: &str) -> anyhow::Result<Card> {
    // The art stays as the file names it; the page fetched it by that path.
    Card::parse(text, Path::new(""))
}

fn js(e: anyhow::Error) -> JsError {
    JsError::new(&format!("{e:#}"))
}

pub fn check_text(text: &str) -> Checked {
    match parse(text) {
        Err(e) => Checked::Refused {
            message: format!("{e:#}"),
        },
        Ok(card) => {
            let reading = read(&card);
            Checked::Card {
                cost: cost_of(&card),
                word: telling_word(&card),
                art: card.art.as_ref().map(|a| a.to_string_lossy().into_owned()),
                lines: reading.lines as u32,
                unread: reading
                    .unread
                    .into_iter()
                    .map(|u| Unread {
                        line: u.line,
                        read: u.read,
                        at: u.at,
                    })
                    .collect(),
                name: card.name,
                type_line: card.type_line,
            }
        }
    }
}

/// JSON of [`Checked`]: the card `text` says, or why it says none, and what
/// Insight made of its rules text.
#[wasm_bindgen]
pub fn check(text: &str) -> String {
    facet_json::to_string(&check_text(text)).expect("Checked serialises")
}

/// Why the card is a real one, or nothing: `named` says Magic has a card by
/// its name, and `same_cost` (JSON of `Real[]`) is every real card at its
/// cost whose text holds its `word`.
#[wasm_bindgen]
pub fn why_real(text: &str, named: bool, same_cost: &str) -> Result<Option<String>, JsError> {
    let card = parse(text).map_err(js)?;
    let reals: Vec<Real> =
        facet_json::from_str(same_cost).map_err(|e| JsError::new(&format!("not Real[]: {e}")))?;
    real_card(
        &card,
        named,
        reals.into_iter().map(|r| Ok((r.name, r.oracle))),
    )
    .map_err(js)
}

/// Art bytes by the path a card names them by.
fn arts(paths: &str, images: &[u8], ends: &[u32]) -> Result<HashMap<PathBuf, Vec<u8>>, JsError> {
    let paths: Vec<String> =
        facet_json::from_str(paths).map_err(|e| JsError::new(&format!("not string[]: {e}")))?;
    if paths.len() != ends.len() {
        return Err(JsError::new("one end per art path"));
    }
    let mut out = HashMap::new();
    let mut start = 0;
    for (path, &end) in paths.into_iter().zip(ends) {
        let bytes = images
            .get(start..end as usize)
            .ok_or_else(|| JsError::new("art ends past the bytes given"))?;
        out.insert(PathBuf::from(path), bytes.to_vec());
        start = end as usize;
    }
    Ok(out)
}

fn compile(
    cards: &[Card],
    layout: Layout,
    art: &HashMap<PathBuf, Vec<u8>>,
) -> Result<render::PagedDocument, JsError> {
    render::compile_with_art(cards, layout, |path| {
        art.get(path)
            .cloned()
            .ok_or_else(|| anyhow!("no art {} in the repo", path.display()))
    })
    .map_err(js)
}

/// The card as a PNG at `ppi`. `art` is its art's bytes, empty for none.
#[wasm_bindgen]
pub fn render_png(text: &str, art: &[u8], ppi: f32) -> Result<Vec<u8>, JsError> {
    let card = parse(text).map_err(js)?;
    let arts = match &card.art {
        Some(path) if !art.is_empty() => HashMap::from([(path.clone(), art.to_vec())]),
        _ => HashMap::new(),
    };
    let doc = compile(std::slice::from_ref(&card), Layout::Cards, &arts)?;
    render::png(&doc, 0, ppi).map_err(js)
}

/// A sheet to print and cut, as a PDF. `cards` is JSON of `SheetCard[]`,
/// `layout` of [`CardSheet`], and the art is every distinct art path (JSON of
/// `string[]`) with its bytes end to end in `images`, the `n`th ending at
/// `ends[n]`.
#[wasm_bindgen]
pub fn sheet_pdf(
    cards: &str,
    layout: &str,
    art_paths: &str,
    images: &[u8],
    ends: &[u32],
) -> Result<Vec<u8>, JsError> {
    let wanted: Vec<SheetCard> =
        facet_json::from_str(cards).map_err(|e| JsError::new(&format!("not SheetCard[]: {e}")))?;
    let l: CardSheet =
        facet_json::from_str(layout).map_err(|e| JsError::new(&format!("not a CardSheet: {e}")))?;
    let sheet = Sheet::new(
        match l.paper {
            SheetPaper::A4 => Paper::A4,
            SheetPaper::A3 => Paper::A3,
            SheetPaper::Letter => Paper::Letter,
            SheetPaper::Legal => Paper::Legal,
        },
        l.gap_mm,
        l.margin_mm,
        l.cut_marks,
    )
    .map_err(js)?;
    let mut all = Vec::new();
    for w in &wanted {
        let card = parse(&w.text).map_err(js)?;
        all.extend(std::iter::repeat_n(card, w.qty as usize));
    }
    if all.is_empty() {
        return Err(JsError::new("no cards to print"));
    }
    let doc = compile(&all, Layout::Sheet(sheet), &arts(art_paths, images, ends)?)?;
    render::pdf(&doc).map_err(js)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GENERATED: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../meldweb-curator/web/src/mockery.gen.ts"
    );
    const DRONE: &str = include_str!("../../core/tests/cards/proofsheet-drone.card.toml");

    #[test]
    fn generated_typescript_is_current() {
        let mut g = facet_typescript::TypeScriptGenerator::new();
        g.add_type::<Checked>();
        g.add_type::<Real>();
        g.add_type::<CardSheet>();
        g.add_type::<SheetCard>();
        let want = format!(
            "// Generated from crates/mirage-mockery/wasm/src/lib.rs. Do not edit:\n\
             // UPDATE_TS=1 cargo test -p mockery-wasm rewrites it.\n\n{}",
            g.finish().trim_end()
        ) + "\n";
        if std::env::var_os("UPDATE_TS").is_some() {
            std::fs::write(GENERATED, &want).unwrap();
        }
        let have = std::fs::read_to_string(GENERATED).unwrap_or_default();
        assert!(
            have == want,
            "{GENERATED} is stale; run UPDATE_TS=1 cargo test -p mockery-wasm"
        );
    }

    #[test]
    fn a_card_reads_with_its_cost_and_a_word_to_search_by() {
        match check_text(DRONE) {
            Checked::Card {
                name, cost, word, ..
            } => {
                assert_eq!(name, "Proofsheet Drone");
                assert_eq!(cost, "{2}{U}{U/P}");
                assert!(word.is_some_and(|w| DRONE.to_lowercase().contains(&w)));
            }
            Checked::Refused { message } => panic!("{message}"),
        }
    }

    #[test]
    fn the_same_text_at_the_same_cost_is_a_real_card() {
        let card = parse(DRONE).unwrap();
        let text = card.rules.join("\n");
        let same = vec![Ok(("Some Drone".to_string(), text))];
        assert!(real_card(&card, false, same).unwrap().is_some());
        assert!(real_card(&card, false, vec![]).unwrap().is_none());
        assert!(real_card(&card, true, vec![]).unwrap().is_some());
    }

    #[test]
    fn a_card_renders_to_a_png() {
        let png = render_png(DRONE, &[], 72.0).unwrap_or_else(|_| panic!("render"));
        assert_eq!(&png[1..4], b"PNG");
    }
}
