//! Printing terms, asked of every printing Scryfall has of a few cards.
//!
//! `fixtures/printings.jsonl` is Scryfall's search answer trimmed to the fields
//! this crate reads; `fixtures/printings.sh` rebuilds it. Each expectation below
//! was read off Scryfall's own search for the same term.

use chip_scryfall::bulk::BulkCard;
use chip_scryfall::{parse, parse_printing, ParseError};

const PRINTINGS: &str = include_str!("fixtures/printings.jsonl");

fn printings_of(name: &str) -> Vec<BulkCard> {
    PRINTINGS
        .lines()
        .map(|l| facet_json::from_str::<BulkCard>(l).expect("a Scryfall card object"))
        .filter(|c| c.name == name)
        .collect()
}

/// `set/num` of every printing of `name` the query matches, in fixture order.
fn hits(name: &str, query: &str) -> Vec<String> {
    let q = parse_printing(query).unwrap_or_else(|e| panic!("{query:?}: {e}"));
    printings_of(name)
        .iter()
        .filter(|c| {
            let card = c.card_of_any_printing(&mut Vec::new());
            q.matches_printing(&card.view(&[]), &c.printing())
        })
        .map(|c| {
            let p = c.printing();
            format!("{}/{}", p.set, p.collector_number)
        })
        .collect()
}

#[test]
fn source_material_is_the_comic_panel_printings_and_nothing_else() {
    assert_eq!(
        hits("Heroic Intervention", "is:sourcematerial"),
        ["mar/80", "mar/79", "mar/78", "tle/43", "mar/34"]
    );
}

#[test]
fn universes_beyond_is_the_promo_type_scryfall_reads() {
    let ub = hits("Heroic Intervention", "is:ub");
    assert_eq!(ub, hits("Heroic Intervention", "is:universesbeyond"));
    assert!(ub.contains(&"pip/202".to_string()), "{ub:?}");
    assert!(!ub.contains(&"cmm/295".to_string()), "{ub:?}");
}

#[test]
fn a_printing_in_another_language_is_answered_and_not_dropped() {
    // The index refuses a Japanese record; a printing choice must see it.
    assert_eq!(
        hits("Counterspell", "-lang:en"),
        ["pmei/2021-1", "sta/78", "4bb/65", "fbb/54"]
    );
    assert_eq!(hits("Counterspell", "lang:ja"), ["pmei/2021-1", "sta/78"]);
    assert_eq!(
        hits("Counterspell", "lang:any").len(),
        printings_of("Counterspell").len()
    );
}

#[test]
fn invocations_gold_borders_and_digital_printings_are_each_one_term() {
    assert_eq!(hits("Counterspell", "set:mp2"), ["mp2/10"]);
    let gold = hits("Counterspell", "border:gold");
    assert_eq!(gold.len(), 11, "{gold:?}");
    assert!(gold
        .iter()
        .all(|p| p.starts_with("wc") || p.starts_with("ptc")));
    let digital = hits("Counterspell", "is:digital");
    assert_eq!(digital, hits("Counterspell", "-game:paper"));
    assert!(digital.contains(&"vma/64".to_string()), "{digital:?}");
}

#[test]
fn card_terms_and_printing_terms_combine() {
    let full_art_basics = hits("Forest", "t:basic is:fullart");
    let textless = hits("Forest", "t:basic is:textless");
    assert!(full_art_basics.contains(&"sld/2120".to_string()));
    // A textless full-art Forest is both, which is why a preference can rank
    // it below other full-art Forests and above a plain one.
    assert!(textless.contains(&"sld/2120".to_string()));
    assert!(hits("Forest", "t:basic -is:fullart").contains(&"m21/274".to_string()));
    assert!(hits("Heroic Intervention", "t:basic is:fullart").is_empty());
}

#[test]
fn frames_and_frame_effects_share_a_key_as_on_scryfall() {
    assert_eq!(
        hits("Counterspell", "frame:showcase"),
        hits("Counterspell", "is:showcase")
    );
    assert_eq!(
        hits("Counterspell", "frame:old"),
        hits("Counterspell", "is:retro")
    );
    assert!(hits("Counterspell", "frame:future").contains(&"mb2/158".to_string()));
}

#[test]
fn a_card_search_refuses_every_printing_term_by_name() {
    for term in [
        "lang:ja",
        "-lang:en",
        "is:fullart",
        "t:basic is:textless",
        "frame:showcase",
        "border:gold",
        "st:masterpiece",
        "game:paper",
        "not:ub",
    ] {
        let err = parse(term).expect_err(term);
        assert!(
            matches!(err, ParseError::AboutAPrinting { .. }),
            "{term}: {err}"
        );
        assert!(parse_printing(term).is_ok(), "{term}");
    }
}

#[test]
fn a_printing_value_outside_the_closed_set_is_refused_with_the_set() {
    let err = parse_printing("lang:jp").unwrap_err();
    assert!(
        matches!(
            err,
            ParseError::NotOneOf {
                what: "language",
                ..
            }
        ),
        "{err}"
    );
    assert!(err.to_string().contains("ja"), "{err}");
    let err = parse_printing("frame:shiny").unwrap_err();
    assert!(err.to_string().contains("extendedart"), "{err}");
    let err = parse_printing("is:shiny").unwrap_err();
    assert!(err.to_string().contains("sourcematerial"), "{err}");
    // A card search's list does not offer what it would then refuse.
    let err = parse("is:shiny").unwrap_err();
    assert!(!err.to_string().contains("sourcematerial"), "{err}");
}

#[test]
fn a_printing_term_asks_whether_not_how_much() {
    assert!(matches!(
        parse_printing("lang>=en"),
        Err(ParseError::NoComparison { .. })
    ));
}

#[test]
fn every_printing_key_parses() {
    let sample = |key: &str| match key {
        "lang" => "en",
        "st" => "masterpiece",
        "frame" => "2015",
        "border" => "black",
        "game" => "paper",
        other => panic!("no sample value for {other}"),
    };
    for row in chip_scryfall::printing::PRINTING_KEYS {
        for word in *row {
            let term = format!("{word}:{}", sample(row[0]));
            assert!(parse_printing(&term).is_ok(), "{term}");
        }
    }
}
