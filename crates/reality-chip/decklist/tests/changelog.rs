//! A save's commit message reads as the deck's changelog (ADR-0021, #124).

use chip_decklist::changelog::commit_message_for_text;
use chip_decklist::deck::{CardRef, CategoryType, Finish};
use chip_decklist::edit::{
    declare_category, remove_card, set_card_finish, set_card_printing, set_card_qty,
    set_categories, set_deck_cover, set_deck_description, AddTo, EditError,
};
use chip_decklist::identity::Names;

/// One more of `card` in exactly `categories`.
fn add_card(
    text: &str,
    card: &CardRef,
    categories: &[String],
    comment: Option<&str>,
) -> Result<String, EditError> {
    chip_decklist::edit::add_card(
        text,
        card,
        AddTo::Categories(categories),
        comment,
        &Names::new(),
    )
    .map(|added| added.text)
}

const PATH: &str = "decks/lantern.deck.toml";

const DECK: &str = r#"name = "Lantern"
format = "commander"

cards = [
  { printing = "cmm/410", in = ["Ramp"] },  # Sol Ring
  { printing = "c21/250", in = ["Ramp"] },  # Mind Stone
  { name = "Island", qty = 8 },
  { printing = "moc/94", in = ["Commander"] },
]

[categories]
Commander = { type = "commander" }
Draw = {}
Ramp = {}
"#;

fn message(after: &str) -> String {
    commit_message_for_text(DECK, after, PATH).unwrap()
}

fn name(n: &str) -> CardRef {
    CardRef::Name(n.into())
}

#[test]
fn the_adr_example_reads_as_written() {
    let text = remove_card(DECK, 1).unwrap();
    let text = set_categories(&text, 0, &["Draw".into()]).unwrap();
    let text = add_card(&text, &name("Arcane Signet"), &["Ramp".into()], None).unwrap();
    assert_eq!(
        message(&text),
        "lantern: +1 Arcane Signet, -1 Mind Stone, Sol Ring: ramp → draw"
    );
}

#[test]
fn each_kind_of_change_says_what_it_is() {
    let add = add_card(DECK, &name("Sol Ring"), &[], None).unwrap();
    assert_eq!(message(&add), "lantern: +1 Sol Ring");

    assert_eq!(
        message(&remove_card(DECK, 2).unwrap()),
        "lantern: -8 Island"
    );
    assert_eq!(
        message(&set_card_qty(DECK, 2, 10).unwrap()),
        "lantern: Island: 8 → 10"
    );
    assert_eq!(
        message(&set_categories(DECK, 2, &["Draw".into(), "Ramp".into()]).unwrap()),
        "lantern: Island: uncategorized → draw, ramp"
    );
    assert_eq!(
        message(&set_card_printing(DECK, 0, "ltc", "3").unwrap()),
        "lantern: Sol Ring: cmm/410 → ltc/3"
    );
    assert_eq!(
        message(&set_card_printing(DECK, 2, "fdn", "279").unwrap()),
        "lantern: Island: any printing → fdn/279"
    );
    assert_eq!(
        message(&set_card_finish(DECK, 0, Finish::Etched).unwrap()),
        "lantern: Sol Ring: nonfoil → etched"
    );
    assert_eq!(
        message(&declare_category(DECK, "Maybe", Some(CategoryType::Maybeboard)).unwrap()),
        "lantern: +category maybe (maybeboard)"
    );
    assert_eq!(
        message(&declare_category(DECK, "Tutors", None).unwrap()),
        "lantern: +category tutors"
    );
    assert_eq!(
        message(&DECK.replace("Draw = {}\n", "")),
        "lantern: -category draw"
    );
    assert_eq!(
        message(&DECK.replace("Draw = {}", "Draw = { type = \"sideboard\" }")),
        "lantern: category draw: untyped → sideboard"
    );
    assert_eq!(
        message(&DECK.replace("name = \"Lantern\"", "name = \"Lantern Control\"")),
        "lantern: name: \"Lantern\" → \"Lantern Control\""
    );
}

#[test]
fn a_cover_is_named_by_the_card_when_the_deck_names_it() {
    let sol_ring = set_deck_cover(DECK, Some("cmm/410")).unwrap();
    assert_eq!(message(&sol_ring), "lantern: cover: none → Sol Ring");
    assert_eq!(
        commit_message_for_text(
            &sol_ring,
            &set_deck_cover(DECK, Some("ltr/1")).unwrap(),
            PATH
        )
        .unwrap(),
        "lantern: cover: Sol Ring → ltr/1"
    );
}

