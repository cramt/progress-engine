//! Tests for the legality rules a single card decides on its own.
//!
//! The failure worth guarding is a violation reported from no evidence. The
//! index is a cache and the fixtures are hand-written subsets of it, so a
//! legality field is often simply absent; if absent read as "illegal" the
//! checker cries wolf over a perfectly legal deck, which is worse than not
//! checking at all. Every rule here therefore gets a missing-data case as well
//! as a real one.

use pe_scryfall::index::Card;
use pe_scryfall::legality::{CommanderLegality, CommanderRoute};
use pe_scryfall::Colors;

/// Cards are parsed from index JSON rather than built as struct literals,
/// because a field can only go missing on the way in, and missing fields are
/// half of what these tests are about.
fn card(json: &str) -> Card {
    facet_json::from_str(json).expect("card fixture should parse")
}

fn colors(letters: &str) -> Colors {
    Colors::from_letters(letters).expect("colour letters should parse")
}

#[test]
fn an_absent_legality_field_is_unknown_rather_than_illegal() {
    // Exactly the shape of the pe-cli fixture index, which predates the field.
    let solemn =
        card(r#"{"name":"Solemn Simulacrum","type_line":"Artifact Creature — Golem","cmc":4.0}"#);
    assert_eq!(solemn.commander_legality(), CommanderLegality::Unknown);
    assert_eq!(solemn.may_be_played_in_commander(), None);
}

#[test]
fn a_word_the_index_grows_later_is_unknown_rather_than_illegal() {
    for word in ["restricted", "future", ""] {
        let c = card(&format!(
            r#"{{"name":"Whatever","commander_legal":"{word}"}}"#
        ));
        assert_eq!(
            c.commander_legality(),
            CommanderLegality::Unknown,
            "word {word:?}"
        );
        assert_eq!(c.may_be_played_in_commander(), None, "word {word:?}");
    }
}

#[test]
fn the_three_words_the_index_actually_uses() {
    // Surveyed across the whole 25MB index: legal, not_legal, banned, nothing
    // else. Banned is kept distinct from not_legal because a banned card is a
    // deckbuilding mistake and a not_legal one is usually an Un-set or an
    // online-only printing.
    let cases = [
        ("legal", CommanderLegality::Legal, Some(true)),
        ("not_legal", CommanderLegality::NotLegal, Some(false)),
        ("banned", CommanderLegality::Banned, Some(false)),
    ];
    for (word, want, playable) in cases {
        let c = card(&format!(
            r#"{{"name":"Whatever","commander_legal":"{word}"}}"#
        ));
        assert_eq!(c.commander_legality(), want, "word {word:?}");
        assert_eq!(c.may_be_played_in_commander(), playable, "word {word:?}");
    }
}

#[test]
fn the_basic_supertype_lifts_the_limit_whatever_any_number_says() {
    // Every one of these carries `any_number: false` in the real index — the
    // supertype is what lifts the limit, and it has to work in an index that
    // lacks the field entirely too. The last is Omnipresent Impostor, a Basic
    // Creature: rule 100.2a is about the supertype, not about lands.
    for type_line in [
        "Basic Land — Forest",
        "Basic Land",
        "Basic Snow Land — Island",
        "Basic Snow Land",
        "Basic Creature — Shapeshifter",
    ] {
        for any_number in [r#","any_number":false"#, ""] {
            let c = card(&format!(
                r#"{{"name":"Whatever","type_line":"{type_line}"{any_number}}}"#
            ));
            assert_eq!(
                c.may_appear_any_number_of_times(),
                Some(true),
                "type line {type_line:?} with {any_number:?}"
            );
        }
    }
}

#[test]
fn a_land_that_merely_has_a_basic_land_type_is_still_singleton() {
    // The distinction the type line draws is supertype versus subtype: Taiga
    // has the Forest type without the Basic supertype, and two Taigas is a
    // rules violation and a very expensive one.
    for type_line in [
        "Land — Mountain Forest",       // Taiga
        "Land Creature — Forest Dryad", // Dryad Arbor
        "Snow Land",                    // Arctic Treeline and friends
    ] {
        let c = card(&format!(
            r#"{{"name":"Whatever","type_line":"{type_line}","any_number":false}}"#
        ));
        assert_eq!(
            c.may_appear_any_number_of_times(),
            Some(false),
            "type line {type_line:?}"
        );
    }
}

#[test]
fn a_card_that_says_so_may_be_repeated() {
    let petitioners = card(
        r#"{"name":"Persistent Petitioners","type_line":"Creature — Human Advisor",
            "any_number":true,"commander_legal":"legal"}"#,
    );
    assert_eq!(petitioners.may_appear_any_number_of_times(), Some(true));
}

#[test]
fn an_absent_any_number_is_unknown_rather_than_a_singleton_violation() {
    // Persistent Petitioners without the field. A checker that reads this as
    // "one copy only" fails a legal deck of thirty Advisors.
    let petitioners =
        card(r#"{"name":"Persistent Petitioners","type_line":"Creature — Human Advisor"}"#);
    assert_eq!(petitioners.may_appear_any_number_of_times(), None);
}

#[test]
fn the_ordinary_route_to_the_command_zone_is_a_legendary_creature() {
    let thrasios = card(
        r#"{"name":"Thrasios, Triton Hero","type_line":"Legendary Creature — Merfolk Wizard",
            "oracle":"Partner (You can have two commanders if both have partner.)"}"#,
    );
    assert_eq!(
        thrasios.commander_route(),
        Some(CommanderRoute::LegendaryCreature)
    );
}

