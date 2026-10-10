//! The wanted list: cards wanted by hand, and the ones the decks hold that
//! the collection does not.

use std::num::NonZeroU32;

use chip_decklist::collection::Collection;
use chip_decklist::deck::{CardRef, Deck, Finish, Printing};
use chip_decklist::identity::Names;
use chip_decklist::wanted::{
    self, commit_message_for_text, missing, owned, set_deck_copies, set_qty, DeckCopies, Missing,
    Wanted, WantedError,
};

fn n(qty: u32) -> NonZeroU32 {
    NonZeroU32::new(qty).unwrap()
}

fn name(n: &str) -> CardRef {
    CardRef::Name(n.to_string())
}

fn add(text: &str, card: &CardRef, qty: u32, finish: Finish) -> String {
    wanted::add(text, card, n(qty), finish, None, &Names::new())
        .unwrap()
        .text
}

const WANTED: &str = r#"cards = [
  { name = "The One Ring" },
  { printing = "ltr/451", finish = "foil" },  # The One Ring
]
"#;

#[test]
fn a_want_is_a_collection_line_without_a_place() {
    let w = Wanted::parse(WANTED).unwrap();
    assert_eq!(w.deck_copies, DeckCopies::Each);
    assert_eq!(w.cards.len(), 2);
    assert_eq!(w.cards[1].finish, Finish::Foil);
    assert!(Wanted::parse(r#"cards = [{ name = "X", at = "Bulk" }]"#).is_err());
}

#[test]
fn the_empty_text_wants_nothing() {
    assert_eq!(Wanted::parse("").unwrap(), Wanted::default());
}

#[test]
fn deck_copies_is_each_or_shared() {
    let w = Wanted::parse("deck_copies = \"shared\"\n").unwrap();
    assert_eq!(w.deck_copies, DeckCopies::Shared);
    assert_eq!(
        Wanted::parse("deck_copies = \"some\"\n"),
        Err(WantedError::DeckCopies("some".into()))
    );
}

#[test]
fn adding_joins_the_line_already_wanting_it() {
    let text = add(WANTED, &name("The One Ring"), 2, Finish::Nonfoil);
    assert!(
        text.contains(r#"{ name = "The One Ring", qty = 3 },"#),
        "{text}"
    );
    let text = add("", &name("Sol Ring"), 1, Finish::Etched);
    assert_eq!(
        text,
        "cards = [\n  { name = \"Sol Ring\", finish = \"etched\" },\n]\n"
    );
}

#[test]
fn zero_drops_the_line_and_its_comment() {
    let text = set_qty(WANTED, 1, 0).unwrap();
    assert_eq!(text, "cards = [\n  { name = \"The One Ring\" },\n]\n");
}

#[test]
fn shared_is_written_above_the_cards_and_each_is_no_key() {
    let shared = set_deck_copies(WANTED, DeckCopies::Shared).unwrap();
    assert!(
        shared.starts_with("deck_copies = \"shared\"\ncards = ["),
        "{shared}"
    );
    assert_eq!(set_deck_copies(&shared, DeckCopies::Each).unwrap(), WANTED);
}

#[test]
fn the_changelog_counts_copies_and_the_setting() {
    let after = set_deck_copies(
        &add(
            &set_qty(WANTED, 1, 0).unwrap(),
            &name("Sol Ring"),
            2,
            Finish::Nonfoil,
        ),
        DeckCopies::Shared,
    )
    .unwrap();
    assert_eq!(
        commit_message_for_text(WANTED, &after, "wanted.toml").unwrap(),
        "wanted: +2 Sol Ring, -1 The One Ring (foil), deck copies: each → shared"
    );
}

const COLLECTION: &str = r#"cards = [
  { name = "Sol Ring", at = "Lantern" },
  { printing = "2xm/270", finish = "foil" },  # Mana Crypt
  { name = "Island", qty = 2 },
]

[places]
Lantern = { deck = "decks/lantern.deck.toml" }
"#;

fn deck(path: &str, text: &str) -> (String, Deck, String) {
    (
        path.to_string(),
        Deck::parse(text).unwrap(),
        text.to_string(),
    )
}

fn decks() -> Vec<(String, Deck, String)> {
    vec![
        deck(
            "decks/lantern.deck.toml",
            r#"cards = [
  { name = "Sol Ring" },
  { name = "Mana Crypt" },
  { name = "Rhystic Study" },
  { name = "Island", qty = 30 },
  { name = "Snow-Covered Forest", qty = 4 },
  { name = "Cyclonic Rift", in = ["Maybe"] },
]

[categories]
Maybe = { type = "maybeboard" }
"#,
        ),
        deck(
            "decks/loam.deck.toml",
            r#"cards = [
  { printing = "c21/263" },  # Sol Ring
  { name = "Rhystic Study", in = ["Side"] },
]

[categories]
Side = { type = "sideboard" }
"#,
        ),
        deck(
            "decks/loam-budget.deck.toml",
            r#"variant_of = "decks/loam.deck.toml"
cards = [
  { name = "Sol Ring" },
  { name = "Rhystic Study", qty = 2 },
]
"#,
        ),
    ]
}

fn report(copies: DeckCopies) -> Vec<Missing> {
    let c = Collection::parse(COLLECTION).unwrap();
    missing(&c, COLLECTION, &decks(), &Names::new(), copies)
}

#[test]
fn each_build_needs_its_own_copies_and_a_variant_is_its_parents_build() {
    // Sol Ring: Lantern 1 + Loam's build 1 (Loam and its budget variant one
    // build), owned 1. Rhystic Study: Lantern 1 + the Loam build's most, 2.
    assert_eq!(
        report(DeckCopies::Each),
        vec![
            Missing {
                name: "Rhystic Study".into(),
                missing: 3,
                owned: 0,
                decks: vec![
                    ("decks/lantern.deck.toml".into(), 1),
                    ("decks/loam.deck.toml".into(), 1),
                    ("decks/loam-budget.deck.toml".into(), 2),
                ],
            },
            Missing {
                name: "Sol Ring".into(),
                missing: 1,
                owned: 1,
                decks: vec![
                    ("decks/lantern.deck.toml".into(), 1),
                    ("decks/loam.deck.toml".into(), 1),
                    ("decks/loam-budget.deck.toml".into(), 1),
                ],
            },
        ]
    );
}

#[test]
fn shared_copies_need_only_the_hungriest_deck() {
    let r = report(DeckCopies::Shared);
    assert_eq!(
        r.iter()
            .map(|m| (m.name.as_str(), m.missing))
            .collect::<Vec<_>>(),
        vec![("Rhystic Study", 2)]
    );
}

#[test]
fn basics_the_maybeboard_and_a_foil_owned_elsewhere_are_not_missing() {
    let r = report(DeckCopies::Each);
    for gone in [
        "Island",
        "Snow-Covered Forest",
        "Cyclonic Rift",
        "Mana Crypt",
    ] {
        assert!(r.iter().all(|m| m.name != gone), "{gone} in {r:?}");
    }
}

#[test]
fn owned_counts_every_printing_of_the_name() {
    let c = Collection::parse(COLLECTION).unwrap();
    let crypt = CardRef::Printing(Printing {
        set: "ema".into(),
        num: "225".into(),
    });
    let names: Names = [("ema/225".to_string(), "Mana Crypt".to_string())].into();
    assert_eq!(owned(&c, COLLECTION, &crypt, &names), 1);
    assert_eq!(owned(&c, COLLECTION, &name("Island"), &names), 2);
    assert_eq!(owned(&c, COLLECTION, &name("Black Lotus"), &names), 0);
}
