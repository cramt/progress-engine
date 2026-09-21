//! The index file format: a header line, then one card per line behind its key.
//!
//! The format exists so that a run costs what your deck costs rather than what
//! Magic costs, and every test here is about one of the two things that buys:
//! cards nobody names are never parsed, and the failures that laziness makes
//! possible are caught rather than absorbed.

use std::path::Path;

use chip_scryfall::index::{keyname, Card, Index, IndexFile, SCHEMA};

fn memory(text: &str) -> Result<IndexFile, chip_scryfall::index::IndexError> {
    IndexFile::parse(Path::new("<memory>"), text.to_string())
}

fn index_of(names: &[&str]) -> Index {
    let mut index = Index {
        schema: Some(SCHEMA),
        updated_at: Some("2026-09-05".into()),
        ..Index::default()
    };
    for name in names {
        index.cards.insert(
            keyname(name),
            Card {
                name: (*name).into(),
                type_line: "Artifact".into(),
                ..Card::default()
            },
        );
    }
    index
}

#[test]
fn a_written_index_reads_back_as_the_cards_that_went_in() {
    let index = index_of(&["Sol Ring", "Arcane Signet"]);
    let text = index.to_lines().expect("should serialise");
    let file = memory(&text).expect("should read back");

    assert_eq!(file.len(), 2);
    assert_eq!(file.schema(), SCHEMA);
    assert!(!file.is_stale());
    assert_eq!(file.updated_at(), Some("2026-09-05"));
    assert_eq!(
        file.get("sol RING").expect("should read").map(|c| c.name),
        Some("Sol Ring".to_string())
    );
    assert!(file.contains("Arcane Signet"));
    assert!(!file.contains("Mana Crypt"));
    assert!(file.get("Mana Crypt").expect("should read").is_none());
}

/// Two syncs of the same data produce the same bytes. Not cosmetic: the run
/// reports a hash of its inputs as provenance, and a hash that moves because a
/// `HashMap` felt like iterating differently is a hash nobody can compare.
#[test]
fn the_same_cards_always_write_the_same_bytes() {
    let names = ["Sol Ring", "Arcane Signet", "Llanowar Elves", "Plains"];
    let first = index_of(&names).to_lines().expect("should serialise");
    let second = index_of(&names).to_lines().expect("should serialise");
    assert_eq!(first, second);
}

/// The claim the format makes, stated as a test: a card nobody names is never
/// parsed. Sol Ring is answered out of a file whose other line is not JSON at
/// all — which only works if the other line was never looked at.
#[test]
fn a_card_nobody_asks_about_is_never_parsed() {
    let file = memory(concat!(
        "{\"schema\":1,\"cards\":2}\n",
        "sol ring\t{\"name\":\"Sol Ring\"}\n",
        "black lotus\tthis is not JSON and never has been\n",
    ))
    .expect("opening should not parse the cards");

    assert_eq!(
        file.get("Sol Ring").expect("should read").map(|c| c.name),
        Some("Sol Ring".to_string())
    );
    assert!(
        file.get("Black Lotus").is_err(),
        "asking for it is what should fail, and only then"
    );
}

/// A JSON document cut in half stops parsing; a list of lines cut in half reads
/// as a shorter list. So the file says how many cards it should have, and a
/// missing hundred is a refusal rather than a hundred cards you do not own.
#[test]
fn a_truncated_index_is_refused_rather_than_read_as_a_smaller_card_pool() {
    let index = index_of(&["Sol Ring", "Arcane Signet", "Plains"]);
    let text = index.to_lines().expect("should serialise");
    let truncated: String = text.lines().take(3).map(|l| format!("{l}\n")).collect();

    let error = memory(&truncated).expect_err("should refuse");
    let message = error.to_string();
    assert!(message.contains("truncated"), "{message}");
    assert!(message.contains('3') && message.contains('2'), "{message}");
}

/// A hand-written fixture that never counted itself is a gap, not a claim of
/// zero — the same rule every other absent field follows.
#[test]
fn an_index_that_never_counted_itself_is_not_accused_of_being_truncated() {
    let file = memory("{}\nsol ring\t{\"name\":\"Sol Ring\"}\n").expect("should open");
    assert_eq!(file.len(), 1);
    assert_eq!(file.schema(), 0, "a header with no schema is schema 0");
    assert!(file.is_stale());
}

#[test]
fn an_entry_with_no_key_is_refused() {
    let error = memory("{}\n{\"name\":\"Sol Ring\"}\n").expect_err("should refuse");
    assert!(error.to_string().contains("no name key"), "{error}");
}

/// The key is written beside the card rather than derived from it, which is
/// what makes a lookup cheap and what lets the two disagree. A card filed under
/// somebody else's name is the wrong card returned confidently, so it is an
/// error — checked on read, which is where it would do harm.
#[test]
fn a_card_filed_under_the_wrong_name_is_refused_when_it_is_read() {
    let file = memory("{}\nblack lotus\t{\"name\":\"Sol Ring\"}\n").expect("should open");
    let error = file.get("Black Lotus").expect_err("should refuse");
    let message = error.to_string();
    assert!(message.contains("Sol Ring"), "{message}");
    assert!(message.contains("black lotus"), "{message}");
}

#[test]
fn a_file_with_no_header_line_is_refused() {
    let error = memory("sol ring\t{\"name\":\"Sol Ring\"}").expect_err("should refuse");
    assert!(error.to_string().contains("no header line"), "{error}");
}

/// The vocabulary `kw:` is checked against comes out of the header, and the
/// header is written from the cards — so an index cannot describe a keyword
/// pool different from the one it holds.
#[test]
fn the_header_carries_the_keywords_of_the_cards_written_with_it() {
    let mut index = index_of(&["Birds of Paradise"]);
    index
        .cards
        .get_mut("birds of paradise")
        .expect("just inserted")
        .keywords = vec!["Flying".into()];

    let text = index.to_lines().expect("should serialise");
    let file = memory(&text).expect("should read back");

    let vocabulary = file.keyword_vocabulary();
    assert!(vocabulary.contains("flying"));
    assert!(!vocabulary.contains("trample"));
}
