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
        let cost = cost_of(card);
        let same_cost = index
            .costing(&cost)
            .map(|real| real.map(|r| (r.name, r.oracle)).map_err(Into::into));
        if let Some(why) = real_card(card, index.contains(&card.name), same_cost)? {
            bail!("{why}");
        }
    }
    Ok(())
}

/// The mana cost as Oracle text writes it, `{2}{U}`, by which the real cards
/// to compare with are found.
pub fn cost_of(card: &Card) -> String {
    card.cost.iter().map(|s| format!("{{{s}}}")).collect()
}

/// Why `card` is a real one, or none: `named` says Magic has a card by its
/// name, and `same_cost` is every real card costing [`cost_of`] it, as name
/// and Oracle text. A caller without the whole index may hand over fewer, so
/// long as none it leaves out could match: the browser asks Scryfall for the
/// cards at that cost whose text holds one of this card's words.
pub fn real_card(
    card: &Card,
    named: bool,
    same_cost: impl IntoIterator<Item = Result<(String, String)>>,
) -> Result<Option<String>> {
    if named {
        return Ok(Some(format!(
            "{:?} is a real card. Mirage Mockery prints custom cards; `metamorph` prints real ones",
            card.name
        )));
    }
    let rules = normalise(&card.rules.join("\n"), &card.name);
    // A vanilla card's text is empty and matches every other vanilla card
    // with the same cost; that is a coincidence of design, not a copy.
    if rules.is_empty() {
        return Ok(None);
    }
    for real in same_cost {
        let (name, oracle) = real?;
        if normalise(&oracle, &name) == rules {
            return Ok(Some(format!(
                "{:?} is {name:?} under another name: same cost, same rules text. \
                 Mirage Mockery prints custom cards; `metamorph` prints real ones",
                card.name
            )));
        }
    }
    Ok(None)
}

/// A word of the rules text as compared, which any real card matching it
/// holds in its Oracle text too: the longest, as the narrowest search.
pub fn telling_word(card: &Card) -> Option<String> {
    normalise(&card.rules.join("\n"), &card.name)
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| w.len() > 2)
        .max_by_key(|w| w.len())
        .map(str::to_string)
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
