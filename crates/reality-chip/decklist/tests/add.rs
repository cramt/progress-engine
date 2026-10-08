//! A card added by name or by a scanned printing joins the line holding it,
//! by one rule for a deck and the collection alike.

use std::num::NonZeroU32;

use chip_decklist::collection::{self, Collection, CollectionError};
use chip_decklist::deck::{CardRef, Deck, Finish, Printing};
use chip_decklist::edit::{add_card, AddTo, Added, EditError};
use chip_decklist::identity::Names;

const QUICK: &str = r#"name = "Test"

cards = [
  { printing = "cmm/410", in = ["Ramp"] },
  { name = "Arcane Signet", in = ["Ramp"] },
  { name = "Counterspell", in = ["Maybe"] },
  { printing = "isd/51" },
]

[categories]
Ramp = {}
Draw = {}
Maybe = { type = "maybeboard" }
"#;

/// What the browser has from Scryfall for the printings the file names.
fn scryfall_names() -> Names {
    Names::from([
        ("cmm/410".to_string(), "Sol Ring".to_string()),
        (
            "isd/51".to_string(),
            "Delver of Secrets // Insectile Aberration".to_string(),
        ),
    ])
}

fn by_name(n: &str) -> CardRef {
    CardRef::Name(n.into())
}

fn quick_add(text: &str, n: &str, to: AddTo<'_>) -> Added {
    add_card(text, &by_name(n), to, None, &scryfall_names()).unwrap()
}

/// The lines of `after` that are not in `before`, and the reverse.
fn diff(before: &str, after: &str) -> (Vec<String>, Vec<String>) {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    (
        b.iter()
            .filter(|l| !a.contains(l))
            .map(|l| l.to_string())
            .collect(),
        a.iter()
            .filter(|l| !b.contains(l))
            .map(|l| l.to_string())
            .collect(),
    )
}

