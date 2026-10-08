//! A diff between two decks, and taking any of its changes from one into the
//! other: what Curator's history, comparison and variants all stand on.

use chip_decklist::deck::{CardRef, CategoryType, Deck, Finish};
use chip_decklist::diff::{apply, parse_pair, Diff};
use chip_decklist::edit::{
    declare_category, remove_card, set_card_finish, set_card_printing, set_card_qty,
    set_categories, set_deck_cover, set_deck_description, set_deck_meta, set_variant_of, AddTo,
    EditError,
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

const DECK: &str = r#"name = "Lantern"
format = "commander"

cards = [
  { printing = "cmm/410", in = ["Ramp"] },  # Sol Ring
  { printing = "c21/250", in = ["Ramp"] },  # Mind Stone
  { name = "Island", qty = 8 },
  { printing = "moc/94", in = ["Commander"] },  # Rashmi and Ragavan
]

[categories]
Commander = { type = "commander" }
Draw = {}
Ramp = {}
"#;

fn deck_file(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/../../../decks/{name}.deck.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// Each change's line, as a commit message would say it.
fn lines(before: &str, after: &str) -> Vec<String> {
    let (b, a, names) = parse_pair(before, after).unwrap();
    Diff::new(&b, &a, |p| names.get(p).cloned())
        .changes
        .into_iter()
        .map(|(_, t)| t)
        .collect()
}

fn take_all(before: &str, after: &str) -> String {
    let n = lines(before, after).len();
    apply(before, after, &(0..n).collect::<Vec<_>>()).unwrap()
}

fn name(n: &str) -> CardRef {
    CardRef::Name(n.into())
}

/// `DECK` with one of every kind of change made to it.
fn everything_changed() -> String {
    let t = remove_card(DECK, 1).unwrap();
    let t = set_card_qty(&t, 1, 7).unwrap();
    let t = set_categories(&t, 0, &["Draw".into()]).unwrap();
    let t = set_card_finish(&t, 0, Finish::Foil).unwrap();
    let t = set_card_printing(&t, 1, "fdn", "279").unwrap();
    let t = declare_category(&t, "Wincons", None).unwrap();
    let t = declare_category(&t, "Sideboard", Some(CategoryType::Sideboard)).unwrap();
    let t = add_card(&t, &name("Arcane Signet"), &["Sideboard".into()], None).unwrap();
    let t = set_deck_meta(&t, "Lantern Control", "brawl").unwrap();
    let t = set_deck_cover(&t, Some("moc/94")).unwrap();
    let t = set_deck_description(&t, Some("Mill them out.")).unwrap();
    set_variant_of(&t, Some("decks/lantern.deck.toml")).unwrap()
}

#[test]
fn taking_every_change_leaves_nothing_to_take() {
    let after = everything_changed();
    assert_eq!(lines(DECK, &after).len(), 13, "{:#?}", lines(DECK, &after));
    let taken = take_all(DECK, &after);
    assert_eq!(lines(&taken, &after), Vec::<String>::new(), "{taken}");
    // And back the other way: restoring the old version.
    let restored = take_all(&after, DECK);
    assert_eq!(lines(&restored, DECK), Vec::<String>::new(), "{restored}");
}

#[test]
fn one_real_deck_becomes_the_other() {
    let (lantern, loam) = (deck_file("lantern"), deck_file("loam"));
    let taken = take_all(&lantern, &loam);
    assert_eq!(lines(&taken, &loam), Vec::<String>::new());
    let back = take_all(&taken, &lantern);
    assert_eq!(lines(&back, &lantern), Vec::<String>::new());
}

#[test]
fn taking_nothing_changes_nothing() {
    assert_eq!(apply(DECK, &everything_changed(), &[]).unwrap(), DECK);
}

#[test]
fn taking_one_card_back_touches_that_card_alone() {
    let later = remove_card(DECK, 1).unwrap();
    let later = set_card_qty(&later, 1, 9).unwrap();
    // Last week's deck had Mind Stone; bring just that back.
    let changes = lines(&later, DECK);
    let i = changes.iter().position(|l| l == "+1 Mind Stone").unwrap();
    let text = apply(&later, DECK, &[i]).unwrap();
    assert!(
        text.contains("{ printing = \"c21/250\", in = [\"Ramp\"] },  # Mind Stone"),
        "{text}"
    );
    assert_eq!(lines(&text, DECK), vec!["Island: 9 → 8"]);
}

#[test]
fn a_card_whose_category_is_missing_brings_the_category() {
    let other = declare_category(DECK, "Sideboard", Some(CategoryType::Sideboard)).unwrap();
    let other = add_card(&other, &name("Pithing Needle"), &["Sideboard".into()], None).unwrap();
    let changes = lines(DECK, &other);
    let i = changes
        .iter()
        .position(|l| l == "+1 Pithing Needle")
        .unwrap();
    let text = apply(DECK, &other, &[i]).unwrap();
    let deck = Deck::parse(&text).unwrap();
    assert_eq!(
        deck.category("Sideboard").and_then(|c| c.kind),
        Some(CategoryType::Sideboard)
    );
    assert_eq!(
        deck.cards.last().map(|c| c.place),
        Some(CategoryType::Sideboard)
    );
}

#[test]
fn dropping_a_category_a_card_still_sits_in_is_refused() {
    let other = remove_card(DECK, 1).unwrap();
    let other = set_categories(&other, 0, &[]).unwrap();
    let other = other.replace("Ramp = {}\n", "");
    let changes = lines(DECK, &other);
    let i = changes.iter().position(|l| l == "-category ramp").unwrap();
    assert_eq!(
        apply(DECK, &other, &[i]),
        Err(EditError::StillUsed("Ramp".into()))
    );
    // With the cards that were in it, it can go.
    assert_eq!(lines(&take_all(DECK, &other), &other), Vec::<String>::new());
}

#[test]
fn a_printing_given_up_for_any_printing_says_the_name() {
    let other = DECK.replace(
        "{ printing = \"cmm/410\", in = [\"Ramp\"] },  # Sol Ring",
        "{ name = \"Sol Ring\", in = [\"Ramp\"] },",
    );
    assert_eq!(
        lines(DECK, &other),
        vec!["Sol Ring: cmm/410 → any printing"]
    );
    let text = take_all(DECK, &other);
    assert!(
        text.contains("  { name = \"Sol Ring\", in = [\"Ramp\"] },\n"),
        "{text}"
    );
}

#[test]
fn a_variant_names_its_parent_and_can_stand_alone() {
    let variant = set_variant_of(DECK, Some("decks/lantern.deck.toml")).unwrap();
    assert!(
        variant.starts_with(
            "name = \"Lantern\"\nformat = \"commander\"\nvariant_of = \"decks/lantern.deck.toml\"\n"
        ),
        "{variant}"
    );
    assert_eq!(
        Deck::parse(&variant).unwrap().variant_of.as_deref(),
        Some("decks/lantern.deck.toml")
    );
    assert_eq!(lines(DECK, &variant), vec!["variant of: none → lantern"]);
    assert_eq!(set_variant_of(&variant, None).unwrap(), DECK);
}
