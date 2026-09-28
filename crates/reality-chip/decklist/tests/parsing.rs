//! Golden tests for the decklist parser.
//!
//! Every case here is behaviour the jq predecessor had (or a bug it had that we
//! are deliberately fixing). These are the contract `scryfall check` and
//! `scryfall play` rely on when they delegate parsing.

use chip_decklist::deck::Finish;
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

/// A printing is a set and a collector number. A parenthetical with no number
/// after it is part of the name, which is where real names end in one, and a
/// bare set is kept in the name rather than dropped, so it fails to resolve
/// out loud instead of quietly becoming a different printing.
#[test]
fn a_parenthetical_without_a_number_is_part_of_the_name() {
    for name in [
        "Erase (Not the Urza's Legacy One)",
        "B.F.M. (Big Furry Monster)",
        "B.F.M. (Big Furry Monster, Right Side)",
    ] {
        let e = one(&format!("1x {name} [Removal]"));
        assert_eq!(e.name, name);
        assert_eq!((e.set, e.num), (None, None), "{name}");
    }
    let e = one("1x Myr Battlesphere (tdc) [Big Colorless,Test]");
    assert_eq!(e.name, "Myr Battlesphere (tdc)");
    assert_eq!((e.set, e.num), (None, None));

    let e = one("1x Erase (Not the Urza's Legacy One) (unh) 42 [Removal]");
    assert_eq!(e.name, "Erase (Not the Urza's Legacy One)");
    assert_eq!(e.set.as_deref(), Some("unh"));
    assert_eq!(e.num.as_deref(), Some("42"));
}

#[test]
fn foil_marker() {
    let e = one("1x Lightning Bolt (sos) 267 *F* [Interaction]");
    assert_eq!(e.finish, Finish::Foil);
    assert_eq!(e.name, "Lightning Bolt");
    assert_eq!(e.category, "Interaction");
    assert_eq!(
        one("1x Lightning Bolt [Interaction]").finish,
        Finish::Nonfoil
    );
}

#[test]
fn etched_marker() {
    let e = one("1x Sol Ring (c21) 263 *E* [Ramp]");
    assert_eq!(e.finish, Finish::Etched);
    assert_eq!(e.name, "Sol Ring");
    assert_eq!(e.set.as_deref(), Some("c21"));
    assert_eq!(e.num.as_deref(), Some("263"));
    assert_eq!(e.category, "Ramp");
}

/// A marker this parser does not know is refused by name, never read as part
/// of the card's name.
#[test]
fn an_unknown_marker_is_refused_not_named() {
    assert_eq!(
        decklist::parse_line("1x Sol Ring (c21) 263 *X* [Ramp]", 3),
        Err(ParseError::UnknownMarker {
            line: 3,
            text: "1x Sol Ring (c21) 263 *X* [Ramp]".into(),
            marker: "*X*".into(),
        })
    );
    assert!(matches!(
        decklist::parse_line("1x Sol Ring *Glossy*", 1),
        Err(ParseError::UnknownMarker { .. })
    ));
}

#[test]
fn multiple_categories_split_on_comma() {
    let e = one("1x Myr Battlesphere (tdc) [Big Colorless,Test]");
    let names: Vec<_> = e.categories.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["Big Colorless", "Test"]);
    // The raw string is preserved so nothing that matched on it breaks silently.
    assert_eq!(e.category, "Big Colorless,Test");
}

/// Archidekt makes a card a commander by `{top}` on its first category, and by
/// nothing else (docs/research/archidekt-import-shapes.md).
#[test]
fn a_commander_is_the_first_category_carrying_top() {
    assert!(one("1x Rashmi and Ragavan [Commander{top}]").is_commander());
    assert!(one("1x Rashmi and Ragavan [Commander{top},Ramp]").is_commander());
    assert!(one("1x Lightning Bolt [Burn{top}]").is_commander());
    // Archidekt shows this under Ramp, with no crown.
    assert!(!one("1x Rashmi and Ragavan [Ramp,Commander{top}]").is_commander());
    // A group named Commander, Premier unticked.
    assert!(!one("1x Kenrith, the Returned King [Commander]").is_commander());
    assert!(!one("1x Sol Ring [Ramp]").is_commander());
}

