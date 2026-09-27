//! Edits change what they say and nothing else in the file.

use chip_decklist::deck::{CardRef, CategoryType, Deck, DeckError, Finish, Printing};
use chip_decklist::edit::{
    add_card, card_comments, declare_category, new_deck, remove_card, set_card_finish,
    set_card_printing, set_card_qty, set_categories, set_commander, set_deck_meta, EditError,
};

const DECK: &str = r#"name = "Izzet Lessons"

# The lessons live in the sideboard.
cards = [
  { printing = "tla/46", in = ["learnboard", "tempo"] },  # Boomerang Basics
  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration
  { name = "Island", qty = 8 },
]

[categories]
learnboard = { type = "sideboard" }
tempo = {}
self-bounce = {}
"#;

fn changed_lines(a: &str, b: &str) -> Vec<(String, String)> {
    assert_eq!(a.lines().count(), b.lines().count(), "{b}");
    a.lines()
        .zip(b.lines())
        .filter(|(x, y)| x != y)
        .map(|(x, y)| (x.to_string(), y.to_string()))
        .collect()
}

/// The lines of `a` that `b` dropped, when `b` is `a` less some lines.
fn dropped_lines(a: &str, b: &str) -> Vec<String> {
    let mut rest = b.lines().peekable();
    let mut dropped = Vec::new();
    for line in a.lines() {
        if rest.peek() == Some(&line) {
            rest.next();
        } else {
            dropped.push(line.to_string());
        }
    }
    assert_eq!(rest.next(), None, "{b}");
    dropped
}

fn one(a: &str, b: &str) -> [(String, String); 1] {
    [(a.to_string(), b.to_string())]
}

#[test]
fn recategorising_a_card_is_a_one_line_diff_that_keeps_its_comment() {
    let edited = set_categories(DECK, 1, &["tempo".into(), "self-bounce".into()]).unwrap();
    assert_eq!(
        changed_lines(DECK, &edited),
        vec![(
            r#"  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration"#.into(),
            r#"  { printing = "msc/183", qty = 4, in = ["tempo", "self-bounce"] },  # Expressive Iteration"#.into(),
        )]
    );
}

#[test]
fn a_card_without_categories_gains_an_in_key() {
    let edited = set_categories(DECK, 2, &["tempo".into()]).unwrap();
    assert_eq!(
        changed_lines(DECK, &edited)[0].1,
        r#"  { name = "Island", qty = 8, in = ["tempo"] },"#
    );
}

#[test]
fn a_card_left_in_no_category_loses_its_in_key_and_keeps_its_spacing() {
    let edited = set_categories(DECK, 1, &[]).unwrap();
    assert_eq!(
        changed_lines(DECK, &edited),
        one(
            r#"  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration"#,
            r#"  { printing = "msc/183", qty = 4 },  # Expressive Iteration"#,
        )
    );
}

#[test]
fn an_edit_the_format_refuses_is_refused() {
    let err = set_categories(DECK, 0, &["nonsense".into()]).unwrap_err();
    assert!(matches!(err, EditError::Invalid(_)), "{err}");
    assert_eq!(
        set_categories(DECK, 9, &[]).unwrap_err(),
        EditError::NoCard(9)
    );
}

#[test]
fn declaring_a_category_appends_it_and_touches_nothing_else() {
    let edited = declare_category(DECK, "Maybe", Some(CategoryType::Maybeboard)).unwrap();
    assert!(edited.starts_with(DECK), "{edited}");
    assert!(
        edited.ends_with("Maybe = { type = \"maybeboard\" }\n"),
        "{edited}"
    );
    assert_eq!(declare_category(DECK, "tempo", None).unwrap(), DECK);
    assert!(matches!(
        declare_category(DECK, "tempo", Some(CategoryType::Sideboard)).unwrap_err(),
        EditError::DeclaredDifferently { .. }
    ));
}

#[test]
fn a_quantity_is_set_in_place_and_one_drops_the_key() {
    let edited = set_card_qty(DECK, 1, 3).unwrap();
    assert_eq!(
        changed_lines(DECK, &edited),
        one(
            r#"  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration"#,
            r#"  { printing = "msc/183", qty = 3, in = ["tempo"] },  # Expressive Iteration"#,
        )
    );
    let edited = set_card_qty(DECK, 0, 2).unwrap();
    assert_eq!(
        changed_lines(DECK, &edited),
        one(
            r#"  { printing = "tla/46", in = ["learnboard", "tempo"] },  # Boomerang Basics"#,
            r#"  { printing = "tla/46", qty = 2, in = ["learnboard", "tempo"] },  # Boomerang Basics"#,
        )
    );
    let edited = set_card_qty(DECK, 2, 1).unwrap();
    assert_eq!(
        changed_lines(DECK, &edited),
        one(
            r#"  { name = "Island", qty = 8 },"#,
            r#"  { name = "Island" },"#
        )
    );
    assert_eq!(set_card_qty(DECK, 0, 1).unwrap(), DECK);
    assert_eq!(set_card_qty(DECK, 9, 1).unwrap_err(), EditError::NoCard(9));
}

