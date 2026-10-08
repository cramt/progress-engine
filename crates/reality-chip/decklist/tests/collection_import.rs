//! An import into the collection: thousands of rows as one edit, joining the
//! lines that already hold each card, or replacing what the places it fills
//! held.

use std::num::NonZeroU32;

use chip_decklist::collection::{commit_message_for_text, import, Collection, Incoming, Merge};
use chip_decklist::deck::{CardRef, Finish, Printing};
use chip_decklist::identity::Names;

const COLLECTION: &str = r#"cards = [
  { printing = "moc/94", at = "Lantern" },  # Rashmi and Ragavan
  { name = "Sol Ring", qty = 3, at = "Bulk" },
  { name = "Island", qty = 40 },
]

[places]
Bulk = {}
Lantern = { deck = "decks/lantern.deck.toml" }
"#;

fn row(card: CardRef, name: Option<&str>, qty: u32, finish: Finish, at: Option<&str>) -> Incoming {
    Incoming {
        card,
        name: name.map(str::to_string),
        qty: NonZeroU32::new(qty).unwrap(),
        finish,
        at: at.map(str::to_string),
    }
}

fn printing(set: &str, num: &str) -> CardRef {
    CardRef::Printing(Printing {
        set: set.into(),
        num: num.into(),
    })
}

fn name(n: &str) -> CardRef {
    CardRef::Name(n.into())
}

#[test]
fn an_import_joins_lines_holding_the_card_alike_and_appends_the_rest() {
    let rows = [
        row(name("Sol Ring"), None, 2, Finish::Nonfoil, Some("Bulk")),
        row(
            printing("2xm", "270"),
            Some("Mana Crypt"),
            1,
            Finish::Foil,
            Some("Bulk"),
        ),
        row(
            printing("2xm", "270"),
            Some("Mana Crypt"),
            1,
            Finish::Foil,
            Some("Bulk"),
        ),
        row(name("Island"), None, 5, Finish::Nonfoil, None),
    ];
    let text = import(COLLECTION, &rows, Merge::Add, &Names::new()).unwrap();
    assert_eq!(
        text,
        COLLECTION
            .replace("qty = 3", "qty = 5")
            .replace("qty = 40", "qty = 45")
            .replace(
                "  { name = \"Island\", qty = 45 },\n",
                "  { name = \"Island\", qty = 45 },\n  { printing = \"2xm/270\", qty = 2, finish = \"foil\", at = \"Bulk\" },  # Mana Crypt\n"
            ),
    );
}

#[test]
fn a_name_joins_the_line_of_its_printing_by_the_comment_beside_it() {
    let rows = [row(
        name("Rashmi and Ragavan"),
        None,
        1,
        Finish::Nonfoil,
        Some("Lantern"),
    )];
    let text = import(COLLECTION, &rows, Merge::Add, &Names::new()).unwrap();
    assert!(
        text.contains(
            "{ printing = \"moc/94\", qty = 2, at = \"Lantern\" },  # Rashmi and Ragavan"
        ),
        "{text}"
    );
}

#[test]
fn a_place_the_file_lacks_is_declared() {
    let rows = [row(
        name("Sol Ring"),
        None,
        1,
        Finish::Nonfoil,
        Some("Trade binder"),
    )];
    let text = import(COLLECTION, &rows, Merge::Add, &Names::new()).unwrap();
    let c = Collection::parse(&text).unwrap();
    assert!(c.place("Trade binder").is_some_and(|p| p.deck.is_none()));
    assert_eq!(c.qty_at(Some("Trade binder")), 1);
    let first = import("", &rows, Merge::Add, &Names::new()).unwrap();
    assert_eq!(
        Collection::parse(&first)
            .unwrap()
            .qty_at(Some("Trade binder")),
        1
    );
}

#[test]
fn replacing_makes_each_filled_place_hold_what_came_and_leaves_the_rest() {
    let rows = [
        row(name("Sol Ring"), None, 1, Finish::Nonfoil, Some("Bulk")),
        row(
            name("Arcane Signet"),
            None,
            1,
            Finish::Nonfoil,
            Some("Bulk"),
        ),
    ];
    let text = import(COLLECTION, &rows, Merge::Replace, &Names::new()).unwrap();
    let c = Collection::parse(&text).unwrap();
    assert_eq!(c.qty_at(Some("Bulk")), 2);
    assert_eq!(c.qty_at(Some("Lantern")), 1);
    assert_eq!(c.qty_at(None), 40);
    assert!(
        text.contains("  { name = \"Sol Ring\", at = \"Bulk\" },\n"),
        "{text}"
    );
    assert_eq!(
        commit_message_for_text(COLLECTION, &text, "collection.toml").unwrap(),
        commit_message_for_text(
            COLLECTION,
            &COLLECTION
                .replace("qty = 3, ", "")
                .replace("  { name = \"Island\", qty = 40 },\n", "  { name = \"Island\", qty = 40 },\n  { name = \"Arcane Signet\", at = \"Bulk\" },\n"),
            "collection.toml"
        )
        .unwrap()
    );
}

#[test]
fn replacing_with_the_same_export_again_changes_nothing() {
    let rows = [
        row(name("Sol Ring"), None, 3, Finish::Nonfoil, Some("Bulk")),
        row(
            printing("moc", "94"),
            Some("Rashmi and Ragavan"),
            1,
            Finish::Nonfoil,
            Some("Lantern"),
        ),
    ];
    assert_eq!(
        import(COLLECTION, &rows, Merge::Replace, &Names::new()).unwrap(),
        COLLECTION
    );
}

#[test]
fn replacing_empties_a_place_only_when_the_import_fills_it() {
    let rows = [row(
        name("Sol Ring"),
        None,
        1,
        Finish::Nonfoil,
        Some("Lantern"),
    )];
    let text = import(COLLECTION, &rows, Merge::Replace, &Names::new()).unwrap();
    let c = Collection::parse(&text).unwrap();
    assert_eq!(c.qty_at(Some("Lantern")), 1);
    assert_eq!(c.qty_at(Some("Bulk")), 3);
    assert!(!text.contains("moc/94"), "{text}");
}
