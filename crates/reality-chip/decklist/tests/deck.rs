//! The `.deck.toml` format, from ADR-0020's own examples outwards.

use chip_decklist::deck::{CardRef, CategoryType, Deck, DeckError, Finish, Printing};

const LESSONS: &str = r#"
name = "Izzet Lessons"
format = "modern"

cards = [
  { printing = "tla/46", in = ["learnboard", "self-bounce", "enemy-bounce", "tempo"] },  # Boomerang Basics
  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration
  { name = "Lightning Bolt", qty = 4, finish = "foil", in = ["tempo"] },
  { name = "Island", qty = 8 },
]

[categories]
learnboard   = { type = "sideboard" }
self-bounce  = {}
enemy-bounce = {}
tempo        = {}
"#;

fn deck(cards: &str, categories: &str) -> Result<Deck, DeckError> {
    Deck::parse(&format!(
        "cards = [\n{cards}\n]\n[categories]\n{categories}"
    ))
}

#[test]
fn the_adr_example_reads_as_written() {
    let deck = Deck::parse(LESSONS).unwrap();
    assert_eq!(deck.name.as_deref(), Some("Izzet Lessons"));
    assert_eq!(deck.format.as_deref(), Some("modern"));

    let [basics, iteration, bolt, island] = &deck.cards[..] else {
        panic!("four cards: {:?}", deck.cards);
    };
    assert_eq!(
        basics.card,
        CardRef::Printing(Printing {
            set: "tla".into(),
            num: "46".into()
        })
    );
    assert_eq!(basics.place, CategoryType::Sideboard);
    assert!(!basics.in_deck());
    assert_eq!(iteration.qty.get(), 4);
    assert!(iteration.in_deck());
    assert_eq!(bolt.card, CardRef::Name("Lightning Bolt".into()));
    assert_eq!(bolt.finish, Finish::Foil);
    assert_eq!(
        island.place,
        CategoryType::InDeck,
        "no categories is in the deck"
    );
}

#[test]
fn a_companion_can_also_be_a_recursion_piece() {
    let deck = deck(
        r#"{ name = "Lurrus of the Dream-Den", in = ["recursion", "companion"] },"#,
        "companion = { type = \"companion\" }\nrecursion = {}\nside = { type = \"sideboard\" }",
    )
    .unwrap();
    assert_eq!(deck.cards[0].place, CategoryType::Companion);
    assert!(CategoryType::Companion.is_within(CategoryType::NotInDeck));
}

#[test]
fn types_on_one_path_resolve_to_the_deepest() {
    let deck = deck(
        r#"{ name = "Lurrus of the Dream-Den", in = ["side", "companion"] },"#,
        "companion = { type = \"companion\" }\nside = { type = \"sideboard\" }",
    )
    .unwrap();
    assert_eq!(deck.cards[0].place, CategoryType::Companion);
}

#[test]
fn a_card_in_two_places_is_refused_by_name() {
    let err = deck(
        r#"{ name = "Rashmi and Ragavan", in = ["commander", "side"] },"#,
        "commander = { type = \"commander\" }\nside = { type = \"sideboard\" }",
    )
    .unwrap_err();
    assert!(matches!(err, DeckError::TwoPlaces { .. }), "{err}");
    assert!(err.to_string().contains("Rashmi and Ragavan"), "{err}");
}

