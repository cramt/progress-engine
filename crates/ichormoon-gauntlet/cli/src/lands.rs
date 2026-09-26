//! What each land in a deck really makes: the card data, read against the
//! rules and against the deck it sits in.
//!
//! Scryfall's `produced_mana` is every kind of mana a card *could* make, with
//! conditions and spending restrictions ignored, and nothing for a land that
//! makes its mana by fetching another land. Taken at its word it overstates
//! Castle Doom, whose colours pay only for an artifact spell, and understates a
//! fetchland, which pays no colour at all. So a land is read here, once, and
//! every reading that is not the face value is named in the run that relied on
//! it — the same stance [`crate::library::CONDITIONAL_TAPLAND`] takes about
//! tapped-ness (HANDS.md hands 8, 38 and 39).

use chip_scryfall::index::Card;
use gauntlet_criteria::{ManaSource, Palette};

use crate::library::{front, is_land, names, Entry, CONDITIONAL_TAPLAND, TAPLAND};

/// How a land was read, where that is not the face value of its card data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub card: String,
    pub reading: String,
}

/// Each entry's mana, parallel to `entries`, and the readings behind it.
pub fn read(entries: &[Entry]) -> (Vec<ManaSource>, Vec<Reading>) {
    let mut readings = Vec::new();
    let sources = entries
        .iter()
        .map(|e| {
            let (source, reading) = land(&e.card, entries);
            if let Some(reading) = reading {
                readings.push(Reading {
                    card: e.card.name.clone(),
                    reading,
                });
            }
            source
        })
        .collect();
    readings.sort_by(|a, b| a.card.cmp(&b.card));
    readings.dedup();
    (sources, readings)
}

fn enters_tapped(card: &Card) -> bool {
    card.tags
        .iter()
        .any(|t| t == TAPLAND || t == CONDITIONAL_TAPLAND)
}

/// One card's mana, and what was assumed to get it.
fn land(card: &Card, deck: &[Entry]) -> (ManaSource, Option<String>) {
    if !is_land(card) {
        // A Sol Ring makes mana and is not here. Getting it onto the
        // battlefield costs mana, which is the budget half of #10.
        return (ManaSource::Spell, None);
    }
    let tapped = enters_tapped(card);
    let listed = Palette::from_letters(&card.produces);
    let make = |produces: Palette, enters_tapped: bool, lasts: Option<u8>| ManaSource::Land {
        enters_tapped,
        produces,
        lasts,
    };
    if listed.is_empty() {
        return match fetches(card) {
            Some(search) => fetchland(&search, deck),
            None => (
                make(Palette::EMPTY, tapped, Some(0)),
                Some("makes no mana: it has no mana ability, so it is a land drop and pays for nothing".into()),
            ),
        };
    }
    if let Some(chapters) = saga_chapters(card) {
        return (
            make(unconditional(card, listed), tapped, Some(chapters)),
            Some(format!(
                "makes mana for {} turns, the one it is played on and the {} after: its last \
                 chapter sacrifices it",
                in_words(chapters),
                in_words(chapters - 1)
            )),
        );
    }
    let kept = unconditional(card, listed);
    if kept != listed {
        let reading = if kept.is_empty() {
            "pays generic and no colour: the colour it makes depends on what an opponent's land \
             could produce, and whether it makes any at all on the first turn of a game"
                .to_string()
        } else {
            format!(
                "pays {} only: its other colours come with a condition this engine cannot see",
                kept.symbols().join("")
            )
        };
        return (make(kept, tapped, None), Some(reading));
    }
    if card
        .oracle
        .contains("When this land enters, return a land you control to its owner's hand")
    {
        return (
            make(listed, tapped, None),
            Some(
                "counted as one mana a turn: the second mana it taps for, and the land it \
                 returns to hand, are not modelled"
                    .into(),
            ),
        );
    }
    (make(listed, tapped, None), None)
}

/// What a fetchland searches for: a set of basic land types, and whether the
/// land has to be basic.
struct Search {
    types: Vec<&'static str>,
    basic: bool,
    tapped: bool,
}

const BASIC_TYPES: [&str; 5] = ["Plains", "Island", "Swamp", "Mountain", "Forest"];

/// Read a fetchland's search off its oracle text, where it is one: "{T}, Pay
/// 1 life, Sacrifice this land: Search your library for a Forest or Island
/// card, put it onto the battlefield". `None` for anything else, including a
/// search whose cost needs mana, which is not a land paying for itself.
fn fetches(card: &Card) -> Option<Search> {
    card.oracle.lines().find_map(|line| {
        let (cost, effect) = line.split_once(": Search your library for ")?;
        if !cost.contains("Sacrifice") || cost.replace("{T}", "").contains('{') {
            return None;
        }
        let (wanted, rest) = effect.split_once(" card")?;
        let onto = rest.split_once("onto the battlefield")?.1;
        let words: Vec<&str> = wanted
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let types: Vec<&'static str> = BASIC_TYPES
            .into_iter()
            .filter(|t| words.contains(t))
            .collect();
        if types.is_empty() && !words.contains(&"land") {
            return None;
        }
        Some(Search {
            types,
            basic: words.contains(&"basic"),
            tapped: onto.starts_with(" tapped"),
        })
    })
}

