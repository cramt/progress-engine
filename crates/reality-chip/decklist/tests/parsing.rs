//! Golden tests for the decklist parser.
//!
//! Every case here is behaviour the jq predecessor had (or a bug it had that we
//! are deliberately fixing). These are the contract `scryfall check` and
//! `scryfall play` rely on when they delegate parsing.

use chip_decklist::{self as decklist, ParseError};

fn one(line: &str) -> decklist::Entry {
    decklist::parse_line(line, 1)
        .expect("should parse")
        .expect("should not be a comment")
}

#[test]
fn quantity_with_and_without_x() {
    assert_eq!(one("3x Plains").qty.get(), 3);
    assert_eq!(one("3 Plains").qty.get(), 3);
    assert_eq!(one("3x Plains").name, "Plains");
    assert_eq!(one("3 Plains").name, "Plains");
}

#[test]
fn multi_word_names_survive_the_lazy_match() {
    assert_eq!(
        one("1x Senu, Keen-Eyed Protector").name,
        "Senu, Keen-Eyed Protector"
    );
}

#[test]
fn set_code_and_collector_number() {
    let e = one("1x Lightning Bolt (sos) 267");
    assert_eq!(e.name, "Lightning Bolt");
    assert_eq!(e.set.as_deref(), Some("sos"));
    assert_eq!(e.num.as_deref(), Some("267"));
}

#[test]
fn set_code_without_collector_number() {
    let e = one("1x Myr Battlesphere (tdc) [Big Colorless,Test]");
    assert_eq!(e.name, "Myr Battlesphere");
    assert_eq!(e.set.as_deref(), Some("tdc"));
    assert_eq!(e.num, None);
}

#[test]
fn foil_marker() {
    let e = one("1x Lightning Bolt (sos) 267 *F* [Interaction]");
    assert!(e.foil);
    assert_eq!(e.name, "Lightning Bolt");
    assert_eq!(e.category, "Interaction");
    assert!(!one("1x Lightning Bolt [Interaction]").foil);
}

#[test]
fn multiple_categories_split_on_comma() {
    let e = one("1x Myr Battlesphere (tdc) [Big Colorless,Test]");
    let names: Vec<_> = e.categories.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["Big Colorless", "Test"]);
    // The raw string is preserved so nothing that matched on it breaks silently.
    assert_eq!(e.category, "Big Colorless,Test");
}

#[test]
fn commander_is_detected_per_category_not_on_the_raw_string() {
    assert!(one("1x Rashmi and Ragavan [Commander{top}]").is_commander());
    // The regression multi-category support could have introduced: the raw
    // bracket no longer starts with "commander", but this is still the commander.
    assert!(one("1x Rashmi and Ragavan [Ramp,Commander{top}]").is_commander());
    assert!(!one("1x Sol Ring [Ramp]").is_commander());
}

#[test]
fn outside_the_deck_is_detected_per_category() {
    assert!(one("1x Lurrus of the Dream-Den [Companion{noDeck}]").is_outside());
    assert!(one("1x Foo [Sideboard]").is_outside());
    assert!(one("1x Foo [Maybeboard]").is_outside());
    assert!(one("1x Foo [Anything{noDeck}]").is_outside());
    // Same regression class as above, in the other direction.
    assert!(one("1x Foo [Ramp,Sideboard]").is_outside());
}

#[test]
fn sticker_package_is_inside_the_deck() {
    // Matching /sticker/ once dropped five real cards from a 100-card list and
    // reported them as companions. Sheets go in Sideboard; these are real cards.
    assert!(!one("1x Park Bleater [Sticker Package]").is_outside());
    assert!(!one("1x Ticketomaton [Sticker Package]").is_outside());
}

#[test]
fn double_faced_names_are_not_truncated_by_the_comment_rule() {
    let e = one("1x Unstable Glyphbridge // Sandswirl Wanderglyph [Interaction]");
    assert_eq!(e.name, "Unstable Glyphbridge // Sandswirl Wanderglyph");
    assert_eq!(e.category, "Interaction");
}

#[test]
fn line_leading_comments_and_blanks_are_skipped() {
    assert!(decklist::parse_line("// a comment", 1).unwrap().is_none());
    assert!(decklist::parse_line("   // indented comment", 1)
        .unwrap()
        .is_none());
    assert!(decklist::parse_line("", 1).unwrap().is_none());
    assert!(decklist::parse_line("   ", 1).unwrap().is_none());
}

#[test]
fn malformed_lines_error_instead_of_vanishing() {
    // The jq predecessor silently emitted nothing here, so the mistake only
    // surfaced later as a wrong total.
    assert!(matches!(
        decklist::parse_line("Plains", 4),
        Err(ParseError::Malformed { line: 4, .. })
    ));
    assert!(matches!(
        decklist::parse_line("3xPlains", 7),
        Err(ParseError::Malformed { line: 7, .. })
    ));
    assert!(matches!(
        decklist::parse_line("0x Plains", 9),
        Err(ParseError::ZeroQuantity { line: 9, .. })
    ));
}

#[test]
fn total_counts_cards_not_lines() {
    let deck = decklist::parse("17x Plains\n1x Sol Ring\n").unwrap();
    assert_eq!(deck.len(), 2);
    assert_eq!(decklist::total(&deck), 18);
}
