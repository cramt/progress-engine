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

const NUMERALS: &[&str] = &[
    "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII",
];

/// What a layout needs its type line to have: chapters a Saga, level bars a
/// Class. Loyalty abilities are drawn as bands only on a planeswalker, and a
/// plain line anywhere else, so they need nothing.
#[derive(Debug, Clone, Copy)]
enum Needs {
    Nothing,
    Subtype(&'static str),
}

const COSTS: &[&str] = &[
    "+1",
    "+2",
    "\u{2212}1",
    "\u{2212}3",
    "\u{2212}X",
    "+X",
    "0",
    "-7",
];

/// Rules text in each layout a card's text can ask for, numbered as the
/// format accepts: plain paragraphs, a Saga's chapters, a Class's levels,
/// a station card's thresholds, the last maybe with power and toughness, or
/// loyalty abilities among static ones.
fn rules() -> impl Strategy<Value = (String, bool, Needs)> {
    let paragraphs = |n| prop::collection::vec(paragraph(), n);
    prop_oneof![
        paragraphs(0..6).prop_map(|p| (p.join("\n"), false, Needs::Nothing)),
        (
            paragraphs(0..2),
            prop::collection::vec((prop::sample::select(COSTS), paragraph()), 1..5),
            paragraphs(0..2)
        )
            .prop_map(|(before, abilities, after)| {
                let mut lines = before;
                lines.extend(
                    abilities
                        .into_iter()
                        .map(|(cost, text)| format!("{cost}: {text}")),
                );
                lines.extend(after);
                (lines.join("\n"), false, Needs::Nothing)
            }),
        (
            paragraphs(0..2),
            prop::collection::vec((1usize..4, paragraph()), 1..5),
            paragraphs(0..2)
        )
            .prop_map(|(before, chapters, after)| {
                let mut lines = before;
                let mut next = 0;
                for (span, text) in chapters {
                    let numerals = &NUMERALS[next..next + span];
                    next += span;
                    lines.push(format!("{} — {text}", numerals.join(", ")));
                }
                lines.extend(after);
                (lines.join("\n"), false, Needs::Subtype("Saga"))
            }),
        (
            paragraphs(0..3),
            prop::collection::vec(
                (prop::collection::vec(symbol(), 1..4), paragraphs(0..3)),
                1..4
            )
        )
            .prop_map(|(before, levels)| {
                let mut lines = before;
                for (n, (cost, text)) in levels.into_iter().enumerate() {
                    lines.push(format!("{}: Level {}", cost.concat(), n + 2));
                    lines.extend(text);
                }
                (lines.join("\n"), false, Needs::Subtype("Class"))
            }),
        (
            paragraphs(0..3),
            prop::collection::vec((1u32..6, paragraph(), paragraphs(0..2)), 1..4),
            any::<bool>()
        )
            .prop_map(|(before, thresholds, pt)| {
                let mut lines = before;
                let mut at = 0;
                for (step, first, rest) in thresholds {
                    at += step;
                    lines.push(format!("{at}+ | {first}"));
                    lines.extend(rest);
                }
                (lines.join("\n"), pt, Needs::Nothing)
            }),
    ]
}

/// One face's fields: its cost line apart, as a split half must have one.
fn face_toml() -> impl Strategy<Value = (String, String)> {
    (
        "[A-Z][a-z]{0,10}( [A-Z][a-z]{0,10}){0,4}",
        prop::collection::vec(prop::sample::select(SYMBOLS).prop_filter("a cost has no {T}", |s| *s != "T"), 0..8),
        any::<bool>(),
        prop::sample::subsequence(TYPES, 1..4),
        prop::sample::subsequence(SUBTYPES, 0..3),
        rules(),
        prop::option::of(paragraph()),
        prop::sample::subsequence(&["W", "U", "B", "R", "G"][..], 0..3),
    )
        .prop_map(|(name, cost, legendary, types, mut subtypes, (rules, station_pt, needs), flavor, indicator)| {
            if let Needs::Subtype(sub) = needs {
                if !subtypes.contains(&sub) {
                    subtypes.push(sub);
                }
            }
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
            } else if station_pt {
                "pt = \"4/4\"\n"
            } else {
                ""
            };
            let cost: String = cost.iter().map(|s| format!("{{{s}}}")).collect();
            let flavor = flavor.map(|f| format!("flavor = \"{f}\"\n")).unwrap_or_default();
            let indicator = if indicator.is_empty() {
                String::new()
            } else {
                format!("color_indicator = \"{}\"\n", indicator.concat())
            };
            (
                cost,
                format!(
                    "name = \"{name}\"\ntype = \"{type_line}\"\nrules = \"\"\"\n{rules}\n\"\"\"\n{flavor}{corner}{indicator}"
                ),
            )
        })
}

