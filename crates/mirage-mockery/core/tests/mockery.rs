//! A custom card renders, and Insight says what it reads of it.

use std::path::{Path, PathBuf};

use mockery_core::card::{Card, SplitKind};
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
    use mockery_core::card::{Color, Colors, Face, Frame};
    use mockery_core::rules::Rules;
    let open = |name: &str| Card::open(&here(&format!("tests/cards/{name}.card.toml"))).unwrap();
    let single = |card: &Card| -> Face {
        match card {
            Card::Single(face) => face.clone(),
            other => panic!("one face expected, not {other:?}"),
        }
    };
    let Card::Transform {
        front: saga,
        back: kaladin,
    } = open("the-cleansing-of-shinovar")
    else {
        panic!("The Cleansing of Shinovar transforms");
    };
    let class = single(&open("jedi-knight-class"));
    let station = single(&open("vindicator"));
    let land = single(&open("the-shattered-plains"));
    let walker = single(&open("jasnah-veristitalian"));
    let words = single(&open("bridge-four-windrunner"));
    assert!(matches!(saga.rules, Rules::Saga(_)));
    assert!(matches!(class.rules, Rules::Class(_)));
    assert!(matches!(station.rules, Rules::Station(_)));
    let Rules::Planeswalker(jasnah) = &walker.rules else {
        panic!("Jasnah is a planeswalker");
    };
    assert_eq!(jasnah.abilities().len(), 4);
    assert!(matches!(words.rules, Rules::Plain(_)));
    assert_eq!(land.frame(), Frame::Land(Colors::of_letters("G").unwrap()));
    assert_eq!(kaladin.frame(), Frame::Gold);
    assert!(kaladin.color_indicator.contains(Color::Red));
    let cards = [class, station, land, walker, words].map(Card::Single);
    let doc = render::compile(&cards, Layout::Cards).unwrap();
    assert_eq!(doc.pages().len(), cards.len());
}

#[test]
fn a_double_faced_card_is_two_pages_and_a_split_card_one() {
    let open = |name: &str| Card::open(&here(&format!("tests/cards/{name}.card.toml"))).unwrap();
    let transform = open("the-cleansing-of-shinovar");
    let modal = open("chunky-chasmfiend");
    let split = open("force-push-force-pull");
    assert!(matches!(modal, Card::Modal { .. }));
    assert_eq!(split.name(), "Force Push // Force Pull");
    assert_eq!(
        split.fuse(),
        Some("Fuse (You may cast one or both halves of this card from your hand.)")
    );
    let aftermath = open("chasm-run-gemhearts");
    let room = open("detention-block-trash-compactor");
    assert!(matches!(
        aftermath,
        Card::Split {
            kind: SplitKind::Aftermath,
            ..
        }
    ));
    assert!(matches!(
        room,
        Card::Split {
            kind: SplitKind::Room { reminder: Some(_) },
            ..
        }
    ));
    for (card, pages) in [
        (&transform, 2),
        (&modal, 2),
        (&split, 1),
        (&aftermath, 1),
        (&room, 1),
    ] {
        assert_eq!(card.images(), pages);
        let doc = render::compile(std::slice::from_ref(card), Layout::Cards).unwrap();
        assert_eq!(doc.pages().len(), pages, "{}", card.name());
    }
    // A sheet gives each face a slot: 2 + 2 + 1 of A4's nine.
    let doc = render::compile(&[transform, modal, split], Layout::Sheet(Sheet::default())).unwrap();
    assert_eq!(doc.pages().len(), 1);
}

#[test]
fn insight_reads_every_face() {
    let open = |name: &str| Card::open(&here(&format!("tests/cards/{name}.card.toml"))).unwrap();
    // The Saga's four chapters and Kaladin's two abilities: the reminder is
    // not an ability.
    assert_eq!(read::read(&open("the-cleansing-of-shinovar")).lines, 5);
    assert_eq!(read::read(&open("force-push-force-pull")).lines, 4);
}

#[test]
fn a_card_with_a_spell_inset_is_one_image() {
    let open = |name: &str| Card::open(&here(&format!("tests/cards/{name}.card.toml"))).unwrap();
    let adventure = open("skybreaker-initiate");
    let prepare = open("ardent-fabrialist");
    assert!(matches!(adventure, Card::Adventure { .. }));
    assert!(matches!(prepare, Card::Prepare { .. }));
    assert_eq!(adventure.name(), "Skybreaker Initiate // Full Lashing");
    for card in [&adventure, &prepare] {
        assert_eq!(card.images(), 1);
        let doc = render::compile(std::slice::from_ref(card), Layout::Cards).unwrap();
        assert_eq!(doc.pages().len(), 1, "{}", card.name());
    }
    // Insight reads the spell as a face of its own, as it is cast
    // (CR 715.3b): the card's two lines and the spell's one.
    assert_eq!(read::read(&prepare).lines, 3);
}

#[test]
fn a_battle_prints_sideways_at_the_card_size() {
    let siege = Card::open(&here("tests/cards/siege-of-narak.card.toml")).unwrap();
    let Card::Transform { front, back } = &siege else {
        panic!("a Siege transforms");
    };
    assert!(front.is_siege() && !back.is_battle());
    let doc = render::compile(std::slice::from_ref(&siege), Layout::Cards).unwrap();
    // Both images are the card as it is held, the front drawn on its side,
    // so a sheet cuts it as any other.
    for page in 0..2 {
        let png = render::png(&doc, page, 300.0).unwrap();
        let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
        assert_eq!((width, height), (744, 1039), "page {page}");
    }
}

#[test]
fn a_flip_card_is_one_card_and_a_meld_pair_four_images_of_card_size() {
    let open = |name: &str| Card::open(&here(&format!("tests/cards/{name}.card.toml"))).unwrap();
    let flip = open("herdazian-squire");
    let meld = open("corellian-smuggler");
    assert!(matches!(flip, Card::Flip { .. }));
    assert!(matches!(meld, Card::Meld { .. }));
    for (card, pages) in [(&flip, 1), (&meld, 4)] {
        assert_eq!(card.images(), pages);
        let doc = render::compile(std::slice::from_ref(card), Layout::Cards).unwrap();
        assert_eq!(doc.pages().len(), pages, "{}", card.name());
        // Every image is a card as it is held, so a sheet cuts each at
        // card size: a meld card's back holds its half of the melded card
        // on its side.
        for page in 0..pages {
            let png = render::png(&doc, page, 300.0).unwrap();
            let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
            assert_eq!((width, height), (744, 1039), "page {page}");
        }
    }
    // Each printed card of the pair is two slots, its back beside its front.
    let doc = render::compile(&[meld, flip], Layout::Sheet(Sheet::default())).unwrap();
    assert_eq!(doc.pages().len(), 1);
    // Insight reads every part: the melded card is a face of its own
    // (CR 712.8). The counterpart's "(Melds with …)" is reminder text, not
    // an ability.
    assert_eq!(read::read(&open("corellian-smuggler")).lines, 2 + 2 + 3);
}
