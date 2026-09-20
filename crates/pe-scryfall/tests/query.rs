//! Tests for the Scryfall syntax subset.
//!
//! The failure tests matter as much as the matching ones: a query that silently
//! matches nothing yields a confidently wrong probability, which is the failure
//! this crate exists to prevent.

use pe_scryfall::index::{Card, Index, IndexFile, TagGap};
use pe_scryfall::legality::Legalities;
use pe_scryfall::{self as query, CardView, Colors, ParseError, Query};

/// An index that said nothing about legality, which is what a hand-written
/// fixture is. Shared because `CardView` borrows it and every literal below
/// needs one to point at.
fn silent() -> &'static Legalities {
    static SILENT: std::sync::OnceLock<Legalities> = std::sync::OnceLock::new();
    SILENT.get_or_init(Legalities::default)
}

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
        full_oracle: oracle,
        mana_cost: "",
        cmc,
        keywords: &[],
        color_identity: ci,
        colors: &[],
        produces: &[],
        rarity: "",
        set: "",
        layout: "normal",
        faces: &[],
        tags: &[],
        legalities: silent(),
        game_changer: None,
        reserved: None,
        categories: cats,
    }
}

/// A card the index gave keywords, which is all the `kw:` tests care about.
fn keyworded<'a>(name: &'a str, oracle: &'a str, keywords: &'a [String]) -> CardView<'a> {
    CardView {
        name,
        type_line: "Creature — Bird",
        oracle,
        full_oracle: oracle,
        mana_cost: "",
        cmc: 2.0,
        keywords,
        color_identity: &[],
        colors: &[],
        produces: &[],
        rarity: "",
        set: "",
        layout: "normal",
        faces: &[],
        tags: &[],
        legalities: silent(),
        game_changer: None,
        reserved: None,
        categories: &[],
    }
}

