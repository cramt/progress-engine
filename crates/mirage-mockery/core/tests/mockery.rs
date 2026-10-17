//! A custom card renders; a real one, by name or under another, never gets
//! that far.

use std::path::{Path, PathBuf};

use chip_scryfall::index::IndexFile;
use mockery_core::card::Card;
use mockery_core::render::{self, Layout, Paper, Sheet};
use mockery_core::symbols::Symbols;
use mockery_core::{read, real};

fn here(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn symbols() -> Symbols {
    Symbols::open(&here("tests/symbols")).unwrap()
}

fn index() -> IndexFile {
    IndexFile::open(&here("../../../decks/index.jsonl")).unwrap()
}

fn drone() -> Card {
    Card::open(&here("tests/cards/proofsheet-drone.card.toml")).unwrap()
}

fn card(toml: &str) -> Card {
    Card::parse(toml, Path::new(".")).unwrap()
}

#[test]
fn a_card_renders_to_a_trimmed_png_and_a_pdf() {
    let doc = render::compile(&[drone()], &symbols(), Layout::Cards).unwrap();
    assert_eq!(doc.pages().len(), 1);
    let pdf = render::pdf(&doc).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let png = render::png(&doc, 0, 300.0).unwrap();
    // The PNG header's width and height: 63mm × 88mm at 300 ppi.
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    assert_eq!((width, height), (744, 1039));
}

#[test]
fn a_sheet_holds_what_the_paper_does() {
    let seventeen = vec![drone(); 17];
    for (paper, per_page) in [
        (Paper::A4, 9),
        (Paper::Letter, 9),
        // Legal's extra length is 3.9 rows, not 4.
        (Paper::Legal, 9),
        (Paper::A3, 16),
    ] {
        let sheet = Sheet::new(paper, 0.0, 5.0, true).unwrap();
        let doc = render::compile(&seventeen, &symbols(), Layout::Sheet(sheet)).unwrap();
        assert_eq!(doc.pages().len(), 17usize.div_ceil(per_page), "{paper:?}");
    }
}

#[test]
fn a_gap_costs_cards_only_where_there_is_no_room_for_it() {
    // A4 less 5 mm a side is 200 × 287 mm. A 5 mm gap still fits 3 × 3
    // (199 × 274); a 15 mm gap fits 2 × 2, so nine cards take three pages.
    let nine = vec![drone(); 9];
    let pages = |gap| {
        let sheet = Sheet::new(Paper::A4, gap, 5.0, true).unwrap();
        render::compile(&nine, &symbols(), Layout::Sheet(sheet))
            .unwrap()
            .pages()
            .len()
    };
    assert_eq!(pages(5.0), 1);
    assert_eq!(pages(15.0), 3);
}

#[test]
fn a_negative_gap_is_refused() {
    assert!(Sheet::new(Paper::A4, -1.0, 5.0, true).is_err());
}

#[test]
fn a_symbol_with_no_svg_is_named() {
    let c = card("name = \"x\"\ncost = \"{S}\"\ntype = \"Instant\"");
    let err = render::compile(&[c], &symbols(), Layout::Cards).unwrap_err();
    assert!(format!("{err:#}").contains("no symbol {S}"), "{err:#}");
}

#[test]
fn a_custom_card_is_not_refused() {
    real::refuse_real(&[drone()], &index()).unwrap();
}

#[test]
fn a_real_card_by_name_is_refused() {
    let c = card(
        "name = \"Serum Visions\"\ncost = \"{U}\"\ntype = \"Sorcery\"\nrules = \"Draw a card.\"",
    );
    let err = real::refuse_real(&[c], &index()).unwrap_err();
    assert!(err.to_string().contains("is a real card"), "{err}");
}

#[test]
fn a_real_card_under_another_name_is_refused() {
    let c = card(
        "name = \"Serum Glimpse\"\ncost = \"{U}\"\ntype = \"Sorcery\"\nrules = \"Draw a card. Scry 2.\"",
    );
    let err = real::refuse_real(&[c], &index()).unwrap_err();
    assert!(
        err.to_string()
            .contains("\"Serum Visions\" under another name"),
        "{err}"
    );
}

#[test]
fn a_vanilla_card_matching_a_real_vanilla_is_not_a_copy() {
    let c = card(
        "name = \"Thicket Bruin\"\ncost = \"{1}{G}\"\ntype = \"Creature — Bear\"\npt = \"2/2\"",
    );
    real::refuse_real(&[c], &index()).unwrap();
}

#[test]
fn insight_reads_every_line_of_the_drone_or_says_where_it_stopped() {
    let reading = read::read(&drone());
    assert_eq!(reading.lines, 3);
    for u in &reading.unread {
        assert!(!u.at.is_empty());
    }
}
