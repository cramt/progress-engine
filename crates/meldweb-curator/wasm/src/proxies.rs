//! Phyrexian Metamorph on the deck page: what is left to print, from
//! `chip-decklist`'s `supply` as the CLI counts it (ADR-0038), and the sheet
//! itself, from `metamorph-sheet`. The page fetches Scryfall's images; this
//! side does no I/O, as the sheet does none.

use std::collections::HashMap;

use chip_decklist::collection::Collection;
use chip_decklist::deck::Deck;
use chip_decklist::supply::supply;
use facet::Facet;
use metamorph_sheet::{Jpeg, Layout, Paper as SheetPaper};
use wasm_bindgen::prelude::{wasm_bindgen, JsError};

use crate::{read_names, refused};

/// Portrait.
#[derive(Debug, Clone, Copy, Facet)]
#[repr(u8)]
#[facet(rename_all = "kebab-case")]
pub enum Paper {
    A4,
    A3,
    Letter,
    Legal,
}

/// How a sheet lays cards out, as `metamorph`'s flags say it.
#[derive(Debug, Clone, Copy, Facet)]
#[facet(rename_all = "camelCase")]
pub struct SheetLayout {
    pub paper: Paper,
    /// Millimetres between cards.
    pub gap_mm: f32,
    /// Millimetres kept clear at the paper's edge.
    pub margin_mm: f32,
    pub cut_marks: bool,
}

impl From<SheetLayout> for Layout {
    fn from(l: SheetLayout) -> Layout {
        Layout {
            paper: match l.paper {
                Paper::A4 => SheetPaper::A4,
                Paper::A3 => SheetPaper::A3,
                Paper::Letter => SheetPaper::Letter,
                Paper::Legal => SheetPaper::Legal,
            },
            gap_mm: l.gap_mm,
            margin_mm: l.margin_mm,
            cut_marks: l.cut_marks,
        }
    }
}

/// One deck line's proxies.
#[derive(Debug, Facet)]
pub struct ProxyLine {
    /// The deck line, a `DeckCard.index`.
    pub index: usize,
    pub qty: u32,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum ToPrint {
    ToPrint {
        lines: Vec<ProxyLine>,
        /// Basic lands left out on request.
        basics: u32,
        /// Copies the collection has, left out.
        owned: u32,
    },
    Refused {
        message: String,
    },
}

pub fn to_print_text(
    path: &str,
    deck_text: &str,
    collection_text: &str,
    names: &HashMap<String, String>,
    leave_out_basics: bool,
) -> ToPrint {
    let deck = match Deck::parse(deck_text) {
        Ok(d) => d,
        Err(e) => {
            return ToPrint::Refused {
                message: format!("the deck: {e}"),
            }
        }
    };
    let mine = match Collection::parse(collection_text) {
        Ok(c) => c,
        Err(e) => {
            return ToPrint::Refused {
                message: format!("your collection: {e}"),
            }
        }
    };
    let p = supply(
        path,
        (&deck, deck_text),
        (&mine, collection_text),
        &[],
        names,
    )
    .to_print(leave_out_basics);
    ToPrint::ToPrint {
        lines: p
            .lines
            .into_iter()
            .map(|(index, qty)| ProxyLine { index, qty })
            .collect(),
        basics: p.basics,
        owned: p.owned,
    }
}

/// JSON of [`ToPrint`]: the proxies the deck at `path` needs, less what
/// `collection` owns (empty text for the whole deck), and with
/// `leave_out_basics`, less the basic lands.
#[wasm_bindgen]
pub fn proxies_to_print(
    path: &str,
    text: &str,
    collection: &str,
    names: Option<String>,
    leave_out_basics: bool,
) -> Result<String, JsError> {
    let p = to_print_text(
        path,
        text,
        collection,
        &read_names(names)?,
        leave_out_basics,
    );
    Ok(facet_json::to_string(&p).expect("ToPrint serialises"))
}

fn layout(json: &str) -> Result<Layout, JsError> {
    let l: SheetLayout =
        facet_json::from_str(json).map_err(|e| refused(format!("not a SheetLayout: {e}")))?;
    Ok(l.into())
}

/// How many cards `layout` (JSON of [`SheetLayout`]) fits on a page.
#[wasm_bindgen]
pub fn proxies_per_page(layout_json: &str) -> Result<u32, JsError> {
    let n = layout(layout_json)?.per_page().map_err(refused)?;
    Ok(n as u32)
}

/// The PDF. `images` is every distinct JPEG end to end, the `n`th ending at
/// `ends[n]`; `cards` is the image of each proxy in print order, so a card
/// printed four times is embedded once.
#[wasm_bindgen]
pub fn proxy_sheet(
    layout_json: &str,
    images: &[u8],
    ends: &[u32],
    cards: &[u32],
) -> Result<Vec<u8>, JsError> {
    let layout = layout(layout_json)?;
    let mut jpegs = Vec::with_capacity(ends.len());
    let mut start = 0;
    for (n, &end) in ends.iter().enumerate() {
        let end = end as usize;
        let bytes = images
            .get(start..end)
            .ok_or_else(|| refused(format!("image {n} ends at {end}, past the bytes given")))?;
        jpegs.push(Jpeg::parse(bytes.to_vec()).map_err(|e| refused(format!("image {n}: {e}")))?);
        start = end;
    }
    let order = cards
        .iter()
        .map(|&i| {
            jpegs
                .get(i as usize)
                .ok_or_else(|| refused(format!("no image {i}")))
        })
        .collect::<Result<Vec<&Jpeg>, JsError>>()?;
    metamorph_sheet::pdf(&layout, &order).map_err(refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whole_deck_or_what_the_collection_lacks() {
        let deck = "cards = [\n  { name = \"Sol Ring\" },\n  { name = \"Island\", qty = 2 },\n]\n";
        let mine = "cards = [{ name = \"Sol Ring\" }]\n";
        let lines = |p: ToPrint| match p {
            ToPrint::ToPrint {
                lines,
                basics,
                owned,
            } => (
                lines.iter().map(|l| (l.index, l.qty)).collect::<Vec<_>>(),
                basics,
                owned,
            ),
            ToPrint::Refused { message } => panic!("{message}"),
        };
        let none = HashMap::new();
        let path = "decks/a.deck.toml";
        assert_eq!(
            lines(to_print_text(path, deck, "", &none, false)),
            (vec![(0, 1), (1, 2)], 0, 0)
        );
        assert_eq!(
            lines(to_print_text(path, deck, mine, &none, true)),
            (vec![], 2, 1)
        );
    }

    #[test]
    fn a_layout_reads_as_metamorph_flags() {
        let l = layout(r#"{"paper":"letter","gapMm":0,"marginMm":5,"cutMarks":true}"#)
            .unwrap_or_else(|_| panic!("refused"));
        assert_eq!(l.per_page().unwrap(), 9);
    }
}
