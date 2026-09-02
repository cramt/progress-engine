//! Tests for the Scryfall syntax subset.
//!
//! The failure tests matter as much as the matching ones: a query that silently
//! matches nothing yields a confidently wrong probability, which is the failure
//! this crate exists to prevent.

use pe_scryfall::{self as query, CardView, Colors, ParseError, Query};

fn card<'a>(
    name: &'a str,
    type_line: &'a str,
    oracle: &'a str,
    cmc: f64,
    ci: &'a [String],
    cats: &'a [String],
) -> CardView<'a> {
    CardView {
        name,
        type_line,
        oracle,
        cmc,
        color_identity: ci,
        categories: cats,
    }
}

fn white() -> Vec<String> {
    vec!["W".to_string()]
}

fn matches(q: &str, c: &CardView<'_>) -> bool {
    query::parse(q).expect("query should parse").matches(c)
}

#[test]
fn type_and_oracle_are_case_insensitive_substrings() {
    let ci = white();
    let cats = vec![];
    let plains = card(
        "Plains",
        "Basic Land — Plains",
        "({T}: Add {W}.)",
        0.0,
        &ci,
        &cats,
    );
    assert!(matches("t:land", &plains));
    assert!(matches("t:LAND", &plains));
    assert!(matches(r#"o:"Add {W}""#, &plains));
    assert!(!matches("t:creature", &plains));
}

#[test]
fn the_kor_haven_trap() {
    // The bug that motivated this crate: Kor Haven taps for {C}, but a naive
    // grep for {W} matches the {W} in its activation cost and miscounts it as a
    // white source. The query language must let you say what you actually mean.
    let ci = white();
    let cats = vec![];
    let kor_haven = card(
        "Kor Haven",
        "Legendary Land",
        "{T}: Add {C}.\n{1}{W}, {T}: Prevent all combat damage that would be dealt by target attacking creature this turn.",
        0.0,
        &ci,
        &cats,
    );
    // Naive: matches, and is wrong.
    assert!(matches(r#"o:"{W}""#, &kor_haven));
    // Precise: does not match, and is right.
    assert!(!matches(r#"o:"Add {W}""#, &kor_haven));
}

#[test]
fn mana_value_comparators() {
    let ci = white();
    let cats = vec![];
    let c = card(
        "Senu",
        "Legendary Creature — Bird Scout",
        "Flying",
        2.0,
        &ci,
        &cats,
    );
    assert!(matches("mv<=2", &c));
    assert!(matches("mv=2", &c));
    assert!(matches("cmc:2", &c));
    assert!(matches("mv<3", &c));
    assert!(!matches("mv>2", &c));
    assert!(matches("mv!=3", &c));
}

#[test]
fn identity_is_subset_not_ordering() {
    let cats = vec![];
    let wu = vec!["W".to_string(), "U".to_string()];
    let w = white();
    let azorius = card("Thing", "Creature", "", 2.0, &wu, &cats);
    let mono_white = card("Other", "Creature", "", 2.0, &w, &cats);
    // "fits inside a mono-white deck"
    assert!(matches("id<=W", &mono_white));
    assert!(!matches("id<=W", &azorius));
    assert!(matches("id<=WU", &azorius));
    assert!(matches("id:W", &mono_white)); // `id:` is a synonym for `id<=`
    assert!(matches("id=WU", &azorius));
}

#[test]
fn categories_come_from_the_decklist() {
    let ci = white();
    let cats = vec!["Exile Outlet".to_string(), "Big Colorless".to_string()];
    let c = card("Safe Haven", "Land", "", 0.0, &ci, &cats);
    assert!(matches(r#"cat:"Exile Outlet""#, &c));
    assert!(matches(r#"cat:"exile outlet""#, &c));
    assert!(!matches(r#"cat:"Draw""#, &c));
}

#[test]
fn boolean_combinators() {
    let ci = white();
    let cats = vec![];
    let teshar = card(
        "Teshar, Ancestor's Apostle",
        "Legendary Creature — Bird Cleric",
        "Flying",
        4.0,
        &ci,
        &cats,
    );
    assert!(matches("t:legendary t:creature", &teshar)); // implicit AND
    assert!(matches("t:legendary and t:creature", &teshar)); // explicit AND is a no-op
    assert!(matches("(o:flying or o:menace)", &teshar));
    assert!(matches("t:creature -t:land", &teshar));
    assert!(!matches("t:creature -t:legendary", &teshar));
    assert!(matches("t:instant or t:creature", &teshar));
}

#[test]
fn is_properties() {
    let ci = white();
    let cats = vec![];
    let land = card("Plains", "Basic Land — Plains", "", 0.0, &ci, &cats);
    let bolt = card("Swords", "Instant", "", 1.0, &ci, &cats);
    let teshar = card("Teshar", "Legendary Creature — Bird", "", 4.0, &ci, &cats);
    assert!(matches("is:permanent", &land));
    assert!(!matches("is:permanent", &bolt));
    assert!(matches("is:spell", &bolt));
    assert!(!matches("is:spell", &land));
    assert!(matches("is:historic", &teshar)); // legendary counts
}

#[test]
fn quoted_or_is_a_value_not_an_operator() {
    let ci = white();
    let cats = vec![];
    let c = card("Orzhov Signet", "Artifact", "", 2.0, &ci, &cats);
    assert!(matches(r#"name:"Or""#, &c));
}

#[test]
fn unsupported_syntax_errors_rather_than_matching_nothing() {
    assert!(matches!(
        query::parse("power>=3"),
        Err(ParseError::UnknownKey { .. })
    ));
    assert!(matches!(
        query::parse("is:vanilla"),
        Err(ParseError::UnknownIsProperty { .. })
    ));
    assert!(matches!(
        query::parse("mv<=notanumber"),
        Err(ParseError::BadNumber { .. })
    ));
    assert!(matches!(
        query::parse("id<=xyz"),
        Err(ParseError::BadColors { .. })
    ));
    assert!(matches!(
        query::parse("t:"),
        Err(ParseError::MissingValue { .. })
    ));
    assert!(matches!(
        query::parse("(t:land"),
        Err(ParseError::UnbalancedParen)
    ));
    assert!(matches!(
        query::parse("t:land)"),
        Err(ParseError::UnbalancedParen)
    ));
    assert!(matches!(query::parse("   "), Err(ParseError::Empty)));
}

#[test]
fn error_messages_name_the_offending_term() {
    let err = query::parse("power>=3").unwrap_err().to_string();
    assert!(err.contains("power"), "message should name the key: {err}");
}

#[test]
fn bare_word_is_a_name_substring() {
    assert_eq!(query::parse("Senu").unwrap(), Query::Name("Senu".into()));
}

#[test]
fn colors_parse_from_letters() {
    assert_eq!(Colors::from_letters("wu"), Colors::from_letters("UW"));
    assert!(Colors::from_letters("c").is_some());
    assert!(Colors::from_letters("z").is_none());
}