#[test]
fn removing_a_card_drops_its_line_and_its_comment() {
    for (index, line) in [
        (
            0,
            r#"  { printing = "tla/46", in = ["learnboard", "tempo"] },  # Boomerang Basics"#,
        ),
        (
            1,
            r#"  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration"#,
        ),
        (2, r#"  { name = "Island", qty = 8 },"#),
    ] {
        let edited = remove_card(DECK, index).unwrap();
        assert_eq!(dropped_lines(DECK, &edited), vec![line.to_string()]);
        assert_eq!(set_card_qty(DECK, index, 0).unwrap(), edited);
    }
    let two = "cards = [\n  { name = \"A\" },  # a\n  { printing = \"b/1\" },  # B\n]\n";
    assert_eq!(
        remove_card(two, 1).unwrap(),
        "cards = [\n  { name = \"A\" },  # a\n]\n"
    );
    assert_eq!(
        remove_card(two, 0).unwrap(),
        "cards = [\n  { printing = \"b/1\" },  # B\n]\n"
    );
    assert_eq!(
        remove_card("cards = [\n  { printing = \"b/1\" },  # B\n]\n", 0).unwrap(),
        "cards = [\n]\n"
    );
    assert_eq!(
        remove_card("cards = [{ name = \"A\" }, { name = \"B\" }]\n", 0).unwrap(),
        "cards = [{ name = \"B\" }]\n"
    );
    assert_eq!(remove_card(DECK, 3).unwrap_err(), EditError::NoCard(3));
}

#[test]
fn a_finish_is_one_key_and_nonfoil_is_its_absence() {
    let foil = set_card_finish(DECK, 1, Finish::Foil).unwrap();
    assert_eq!(
        changed_lines(DECK, &foil),
        one(
            r#"  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration"#,
            r#"  { printing = "msc/183", qty = 4, finish = "foil", in = ["tempo"] },  # Expressive Iteration"#,
        )
    );
    let etched = set_card_finish(&foil, 1, Finish::Etched).unwrap();
    assert_eq!(
        changed_lines(&foil, &etched),
        one(
            r#"  { printing = "msc/183", qty = 4, finish = "foil", in = ["tempo"] },  # Expressive Iteration"#,
            r#"  { printing = "msc/183", qty = 4, finish = "etched", in = ["tempo"] },  # Expressive Iteration"#,
        )
    );
    assert_eq!(set_card_finish(&foil, 1, Finish::Nonfoil).unwrap(), DECK);
    assert_eq!(set_card_finish(DECK, 1, Finish::Nonfoil).unwrap(), DECK);
}

#[test]
fn a_new_printing_replaces_the_old_and_a_name_becomes_the_comment() {
    let edited = set_card_printing(DECK, 1, "STA", "18").unwrap();
    assert_eq!(
        changed_lines(DECK, &edited),
        one(
            r#"  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration"#,
            r#"  { printing = "sta/18", qty = 4, in = ["tempo"] },  # Expressive Iteration"#,
        )
    );
    let edited = set_card_printing(DECK, 2, "fdn", "279").unwrap();
    assert_eq!(
        changed_lines(DECK, &edited),
        one(
            r#"  { name = "Island", qty = 8 },"#,
            r#"  { printing = "fdn/279", qty = 8 },  # Island"#,
        )
    );
    assert!(matches!(
        set_card_printing(DECK, 0, "tla", "").unwrap_err(),
        EditError::Invalid(DeckError::BadPrinting { .. })
    ));
}

#[test]
fn a_commander_joins_the_commander_category_first_and_leaves_the_sideboard() {
    // No commander category yet: one is declared, and the card's typed
    // sideboard category goes, since a commander is in the deck.
    let edited = set_commander(DECK, 0).unwrap();
    let lines = DECK.lines().count();
    let cards: String = edited
        .lines()
        .take(lines)
        .map(|l| format!("{l}\n"))
        .collect();
    let declared: String = edited
        .lines()
        .skip(lines)
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(
        changed_lines(DECK, &cards),
        one(
            r#"  { printing = "tla/46", in = ["learnboard", "tempo"] },  # Boomerang Basics"#,
            r#"  { printing = "tla/46", in = ["Commander", "tempo"] },  # Boomerang Basics"#,
        )
    );
    assert_eq!(declared, "Commander = { type = \"commander\" }\n");
    assert!(Deck::parse(&edited).unwrap().cards[0].is_commander());

    // A commander category that exists is the one joined.
    let declared = declare_category(DECK, "Cmdr", Some(CategoryType::Commander)).unwrap();
    let edited = set_commander(&declared, 2).unwrap();
    assert_eq!(
        changed_lines(&declared, &edited),
        one(
            r#"  { name = "Island", qty = 8 },"#,
            r#"  { name = "Island", qty = 8, in = ["Cmdr"] },"#,
        )
    );
    assert_eq!(set_commander(&edited, 2).unwrap(), edited);
}

#[test]
fn an_added_card_is_a_new_last_line_in_the_file_s_style() {
    let edited = add_card(
        DECK,
        &CardRef::Name("Sol Ring".into()),
        &["tempo".into()],
        None,
    )
    .unwrap();
    assert_eq!(
        edited,
        DECK.replace(
            "  { name = \"Island\", qty = 8 },\n",
            "  { name = \"Island\", qty = 8 },\n  { name = \"Sol Ring\", in = [\"tempo\"] },\n"
        )
    );

    // After a commented line, each comment stays beside its own card.
    let commented = "cards = [\n  { printing = \"b/1\" },  # B\n]\n";
    let printing = CardRef::Printing(Printing {
        set: "cmm".into(),
        num: "410".into(),
    });
    let edited = add_card(commented, &printing, &[], Some("Sol Ring")).unwrap();
    assert_eq!(
        edited,
        "cards = [\n  { printing = \"b/1\" },  # B\n  { printing = \"cmm/410\" },  # Sol Ring\n]\n"
    );

    // The same card in the same categories is one more of it.
    let again = add_card(&edited, &printing, &[], None).unwrap();
    assert_eq!(
        changed_lines(&edited, &again),
        one(
            "  { printing = \"cmm/410\" },  # Sol Ring",
            "  { printing = \"cmm/410\", qty = 2 },  # Sol Ring",
        )
    );

    // An undeclared category is refused, as every edit refuses.
    assert!(matches!(
        add_card(DECK, &printing, &["nope".into()], None).unwrap_err(),
        EditError::Invalid(DeckError::Undeclared { .. })
    ));
}

#[test]
fn a_new_deck_is_empty_and_takes_cards() {
    let text = new_deck("Izzet Lessons", "modern").unwrap();
    assert_eq!(
        text,
        "name = \"Izzet Lessons\"\nformat = \"modern\"\n\ncards = [\n]\n"
    );
    assert!(Deck::parse(&text).unwrap().cards.is_empty());
    assert_eq!(new_deck("X", "").unwrap(), "name = \"X\"\n\ncards = [\n]\n");
    let island = CardRef::Name("Island".into());
    assert_eq!(
        add_card(&text, &island, &[], None).unwrap(),
        "name = \"Izzet Lessons\"\nformat = \"modern\"\n\ncards = [\n  { name = \"Island\" },\n]\n"
    );
    assert_eq!(
        add_card("cards = []\n", &island, &[], None).unwrap(),
        "cards = [\n  { name = \"Island\" },\n]\n"
    );
}

#[test]
fn deck_meta_is_set_in_place_or_written_at_the_top() {
    // In place: only the name line changes, and an empty format is left be.
    let renamed = set_deck_meta(DECK, "Izzet Tempo", "").unwrap();
    assert_eq!(
        changed_lines(DECK, &renamed),
        one(r#"name = "Izzet Lessons""#, r#"name = "Izzet Tempo""#)
    );
    // A missing format goes under the name.
    let formatted = set_deck_meta(DECK, "Izzet Lessons", "modern").unwrap();
    assert_eq!(
        formatted,
        DECK.replacen(
            "name = \"Izzet Lessons\"\n",
            "name = \"Izzet Lessons\"\nformat = \"modern\"\n",
            1
        )
    );

    // A file with neither, as an Archidekt import writes, gains both at its
    // top and is otherwise the same bytes.
    let lantern = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../decks/lantern.deck.toml"
    ))
    .unwrap();
    let named = set_deck_meta(&lantern, "Lantern", "commander").unwrap();
    assert_eq!(
        named,
        format!("name = \"Lantern\"\nformat = \"commander\"\n\n{lantern}")
    );
    let deck = Deck::parse(&named).unwrap();
    assert_eq!(deck.name.as_deref(), Some("Lantern"));
    assert_eq!(deck.format.as_deref(), Some("commander"));
    assert_eq!(
        set_deck_meta("", "Empty", "").unwrap(),
        "name = \"Empty\"\n"
    );
}

#[test]
fn comments_are_read_per_card() {
    assert_eq!(
        card_comments(DECK),
        vec![
            Some("Boomerang Basics".to_string()),
            Some("Expressive Iteration".to_string()),
            None
        ]
    );
}

#[test]
fn every_card_edit_on_lantern_touches_one_line() {
    let lantern = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../decks/lantern.deck.toml"
    ))
    .unwrap();
    let n = Deck::parse(&lantern).unwrap().cards.len();
    for i in [0, n / 2, n - 1] {
        for edited in [
            set_card_qty(&lantern, i, 2).unwrap(),
            set_card_finish(&lantern, i, Finish::Foil).unwrap(),
            set_card_printing(&lantern, i, "sld", "1").unwrap(),
        ] {
            assert_eq!(changed_lines(&lantern, &edited).len(), 1, "{i}");
        }
        assert_eq!(
            dropped_lines(&lantern, &remove_card(&lantern, i).unwrap()).len(),
            1
        );
    }
}
