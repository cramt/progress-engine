//! The search syntax, exercised end to end against real Scryfall records.
//!
//! `query.rs` tests the parser on hand-built views; this file goes through the
//! whole path a real run takes — bulk record, index, `CardView`, query — so
//! that a key which parses but reads the wrong field is caught. Several of
//! these keys exist precisely because reading the wrong field is the bug this
//! project is about, and a test that built the view by hand would be free to
//! put the right value in the wrong place.

use pe_scryfall::bulk::BulkCard;
use pe_scryfall::index::{Card, Index};

const SAMPLE: &str = include_str!("fixtures/bulk-sample.jsonl");

fn index() -> Index {
    let records: Vec<BulkCard> = SAMPLE
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| facet_json::from_str(line).expect("real bulk record should parse"))
        .collect();
    Index::build(records, None).0
}

fn get(index: &Index, name: &str) -> Card {
    index
        .get(name)
        .unwrap_or_else(|| panic!("{name} indexed"))
        .clone()
}

fn hits(index: &Index, query: &str) -> Vec<String> {
    let q = pe_scryfall::parse(query).unwrap_or_else(|e| panic!("{query:?}: {e}"));
    let mut names: Vec<String> = index
        .cards
        .values()
        .filter(|c| q.matches(&c.view(&[])))
        .map(|c| c.name.clone())
        .collect();
    names.sort();
    names
}

fn matches(card: &Card, query: &str) -> bool {
    pe_scryfall::parse(query)
        .unwrap_or_else(|e| panic!("{query:?}: {e}"))
        .matches(&card.view(&[]))
}

/// A card's colour and its colour identity are different facts, and conflating
/// them is the classic quiet error. Kor Haven is the clearest case in the
/// fixture: it is a colourless land whose identity is white.
#[test]
fn colour_is_not_colour_identity() {
    let index = index();
    let kor_haven = get(&index, "Kor Haven");

    assert!(matches(&kor_haven, "id:w"), "its identity is white");
    assert!(matches(&kor_haven, "c:c"), "its colour is nothing at all");
    assert!(!matches(&kor_haven, "c:w"), "it is not a white card");
    assert!(
        !matches(&kor_haven, "id:c"),
        "its identity is not colourless"
    );
}

/// The single most pasted-and-misread part of Scryfall's syntax: a colon means
/// "contains all of" for `c:` and "fits inside" for `id:`. They point opposite
/// ways, so a query that meant one and got the other answers about a different
/// deck entirely.
#[test]
fn a_colon_means_contains_for_colour_and_fits_inside_for_identity() {
    let index = index();
    let birds = get(&index, "Birds of Paradise"); // mono-green, five-colour identity
    let temple = get(&index, "Temple Garden"); // colourless card, GW identity

    assert!(matches(&birds, "c:g"));
    assert!(!matches(&birds, "c:gw"), "c: is AND: Birds is not white");
    assert!(matches(&birds, "c<=g"), "and it does fit inside green");

    assert!(matches(&temple, "id:gw"), "id: is fits-inside");
    assert!(matches(&temple, "id:gwu"), "and a wider deck still fits it");
    assert!(!matches(&temple, "id:g"), "but a mono-green one does not");
    assert!(matches(&temple, "id>=gw"), "asked the other way round");
}

#[test]
fn colours_can_be_named_by_their_nicknames() {
    let index = index();
    let temple = get(&index, "Temple Garden");
    let birds = get(&index, "Birds of Paradise");

    assert!(
        matches(&temple, "id:selesnya"),
        "the guild name is the pair"
    );
    assert!(matches(&temple, "id:gw"), "and means exactly the letters");
    assert!(matches(&temple, "id:bant"), "a shard that contains it");
    assert!(
        !matches(&temple, "id:green"),
        "but a Selesnya card does not fit inside mono-green"
    );
    assert!(matches(&birds, "c:green"), "a colour name is a colour");
}

#[test]
fn colours_can_be_counted() {
    let index = index();
    let birds = get(&index, "Birds of Paradise");
    let wear = get(&index, "Wear // Tear"); // {1}{R} // {W}

    assert!(matches(&birds, "c=1"));
    assert!(matches(&wear, "c=2"));
    assert!(matches(&wear, "c:m"), "multicolour is two or more");
    assert!(!matches(&birds, "c:m"));
    assert!(matches(&wear, "c>=2"));
}

