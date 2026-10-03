//! The collection: what is owned, where each copy is, and edits that change
//! what they say and nothing else in the file.

use chip_decklist::collection::{
    add, commit_message_for_text, declare_place, move_cards, move_lines, remove, rename_place,
    reprint, set_finish, set_printing, set_qty, undeclare_place, Collection, CollectionError,
};
use chip_decklist::deck::{CardRef, DeckError, Finish, Printing};
use chip_decklist::edit::EditError;

const COLLECTION: &str = r#"# What I own.
cards = [
  { printing = "moc/94", at = "Lantern" },  # Rashmi and Ragavan
  { name = "Sol Ring", qty = 3, at = "Bulk" },
  { name = "Island", qty = 40 },
]

[places]
Bulk = {}
"Trade binder" = {}
Lantern = { deck = "decks/lantern.deck.toml" }
"#;

fn name(n: &str) -> CardRef {
    CardRef::Name(n.into())
}

fn changed_lines(a: &str, b: &str) -> Vec<(String, String)> {
    assert_eq!(a.lines().count(), b.lines().count(), "{b}");
    a.lines()
        .zip(b.lines())
        .filter(|(x, y)| x != y)
        .map(|(x, y)| (x.to_string(), y.to_string()))
        .collect()
}

#[test]
fn a_card_is_in_one_place_or_unsorted() {
    let c = Collection::parse(COLLECTION).unwrap();
    assert_eq!(c.cards.len(), 3);
    assert_eq!(c.cards[0].at.as_deref(), Some("Lantern"));
    assert_eq!(
        c.cards[0].card,
        CardRef::Printing(Printing {
            set: "moc".into(),
            num: "94".into()
        })
    );
    assert_eq!(c.cards[2].at, None);
    assert_eq!(c.qty_at(Some("Bulk")), 3);
    assert_eq!(c.qty_at(None), 40);
    assert_eq!(
        c.place("Lantern").unwrap().deck.as_deref(),
        Some("decks/lantern.deck.toml")
    );
    assert_eq!(c.place("Bulk").unwrap().deck, None);
}

#[test]
fn the_empty_text_is_the_empty_collection() {
    assert_eq!(Collection::parse("").unwrap(), Collection::default());
}

