//! The refusal: Mirage Mockery prints custom cards, and a real one is
//! Phyrexian Metamorph's, from Scryfall's own image (ADR-0039). A card the
//! index knows by name is refused, and so is a real card under another name:
//! the same cost and the same rules text.

use anyhow::{bail, Result};
use chip_scryfall::bulk::strip_reminder_text;
use chip_scryfall::index::IndexFile;

use crate::card::Card;

/// Ok when nothing in Magic is any of these cards; otherwise the first that
/// is, and the real card it is.
pub fn refuse_real(cards: &[Card], index: &IndexFile) -> Result<()> {
    for card in cards {
        if index.contains(&card.name) {
            bail!(
                "{:?} is a real card. Mirage Mockery prints custom cards; `metamorph` prints real ones",
                card.name
            );
        }
        let rules = normalise(&card.rules.join("\n"), &card.name);
        // A vanilla card's text is empty and matches every other vanilla card
        // with the same cost; that is a coincidence of design, not a copy.
        if rules.is_empty() {
            continue;
        }
        let cost: String = card.cost.iter().map(|s| format!("{{{s}}}")).collect();
        for real in index.costing(&cost) {
            let real = real?;
            if normalise(&real.oracle, &real.name) == rules {
                bail!(
                    "{:?} is {:?} under another name: same cost, same rules text. \
                     Mirage Mockery prints custom cards; `metamorph` prints real ones",
                    card.name,
                    real.name
                );
            }
        }
    }
    Ok(())
}

/// Rules text as compared: the card's own name as `~`, no reminder text, and
/// whitespace and case not counted.
fn normalise(text: &str, name: &str) -> String {
    strip_reminder_text(&text.replace(name, "~"))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