/// Issue #3, and the bug the README opens with. `o:"{W}"` finds Kor Haven,
/// whose `{W}` is in an activation cost; `produces:w` does not, because
/// producing mana is a fact Scryfall records rather than one you regex for.
#[test]
fn produces_asks_what_a_card_makes_not_what_its_text_says() {
    let index = index();

    assert!(
        hits(&index, r#"o:"{W}""#).contains(&"Kor Haven".to_string()),
        "the loaded gun is still loaded"
    );
    assert_eq!(
        hits(&index, "produces:w"),
        ["Birds of Paradise", "Temple Garden"],
        "and pointed at the cards that actually make white"
    );

    let kor_haven = get(&index, "Kor Haven");
    assert!(matches(&kor_haven, "produces:c"), "it makes colourless");
    assert!(!matches(&kor_haven, "produces:w"));
}

/// `produces:` takes multiple letters as AND, like `c:` and unlike `id:`.
#[test]
fn produces_with_several_letters_wants_all_of_them() {
    let index = index();
    let birds = get(&index, "Birds of Paradise");
    let temple = get(&index, "Temple Garden");

    assert!(matches(&birds, "produces:wubrg"));
    assert!(matches(&temple, "produces:gw"));
    assert!(!matches(&temple, "produces:gwu"));
    assert!(matches(&birds, "produces>=3"));
    assert!(!matches(&temple, "produces>=3"));
}

/// Any face may satisfy a statistic. Delver of Secrets is a 1/1 that becomes a
/// 3/2, and a search for three power that missed it would be answering about
/// the front of the card rather than about the card.
#[test]
fn power_and_toughness_read_every_face() {
    let index = index();
    let delver = get(&index, "Delver of Secrets // Insectile Aberration");

    assert!(matches(&delver, "pow=1"), "the front face");
    assert!(matches(&delver, "pow>=3"), "and the back one");
    assert!(matches(&delver, "pow>tou"), "3/2 is top-heavy");
    assert!(matches(&delver, "pt=5"), "power plus toughness");
    assert!(!matches(&delver, "pow>=4"));
}

/// `*` is a real printed power and is not a number. Calling it zero would put
/// Tarmogoyf in `pow=0`; the honest answer is that no numeric comparison holds,
/// in either direction, so `-pow>=1` is where "we cannot say" lands.
#[test]
fn a_starred_power_satisfies_no_numeric_comparison() {
    let index = index();
    let goyf = get(&index, "Tarmogoyf");
    assert_eq!(goyf.faces[0].power.as_deref(), Some("*"));

    assert!(!matches(&goyf, "pow>=1"));
    assert!(!matches(&goyf, "pow<=1"));
    assert!(!matches(&goyf, "pow=0"));
    assert!(matches(&goyf, "-pow>=1"));
}

#[test]
fn rarity_compares_in_scryfalls_order() {
    let index = index();
    let sol_ring = get(&index, "Sol Ring"); // uncommon in this printing
    assert_eq!(sol_ring.rarity, "uncommon");

    assert!(matches(&sol_ring, "r:uncommon"));
    assert!(matches(&sol_ring, "r:u"));
    assert!(matches(&sol_ring, "r>=common"));
    assert!(!matches(&sol_ring, "r>=rare"));
    assert!(matches(&sol_ring, "r<rare"));
}

#[test]
fn format_legality_is_asked_by_name() {
    let index = index();
    let sol_ring = get(&index, "Sol Ring");
    let goyf = get(&index, "Tarmogoyf");

    assert!(matches(&sol_ring, "f:commander"));
    assert!(!matches(&sol_ring, "f:modern"));
    assert!(matches(&goyf, "f:modern"));
    assert!(matches(&goyf, "f:legacy"));
    // Nothing in the fixture is banned anywhere, which is itself worth
    // asserting: `banned:` must not fall back to matching everything.
    assert_eq!(hits(&index, "banned:commander"), Vec::<String>::new());
}

#[test]
fn is_properties_read_the_layout_and_the_cost() {
    let index = index();
    let delver = get(&index, "Delver of Secrets // Insectile Aberration");
    let wear = get(&index, "Wear // Tear");
    let giant = get(&index, "Bonecrusher Giant // Stomp");

    assert!(matches(&delver, "is:transform"));
    assert!(matches(&delver, "is:dfc"));
    assert!(!matches(&delver, "is:mdfc"));
    assert!(matches(&wear, "is:split"));
    assert!(!matches(&wear, "is:dfc"), "a split card has one side");
    assert!(matches(&giant, "is:adventure"));

    // `not:` is Scryfall's inverted `is:`.
    assert!(matches(&wear, "not:dfc"));
}

#[test]
fn a_vanilla_creature_is_one_with_no_rules_text() {
    let index = index();
    let goyf = get(&index, "Tarmogoyf");
    let birds = get(&index, "Birds of Paradise");
    assert!(
        !matches(&goyf, "is:vanilla"),
        "Tarmogoyf defines its own P/T"
    );
    assert!(!matches(&birds, "is:vanilla"));
}

#[test]
fn commander_eligibility_is_a_query() {
    let index = index();
    assert_eq!(hits(&index, "is:commander"), Vec::<String>::new());
    // Nothing in the fixture is legendary; the rule itself is tested against
    // real commanders in legality.rs. What matters here is that the key routes
    // to that rule rather than to a guess.
}

/// `o:` and `fo:` differ by exactly the reminder text, which is why both exist.
#[test]
fn oracle_and_full_oracle_differ_by_the_reminder_text() {
    let index = index();
    let angel = get(&index, "Serra Angel");

    assert!(matches(&angel, "o:vigilance"));
    assert!(matches(&angel, "fo:vigilance"));
    assert!(
        !matches(&angel, r#"o:"Attacking doesn't cause""#),
        "`o:` does not search reminder text"
    );
    assert!(
        matches(&angel, r#"fo:"Attacking doesn't cause""#),
        "`fo:` does"
    );
}

#[test]
fn mana_value_has_a_parity() {
    let index = index();
    let sol_ring = get(&index, "Sol Ring"); // mv 1
    let birds = get(&index, "Birds of Paradise"); // mv 1
    let goyf = get(&index, "Tarmogoyf"); // mv 2

    assert!(matches(&sol_ring, "mv:odd"));
    assert!(matches(&birds, "mv:odd"));
    assert!(matches(&goyf, "mv:even"));
    assert!(!matches(&goyf, "mv:odd"));
}

#[test]
fn a_set_code_selects_the_printing_the_index_carries() {
    let index = index();
    let sol_ring = get(&index, "Sol Ring");
    assert!(matches(&sol_ring, &format!("s:{}", sol_ring.set)));
    assert!(!matches(&sol_ring, "s:lea"));
}

/// A second fixture, for the `is:` properties whose derivations were wrong
/// until they were checked against Scryfall's own answers.
fn properties() -> Index {
    let records: Vec<BulkCard> = include_str!("fixtures/bulk-properties.jsonl")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| facet_json::from_str(line).expect("real bulk record should parse"))
        .collect();
    Index::build(records, None).0
}

/// Most Phyrexian mana is in an activation cost rather than a mana cost.
/// Blinding Souleater costs `{3}` and its only `{W/P}` is in its ability, and
/// reading the mana cost alone found forty of the seventy-three cards Scryfall
/// does.
#[test]
fn phyrexian_mana_is_found_in_a_cost_a_card_pays_as_well_as_the_one_it_is_cast_for() {
    let index = properties();
    let souleater = get(&index, "Blinding Souleater");
    assert_eq!(
        souleater.mana_cost, "{3}",
        "no Phyrexian symbol in the cost"
    );
    assert!(matches(&souleater, "is:phyrexian"));
    // Hybrid is read from the printed cost only, which is Scryfall's asymmetry
    // rather than ours: counting activation costs finds cards it does not.
    assert!(!matches(&souleater, "is:hybrid"));
    assert!(matches(&get(&index, "Boros Charm"), "c:rw"));
}

/// Every flavour of the partner mechanic prints as its own keyword, and a
/// Background carries none at all — it is a subtype. Matching only the word
/// "partner" found a hundred and forty of the two hundred and twenty-eight
/// cards Scryfall finds.
#[test]
fn partner_covers_every_flavour_of_the_mechanic() {
    let index = properties();
    let abdel = get(&index, "Abdel Adrian, Gorion's Ward");
    let adric = get(&index, "Adric, Mathematical Genius");
    let background = get(&index, "Raised by Giants");

    assert!(matches(&abdel, "is:partner"), "Choose a Background");
    assert!(matches(&adric, "is:partner"), "Doctor's companion");
    assert!(matches(&background, "is:partner"), "the Background itself");
    assert!(!matches(&get(&index, "Grizzly Bears"), "is:partner"));
}

/// Scryfall counts a Background as something that can be your commander — all
/// thirty of them — even though one cannot command on its own.
#[test]
fn a_background_counts_as_a_commander_the_way_scryfall_counts_it() {
    let index = properties();
    assert!(matches(&get(&index, "Raised by Giants"), "is:commander"));
    assert!(matches(
        &get(&index, "Abdel Adrian, Gorion's Ward"),
        "is:commander"
    ));
    assert!(!matches(&get(&index, "Grizzly Bears"), "is:commander"));
}

#[test]
fn a_bear_is_a_two_mana_two_by_two() {
    let index = properties();
    assert!(matches(&get(&index, "Grizzly Bears"), "is:bear"));
    assert!(matches(&get(&index, "Grizzly Bears"), "is:vanilla"));
    assert!(!matches(&get(&index, "Blinding Souleater"), "is:vanilla"));
}

/// French vanilla is not derivable from the bulk data and is declined by name.
///
/// Scryfall's `keywords` array mixes ability words — "Mark of Chaos Ascendant"
/// — in with real keyword abilities, and its own answer excludes keywords with
/// numeric parameters such as `Modular 3` and `Rampage 3` for reasons no field
/// records. Every derivation tried came out twenty per cent away from
/// Scryfall's, in one direction or the other, and a key that looks like
/// Scryfall's and disagrees with it is worse than one that says it is missing.
#[test]
fn french_vanilla_is_declined_rather_than_approximated() {
    assert!(matches!(
        pe_scryfall::parse("is:frenchvanilla"),
        Err(pe_scryfall::ParseError::UnknownIsProperty { .. })
    ));
}

/// A mana cost is a multiset of symbols, not a string. `m:` means "contains at
/// least", matching Scryfall, so `m:{G}` finds every green card rather than the
/// one card that costs exactly `{G}`.
#[test]
fn mana_costs_compare_as_multisets_of_symbols() {
    let index = index();
    let angel = get(&index, "Serra Angel"); // {3}{W}{W}
    let birds = get(&index, "Birds of Paradise"); // {G}

    assert!(matches(&angel, "m:{W}{W}"));
    assert!(matches(&angel, "m:{W}"), "a colon is at least, not exactly");
    assert!(!matches(&angel, "m:{W}{W}{W}"));
    assert!(matches(&angel, "m={3}{W}{W}"), "and = is exactly");
    assert!(!matches(&angel, "m={W}{W}"));
    assert!(matches(&angel, "m>{2}{W}{W}"), "five is more than four");
    assert!(!matches(&angel, "m>{3}{W}{W}"), "and not more than itself");

    // Shorthand is allowed for symbols that are not split.
    assert_eq!(
        pe_scryfall::parse("m:3WW").unwrap(),
        pe_scryfall::parse("m:{3}{W}{W}").unwrap()
    );
    assert!(matches(&birds, "m=G"));
}

/// Half of Wear // Tear costs `{1}{R}` and the other half `{W}`; neither is
/// "the cost of the card", so each face is asked separately. Reading the stored
/// `{1}{R} // {W}` as one multiset would invent a three-mana two-colour spell
/// that nobody can cast.
#[test]
fn each_face_is_costed_separately() {
    let index = index();
    let wear = get(&index, "Wear // Tear");
    assert_eq!(wear.mana_cost, "{1}{R} // {W}");

    assert!(matches(&wear, "m:{W}"), "the Tear half");
    assert!(matches(&wear, "m:{1}{R}"), "the Wear half");
    assert!(
        !matches(&wear, "m:{R}{W}"),
        "no single face costs both, and no cost is the two joined"
    );
}

/// A hybrid symbol has a canonical order and a query may not use it.
#[test]
fn a_hybrid_symbol_reads_the_same_written_either_way() {
    use pe_scryfall::mana::ManaCost;
    assert_eq!(ManaCost::parse("{U/W}"), ManaCost::parse("{W/U}"));
    // But a marker is not a colour and its position is fixed: sorting `{W/P}`
    // would invent a symbol.
    assert_ne!(ManaCost::parse("{W/P}"), ManaCost::parse("{P/W}"));
    assert_eq!(ManaCost::parse("2WW"), ManaCost::parse("{2}{W}{W}"));
    assert_eq!(ManaCost::parse("{1}{1}"), ManaCost::parse("{2}"));
}

/// Only a permanent gives devotion, because only a permanent is on the
/// battlefield to give it. Counting the blue symbols on a Counterspell found
/// two thousand cards Scryfall does not.
#[test]
fn devotion_is_counted_only_on_permanents() {
    let index = index();
    let angel = get(&index, "Serra Angel"); // {3}{W}{W}, a creature
    let properties = properties();
    let charm = get(&properties, "Boros Charm"); // {R}{W}, an instant

    assert!(matches(&angel, "devotion:{w}{w}"));
    assert!(matches(&angel, "devotion:{w}"), "a colon is at least");
    assert!(!matches(&angel, "devotion:{w}{w}{w}"));
    assert!(matches(&angel, "devotion={w}{w}"), "and = is exactly");

    assert!(
        !matches(&charm, "devotion:{w}"),
        "an instant is never on the battlefield to give devotion"
    );
}

/// A devotion term asks about one colour or one pair. Two different colours in
/// one term is two questions, and answering either would be a guess.
#[test]
fn a_devotion_term_naming_two_colours_is_refused() {
    assert!(matches!(
        pe_scryfall::parse("devotion:{u}{b}"),
        Err(pe_scryfall::ParseError::MixedDevotion { .. })
    ));
    // The pair asked for together is fine, and is a different question.
    assert!(pe_scryfall::parse("devotion:{u/b}{u/b}").is_ok());
}
