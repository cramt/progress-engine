//! A keystroke, menu pick or drop on one card or a selection is one edit, and
//! says where every line went.

use chip_decklist::deck::{Category, CategoryType, Deck};
use chip_decklist::edit::{drop_onto, edit_cards, Board, CardEdit, Dest, EditError, Target};

fn lantern() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../decks/lantern.deck.toml"
    ))
    .unwrap()
}

/// The card whose comment names it, reached from its first category's stack.
fn find(text: &str, name: &str) -> (Target, String) {
    let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("  {")).collect();
    let index = lines
        .iter()
        .position(|l| l.ends_with(&format!("# {name}")))
        .unwrap_or_else(|| panic!("no {name} in lantern"));
    let deck = Deck::parse(text).unwrap();
    let from = deck.cards[index].categories.first().cloned();
    (Target { index, from }, lines[index].to_string())
}

fn changed_lines(before: &str, after: &str) -> Vec<String> {
    let a: Vec<&str> = before.lines().collect();
    after
        .lines()
        .enumerate()
        .filter(|(i, l)| a.get(*i) != Some(l))
        .map(|(_, l)| l.to_string())
        .collect()
}

fn without(text: &str, lines: &[&str]) -> String {
    text.lines()
        .filter(|l| !lines.contains(l))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn total(text: &str) -> u32 {
    Deck::parse(text)
        .unwrap()
        .cards
        .iter()
        .filter(|c| c.in_deck())
        .map(|c| c.qty.get())
        .sum()
}

fn on(text: &str, edit: CardEdit, targets: &[Target]) -> String {
    edit_cards(text, &edit, targets).unwrap().text
}

#[test]
fn plus_and_minus_change_only_that_line_and_minus_at_one_removes_it() {
    let text = lantern();
    let (sol, line) = find(&text, "Sol Ring");
    assert_eq!(
        changed_lines(
            &text,
            &on(&text, CardEdit::Increase, std::slice::from_ref(&sol))
        ),
        [r#"  { printing = "cmr/472", qty = 2, in = ["Artifact Count"] },  # Sol Ring"#]
    );
    let gone = edit_cards(&text, &CardEdit::Decrease, std::slice::from_ref(&sol)).unwrap();
    assert_eq!(gone.text, without(&text, &[&line]));
    assert_eq!(gone.lines[sol.index], None);
}

#[test]
fn remove_takes_the_line_and_automatic_leaves_it_in_no_category() {
    let text = lantern();
    let (sol, _) = find(&text, "Sol Ring");
    assert_eq!(
        total(&on(&text, CardEdit::Remove, std::slice::from_ref(&sol))),
        99
    );
    assert_eq!(
        changed_lines(&text, &on(&text, CardEdit::Automatic, &[sol])),
        [r#"  { printing = "cmr/472" },  # Sol Ring"#]
    );
}

#[test]
fn a_board_move_declares_the_board_once_as_the_drag_strip_does() {
    let text = lantern();
    let (sol, _) = find(&text, "Sol Ring");
    let to_maybe = CardEdit::Move {
        to: Dest::Board(Board::Maybeboard),
        secondary: false,
    };
    let next = on(&text, to_maybe.clone(), std::slice::from_ref(&sol));
    let deck = Deck::parse(&next).unwrap();
    assert!(deck.categories.contains(&Category {
        name: "Maybeboard".into(),
        kind: Some(CategoryType::Maybeboard),
    }));
    assert_eq!(deck.cards[sol.index].categories, ["Maybeboard"]);
    assert_eq!(total(&next), 99);
    let before: Vec<&str> = text.lines().collect();
    let new: Vec<&str> = next.lines().filter(|l| !before.contains(l)).collect();
    assert_eq!(
        new,
        [
            r#"  { printing = "cmr/472", in = ["Maybeboard"] },  # Sol Ring"#,
            r#"Maybeboard = { type = "maybeboard" }"#,
        ]
    );
    // A second card goes on the same board rather than declaring another.
    let (counterspell, _) = find(&next, "Counterspell");
    let again = on(&next, to_maybe, &[counterspell]);
    assert_eq!(changed_lines(&next, &again).len(), 1);
}

#[test]
fn a_board_the_deck_declares_under_another_name_is_the_one_used() {
    let text =
        "cards = [\n  { name = \"Opt\" },\n]\n\n[categories]\nSide = { type = \"sideboard\" }\n";
    let target = Target {
        index: 0,
        from: None,
    };
    let to_side = CardEdit::Move {
        to: Dest::Board(Board::Sideboard),
        secondary: false,
    };
    assert_eq!(
        on(text, to_side, &[target]),
        text.replace("{ name = \"Opt\" }", "{ name = \"Opt\", in = [\"Side\"] }")
    );
}

#[test]
fn a_move_to_a_category_replaces_the_stack_it_came_from_and_declares_a_new_one() {
    let text = lantern();
    let (sol, _) = find(&text, "Sol Ring");
    let to = |name: &str| CardEdit::Move {
        to: Dest::Category(name.into()),
        secondary: false,
    };
    assert_eq!(
        changed_lines(&text, &on(&text, to("Draw"), std::slice::from_ref(&sol))),
        [r#"  { printing = "cmr/472", in = ["Draw"] },  # Sol Ring"#]
    );
    let fresh = Deck::parse(&on(&text, to("Fast Mana"), std::slice::from_ref(&sol))).unwrap();
    assert_eq!(fresh.cards[sol.index].categories, ["Fast Mana"]);
    assert_eq!(fresh.category("Fast Mana").map(|c| c.kind), Some(None));
}

#[test]
fn commander_puts_it_in_the_commander_category_first_keeping_its_labels() {
    let text = lantern();
    let (sol, _) = find(&text, "Sol Ring");
    assert_eq!(
        changed_lines(&text, &on(&text, CardEdit::Commander, &[sol])),
        [r#"  { printing = "cmr/472", in = ["Commander", "Artifact Count"] },  # Sol Ring"#]
    );
}

#[test]
fn a_refusal_on_any_card_edits_none() {
    let text = lantern();
    let (sol, _) = find(&text, "Sol Ring");
    let nowhere = Target {
        index: 999,
        from: None,
    };
    assert_eq!(
        edit_cards(&text, &CardEdit::Remove, &[sol, nowhere]).unwrap_err(),
        EditError::NoCard(999)
    );
}

#[test]
fn a_selection_is_removed_whatever_order_it_was_picked_in() {
    let text = lantern();
    let (a, a_line) = find(&text, "Sol Ring");
    let (b, b_line) = find(&text, "Arcane Signet");
    let (c, c_line) = find(&text, "Counterspell");
    let edited = edit_cards(&text, &CardEdit::Remove, &[b.clone(), c.clone(), a.clone()]).unwrap();
    assert_eq!(edited.text, without(&text, &[&a_line, &b_line, &c_line]));
    assert_eq!(
        Deck::parse(&edited.text).unwrap().cards.len(),
        Deck::parse(&text).unwrap().cards.len() - 3
    );
}

#[test]
fn a_selection_is_raised_by_one_each_on_its_own_line() {
    let text = lantern();
    let (a, _) = find(&text, "Sol Ring");
    let (b, _) = find(&text, "Arcane Signet");
    let next = on(&text, CardEdit::Increase, &[a, b.clone(), b]);
    assert_eq!(changed_lines(&text, &next).len(), 2);
    assert_eq!(total(&next), 102);
}

#[test]
fn every_line_after_a_removed_one_moves_up_and_the_rest_stay() {
    let text = "cards = [\n  { name = \"A\" },\n  { name = \"B\", qty = 2 },\n  { name = \"C\" },\n  { name = \"D\" },\n  { name = \"E\" },\n]\n";
    let at = |index| Target { index, from: None };
    let edited = edit_cards(text, &CardEdit::Decrease, &[at(0), at(1), at(3)]).unwrap();
    assert_eq!(edited.lines, [None, Some(0), Some(1), None, Some(2)]);
    let deck = Deck::parse(&edited.text).unwrap();
    let names: Vec<String> = deck.cards.iter().map(|c| c.card.to_string()).collect();
    assert_eq!(names, ["B", "C", "E"]);
    let untouched = edit_cards(text, &CardEdit::Increase, &[at(2)]).unwrap();
    assert_eq!(
        untouched.lines,
        [Some(0), Some(1), Some(2), Some(3), Some(4)]
    );
}

#[test]
fn a_drop_takes_the_place_of_the_category_it_was_dragged_out_of() {
    let cats = |c: &[&str]| c.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        drop_onto(
            &cats(&["tempo", "self-bounce"]),
            Some("tempo"),
            "enemy-bounce",
            false
        ),
        ["enemy-bounce", "self-bounce"]
    );
}

#[test]
fn a_drop_into_a_category_the_card_has_does_not_list_it_twice() {
    let cats = vec!["tempo".to_string(), "self-bounce".to_string()];
    assert_eq!(
        drop_onto(&cats, Some("tempo"), "self-bounce", false),
        ["self-bounce"]
    );
    assert_eq!(drop_onto(&cats, Some("tempo"), "self-bounce", true), cats);
}

#[test]
fn a_secondary_drop_adds_the_category_behind_the_others() {
    assert_eq!(
        drop_onto(&["tempo".to_string()], Some("tempo"), "learnboard", true),
        ["tempo", "learnboard"]
    );
}

#[test]
fn an_uncategorised_card_dropped_gets_its_first_category() {
    assert_eq!(drop_onto(&[], None, "tempo", false), ["tempo"]);
}