/// A fetchland as the lands it can find in this deck.
///
/// A land it puts onto the battlefield untapped pays the turn it is cracked,
/// so the fetchland is a source of every colour those lands make. The
/// assumption, named in the run, is that one of them is still in the library
/// to find — which ignores running out, and with several targets that is
/// rare. A shockland it could find is left out, for the reason hand 8 gives:
/// it enters tapped unless somebody pays the life.
fn fetchland(search: &Search, deck: &[Entry]) -> (ManaSource, Option<String>) {
    let targets: Vec<&Card> = deck
        .iter()
        .map(|e| &e.card)
        .filter(|c| {
            // In the library a card has only its front face.
            let face = front(c);
            names(face, "land")
                && !Palette::from_letters(&c.produces).is_empty()
                && (!search.basic || names(face, "basic"))
                && (search.types.is_empty() || search.types.iter().any(|t| names(face, t)))
        })
        .collect();
    let untapped: Vec<&Card> = targets
        .iter()
        .copied()
        .filter(|c| !enters_tapped(c))
        .collect();
    let (found, tapped) = if search.tapped || untapped.is_empty() {
        (targets, true)
    } else {
        (untapped, false)
    };
    if found.is_empty() {
        return (
            ManaSource::Land {
                enters_tapped: false,
                produces: Palette::EMPTY,
                lasts: Some(0),
            },
            Some("makes no mana: this deck holds no land it can fetch".into()),
        );
    }
    let produces = found.iter().fold(Palette::EMPTY, |p, c| {
        p.union(unconditional(c, Palette::from_letters(&c.produces)))
    });
    let mut found_names: Vec<&str> = found.iter().map(|c| c.name.as_str()).collect();
    found_names.sort_unstable();
    found_names.dedup();
    (
        ManaSource::Land {
            enters_tapped: tapped,
            produces,
            lasts: None,
        },
        Some(format!(
            "a fetchland, read as the {} lands it can find in this deck ({}): pays {}{}, \
             assuming one is still in the library to find",
            if tapped { "tapped" } else { "untapped" },
            found_names.join(", "),
            produces.symbols().join(""),
            if tapped { ", and enters tapped" } else { "" }
        )),
    )
}

/// A mana ability whose colour comes with a condition: spent only on some
/// spells (Castle Doom, CR 106.6), activated only in some states (Spire of
/// Industry), or depending on what an opponent's lands could make (Exotic
/// Orchard, CR 106.7).
fn conditional(line: &str) -> bool {
    line.contains("Add")
        && (line.contains("Spend this mana only")
            || line.contains("Activate only if")
            || line.contains("could produce"))
}

/// The part of `listed` a land makes with no condition on it: the colours
/// named by its unconditional mana abilities. A land with no conditional
/// ability keeps everything Scryfall lists.
fn unconditional(card: &Card, listed: Palette) -> Palette {
    if !card.oracle.lines().any(conditional) {
        return listed;
    }
    let mut made = Palette::EMPTY;
    for line in card
        .oracle
        .lines()
        .filter(|l| l.contains("Add") && !conditional(l))
    {
        if line.contains("any color") || line.contains("any one color") {
            made = made.union(Palette::from_letters(["WUBRG"]));
        }
        made = made.union(Palette::from_letters(
            line.split('{')
                .skip(1)
                .filter_map(|s| s.split_once('}').map(|(sym, _)| sym))
                .filter(|sym| sym.len() == 1),
        ));
    }
    listed.intersect(made)
}

/// How many chapters a Saga land has, which is how many turns it stays: the
/// last chapter sacrifices it (CR 714.4).
fn saga_chapters(card: &Card) -> Option<u8> {
    if !names(front(card), "saga") {
        return None;
    }
    ["I", "II", "III", "IV", "V", "VI"]
        .iter()
        .rposition(|numeral| {
            card.oracle.lines().any(|l| {
                l.split(" — ")
                    .next()
                    .is_some_and(|c| c.split(", ").any(|n| n == *numeral))
            })
        })
        .map(|i| i as u8 + 1)
}

fn in_words(n: u8) -> String {
    match n {
        1 => "one".into(),
        2 => "two".into(),
        3 => "three".into(),
        4 => "four".into(),
        5 => "five".into(),
        6 => "six".into(),
        n => n.to_string(),
    }
}