/// An inset spell: an instant or a sorcery of plain lines, with a cost, and
/// the Adventure subtype on an Adventure.
fn spell_toml() -> impl Strategy<Value = (String, String)> {
    (
        "[A-Z][a-z]{0,10}( [A-Z][a-z]{0,10}){0,3}",
        prop::collection::vec(prop::sample::select(&SYMBOLS[..9]), 1..6),
        prop::sample::select(&["Instant", "Sorcery"][..]),
        prop::collection::vec(paragraph(), 0..3),
    )
        .prop_map(|(name, cost, ty, rules)| {
            let cost: String = cost.iter().map(|s| format!("{{{s}}}")).collect();
            (
                ty.to_string(),
                format!(
                    "name = \"{name}\"\ncost = \"{cost}\"\nrules = \"\"\"\n{}\n\"\"\"\n",
                    rules.join("\n")
                ),
            )
        })
}

/// A card of one face, or two in any of the layouts that have two.
fn card_toml() -> impl Strategy<Value = String> {
    (face_toml(), face_toml(), spell_toml(), prop::collection::vec(paragraph(), 0..4), 0..6usize).prop_map(|((cost, front), (back_cost, back), (spell_type, spell), plain, kind)| {
        let cost_line = |c: &str, split: bool| match (c.is_empty(), split) {
            (true, true) => "cost = \"{1}\"\n".to_string(),
            (true, false) => String::new(),
            (false, _) => format!("cost = \"{c}\"\n"),
        };
        let split = kind == 3;
        let top = format!("{}{front}", cost_line(&cost, split));
        let second = format!("{}{back}", cost_line(&back_cost, split));
        // A card with a spell inset shares its box with it, so its own text
        // is plain lines, and only a permanent is prepared: a creature here.
        let inset = |layout: &str, ty: &str| {
            format!(
                "name = \"Inset\"\n{}type = \"Creature — Elf\"\npt = \"2/2\"\nrules = \"\"\"\n{}\n\"\"\"\nlayout = \"{layout}\"\n[spell]\n{spell}type = \"{ty}\"\n",
                cost_line(&cost, false),
                plain.join("\n"),
            )
        };
        match kind {
            0 => top,
            1 => format!("{top}layout = \"transform\"\n[back]\n{second}"),
            2 => format!("{top}layout = \"modal\"\n[back]\n{second}"),
            3 => format!("{top}layout = \"split\"\n[right]\n{second}"),
            4 => inset("adventure", &format!("{spell_type} — Adventure")),
            _ => inset("prepare", &spell_type),
        }
    })
}

proptest! {
    // A case is a Typst layout: 96 take 10s optimised, which is how the flake
    // runs them, and the dev profile is slow enough that 24 have to do (14s).
    #![proptest_config(ProptestConfig::with_cases(if cfg!(debug_assertions) { 24 } else { 96 }))]

    #[test]
    fn every_card_the_format_accepts_prints_or_is_refused_for_space(toml in card_toml()) {
        let card = Card::parse(&toml, Path::new(".")).expect("the generator writes only valid cards");
        match render::compile(std::slice::from_ref(&card), Layout::Cards) {
            Ok(doc) => prop_assert_eq!(doc.pages().len(), card.images()),
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
    let err = render::compile(&[card], Layout::Cards).unwrap_err();
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
    let err = render::compile(&[card], Layout::Cards).unwrap_err();
    assert!(
        format!("{err:#}").contains("the name does not fit"),
        "{err:#}"
    );
}