#[test]
fn a_card_is_named_exactly_once() {
    let twice = deck(r#"{ name = "Sol Ring", printing = "cmr/472" },"#, "").unwrap_err();
    assert_eq!(twice, DeckError::NamedTwice { index: 1 });
    let never = deck(r#"{ qty = 2 },"#, "").unwrap_err();
    assert_eq!(never, DeckError::Unnamed { index: 1 });
}

#[test]
fn a_printing_is_set_slash_number() {
    let err = deck(r#"{ printing = "cmr472" },"#, "").unwrap_err();
    assert!(matches!(err, DeckError::BadPrinting { .. }), "{err}");
    let deck = deck(r#"{ printing = "PLST/JOU-35" },"#, "").unwrap();
    assert_eq!(deck.cards[0].card.to_string(), "plst/JOU-35");
}

#[test]
fn an_undeclared_category_is_a_typo_not_a_new_category() {
    let err = deck(r#"{ name = "Snap", in = ["tmepo"] },"#, "tempo = {}").unwrap_err();
    assert!(matches!(err, DeckError::Undeclared { .. }), "{err}");
}

#[test]
fn unknown_types_keys_and_finishes_are_refused() {
    assert!(matches!(
        deck("", "side = { type = \"sidebaord\" }").unwrap_err(),
        DeckError::UnknownType { .. }
    ));
    assert!(matches!(
        deck(r#"{ name = "Snap", categroy = "x" },"#, "").unwrap_err(),
        DeckError::Toml(_)
    ));
    assert!(matches!(
        deck(r#"{ name = "Snap", finish = "shiny" },"#, "").unwrap_err(),
        DeckError::BadFinish { .. }
    ));
    assert!(matches!(
        deck(r#"{ name = "Snap", qty = 0 },"#, "").unwrap_err(),
        DeckError::ZeroQty { .. }
    ));
}

/// The committed decks as Archidekt exported them, kept as import fixtures
/// when `decks/` moved to `.deck.toml`.
fn archidekt(deck: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{deck}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// Gauntlet decided what is in the library with `is_commander` and
/// `is_outside`. An imported deck must put every card of both committed decks
/// in the same place, or migrating them would move a number.
#[test]
fn importing_archidekt_places_every_card_where_gauntlet_did() {
    for name in ["lantern.txt", "loam.txt"] {
        let text = archidekt(name);
        let old = chip_decklist::parse(&text).unwrap();
        let new = Deck::from_archidekt(&text).unwrap();
        assert_eq!(old.len(), new.cards.len(), "{name}");
        for (o, n) in old.iter().zip(&new.cards) {
            assert_eq!(o.is_commander(), n.is_commander(), "{name}: {}", o.name);
            assert_eq!(o.is_outside(), !n.in_deck(), "{name}: {}", o.name);
            assert_eq!(o.qty, n.qty, "{name}: {}", o.name);
            let names: Vec<&str> = o.categories.iter().map(|c| c.name.as_str()).collect();
            assert_eq!(names, n.categories, "{name}: {}", o.name);
        }
    }
}

#[test]
fn writing_a_deck_reads_back_as_the_same_deck() {
    for deck in [
        Deck::parse(LESSONS).unwrap(),
        Deck::from_archidekt(&archidekt("lantern.txt")).unwrap(),
        Deck::from_archidekt(&archidekt("loam.txt")).unwrap(),
    ] {
        let text = deck.to_toml(|p| Some(format!("name of {p}")));
        assert_eq!(Deck::parse(&text).unwrap(), deck, "{text}");
    }
}

#[test]
fn a_description_is_markdown_written_as_prose_and_read_back_whole() {
    let mut deck = Deck::parse(LESSONS).unwrap();
    let description = "# Plan\n\nBounce *their* stuff, \"learn\" \\ draw.\n\n- tab:\there\n- \"\"\"quotes\"\"\"\n";
    deck.description = Some(description.into());
    let text = deck.to_toml(|_| None);
    assert!(
        text.contains("description = \"\"\"\n# Plan\n\nBounce"),
        "{text}"
    );
    assert_eq!(
        Deck::parse(&text).unwrap().description.as_deref(),
        Some(description)
    );
}

#[test]
fn a_printing_carries_its_name_as_a_comment_only() {
    let deck = Deck::parse(LESSONS).unwrap();
    let text = deck.to_toml(|p| (p.to_string() == "tla/46").then(|| "Boomerang Basics".into()));
    assert!(
        text.contains(r#"  { printing = "tla/46", in = ["learnboard", "self-bounce", "enemy-bounce", "tempo"] },  # Boomerang Basics"#),
        "{text}"
    );
    assert!(
        text.contains("learnboard = { type = \"sideboard\" }"),
        "{text}"
    );
}