#[test]
fn a_card_added_by_name_joins_the_line_naming_its_printing() {
    let added = quick_add(QUICK, "Sol Ring", AddTo::Automatic);
    assert_eq!((added.line, added.made), (0, false));
    assert_eq!(
        diff(QUICK, &added.text),
        (
            vec![r#"  { printing = "cmm/410", qty = 2, in = ["Ramp"] },"#.to_string()],
            vec![r#"  { printing = "cmm/410", in = ["Ramp"] },"#.to_string()],
        )
    );
}

#[test]
fn a_card_added_to_a_category_joins_the_line_already_there_whatever_its_case() {
    let ramp = ["Ramp".to_string()];
    let added = quick_add(QUICK, "arcane signet", AddTo::Categories(&ramp));
    assert_eq!((added.line, added.made), (1, false));
    assert_eq!(
        diff(QUICK, &added.text).0,
        [r#"  { name = "Arcane Signet", qty = 2, in = ["Ramp"] },"#]
    );
}

#[test]
fn a_card_added_to_a_category_it_is_not_in_is_a_new_line_there() {
    let draw = ["Draw".to_string()];
    let added = quick_add(QUICK, "Sol Ring", AddTo::Categories(&draw));
    assert_eq!((added.line, added.made), (4, true));
    assert_eq!(
        diff(QUICK, &added.text),
        (
            vec![r#"  { name = "Sol Ring", in = ["Draw"] },"#.to_string()],
            vec![]
        )
    );
}

#[test]
fn a_new_card_added_automatically_is_in_no_category() {
    let added = quick_add(QUICK, "Brainstorm", AddTo::Automatic);
    assert_eq!((added.line, added.made), (4, true));
    assert_eq!(
        diff(QUICK, &added.text),
        (vec![r#"  { name = "Brainstorm" },"#.to_string()], vec![])
    );
}

#[test]
fn a_card_held_only_outside_the_deck_gains_the_copy_automatically() {
    let added = quick_add(QUICK, "Counterspell", AddTo::Automatic);
    assert_eq!((added.line, added.made), (2, false));
    assert_eq!(
        diff(QUICK, &added.text).0,
        [r#"  { name = "Counterspell", qty = 2, in = ["Maybe"] },"#]
    );
}

#[test]
fn automatically_a_line_in_the_deck_comes_before_one_outside_it() {
    let text = QUICK.replace(
        "  { printing = \"isd/51\" },\n",
        "  { printing = \"isd/51\" },\n  { name = \"Counterspell\" },\n",
    );
    let added = quick_add(&text, "Counterspell", AddTo::Automatic);
    assert_eq!((added.line, added.made), (4, false));
}

#[test]
fn a_double_faced_printing_is_held_by_its_front_face_name() {
    let added = quick_add(QUICK, "Delver of Secrets", AddTo::Automatic);
    assert_eq!((added.line, added.made), (3, false));
    assert!(added.text.contains(r#"{ printing = "isd/51", qty = 2 }"#));
}

#[test]
fn a_whole_double_faced_name_joins_the_line_naming_its_front_face() {
    let text = "cards = [\n  { name = \"Delver of Secrets\" },\n]\n";
    let added = quick_add(
        text,
        "Delver of Secrets // Insectile Aberration",
        AddTo::Automatic,
    );
    assert_eq!((added.line, added.made), (0, false));
}

#[test]
fn a_back_face_or_another_card_behind_the_same_front_is_a_new_line() {
    assert!(quick_add(QUICK, "Insectile Aberration", AddTo::Automatic).made);
    let text = "cards = [\n  { name = \"A // B\" },\n]\n";
    assert!(quick_add(text, "A // C", AddTo::Automatic).made);
}

#[test]
fn a_printing_s_comment_names_it_when_the_names_given_do_not() {
    let text = "cards = [\n  { printing = \"lea/232\" },  # Black Lotus\n]\n";
    let added = add_card(
        text,
        &by_name("Black Lotus"),
        AddTo::Automatic,
        None,
        &Names::new(),
    )
    .unwrap();
    assert_eq!((added.line, added.made), (0, false));
}

#[test]
fn a_card_added_by_name_never_joins_a_foil_line() {
    let text = "cards = [\n  { name = \"Sol Ring\", finish = \"foil\" },\n]\n";
    let added = quick_add(text, "Sol Ring", AddTo::Automatic);
    assert_eq!((added.line, added.made), (1, true));
}

const SCANNED: &str = r#"name = "Test"

cards = [
  { printing = "lea/232", in = ["Maybe"] },  # Black Lotus
  { printing = "m21/159", in = ["Burn"] },  # Shock
  { name = "Thoughtseize" },
]

[categories]
Burn = {}
Maybe = { type = "maybeboard" }
"#;

fn scan(text: &str, set: &str, num: &str, n: &str) -> Added {
    let printing = CardRef::Printing(Printing {
        set: set.to_ascii_lowercase(),
        num: num.into(),
    });
    add_card(text, &printing, AddTo::Automatic, Some(n), &Names::new()).unwrap()
}

#[test]
fn a_scanned_printing_adds_a_copy_to_the_line_that_already_names_it() {
    let added = scan(SCANNED, "M21", "159", "Shock");
    assert_eq!((added.line, added.made), (1, false));
    assert!(added
        .text
        .contains(r#"{ printing = "m21/159", qty = 2, in = ["Burn"] }"#));
}

#[test]
fn a_scanned_printing_the_deck_lacks_is_a_new_line_named_in_its_comment() {
    let added = scan(SCANNED, "ths", "107", "Thoughtseize");
    assert_eq!((added.line, added.made), (3, true));
    let card = &Deck::parse(&added.text).unwrap().cards[3];
    assert!(card.categories.is_empty() && card.in_deck());
    assert!(added.text.contains("# Thoughtseize"));
}

#[test]
fn a_scanned_printing_never_joins_a_line_by_name_or_of_another_printing() {
    assert!(
        scan(SCANNED, "ths", "107", "Thoughtseize").made,
        "joined the name-only line"
    );
    assert!(
        scan(SCANNED, "2ed", "233", "Black Lotus").made,
        "joined the other printing's line"
    );
}

const OWNED: &str = r#"cards = [
  { printing = "moc/94", at = "Lantern" },  # Rashmi and Ragavan
  { name = "Sol Ring", qty = 3, at = "Bulk" },
  { name = "Island", qty = 40 },
  { printing = "isd/51", at = "Bulk" },
  { printing = "lea/232", at = "Binder" },  # Black Lotus
]

[places]
Binder = {}
Bulk = {}
Lantern = { deck = "decks/lantern.deck.toml" }
"#;

fn one() -> NonZeroU32 {
    NonZeroU32::MIN
}

fn own(card: &CardRef, at: Option<&str>, finish: Finish) -> Added {
    collection::add(OWNED, card, one(), finish, at, None, &scryfall_names()).unwrap()
}

fn lotus() -> CardRef {
    CardRef::Printing(Printing {
        set: "lea".into(),
        num: "232".into(),
    })
}

#[test]
fn an_owned_card_added_by_name_joins_its_line_there_whatever_its_case() {
    let added = own(&by_name("sol ring"), Some("Bulk"), Finish::Nonfoil);
    assert_eq!((added.line, added.made), (1, false));
    assert_eq!(added.text, OWNED.replace("qty = 3", "qty = 4"));
}

#[test]
fn an_owned_card_added_by_name_joins_the_line_of_its_printing() {
    let added = own(
        &by_name("Rashmi and Ragavan"),
        Some("Lantern"),
        Finish::Nonfoil,
    );
    assert_eq!((added.line, added.made), (0, false));
    assert!(added
        .text
        .contains(r#"{ printing = "moc/94", qty = 2, at = "Lantern" },"#));
}

#[test]
fn an_owned_double_faced_printing_is_held_by_its_front_face_name() {
    let added = own(&by_name("Delver of Secrets"), Some("Bulk"), Finish::Nonfoil);
    assert_eq!((added.line, added.made), (3, false));
}

#[test]
fn an_owned_card_is_a_new_line_in_another_place_or_another_finish() {
    let unsorted = own(&by_name("Sol Ring"), None, Finish::Nonfoil);
    assert_eq!((unsorted.line, unsorted.made), (5, true));
    assert!(unsorted.text.contains("  { name = \"Sol Ring\" },\n]"));
    let foil = own(&lotus(), Some("Binder"), Finish::Foil);
    assert_eq!((foil.line, foil.made), (5, true));
    assert_eq!(
        Collection::parse(&foil.text).unwrap().cards[5]
            .at
            .as_deref(),
        Some("Binder")
    );
}

#[test]
fn a_scanned_copy_joins_the_line_of_its_printing_there_and_no_other() {
    let added = own(&lotus(), Some("Binder"), Finish::Nonfoil);
    assert_eq!((added.line, added.made), (4, false));
    assert!(added
        .text
        .contains(r#"{ printing = "lea/232", qty = 2, at = "Binder" }"#));
    let text = OWNED.replace(
        "  { name = \"Island\", qty = 40 },\n",
        "  { name = \"Island\", qty = 40 },\n  { name = \"Black Lotus\", at = \"Binder\" },\n",
    );
    let scanned = collection::add(
        &text,
        &lotus(),
        one(),
        Finish::Nonfoil,
        Some("Binder"),
        None,
        &Names::new(),
    )
    .unwrap();
    assert_eq!(scanned.line, 5, "joined the line by name");
}

#[test]
fn a_copy_taken_back_comes_off_the_line_it_went_on() {
    let added = own(&lotus(), Some("Binder"), Finish::Nonfoil);
    let back = collection::take(
        &added.text,
        &lotus(),
        one(),
        Finish::Nonfoil,
        Some("Binder"),
        &scryfall_names(),
    )
    .unwrap();
    assert_eq!(back, OWNED);
    let delver = own(&by_name("Delver of Secrets"), Some("Bulk"), Finish::Nonfoil);
    let back = collection::take(
        &delver.text,
        &CardRef::Name("Delver of Secrets".into()),
        one(),
        Finish::Nonfoil,
        Some("Bulk"),
        &scryfall_names(),
    )
    .unwrap();
    assert_eq!(back, OWNED);
}

#[test]
fn the_last_copy_taken_back_takes_its_line() {
    let back = collection::take(
        OWNED,
        &lotus(),
        one(),
        Finish::Nonfoil,
        Some("Binder"),
        &Names::new(),
    )
    .unwrap();
    assert!(!back.contains("lea/232"), "{back}");
}

#[test]
fn a_copy_no_longer_where_it_went_is_not_taken() {
    let err =
        collection::take(OWNED, &lotus(), one(), Finish::Nonfoil, None, &Names::new()).unwrap_err();
    assert_eq!(
        err,
        EditError::Collection(CollectionError::NoLine {
            card: lotus(),
            place: None
        })
    );
    assert!(
        err.to_string().contains("no line holds lea/232 unsorted"),
        "{err}"
    );
    let foil = collection::take(
        OWNED,
        &lotus(),
        one(),
        Finish::Foil,
        Some("Binder"),
        &Names::new(),
    );
    assert!(foil.is_err());
    let too_many = collection::take(
        OWNED,
        &lotus(),
        NonZeroU32::new(2).unwrap(),
        Finish::Nonfoil,
        Some("Binder"),
        &Names::new(),
    );
    assert!(matches!(
        too_many,
        Err(EditError::Collection(CollectionError::TooMany {
            have: 1,
            qty: 2,
            ..
        }))
    ));
}