/// Keywords as the index prints them: Scryfall's own capitalisation, spaces
/// and all.
fn keywords(list: &[&str]) -> Vec<String> {
    list.iter().map(|k| k.to_string()).collect()
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
    // Keys that read data the index does not carry, or that Scryfall answers
    // from something other than a field, name themselves rather than matching
    // nothing.
    for term in ["cube:vintage", "edhrecrank<=100", "cn:5", "year>=2020"] {
        assert!(
            matches!(query::parse(term), Err(ParseError::UnknownKey { .. })),
            "{term} should be refused by name"
        );
    }
    // The land cycles are curated lists on Scryfall's side rather than
    // fields in the bulk data, so this crate declines them by name instead of
    // guessing at them from oracle text.
    assert!(matches!(
        query::parse("is:shockland"),
        Err(ParseError::UnknownIsProperty { .. })
    ));
    assert!(matches!(
        query::parse("is:tapland"),
        Err(ParseError::UnknownIsProperty { .. })
    ));
    assert!(matches!(
        query::parse("f:pauperr"),
        Err(ParseError::BadFormat { .. })
    ));
    assert!(matches!(
        query::parse("r:legendary"),
        Err(ParseError::BadRarity { .. })
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

/// `otag:` parses, and the check that it names a real tag lives at the index
/// rather than at the parser.
///
/// The same argument `kw:` already makes: a tag list hard-coded beside the
/// parser would be a second opinion about what Scryfall says, and there is no
/// deriving the answer from a card. So the parser accepts the term and the
/// index — which knows which tags it was told to fetch — is what refuses it.
#[test]
fn otag_parses_and_is_checked_against_the_index_not_the_parser() {
    assert!(matches!(
        query::parse("otag:surveil-land"),
        Ok(Query::Tag(t)) if t == "surveil-land"
    ));
    assert!(matches!(
        query::parse("oracletag:tapland"),
        Ok(Query::Tag(t)) if t == "tapland"
    ));
}

#[test]
fn error_messages_name_the_offending_term() {
    let err = query::parse("cube:vintage").unwrap_err().to_string();
    assert!(err.contains("cube"), "message should name the key: {err}");

    // Where the accepted values are a closed set, the message lists them:
    // a typo is worth one line of help rather than a silent 0%.
    let err = query::parse("f:pauperr").unwrap_err().to_string();
    assert!(
        err.contains("pauperr"),
        "message should name the value: {err}"
    );
    assert!(
        err.contains("paupercommander"),
        "message should list formats: {err}"
    );

    let err = query::parse("is:tapland").unwrap_err().to_string();
    assert!(
        err.contains("tapland"),
        "message should name the property: {err}"
    );
    assert!(
        err.contains("vanilla"),
        "message should list properties: {err}"
    );
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

#[test]
fn a_card_matches_only_the_keywords_it_has() {
    let birds = keywords(&["Flying"]);
    let birds = keyworded(
        "Birds of Paradise",
        "Flying\n{T}: Add one mana of any color.",
        &birds,
    );
    let bolt = keywords(&[]);
    let bolt = keyworded(
        "Lightning Bolt",
        "Lightning Bolt deals 3 damage to any target.",
        &bolt,
    );
    assert!(matches("kw:flying", &birds));
    assert!(!matches("kw:trample", &birds));
    assert!(!matches("kw:flying", &bolt));
}

#[test]
fn keyword_matching_ignores_case_the_way_scryfall_does() {
    // The index prints Scryfall's capitalisation ("Flying", "Double strike",
    // "Council's dilemma"), which nobody types.
    let kws = keywords(&["Flying", "Double strike"]);
    let c = keyworded("Adorned Pouncer", "Double strike", &kws);
    assert!(matches("kw:FLYING", &c));
    assert!(matches("kw:Flying", &c));
    assert!(matches(r#"kw:"DOUBLE STRIKE""#, &c));
    assert!(matches("keyword:flying", &c));
}

#[test]
fn a_keyword_is_a_whole_value_not_a_substring() {
    // The Plane-inside-planeswalker mistake, in a new place: a keyword is a
    // discrete entry in a list, so a prefix of one is not one.
    let kws = keywords(&["Trample"]);
    let c = keyworded("Colossal Dreadmaw", "Trample", &kws);
    assert!(matches("kw:trample", &c));
    assert!(!matches("kw:tramp", &c));
    assert!(!matches("kw:trampleover", &c));

    // Real data, not hypothetical: Scryfall lists "Hexproof from" separately,
    // and gives plain "Hexproof" to the cards that also have it. Whole-value
    // matching loses nothing because the index already says both.
    let hexproof = keywords(&["Hexproof from", "Hexproof"]);
    let valkyrie = keyworded(
        "Eradicator Valkyrie",
        "Hexproof from planeswalkers",
        &hexproof,
    );
    assert!(matches(r#"kw:"hexproof from""#, &valkyrie));
    assert!(matches("kw:hexproof", &valkyrie));

    let only_from = keywords(&["Hexproof from"]);
    let only_from = keyworded("Hypothetical", "Hexproof from red", &only_from);
    assert!(!matches("kw:hexproof", &only_from));
}

#[test]
fn kw_asks_what_a_card_has_where_o_asks_what_it_says() {
    // The reason the key is worth having, and the Kor Haven error in another
    // costume: Plummet says "flying" and has no keywords at all, so counting
    // evasive creatures with o:flying counts the card that kills them.
    let none = keywords(&[]);
    let plummet = keyworded("Plummet", "Destroy target creature with flying.", &none);
    assert!(matches("o:flying", &plummet));
    assert!(!matches("kw:flying", &plummet));
}

#[test]
fn a_card_the_index_never_gave_keywords_has_none_rather_than_panicking() {
    // Exactly the shape of the pe-cli fixture index, which predates the field.
    let solemn: Card = facet_json::from_str(
        r#"{"name":"Solemn Simulacrum","type_line":"Artifact Creature — Golem","cmc":4.0}"#,
    )
    .expect("card fixture should parse");
    let view = solemn.view(&[]);
    assert!(!matches("kw:flying", &view));
    assert!(matches("t:artifact", &view));
}

#[test]
fn kw_takes_a_value_not_a_comparison() {
    // Scryfall answers kw>=flying with "didn't match any cards"; a silent
    // no-match is the one thing this parser will not do.
    assert!(matches!(
        query::parse("kw>=flying"),
        Err(ParseError::NoComparison { .. })
    ));
    assert!(matches!(
        query::parse("kw!=flying"),
        Err(ParseError::NoComparison { .. })
    ));
    assert!(matches!(
        query::parse("kw:"),
        Err(ParseError::MissingValue { .. })
    ));
    let err = query::parse("kw<flying").unwrap_err().to_string();
    assert!(
        err.contains("kw<flying"),
        "message should name the term: {err}"
    );
    // `kw=flying` is a synonym Scryfall accepts, so we do too.
    assert_eq!(
        query::parse("kw=flying").unwrap(),
        Query::Keyword("flying".into())
    );
}

/// An index of these cards, written out and reopened.
///
/// Through the file rather than in memory because that is the only way the
/// vocabulary is ever read: `sync` derives it from the cards into the header,
/// and a run reads the header. Testing an in-memory shortcut would prove
/// nothing about the round trip that has to hold.
fn written(cards: &[(&str, &[&str])]) -> IndexFile {
    let mut index = Index {
        schema: Some(pe_scryfall::index::SCHEMA),
        ..Index::default()
    };
    for (name, keywords) in cards {
        index.cards.insert(
            pe_scryfall::index::keyname(name),
            Card {
                name: (*name).into(),
                keywords: keywords.iter().map(|k| (*k).to_string()).collect(),
                ..Card::default()
            },
        );
    }
    let text = index.to_lines().expect("an index should serialise");
    IndexFile::parse(std::path::Path::new("<memory>"), text).expect("and read back")
}

#[test]
fn a_keyword_no_card_carries_is_a_typo_the_index_can_name() {
    let index = written(&[("Birds of Paradise", &["Flying"])]);
    let vocabulary = index.keyword_vocabulary();
    let q = query::parse("kw:flyign or (t:land -kw:Flying)").expect("query should parse");
    assert_eq!(q.unknown_keywords(&vocabulary), vec!["flyign".to_string()]);
}

#[test]
fn an_index_silent_about_keywords_accuses_no_query_of_a_typo() {
    let index = written(&[("Plains", &[])]);
    let q = query::parse("kw:flying").expect("query should parse");
    assert!(q.unknown_keywords(&index.keyword_vocabulary()).is_empty());
}

/// An index carrying `tags`, written out and reopened, as above.
fn written_with_tags(tags: &[&str]) -> IndexFile {
    let index = Index {
        schema: Some(pe_scryfall::index::SCHEMA),
        tags: tags.iter().map(|t| (*t).to_string()).collect(),
        ..Index::default()
    };
    let text = index.to_lines().expect("an index should serialise");
    IndexFile::parse(std::path::Path::new("<memory>"), text).expect("and read back")
}

#[test]
fn a_tag_the_index_did_not_fetch_is_named_rather_than_counted_as_zero() {
    let index = written_with_tags(&["surveil", "scry"]);
    let q = query::parse("t:land (otag:mill or otag:surveil)").expect("query should parse");
    assert_eq!(
        q.tag_gap(&index.tag_vocabulary()),
        Some(TagGap::NotCarried(vec!["mill".to_string()]))
    );
}

#[test]
fn an_index_with_no_tags_says_so_rather_than_shrugging() {
    // Unlike a keyword vocabulary, an empty tag list is not silence: keywords
    // are derived from the cards, so an old index cannot know, whereas the tag
    // list is what sync fetched and the header states it. So `--from`'s tagless
    // index answers an `otag:` question with *I never asked*, which is what it
    // failed to do in #50.
    let index = written_with_tags(&[]);
    let q = query::parse("otag:surveil or otag:surveil").expect("query should parse");
    assert_eq!(
        q.tag_gap(&index.tag_vocabulary()),
        // Once, though it was named twice: a query is refused with the term,
        // not once per mention.
        Some(TagGap::NoneFetched(vec!["surveil".to_string()]))
    );
}

#[test]
fn a_tag_the_index_carries_is_no_gap_at_all() {
    let index = written_with_tags(&["surveil"]);
    let q = query::parse("t:land otag:surveil").expect("query should parse");
    assert_eq!(q.tag_gap(&index.tag_vocabulary()), None);
}