#[test]
fn a_description_change_is_one_line_however_long_the_prose() {
    let described = set_deck_description(DECK, Some("Lock them out.\n")).unwrap();
    assert_eq!(message(&described), "lantern: description: added");
    assert_eq!(
        commit_message_for_text(
            &described,
            &set_deck_description(&described, Some("Lock them out, then win.\n")).unwrap(),
            PATH
        )
        .unwrap(),
        "lantern: description: rewritten"
    );
    assert_eq!(
        commit_message_for_text(&described, DECK, PATH).unwrap(),
        "lantern: description: removed"
    );
}

#[test]
fn a_printing_without_a_name_is_its_set_and_number() {
    assert_eq!(
        message(&remove_card(DECK, 3).unwrap()),
        "lantern: -1 moc/94"
    );
    // A name the new text gives it is used for the old one too.
    let named = DECK.replace(
        "{ printing = \"moc/94\", in = [\"Commander\"] },",
        "{ printing = \"moc/94\", in = [\"Commander\"] },  # Rashmi and Ragavan",
    );
    assert_eq!(
        message(&set_card_finish(&named, 3, Finish::Foil).unwrap()),
        "lantern: Rashmi and Ragavan: nonfoil → foil"
    );
}

#[test]
fn changes_are_ordered_by_kind_then_card() {
    let text = set_card_finish(DECK, 0, Finish::Foil).unwrap();
    let text = set_card_qty(&text, 2, 9).unwrap();
    let text = remove_card(&text, 1).unwrap();
    // Given out of order, both adds come first, alphabetically.
    let text = add_card(&text, &name("Talisman of Dominance"), &[], None).unwrap();
    let text = add_card(&text, &name("Arcane Signet"), &[], None).unwrap();
    let message = message(&text);
    let body: Vec<&str> = message.lines().skip(2).collect();
    assert_eq!(
        body,
        [
            "+1 Arcane Signet",
            "+1 Talisman of Dominance",
            "-1 Mind Stone",
            "Island: 8 → 9",
            "Sol Ring: nonfoil → foil",
        ]
    );
    // The same edits in another order make the same message.
    let other = add_card(DECK, &name("Arcane Signet"), &[], None).unwrap();
    let other = add_card(&other, &name("Talisman of Dominance"), &[], None).unwrap();
    let other = remove_card(&other, 1).unwrap();
    let other = set_card_qty(&other, 1, 9).unwrap();
    let other = set_card_finish(&other, 0, Finish::Foil).unwrap();
    assert_eq!(crate::message(&other), message);
}

#[test]
fn past_three_changes_the_subject_truncates_and_the_body_lists_every_one() {
    let mut text = DECK.to_string();
    for card in ["A", "B", "C", "D", "E"] {
        text = add_card(&text, &name(card), &[], None).unwrap();
    }
    assert_eq!(
        message(&text),
        "lantern: +1 A, +1 B, +1 C, and 2 more\n\n+1 A\n+1 B\n+1 C\n+1 D\n+1 E\n"
    );
    let three = add_card(DECK, &name("A"), &[], None).unwrap();
    let three = add_card(&three, &name("B"), &[], None).unwrap();
    let three = add_card(&three, &name("C"), &[], None).unwrap();
    assert_eq!(message(&three), "lantern: +1 A, +1 B, +1 C");
}

#[test]
fn a_change_of_formatting_alone_is_a_reformat() {
    let reformatted = DECK
        .replace("  # Sol Ring", "")
        .replace("{ name = \"Island\", qty = 8 }", "{name=\"Island\",qty=8}");
    assert_ne!(reformatted, DECK);
    assert_eq!(message(&reformatted), "lantern: reformat");
    assert_eq!(message(DECK), "lantern: reformat");
}

#[test]
fn the_subject_is_the_file_stem_lowercased() {
    assert_eq!(
        commit_message_for_text(DECK, DECK, "decks/Izzet-Lessons.deck.toml").unwrap(),
        "izzet-lessons: reformat"
    );
    assert_eq!(
        commit_message_for_text(DECK, DECK, "loam.toml").unwrap(),
        "loam: reformat"
    );
}

#[test]
fn a_first_save_is_every_card_added() {
    let message = commit_message_for_text("", DECK, PATH).unwrap();
    assert!(
        message.starts_with("lantern: +8 Island, +1 Mind Stone, +1 moc/94, and "),
        "{message}"
    );
}

#[test]
fn a_refused_deck_is_an_error_not_a_message() {
    assert!(commit_message_for_text(DECK, "cards = [{ qty = 1 }]", PATH).is_err());
}
