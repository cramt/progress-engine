//! Tests for the card types that are never in a library.
//!
//! The trap here is that substring matching looks like it works: `Plane` sits
//! inside `Planeswalker`, `Scheme` inside `Schemer`. Every excluded type gets a
//! real type line, and so does every near-miss.

use chip_scryfall::{outside_library, OutsideLibrary};

#[test]
fn each_excluded_type_is_recognised_from_its_type_line() {
    let cases = [
        ("Stickers", OutsideLibrary::Stickers),
        ("Artifact — Attraction", OutsideLibrary::Attraction),
        ("Plane — Dominaria", OutsideLibrary::Plane),
        ("Phenomenon", OutsideLibrary::Phenomenon),
        ("Scheme", OutsideLibrary::Scheme),
        ("Ongoing Scheme", OutsideLibrary::Scheme),
        ("Vanguard", OutsideLibrary::Vanguard),
        ("Conspiracy", OutsideLibrary::Conspiracy),
        ("Dungeon", OutsideLibrary::Dungeon),
        ("Emblem", OutsideLibrary::Emblem),
        ("Emblem — Ajani", OutsideLibrary::Emblem),
    ];
    for (type_line, want) in cases {
        assert_eq!(
            outside_library(type_line),
            Some(want),
            "type line {type_line:?}"
        );
    }
}

#[test]
fn a_planeswalker_is_not_a_plane() {
    // `type_line.contains("Plane")` matches every planeswalker ever printed.
    // Two of them sit in the fixture deck, so getting this wrong shrinks a
    // 99-card library to 97 and moves every number in the report.
    assert_eq!(outside_library("Legendary Planeswalker — Jace"), None);
    assert_eq!(outside_library("Planeswalker — Vivien"), None);
}

#[test]
fn ordinary_cards_stay_in_the_library() {
    for type_line in [
        "Basic Land — Forest",
        "Creature — Elf Druid",
        "Legendary Creature — Elf Druid",
        "Artifact Creature — Golem",
        "Sorcery — Arcane",
        "Instant",
        "Enchantment — Aura",
        "Legendary Artifact — Equipment",
        "Battle — Siege",
        // Real cards that apply stickers. Archidekt files them under a
        // "Sticker Package" category, and matching that name once dropped five
        // of them from a 100-card list as if they were companions.
        "Creature — Goat Employee",
        "Artifact Creature — Robot",
    ] {
        assert_eq!(outside_library(type_line), None, "type line {type_line:?}");
    }
}

#[test]
fn attraction_is_read_as_a_subtype_and_plane_as_a_card_type() {
    // The two halves are not interchangeable, so the last two cases are
    // constructed: swapping the sides of a real type line must stop matching.
    assert_eq!(
        outside_library("Artifact — Attraction"),
        Some(OutsideLibrary::Attraction)
    );
    assert_eq!(outside_library("Attraction — Artifact"), None);
    assert_eq!(
        outside_library("Plane — Equilor"),
        Some(OutsideLibrary::Plane)
    );
    assert_eq!(outside_library("Legendary Land — Equilor"), None);
}

#[test]
fn both_faces_of_a_double_faced_card_are_read() {
    // Faces are joined with "//" in the index, and each has its own type line.
    assert_eq!(
        outside_library("Creature — Human // Creature — Werewolf"),
        None
    );
    assert_eq!(
        outside_library("Enchantment — Saga // Enchantment Creature — Avatar"),
        None
    );
}

#[test]
fn matching_is_case_insensitive() {
    assert_eq!(outside_library("STICKERS"), Some(OutsideLibrary::Stickers));
    assert_eq!(
        outside_library("artifact — attraction"),
        Some(OutsideLibrary::Attraction)
    );
}

#[test]
fn the_reported_name_is_the_card_type() {
    assert_eq!(OutsideLibrary::Attraction.to_string(), "Attraction");
    assert_eq!(OutsideLibrary::Stickers.as_str(), "Stickers");
}