/// Out of the deck by the first category alone: `{noDeck}`, or exactly
/// `Sideboard` or `Maybeboard`.
#[test]
fn outside_the_deck_is_read_from_the_first_category() {
    assert!(one("1x Lurrus of the Dream-Den [Companion{noDeck}]").is_outside());
    assert!(one("1x Foo [Sideboard]").is_outside());
    assert!(one("1x Foo [Maybeboard]").is_outside());
    assert!(one("1x Foo [Maybeboard{noDeck}{noPrice}]").is_outside());
    assert!(one("1x Foo [Anything{noDeck}]").is_outside());
    assert!(one("1x Path to Exile [Sideboard,Removal]").is_outside());
    // Each of these counted toward Size in Archidekt.
    assert!(!one("1x Wrath of God [Removal,Sideboard]").is_outside());
    assert!(!one("1x Foo [sideboard]").is_outside());
    assert!(!one("1x Foo [Sideboard Lessons]").is_outside());
    assert!(!one("1x Lurrus of the Dream-Den [Companion]").is_outside());
    assert!(!one("1x Foo [Draw,Maybeboard]").is_outside());
}

#[test]
fn flags_are_every_brace_group_after_the_name() {
    let e = one("1x Brainstorm [Maybeboard{noDeck}{noPrice}]");
    assert_eq!(e.categories[0].name, "Maybeboard");
    assert_eq!(e.categories[0].flags, vec!["nodeck", "noprice"]);
}

/// A comma between categories splits them; one inside braces is between flags.
#[test]
fn a_comma_inside_braces_separates_flags_not_categories() {
    let e = one("1x Brainstorm [Maybeboard{noDeck,noPrice},Draw]");
    assert_eq!(
        e.categories,
        vec![
            category("Maybeboard", &["nodeck", "noprice"]),
            category("Draw", &[])
        ]
    );
}

/// What a category is written as reads back as that category, bracketed with
/// others.
#[test]
fn a_written_category_reads_back_as_itself() {
    let written = [
        category("Maybeboard", &["nodeck", "noprice"]),
        category("Ramp", &[]),
    ];
    assert_eq!(written[0].to_string(), "Maybeboard{nodeck}{noprice}");
    let line = decklist::with_categories("1x Brainstorm", &written).unwrap();
    assert_eq!(one(&line).categories, written, "{line}");
}

#[test]
fn a_heading_is_the_first_category_of_every_card_below_it() {
    let entries = decklist::parse(
        "# Ramp\n1x Sol Ring [Artifacts]\n1x Arcane Signet [Ramp]\n# Commander\n1x Kenrith, the Returned King\n",
    )
    .unwrap();
    let names = |e: &decklist::Entry| -> Vec<String> {
        e.categories.iter().map(|c| c.to_string()).collect()
    };
    assert_eq!(names(&entries[0]), vec!["Ramp", "Artifacts"]);
    assert_eq!(names(&entries[1]), vec!["Ramp"], "one Ramp, not two");
    // `# Commander` sets Premier by itself, unlike `[Commander]`.
    assert!(entries[2].is_commander());
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

fn category(name: &str, flags: &[&str]) -> decklist::Category {
    decklist::Category {
        name: name.to_string(),
        flags: flags.iter().map(|f| f.to_string()).collect(),
    }
}

#[test]
fn rewriting_categories_touches_nothing_else_on_the_line() {
    let line = "1x Boseiju, Who Endures (pneo) 266s *F* [Land - Utility]";
    assert_eq!(
        decklist::with_categories(line, &[category("Ramp", &[]), category("Land", &["top"])]),
        Some("1x Boseiju, Who Endures (pneo) 266s *F* [Ramp,Land{top}]".to_string())
    );
    assert_eq!(
        decklist::with_categories("2 Island", &[category("Land", &[])]),
        Some("2 Island [Land]".to_string())
    );
    assert_eq!(
        decklist::with_categories("1 Sol Ring [Ramp]  ", &[]),
        Some("1 Sol Ring  ".to_string())
    );
    assert_eq!(decklist::with_categories("// a comment", &[]), None);
}

/// Every line of both decks as Archidekt exported them, given back its own
/// categories, parses to the same entry: the writer and the reader agree.
#[test]
fn rewriting_a_real_deck_round_trips_through_the_parser() {
    for deck in ["lantern.txt", "loam.txt"] {
        let path = format!("{}/tests/fixtures/{deck}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).unwrap();
        for (i, line) in text.lines().enumerate() {
            let Some(entry) = decklist::parse_line(line, i + 1).unwrap() else {
                continue;
            };
            let rewritten = decklist::with_categories(line, &entry.categories).unwrap();
            let mut again = decklist::parse_line(&rewritten, i + 1).unwrap().unwrap();
            // The raw bracket text is the one field allowed to change: flags
            // come back lowercased.
            again.category = entry.category.clone();
            assert_eq!(again, entry, "{deck}:{}: {line}", i + 1);
        }
    }
}
