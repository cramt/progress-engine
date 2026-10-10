//! What another player's collection holds of the wanted list (ADR-0035): the
//! copies they could bring to a trade, and the place each one is in, so they
//! know where to look before the game night.
//!
//! Both sides count by name, as the wanted list does: any printing or finish
//! of theirs answers a want, and the copy they hold says which it is.

use std::collections::HashMap;

use crate::collection::Collection;
use crate::deck::{CardRef, Deck, Finish};
use crate::edit::names_with_comments;
use crate::identity::Names;
use crate::wanted::{key_of, missing_by_key, owned, Wanted};

/// Copies of one of their lines to take out of the place it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pull {
    /// Their place, `None` for their unsorted copies.
    pub place: Option<String>,
    /// The place is one of their decks, so these copies are sleeved in it.
    pub in_deck: bool,
    pub card: CardRef,
    pub finish: Finish,
    /// More than none, and never more than the line holds.
    pub qty: u32,
}

/// A card the wanted list is short of that their collection holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The card's name, or `set/num` for a printing nobody named.
    pub name: String,
    /// Copies still wanted: what the hand wants are short of, and what the
    /// decks lack.
    pub short: u32,
    /// Where the copies to bring are, at most `short` of them between them.
    pub pulls: Vec<Pull>,
}

/// The cards `wanted` (with `decks`, against `mine`) is short of that
/// `theirs` holds, by name. A hand want is short what it asks less what
/// `mine` holds of the card; the decks are short what [`crate::wanted::missing`]
/// says, and a card short both ways is short the sum. Their copies outside a
/// deck are taken first, each place in file order, and copies sleeved in a
/// deck only after. `names` names printings no file comments.
pub fn offers(
    wanted: (&Wanted, &str),
    mine: (&Collection, &str),
    decks: &[(String, Deck, String)],
    theirs: (&Collection, &str),
    names: &Names,
) -> Vec<Offer> {
    let (wanted, wanted_text) = wanted;
    let (mine, mine_text) = mine;
    let (theirs, theirs_text) = theirs;
    let mut all = names_with_comments(wanted_text, wanted.cards.iter().map(|w| &w.card), names);
    all = names_with_comments(theirs_text, theirs.cards.iter().map(|o| &o.card), &all);

    // Each card short, by key, in the order first met.
    let mut short: Vec<(String, String, u32)> = Vec::new();
    let mut hand: Vec<(String, String, u32, &CardRef)> = Vec::new();
    for w in &wanted.cards {
        let (key, name) = key_of(&w.card, &all);
        match hand.iter_mut().find(|(k, ..)| *k == key) {
            Some((_, _, qty, _)) => *qty += w.qty.get(),
            None => hand.push((key, name, w.qty.get(), &w.card)),
        }
    }
    for (key, name, qty, card) in hand {
        let short_by = qty.saturating_sub(owned(mine, mine_text, card, &all));
        if short_by > 0 {
            short.push((key, name, short_by));
        }
    }
    for (key, m) in missing_by_key(mine, mine_text, decks, &all, wanted.deck_copies) {
        match short.iter_mut().find(|(k, ..)| *k == key) {
            Some((_, _, qty)) => *qty += m.missing,
            None => short.push((key, m.name, m.missing)),
        }
    }

    let in_deck = |at: &Option<String>| {
        at.as_deref()
            .and_then(|p| theirs.place(p))
            .is_some_and(|p| p.deck.is_some())
    };
    let mut lines: Vec<(String, &crate::collection::Owned)> = theirs
        .cards
        .iter()
        .map(|o| (key_of(&o.card, &all).0, o))
        .collect();
    // Stable, so file order holds within each half.
    lines.sort_by_key(|(_, o)| in_deck(&o.at));

    let mut by_key: HashMap<String, Vec<Pull>> = HashMap::new();
    let mut left: HashMap<&str, u32> = short.iter().map(|(k, _, n)| (k.as_str(), *n)).collect();
    for (key, o) in lines {
        let Some(want) = left.get_mut(key.as_str()).filter(|n| **n > 0) else {
            continue;
        };
        let qty = (*want).min(o.qty.get());
        *want -= qty;
        by_key.entry(key).or_default().push(Pull {
            place: o.at.clone(),
            in_deck: in_deck(&o.at),
            card: o.card.clone(),
            finish: o.finish,
            qty,
        });
    }

    let mut out: Vec<Offer> = short
        .iter()
        .filter_map(|(key, name, qty)| {
            by_key.remove(key.as_str()).map(|pulls| Offer {
                name: name.clone(),
                short: *qty,
                pulls,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}