#[test]
fn ordinary_cards_cannot_command() {
    let cases = [
        (
            // Legendary, but not a creature.
            r#""name":"The One Ring","type_line":"Legendary Artifact""#,
            "legendary artifact",
        ),
        (
            r#""name":"Reclamation Sage","type_line":"Creature — Elf Shaman""#,
            "non-legendary creature",
        ),
        (
            r#""name":"Jace, the Mind Sculptor","type_line":"Legendary Planeswalker — Jace""#,
            "planeswalker that never says it can",
        ),
        (
            // The back face being a legendary creature is not enough; the
            // command zone only ever sees the front.
            r#""name":"Delver of Secrets // Insectile Aberration",
               "type_line":"Creature — Human Wizard // Legendary Creature — Human Insect""#,
            "legendary only on the back",
        ),
    ];
    for (body, what) in cases {
        assert_eq!(
            card(&format!("{{{body}}}")).commander_route(),
            None,
            "{what}"
        );
    }
}

#[test]
fn a_card_that_grants_itself_the_command_zone_in_its_own_text() {
    // The three printed shapes, all taken from the real index.
    let aminatou = card(
        r#"{"name":"Aminatou, the Fateshifter","type_line":"Legendary Planeswalker — Aminatou",
            "oracle":"+1: Draw a card, then put a card from your hand on top of your library.\nAminatou, the Fateshifter can be your commander."}"#,
    );
    assert_eq!(aminatou.commander_route(), Some(CommanderRoute::SaysSo));

    // Svega names itself by the short name printed before the comma.
    let svega = card(
        r#"{"name":"Svega, the Unconventional","type_line":"Legendary Planeswalker — Svega",
            "oracle":"Landfall — Whenever a land enters under your control, put a loyalty counter on target planeswalker.\nSvega can be your commander."}"#,
    );
    assert_eq!(svega.commander_route(), Some(CommanderRoute::SaysSo));

    // A spell commander says it inside reminder text, as "This card".
    let ransack = card(
        r#"{"name":"Ransack, the Lab","type_line":"Sorcery",
            "oracle":"Spell commander (This card can be your commander. In Limited, it can partner like other monocolored legends.)\nLook at the top three cards of your library."}"#,
    );
    assert_eq!(ransack.commander_route(), Some(CommanderRoute::SaysSo));
}

#[test]
fn a_background_commands_as_a_background_not_by_its_reminder_text() {
    // The trap: this reminder text contains "can be your commander", but it is
    // a promise about somebody else's creature. A substring search would call
    // this the SaysSo route and would also fire on any future card that
    // mentions the phrase in passing.
    let wizard = card(
        r#"{"name":"Wizard from Beyond","type_line":"Legendary Enchantment — Background",
            "oracle":"Create a Character (Any nonlegendary creature can choose this as its Background. It becomes legendary and can be your commander.)\nCommander creatures you own are Clerics, Rogues, Warriors, and Wizards in addition to their other types."}"#,
    );
    assert_eq!(
        wizard.commander_route(),
        Some(CommanderRoute::Background),
        "Backgrounds command by rule 903.3d, not by that sentence"
    );
}

#[test]
fn a_grant_must_name_the_card_it_is_printed_on() {
    // Word boundaries, not substrings: "Lucid" ends with "Cid".
    let cid =
        card(r#"{"name":"Cid","type_line":"Sorcery","oracle":"Lucid can be your commander."}"#);
    assert_eq!(cid.commander_route(), None);
}

#[test]
fn colour_identity_inside_and_outside_the_deck_s_colours() {
    let kodamas_reach = card(r#"{"name":"Kodama's Reach","ci":["G"],"type_line":"Sorcery"}"#);
    assert!(kodamas_reach.identity_fits_within(colors("BG")));
    assert!(kodamas_reach.identity_fits_within(colors("G")));
    assert!(!kodamas_reach.identity_fits_within(colors("WU")));

    // Both colours must be there: a two-colour card in a mono-green deck is
    // the single most common illegal inclusion there is.
    let trophy = card(r#"{"name":"Assassin's Trophy","ci":["B","G"],"type_line":"Instant"}"#);
    assert!(trophy.identity_fits_within(colors("BG")));
    assert!(!trophy.identity_fits_within(colors("G")));
}

#[test]
fn a_colourless_card_fits_every_identity_and_an_empty_one_admits_only_those() {
    let sol_ring = card(r#"{"name":"Sol Ring","ci":[],"type_line":"Artifact"}"#);
    assert!(sol_ring.identity_fits_within(colors("WUBRG")));
    assert!(sol_ring.identity_fits_within(colors("G")));
    // Kozilek's deck: an empty identity is a real deck, not "no restriction".
    assert!(sol_ring.identity_fits_within(Colors::default()));

    let sage =
        card(r#"{"name":"Reclamation Sage","ci":["G"],"type_line":"Creature — Elf Shaman"}"#);
    assert!(!sage.identity_fits_within(Colors::default()));
}
