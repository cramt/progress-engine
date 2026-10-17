//! Anything the card format accepts prints, or is refused for not fitting.
//!
//! The frame is Mirage Mockery's own, so no combination of types is
//! unprintable for want of a frame drawn for it: an enchantment creature
//! Class, a Saga battle, a land planeswalker. What can fail is space, and that
//! must be a refusal naming what overflowed, never a card printed with its
//! text spilling out of the frame or a Typst error.

use std::path::Path;

use mockery_core::card::Card;
use mockery_core::render::{self, Layout};
use mockery_core::symbols::Symbols;
use proptest::prelude::*;

const SYMBOLS: &[&str] = &["1", "2", "X", "W", "U", "B", "R", "G", "U/P", "T"];
const TYPES: &[&str] = &[
    "Artifact",
    "Battle",
    "Creature",
    "Enchantment",
    "Instant",
    "Kindred",
    "Land",
    "Planeswalker",
    "Sorcery",
];
const SUBTYPES: &[&str] = &[
    "Class",
    "Saga",
    "Vehicle",
    "Equipment",
    "Elf",
    "Phyrexian",
    "Siege",
];
const WORDS: &[&str] = &[
    "target",
    "creature",
    "you",
    "control",
    "draw",
    "a",
    "card",
    "each",
    "opponent",
    "loses",
    "life",
    "until",
    "end",
    "of",
    "turn",
    "gets",
    "+1/+1",
    "counter",
    "on",
    "it",
    "proliferate.",
    "whenever",
    "enters,",
    "sacrifice",
    "artifact",
    "Phyrexian",
    "Indestructible",
];

fn symbol() -> impl Strategy<Value = String> {
    prop::sample::select(SYMBOLS).prop_map(|s| format!("{{{s}}}"))
}

fn paragraph() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        6 => prop::sample::select(WORDS).prop_map(String::from),
        1 => symbol(),
        1 => prop::collection::vec(prop::sample::select(WORDS), 1..8)
            .prop_map(|w| format!("({})", w.join(" "))),
    ];
    prop::collection::vec(piece, 1..40).prop_map(|p| p.join(" "))
}

fn card_toml() -> impl Strategy<Value = String> {
    (
        "[A-Z][a-z]{0,10}( [A-Z][a-z]{0,10}){0,4}",
        prop::collection::vec(prop::sample::select(SYMBOLS).prop_filter("a cost has no {T}", |s| *s != "T"), 0..8),
        any::<bool>(),
        prop::sample::subsequence(TYPES, 1..4),
        prop::sample::subsequence(SUBTYPES, 0..3),
        prop::collection::vec(paragraph(), 0..6),
        prop::option::of(paragraph()),
    )
        .prop_map(|(name, cost, legendary, types, subtypes, rules, flavor)| {
            let mut type_line = String::new();
            if legendary {
                type_line.push_str("Legendary ");
            }
            type_line.push_str(&types.join(" "));
            if !subtypes.is_empty() {
                type_line.push_str(" — ");
                type_line.push_str(&subtypes.join(" "));
            }
            let has = |w: &str| types.contains(&w) || subtypes.contains(&w);
            let corner = if has("Creature") || has("Vehicle") {
                "pt = \"3/3\"\n"
            } else if has("Planeswalker") {
                "loyalty = \"4\"\n"
            } else if has("Battle") {
                "defense = \"5\"\n"
            } else {
                ""
            };
            let cost: String = cost.iter().map(|s| format!("{{{s}}}")).collect();
            let flavor = flavor.map(|f| format!("flavor = \"{f}\"\n")).unwrap_or_default();
            format!(
                "name = \"{name}\"\ncost = \"{cost}\"\ntype = \"{type_line}\"\nrules = \"\"\"\n{}\n\"\"\"\n{flavor}{corner}",
                rules.join("\n")
            )
        })
}

proptest! {
    // A case is a Typst layout: 96 take 7s optimised, which is how the flake
    // runs them, and two minutes at the dev profile, where 24 have to do.
    #![proptest_config(ProptestConfig::with_cases(if cfg!(debug_assertions) { 24 } else { 96 }))]

    #[test]
    fn every_card_the_format_accepts_prints_or_is_refused_for_space(toml in card_toml()) {
        let card = Card::parse(&toml, Path::new(".")).expect("the generator writes only valid cards");
        let symbols = Symbols::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/symbols")).unwrap();
        match render::compile(&[card], &symbols, Layout::Cards) {
            Ok(doc) => prop_assert_eq!(doc.pages().len(), 1),
            Err(e) => {
                let e = format!("{e:#}");
                prop_assert!(e.contains("does not fit"), "{}\n{}", e, toml);
            }
        }
    }
}

#[test]
fn rules_text_too_long_for_any_size_is_refused() {
    let rules = "Draw a card. ".repeat(200);
    let card = Card::parse(
        &format!("name = \"Wall of Text\"\ntype = \"Sorcery\"\nrules = \"{rules}\""),
        Path::new("."),
    )
    .unwrap();
    let symbols =
        Symbols::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/symbols")).unwrap();
    let err = render::compile(&[card], &symbols, Layout::Cards).unwrap_err();
    assert!(
        format!("{err:#}").contains("the rules text does not fit"),
        "{err:#}"
    );
}

#[test]
fn a_name_too_long_for_one_line_is_refused() {
    let card = Card::parse(
        "name = \"Gitaxian Progress Tyrant of the Ninefold Spires and All Their Proofsheets\"\ntype = \"Sorcery\"",
        Path::new("."),
    )
    .unwrap();
    let symbols =
        Symbols::open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/symbols")).unwrap();
    let err = render::compile(&[card], &symbols, Layout::Cards).unwrap_err();
    assert!(
        format!("{err:#}").contains("the name does not fit"),
        "{err:#}"
    );
}
