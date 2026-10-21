//! The size the rules text comes out at. Printed cards set rules text near
//! 9pt and shrink it only for a long text, so a layout whose column is too
//! narrow shows up as text shrunk far below that on an ordinary card.

use std::collections::BTreeMap;
use std::path::Path;

use mockery_core::card::Card;
use mockery_core::render::{self, Layout};
use typst::layout::{Frame, FrameItem};

/// Glyphs set at each size, in hundredths of a point, over the page.
fn sizes(frame: &Frame, out: &mut BTreeMap<i64, usize>) {
    for (_, item) in frame.items() {
        match item {
            FrameItem::Group(g) => sizes(&g.frame, out),
            FrameItem::Text(t) => {
                *out.entry((t.size.to_pt() * 100.0).round() as i64)
                    .or_default() += t.glyphs.len()
            }
            _ => {}
        }
    }
}

/// The size most of the page's glyphs are set at: on a card with any text
/// to speak of, the rules text's.
fn body_size(path: &Path) -> f64 {
    let card = Card::open(path).unwrap();
    let doc = render::compile(&[card], Layout::Cards).unwrap();
    let mut out = BTreeMap::new();
    sizes(&doc.pages()[0].frame, &mut out);
    let (size, _) = out.into_iter().max_by_key(|&(_, n)| n).unwrap();
    size as f64 / 100.0
}

fn test_card(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/cards/{name}.card.toml"))
}

/// `MOCKERY_SIZES="a.card.toml b.card.toml" cargo test --test sizes -- --ignored --nocapture`
/// prints each card's body size: the bench for a layout's proportions.
#[test]
#[ignore]
fn report_body_sizes() {
    for path in std::env::var("MOCKERY_SIZES")
        .unwrap_or_default()
        .split_whitespace()
    {
        println!("{path}: {}pt", body_size(Path::new(path)));
    }
}

/// Each striated layout's test card against a floor under the size it is set
/// at now, so a change to the frame that narrows a column shows up here. The
/// Saga's chapter symbols straddling the column's edge took The Cleansing of
/// Shinovar from 6.25pt to 6.75pt.
#[test]
fn striated_text_is_set_near_printed_sizes() {
    for (name, floor) in [
        ("the-cleansing-of-shinovar", 6.5),
        ("jedi-knight-class", 7.5),
        ("jasnah-veristitalian", 8.0),
    ] {
        let size = body_size(&test_card(name));
        assert!(size >= floor, "{name} is set at {size}pt, under {floor}pt");
    }
}
