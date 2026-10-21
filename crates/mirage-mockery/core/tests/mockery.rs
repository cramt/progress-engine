//! A custom card renders, and Insight says what it reads of it.

use std::path::{Path, PathBuf};

use mockery_core::card::Card;
use mockery_core::read;
use mockery_core::render::{self, Layout, Paper, Sheet};

fn here(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn drone() -> Card {
    Card::open(&here("tests/cards/proofsheet-drone.card.toml")).unwrap()
}

fn card(toml: &str) -> Card {
    Card::parse(toml, Path::new(".")).unwrap()
}

#[test]
fn a_card_renders_to_a_trimmed_png_and_a_pdf() {
    let doc = render::compile(&[drone()], Layout::Cards).unwrap();
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
        let doc = render::compile(&seventeen, Layout::Sheet(sheet)).unwrap();
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
        render::compile(&nine, Layout::Sheet(sheet))
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
    let c = card("name = \"x\"\ncost = \"{K}\"\ntype = \"Instant\"");
    let err = render::compile(&[c], Layout::Cards).unwrap_err();
    assert!(format!("{err:#}").contains("no symbol {K}"), "{err:#}");
}

#[test]
fn insight_reads_every_line_of_the_drone_or_says_where_it_stopped() {
    let reading = read::read(&drone());
    assert_eq!(reading.lines, 3);
    for u in &reading.unread {
        assert!(!u.at.is_empty());
    }
}

#[test]
fn each_layout_the_text_asks_for_prints_on_one_page() {
    use mockery_core::card::{Color, Colors, Frame};
    use mockery_core::rules::Rules;
    let open = |name: &str| Card::open(&here(&format!("tests/cards/{name}.card.toml"))).unwrap();
    let saga = open("the-cleansing-of-shinovar");
    let class = open("jedi-knight-class");
    let station = open("vindicator");
    let land = open("the-shattered-plains");
    let kaladin = open("kaladin-herald-of-second-chances");
    assert!(matches!(saga.rules, Rules::Saga(_)));
    assert!(matches!(class.rules, Rules::Class(_)));
    assert!(matches!(station.rules, Rules::Station(_)));
    assert_eq!(land.frame(), Frame::Land(Colors::of_letters("G").unwrap()));
    assert_eq!(kaladin.frame(), Frame::Gold);
    assert!(kaladin.color_indicator.contains(Color::Red));
    let cards = [saga, class, station, land, kaladin];
    let doc = render::compile(&cards, Layout::Cards).unwrap();
    assert_eq!(doc.pages().len(), cards.len());
}
