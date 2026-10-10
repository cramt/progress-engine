//! A deck as the text other tools import. Each target gets its own writer,
//! even where two are near the same, so the person exporting never has to
//! know which tools share a format.
//!
//! - **Cockatrice**'s *Load deck from clipboard* (`DeckListPlainText::parse`):
//!   `1 Sol Ring (C21) 263`, a printing it resolves from 2.10 on and strips
//!   before. Its decks have no commander zone, and its own Archidekt import
//!   puts a commander in the sideboard, so this does too, each line marked
//!   `SB:`, which is the one sideboard marker blank lines cannot confuse.
//! - **Cardmarket**'s *Add Deck List* on a wants list: `1 Sol Ring`, with no
//!   expansion, since buying a deck means the cheapest copy and Cardmarket
//!   names expansions its own way. One line per card, copies summed.
//! - **Tabletop Simulator**'s *MTG Deck/Draft/Cube Importer* (workshop
//!   2265064081), which hands the text to its server: Arena's
//!   `1 Sol Ring (C21) 263` under `Commander`, `Deck` and `Sideboard`, each its
//!   own pile on the table. It resolves every printing Scryfall numbers,
//!   `BLC-129` and `1494★` too, and a printing it cannot find keeps the name.
//!   Its `Companion` heading piles the companion with the commander, so the
//!   companion goes to the sideboard, where it starts the game.

use std::collections::HashMap;

use thiserror::Error;

use crate::deck::{Card, CardRef, CategoryType, Deck, DeckError, Printing};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExportError {
    #[error(transparent)]
    Deck(#[from] DeckError),
    #[error(
        "no name for {}: the text names every card, and the deck file names a printing only by set and number",
        .0.iter().map(Printing::to_string).collect::<Vec<_>>().join(", ")
    )]
    Unnamed(Vec<Printing>),
}

/// Where a card goes in a deck another tool holds: played, the commander,
/// beside the deck (companion, sideboard), or not exported at all (the
/// maybeboard, cards set aside, attractions and stickers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Zone {
    Commander,
    Main,
    Side,
}

fn zone(card: &Card) -> Option<Zone> {
    use CategoryType::*;
    match card.place {
        Commander => Some(Zone::Commander),
        Sideboard | Companion => Some(Zone::Side),
        p if p.is_within(InDeck) => Some(Zone::Main),
        _ => None,
    }
}

/// Every exported card with its name, or the printings no name was given for.
fn named<'a>(
    deck: &'a Deck,
    names: &HashMap<String, String>,
) -> Result<Vec<(&'a Card, String, Zone)>, ExportError> {
    let mut unnamed = Vec::new();
    let mut out = Vec::new();
    for card in &deck.cards {
        let Some(zone) = zone(card) else { continue };
        match &card.card {
            CardRef::Name(n) => out.push((card, n.clone(), zone)),
            CardRef::Printing(p) => match names.get(&p.to_string()) {
                Some(n) => out.push((card, n.clone(), zone)),
                None => unnamed.push(p.clone()),
            },
        }
    }
    if unnamed.is_empty() {
        Ok(out)
    } else {
        Err(ExportError::Unnamed(unnamed))
    }
}

/// `(C21) 263` when Cockatrice reads the printing back as this one. It takes a
/// set of three letters or more and a number that starts with digits and has
/// no letter after them; `263a` would come back as `263`, another card, so
/// such a printing goes by name.
fn cockatrice_printing(p: &Printing) -> Option<String> {
    let digits = p.num.chars().take_while(char::is_ascii_digit).count();
    let number_reads = digits > 0
        && p.num[digits..]
            .chars()
            .all(|c| !c.is_alphanumeric() && !c.is_whitespace());
    let set_reads = p.set.len() >= 3 && p.set.chars().all(|c| c.is_ascii_alphanumeric());
    (number_reads && set_reads).then(|| format!("({}) {}", p.set.to_uppercase(), p.num))
}

/// The deck as Cockatrice's clipboard text: the deck in file order, then the
/// commander, companion and sideboard as `SB:` lines.
pub fn export_cockatrice(
    text: &str,
    names: &HashMap<String, String>,
) -> Result<String, ExportError> {
    let deck = Deck::parse(text)?;
    let cards = named(&deck, names)?;
    let line = |card: &Card, name: &str| {
        let printing = match &card.card {
            CardRef::Printing(p) => cockatrice_printing(p),
            CardRef::Name(_) => None,
        };
        match printing {
            Some(p) => format!("{} {name} {p}\n", card.qty),
            None => format!("{} {name}\n", card.qty),
        }
    };
    let mut out = String::new();
    for (card, name, _) in cards.iter().filter(|(_, _, z)| *z == Zone::Main) {
        out += &line(card, name);
    }
    let side: Vec<_> = cards.iter().filter(|(_, _, z)| *z != Zone::Main).collect();
    if !side.is_empty() {
        out.push('\n');
        for (card, name, _) in side {
            out += &format!("SB: {}", line(card, name));
        }
    }
    Ok(out)
}

/// The deck as Tabletop Simulator's importer reads it: a `Commander`, `Deck`
/// and `Sideboard` section, each left out when empty, every card in file order
/// with its printing when the file names one.
pub fn export_tabletop_simulator(
    text: &str,
    names: &HashMap<String, String>,
) -> Result<String, ExportError> {
    let deck = Deck::parse(text)?;
    let cards = named(&deck, names)?;
    let mut sections = Vec::new();
    for (heading, zone) in [
        ("Commander", Zone::Commander),
        ("Deck", Zone::Main),
        ("Sideboard", Zone::Side),
    ] {
        let lines: String = cards
            .iter()
            .filter(|(_, _, z)| *z == zone)
            .map(|(card, name, _)| match &card.card {
                CardRef::Printing(p) => {
                    format!("{} {name} ({}) {}\n", card.qty, p.set.to_uppercase(), p.num)
                }
                CardRef::Name(_) => format!("{} {name}\n", card.qty),
            })
            .collect();
        if !lines.is_empty() {
            sections.push(format!("{heading}\n{lines}"));
        }
    }
    Ok(sections.join("\n"))
}

/// The deck as Cardmarket's wants-list text: every card it takes to play the
/// deck, commander and sideboard included, one line per name with the copies
/// summed, in the order the file first lists each.
pub fn export_cardmarket(
    text: &str,
    names: &HashMap<String, String>,
) -> Result<String, ExportError> {
    let deck = Deck::parse(text)?;
    let mut lines: Vec<(String, u32)> = Vec::new();
    for (card, name, _) in named(&deck, names)? {
        match lines
            .iter_mut()
            .find(|(n, _)| n.eq_ignore_ascii_case(&name))
        {
            Some((_, qty)) => *qty += card.qty.get(),
            None => lines.push((name, card.qty.get())),
        }
    }
    Ok(lines
        .into_iter()
        .map(|(name, qty)| format!("{qty} {name}\n"))
        .collect())
}