#[test]
fn a_place_must_be_declared_and_a_card_line_is_a_decks() {
    let err = Collection::parse(r#"cards = [{ name = "Sol Ring", at = "Bulk" }]"#).unwrap_err();
    assert!(
        matches!(&err, CollectionError::Undeclared { place, .. } if place == "Bulk"),
        "{err}"
    );
    let err =
        Collection::parse(r#"cards = [{ name = "Sol Ring", printing = "c21/263" }]"#).unwrap_err();
    assert_eq!(
        err,
        CollectionError::Card(DeckError::NamedTwice { index: 1 })
    );
}

#[test]
fn a_card_has_no_categories_and_a_deck_is_one_place() {
    let err = Collection::parse(r#"cards = [{ name = "Sol Ring", in = ["ramp"] }]"#).unwrap_err();
    assert!(matches!(err, CollectionError::Toml(_)), "{err}");
    let err = Collection::parse(
        "[places]\na = { deck = \"decks/x.deck.toml\" }\nb = { deck = \"decks/x.deck.toml\" }\n",
    )
    .unwrap_err();
    assert!(matches!(err, CollectionError::SameDeck { .. }), "{err}");
}

#[test]
fn moving_a_whole_line_is_a_one_line_diff_that_keeps_its_comment() {
    let moved = move_cards(COLLECTION, 0, 1, Some("Trade binder")).unwrap();
    assert_eq!(
        changed_lines(COLLECTION, &moved),
        vec![(
            r#"  { printing = "moc/94", at = "Lantern" },  # Rashmi and Ragavan"#.into(),
            r#"  { printing = "moc/94", at = "Trade binder" },  # Rashmi and Ragavan"#.into(),
        )]
    );
    let unsorted = move_cards(COLLECTION, 1, 3, None).unwrap();
    assert_eq!(
        changed_lines(COLLECTION, &unsorted),
        vec![(
            r#"  { name = "Sol Ring", qty = 3, at = "Bulk" },"#.into(),
            r#"  { name = "Sol Ring", qty = 3 },"#.into(),
        )]
    );
    let placed = move_cards(COLLECTION, 2, 40, Some("Bulk")).unwrap();
    assert_eq!(
        changed_lines(COLLECTION, &placed),
        vec![(
            r#"  { name = "Island", qty = 40 },"#.into(),
            r#"  { name = "Island", qty = 40, at = "Bulk" },"#.into(),
        )]
    );
}

#[test]
fn moving_part_of_a_line_leaves_the_rest_and_starts_a_line_with_its_comment() {
    let text = move_cards(COLLECTION, 0, 1, None).unwrap();
    let text = add(
        &text,
        &CardRef::Printing(Printing {
            set: "moc".into(),
            num: "94".into(),
        }),
        2,
        Finish::Nonfoil,
        None,
        None,
    )
    .unwrap();
    let moved = move_cards(&text, 0, 2, Some("Lantern")).unwrap();
    let c = Collection::parse(&moved).unwrap();
    assert_eq!(c.cards[0].qty.get(), 1);
    assert_eq!(c.cards[0].at, None);
    assert!(
        moved.contains(
            "  { printing = \"moc/94\", qty = 2, at = \"Lantern\" },  # Rashmi and Ragavan\n]"
        ),
        "{moved}"
    );
}

#[test]
fn moved_copies_join_a_line_already_holding_them_there() {
    let text = add(
        COLLECTION,
        &name("Sol Ring"),
        1,
        Finish::Nonfoil,
        None,
        None,
    )
    .unwrap();
    let moved = move_cards(&text, 3, 1, Some("Bulk")).unwrap();
    assert_eq!(moved, COLLECTION.replace("qty = 3", "qty = 4"));
    let back = move_cards(&moved, 1, 4, None).unwrap();
    let c = Collection::parse(&back).unwrap();
    assert_eq!(c.qty_at(Some("Bulk")), 0);
    assert_eq!(c.qty_at(None), 44);
}

#[test]
fn a_move_cannot_take_more_than_the_line_holds_or_go_nowhere() {
    assert_eq!(
        move_cards(COLLECTION, 1, 4, None).unwrap_err(),
        EditError::Collection(CollectionError::TooMany {
            index: 1,
            have: 3,
            qty: 4
        })
    );
    assert_eq!(
        move_cards(COLLECTION, 1, 1, Some("Shoebox")).unwrap_err(),
        EditError::Collection(CollectionError::NoPlace("Shoebox".into()))
    );
    assert_eq!(
        move_cards(COLLECTION, 1, 2, Some("Bulk")).unwrap(),
        COLLECTION
    );
}

#[test]
fn adding_a_card_held_alike_raises_its_count_and_otherwise_appends_a_line() {
    let more = add(
        COLLECTION,
        &name("Sol Ring"),
        2,
        Finish::Nonfoil,
        Some("Bulk"),
        None,
    )
    .unwrap();
    assert_eq!(more, COLLECTION.replace("qty = 3", "qty = 5"));
    let foil = add(
        COLLECTION,
        &name("Sol Ring"),
        1,
        Finish::Foil,
        Some("Bulk"),
        None,
    )
    .unwrap();
    assert!(
        foil.contains(
            "  { name = \"Island\", qty = 40 },\n  { name = \"Sol Ring\", finish = \"foil\", at = \"Bulk\" },\n]"
        ),
        "{foil}"
    );
}

#[test]
fn the_first_card_makes_the_file() {
    let text = add("", &name("Sol Ring"), 1, Finish::Nonfoil, None, None).unwrap();
    assert_eq!(text, "cards = [\n  { name = \"Sol Ring\" },\n]\n");
    let text = declare_place("", "Bulk", None).unwrap();
    let text = add(
        &text,
        &name("Sol Ring"),
        1,
        Finish::Nonfoil,
        Some("Bulk"),
        None,
    )
    .unwrap();
    assert_eq!(
        Collection::parse(&text).unwrap().qty_at(Some("Bulk")),
        1,
        "{text}"
    );
    assert!(
        text.starts_with("cards = [\n  { name = \"Sol Ring\", at = \"Bulk\" },\n]\n"),
        "{text}"
    );
}

#[test]
fn places_are_declared_once_and_dropped_only_when_empty() {
    let text = declare_place(COLLECTION, "Loam", Some("decks/loam.deck.toml")).unwrap();
    assert!(
        text.ends_with("Loam = { deck = \"decks/loam.deck.toml\" }\n"),
        "{text}"
    );
    assert_eq!(
        declare_place(&text, "Loam", Some("decks/loam.deck.toml")).unwrap(),
        text
    );
    assert!(matches!(
        declare_place(&text, "Loam", None).unwrap_err(),
        EditError::Collection(CollectionError::DeclaredDifferently { .. })
    ));
    assert!(matches!(
        declare_place(&text, "Other", Some("decks/loam.deck.toml")).unwrap_err(),
        EditError::Collection(CollectionError::SameDeck { .. })
    ));
    assert_eq!(undeclare_place(&text, "Loam").unwrap(), COLLECTION);
    assert_eq!(
        undeclare_place(COLLECTION, "Bulk").unwrap_err(),
        EditError::Collection(CollectionError::NotEmpty {
            name: "Bulk".into(),
            qty: 3
        })
    );
}

#[test]
fn a_renamed_place_keeps_its_spot_its_deck_and_its_cards() {
    let text = rename_place(COLLECTION, "Bulk", "Shoebox").unwrap();
    assert_eq!(
        changed_lines(COLLECTION, &text),
        vec![
            (
                r#"  { name = "Sol Ring", qty = 3, at = "Bulk" },"#.into(),
                r#"  { name = "Sol Ring", qty = 3, at = "Shoebox" },"#.into(),
            ),
            ("Bulk = {}".into(), "Shoebox = {}".into()),
        ]
    );
    let text = rename_place(COLLECTION, "Lantern", "Lantern, sleeved").unwrap();
    let c = Collection::parse(&text).unwrap();
    assert_eq!(
        c.place("Lantern, sleeved").unwrap().deck.as_deref(),
        Some("decks/lantern.deck.toml")
    );
    assert_eq!(c.qty_at(Some("Lantern, sleeved")), 1);
    assert_eq!(
        rename_place(COLLECTION, "Bulk", "Bulk").unwrap(),
        COLLECTION
    );
    assert_eq!(
        rename_place(COLLECTION, "Bulk", "Trade binder").unwrap_err(),
        EditError::Collection(CollectionError::Taken("Trade binder".into()))
    );
    assert_eq!(
        rename_place(COLLECTION, "Shoebox", "Bulk").unwrap_err(),
        EditError::Collection(CollectionError::NoPlace("Shoebox".into()))
    );
}

#[test]
fn line_edits_are_the_decks() {
    let text = set_qty(COLLECTION, 1, 1).unwrap();
    assert_eq!(
        changed_lines(COLLECTION, &text),
        vec![(
            r#"  { name = "Sol Ring", qty = 3, at = "Bulk" },"#.into(),
            r#"  { name = "Sol Ring", at = "Bulk" },"#.into(),
        )]
    );
    let text = set_finish(COLLECTION, 1, Finish::Foil).unwrap();
    assert!(
        text.contains(r#"{ name = "Sol Ring", qty = 3, finish = "foil", at = "Bulk" },"#),
        "{text}"
    );
    let text = set_printing(COLLECTION, 1, "C21", "263").unwrap();
    assert!(
        text.contains(r#"{ printing = "c21/263", qty = 3, at = "Bulk" },  # Sol Ring"#),
        "{text}"
    );
    let text = remove(COLLECTION, 0).unwrap();
    assert!(!text.contains("Rashmi"), "{text}");
    assert_eq!(Collection::parse(&text).unwrap().cards.len(), 2);
}

#[test]
fn the_changelog_is_about_copies_moving_not_lines() {
    let msg = |after: &str| commit_message_for_text(COLLECTION, after, "collection.toml").unwrap();
    assert_eq!(
        msg(&move_cards(COLLECTION, 1, 2, Some("Trade binder")).unwrap()),
        "collection: 2 Sol Ring: bulk → trade binder"
    );
    assert_eq!(
        msg(&move_cards(COLLECTION, 0, 1, None).unwrap()),
        "collection: 1 Rashmi and Ragavan: lantern → unsorted"
    );
    assert_eq!(
        msg(&add(
            COLLECTION,
            &name("Sol Ring"),
            1,
            Finish::Nonfoil,
            Some("Bulk"),
            None
        )
        .unwrap()),
        "collection: +1 Sol Ring to bulk"
    );
    assert_eq!(
        msg(&set_qty(COLLECTION, 2, 30).unwrap()),
        "collection: -10 Island"
    );
    assert_eq!(
        msg(&set_finish(COLLECTION, 1, Finish::Foil).unwrap()),
        "collection: 3 Sol Ring: nonfoil → foil"
    );
    assert_eq!(
        msg(&set_printing(COLLECTION, 1, "c21", "263").unwrap()),
        "collection: 3 Sol Ring: any printing → c21/263"
    );
    assert_eq!(
        msg(&declare_place(COLLECTION, "Loam", Some("decks/loam.deck.toml")).unwrap()),
        "collection: +place loam (decks/loam.deck.toml)"
    );
    assert_eq!(
        msg(&rename_place(COLLECTION, "Bulk", "Shoebox").unwrap()),
        "collection: place bulk → shoebox"
    );
    assert_eq!(
        msg(&rename_place(COLLECTION, "Trade binder", "Binder").unwrap()),
        "collection: place trade binder → binder"
    );
    assert_eq!(
        commit_message_for_text("", COLLECTION, "collection.toml").unwrap(),
        "collection: +40 Island, +1 Rashmi and Ragavan to lantern, +3 Sol Ring to bulk, and 3 more\n\n\
         +40 Island\n+1 Rashmi and Ragavan to lantern\n+3 Sol Ring to bulk\n\
         +place bulk\n+place lantern (decks/lantern.deck.toml)\n+place trade binder\n"
    );
}

fn printing(set: &str, num: &str) -> Printing {
    Printing {
        set: set.into(),
        num: num.into(),
    }
}

#[test]
fn reprinting_all_of_a_line_changes_it_in_place() {
    let text = reprint(
        COLLECTION,
        1,
        3,
        Some(&printing("C21", "263")),
        Finish::Foil,
    )
    .unwrap();
    assert_eq!(
        changed_lines(COLLECTION, &text),
        vec![(
            r#"  { name = "Sol Ring", qty = 3, at = "Bulk" },"#.into(),
            r#"  { printing = "c21/263", qty = 3, finish = "foil", at = "Bulk" },  # Sol Ring"#
                .into(),
        )]
    );
    assert_eq!(
        reprint(COLLECTION, 1, 3, None, Finish::Nonfoil).unwrap(),
        COLLECTION
    );
}

#[test]
fn reprinting_part_of_a_line_leaves_the_rest_and_starts_a_line_named_for_the_card() {
    let text = reprint(COLLECTION, 2, 1, None, Finish::Foil).unwrap();
    let c = Collection::parse(&text).unwrap();
    assert_eq!(c.cards[2].qty.get(), 39);
    assert_eq!(c.cards[3].card, name("Island"));
    assert_eq!((c.cards[3].qty.get(), c.cards[3].finish), (1, Finish::Foil));
    assert!(
        text.contains("  { name = \"Island\", finish = \"foil\" },\n]"),
        "{text}"
    );

    let text = reprint(
        COLLECTION,
        1,
        2,
        Some(&printing("c21", "263")),
        Finish::Nonfoil,
    )
    .unwrap();
    assert!(
        text.contains("  { printing = \"c21/263\", qty = 2, at = \"Bulk\" },  # Sol Ring\n]"),
        "{text}"
    );
    assert_eq!(Collection::parse(&text).unwrap().cards[1].qty.get(), 1);
}

#[test]
fn reprinted_copies_join_a_line_already_holding_them_alike_in_that_place() {
    let text = add(
        COLLECTION,
        &CardRef::Printing(printing("c21", "263")),
        1,
        Finish::Nonfoil,
        Some("Bulk"),
        Some("Sol Ring"),
    )
    .unwrap();
    // All of a line: it joins the other and is gone.
    let all = reprint(&text, 1, 3, Some(&printing("c21", "263")), Finish::Nonfoil).unwrap();
    let c = Collection::parse(&all).unwrap();
    assert_eq!(c.cards.len(), 3);
    assert_eq!(c.cards[2].card, CardRef::Printing(printing("c21", "263")));
    assert_eq!(c.cards[2].qty.get(), 4);
    // Part of one: the other grows and no line is added.
    let part = reprint(&text, 1, 1, Some(&printing("c21", "263")), Finish::Nonfoil).unwrap();
    let c = Collection::parse(&part).unwrap();
    assert_eq!(c.cards.len(), 4);
    assert_eq!((c.cards[1].qty.get(), c.cards[3].qty.get()), (2, 2));
}

#[test]
fn a_reprint_cannot_take_more_than_the_line_holds() {
    assert_eq!(
        reprint(COLLECTION, 1, 4, None, Finish::Foil).unwrap_err(),
        EditError::Collection(CollectionError::TooMany {
            index: 1,
            have: 3,
            qty: 4
        })
    );
}

#[test]
fn moving_lines_moves_all_of_each_and_joins_alike_ones_there() {
    let text = add(
        COLLECTION,
        &name("Sol Ring"),
        2,
        Finish::Nonfoil,
        None,
        None,
    )
    .unwrap();
    // Sol Ring in Bulk, Island unsorted and Sol Ring unsorted, all to the
    // trade binder: the two Sol Ring lines become one there.
    let moved = move_lines(&text, &[1, 2, 3], Some("Trade binder")).unwrap();
    let c = Collection::parse(&moved).unwrap();
    assert_eq!(c.qty_at(Some("Trade binder")), 45);
    assert_eq!(c.qty_at(Some("Bulk")), 0);
    assert_eq!(c.cards.len(), 3);
    assert_eq!(
        move_lines(&text, &[3, 1, 3], Some("Trade binder")).unwrap(),
        move_lines(&text, &[1, 3], Some("Trade binder")).unwrap()
    );
    assert_eq!(
        move_lines(&text, &[9], None).unwrap_err(),
        EditError::NoCard(9)
    );
}

#[test]
fn the_changelog_says_which_copies_were_reprinted_or_moved_together() {
    let msg = |after: &str| commit_message_for_text(COLLECTION, after, "collection.toml").unwrap();
    assert_eq!(
        msg(&reprint(COLLECTION, 2, 1, None, Finish::Foil).unwrap()),
        "collection: 1 Island: nonfoil → foil"
    );
    assert_eq!(
        msg(&reprint(
            COLLECTION,
            1,
            2,
            Some(&printing("c21", "263")),
            Finish::Nonfoil
        )
        .unwrap()),
        "collection: 2 Sol Ring: any printing → c21/263"
    );
    assert_eq!(
        msg(&move_lines(COLLECTION, &[1, 2], Some("Trade binder")).unwrap()),
        "collection: 40 Island: unsorted → trade binder, 3 Sol Ring: bulk → trade binder"
    );
}
