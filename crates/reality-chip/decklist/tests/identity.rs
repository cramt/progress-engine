//! A card's line is found by one rule, by name or by printing.

use chip_decklist::deck::{CardRef, Printing};
use chip_decklist::identity::{holds, same_name, Names};

fn printing(set: &str, num: &str) -> CardRef {
    CardRef::Printing(Printing {
        set: set.into(),
        num: num.into(),
    })
}

fn name(n: &str) -> CardRef {
    CardRef::Name(n.into())
}

const DELVER: &str = "Delver of Secrets // Insectile Aberration";

fn names() -> Names {
    Names::from([
        ("isd/51".to_string(), DELVER.to_string()),
        ("cmm/410".to_string(), "Sol Ring".to_string()),
    ])
}

#[test]
fn a_name_is_the_same_whatever_its_case() {
    assert!(same_name("Sol Ring", "sol ring"));
    assert!(same_name("Lim-Dûl's Vault", "LIM-DÛL'S VAULT"));
    assert!(!same_name("Sol Ring", "Sol Talisman"));
}

#[test]
fn a_double_faced_card_is_named_by_its_front_face_or_whole() {
    assert!(same_name(DELVER, "Delver of Secrets"));
    assert!(same_name("delver of secrets", DELVER));
    assert!(same_name(DELVER, DELVER));
}

#[test]
fn a_back_face_alone_names_nothing() {
    assert!(!same_name(DELVER, "Insectile Aberration"));
}

#[test]
fn two_cards_sharing_a_front_face_are_two_cards() {
    assert!(!same_name("A // B", "A // C"));
}

#[test]
fn a_name_is_held_by_a_line_of_its_printing() {
    assert!(holds(
        &printing("isd", "51"),
        &name("Delver of Secrets"),
        &names()
    ));
    assert!(holds(&name("Sol Ring"), &name("sol ring"), &names()));
}

#[test]
fn a_printing_with_no_name_known_holds_no_name() {
    assert!(!holds(
        &printing("lea", "232"),
        &name("Black Lotus"),
        &names()
    ));
}

#[test]
fn a_printing_is_held_only_by_a_line_of_that_printing() {
    assert!(holds(
        &printing("cmm", "410"),
        &printing("CMM", "410"),
        &names()
    ));
    assert!(!holds(&name("Sol Ring"), &printing("cmm", "410"), &names()));
    assert!(!holds(
        &printing("ltc", "3"),
        &printing("cmm", "410"),
        &names()
    ));
}
