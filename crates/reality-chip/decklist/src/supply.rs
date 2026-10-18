//! Where one deck's cards come from: the copies of the collection
//! (ADR-0023) each line of the deck is made of, the copies its deck place
//! holds that no line takes, and what is left to find.
//!
//! The deck page lays it out two ways. *Sourcing* asks what the deck takes
//! from the collection and what it still needs, and from whom. *Build* asks
//! what the physical deck holds against the list: what to pull into it, from
//! which place, and what to take out.
//!
//! The deck is taken alone, as a trade for one deck takes it (ADR-0035): the
//! whole collection answers it, as if no other deck asked for a copy, and a
//! copy sleeved in another deck says so.

use std::collections::HashMap;

use crate::collection::{Collection, Owned};
use crate::deck::{CardRef, Deck, Finish, Printing};
use crate::edit::names_with_comments;
use crate::identity::Names;
use crate::trade::{offers_by_key, Pull};
use crate::wanted::{is_basic, key_of, needs_copy, Wanted};

/// Where an owned copy is, as seen from the deck being built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Whereabouts {
    /// Sleeved in this deck already: its deck place.
    ThisDeck,
    /// In a binder, a box, or unsorted.
    Loose,
    /// Sleeved in another deck.
    OtherDeck,
}

/// Copies of one collection line that make up part of a deck line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copies {
    /// The collection line, to move them by.
    pub line: usize,
    /// Its place, `None` for unsorted.
    pub place: Option<String>,
    pub whereabouts: Whereabouts,
    pub card: CardRef,
    pub finish: Finish,
    pub qty: u32,
    /// The copy is the one the deck line names: its printing, where the line
    /// names one, and its finish. Otherwise it is the card in another
    /// printing or finish, or a copy logged by name only.
    pub exact: bool,
}

/// One deck line that needs physical copies, and the copies it gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The deck line.
    pub index: usize,
    /// The key the deck's lines and the collection's are matched by.
    pub key: String,
    /// The card's name, or `set/num` for a printing nobody named.
    pub name: String,
    pub need: u32,
    /// Exact copies first, then the rest, each half nearest first.
    pub copies: Vec<Copies>,
}

impl Line {
    pub fn got(&self) -> u32 {
        self.copies.iter().map(|c| c.qty).sum()
    }
}

/// A card the collection has too few copies of for the deck.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Short {
    pub key: String,
    pub name: String,
    /// The first deck line holding the card.
    pub line: usize,
    /// Copies no owned one answers: more than none.
    pub short: u32,
    /// A basic land, which few people log and which a store has in bulk.
    pub basic: bool,
    /// Each friend whose collection holds some, in the order given, and where.
    pub friends: Vec<(String, Vec<Pull>)>,
}

/// Copies in the deck's place that no line takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extra {
    /// The collection line.
    pub line: usize,
    pub name: String,
    pub card: CardRef,
    pub finish: Finish,
    pub qty: u32,
    /// The deck lists the card: this is a copy too many, or the card in a
    /// printing or finish the list does not name.
    pub listed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Supply {
    /// The deck's place in the collection, if it has one.
    pub place: Option<String>,
    /// Every line needing a copy, in deck order: not the maybeboard, nor
    /// what is set aside.
    pub lines: Vec<Line>,
    /// In deck order of the card's first line.
    pub short: Vec<Short>,
    /// In collection order.
    pub extra: Vec<Extra>,
}

/// What is left to print as proxies: Phyrexian Metamorph's sheet, from the
/// CLI or Curator's deck page, so the two never count a deck apart.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToPrint {
    /// Deck line and copies, in deck order.
    pub lines: Vec<(usize, u32)>,
    /// Basic lands left out on request.
    pub basics: u32,
    /// Copies the collection answered, which are left out.
    pub owned: u32,
}

impl Supply {
    /// Each line's need less what the collection gave it, and with
    /// `leave_out_basics`, none of the basic lands. Against an empty
    /// collection that is the whole deck.
    pub fn to_print(&self, leave_out_basics: bool) -> ToPrint {
        let mut out = ToPrint::default();
        for line in &self.lines {
            out.owned += line.got();
            let missing = line.need - line.got();
            if missing == 0 {
                continue;
            }
            if leave_out_basics && is_basic(&line.key) {
                out.basics += missing;
                continue;
            }
            out.lines.push((line.index, missing));
        }
        out
    }
}

fn same_printing(a: &Printing, b: &Printing) -> bool {
    a.set.eq_ignore_ascii_case(&b.set) && a.num == b.num
}

/// `owned` is the copy deck line `card` in `finish` names. A line by name
/// takes any printing; a printing only itself.
fn exact(card: &CardRef, finish: Finish, owned: &Owned) -> bool {
    owned.finish == finish
        && match (card, &owned.card) {
            (CardRef::Name(_), _) => true,
            (CardRef::Printing(p), CardRef::Printing(q)) => same_printing(p, q),
            (CardRef::Printing(_), CardRef::Name(_)) => false,
        }
}

/// The deck at `path`, with its `text`, made of `mine`'s copies, and what
/// each of `friends` (their login, collection and its text) holds of what is
/// still short. `names` names printings no file comments.
///
/// Each line takes copies the one it names first, then the card in any other
/// printing or finish, and within each, the copies already in the deck
/// before loose ones, and those before copies sleeved in another deck.
pub fn supply(
    path: &str,
    deck: (&Deck, &str),
    mine: (&Collection, &str),
    friends: &[(String, Collection, String)],
    names: &Names,
) -> Supply {
    let (deck, deck_text) = deck;
    let (mine, mine_text) = mine;
    let mut all = names_with_comments(deck_text, deck.cards.iter().map(|c| &c.card), names);
    all = names_with_comments(mine_text, mine.cards.iter().map(|o| &o.card), &all);

    let place = mine
        .places
        .iter()
        .find(|p| p.deck.as_deref() == Some(path))
        .map(|p| p.name.clone());
    let whereabouts = |o: &Owned| match o.at.as_deref().and_then(|at| mine.place(at)) {
        Some(p) if p.deck.as_deref() == Some(path) => Whereabouts::ThisDeck,
        Some(p) if p.deck.is_some() => Whereabouts::OtherDeck,
        _ => Whereabouts::Loose,
    };

    // The collection's lines by key, nearest first; stable, so file order
    // holds within each.
    let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, o) in mine.cards.iter().enumerate() {
        by_key.entry(key_of(&o.card, &all).0).or_default().push(i);
    }
    for lines in by_key.values_mut() {
        lines.sort_by_key(|&i| whereabouts(&mine.cards[i]));
    }
    let mut left: Vec<u32> = mine.cards.iter().map(|o| o.qty.get()).collect();

    let mut lines: Vec<Line> = deck
        .cards
        .iter()
        .enumerate()
        .filter(|(_, c)| needs_copy(c.place))
        .map(|(index, c)| {
            let (key, name) = key_of(&c.card, &all);
            Line {
                index,
                key,
                name,
                need: c.qty.get(),
                copies: Vec::new(),
            }
        })
        .collect();

    // Every line takes the copies it names before any line settles for
    // another printing, so one line's stand-in never takes another's own.
    for exact_pass in [true, false] {
        for line in &mut lines {
            let card = &deck.cards[line.index];
            for &i in by_key.get(&line.key).map_or(&[][..], Vec::as_slice) {
                let want = line.need - line.got();
                if want == 0 {
                    break;
                }
                let o = &mine.cards[i];
                if left[i] == 0 || exact(&card.card, card.finish, o) != exact_pass {
                    continue;
                }
                let qty = want.min(left[i]);
                left[i] -= qty;
                line.copies.push(Copies {
                    line: i,
                    place: o.at.clone(),
                    whereabouts: whereabouts(o),
                    card: o.card.clone(),
                    finish: o.finish,
                    qty,
                    exact: exact_pass,
                });
            }
        }
    }

    let mut short: Vec<Short> = Vec::new();
    for line in &lines {
        let missing = line.need - line.got();
        if missing == 0 {
            continue;
        }
        match short.iter_mut().find(|s| s.key == line.key) {
            Some(s) => s.short += missing,
            None => short.push(Short {
                line: line.index,
                key: line.key.clone(),
                name: line.name.clone(),
                short: missing,
                basic: is_basic(&line.key),
                friends: Vec::new(),
            }),
        }
    }

    // A friend is asked what the deck alone lacks, as a trade for one deck
    // asks, which leaves the basics out.
    let alone = [(path.to_string(), deck.clone(), deck_text.to_string())];
    for (who, theirs, theirs_text) in friends {
        for (key, offer) in offers_by_key(
            (&Wanted::default(), ""),
            (mine, mine_text),
            &alone,
            (theirs, theirs_text),
            &all,
        ) {
            if let Some(s) = short.iter_mut().find(|s| s.key == key) {
                s.friends.push((who.clone(), offer.pulls));
            }
        }
    }

    let extra = mine
        .cards
        .iter()
        .enumerate()
        .filter(|&(i, o)| left[i] > 0 && whereabouts(o) == Whereabouts::ThisDeck)
        .map(|(i, o)| {
            let (key, name) = key_of(&o.card, &all);
            Extra {
                line: i,
                name,
                card: o.card.clone(),
                finish: o.finish,
                qty: left[i],
                listed: lines.iter().any(|l| l.key == key),
            }
        })
        .collect();

    Supply {
        place,
        lines,
        short,
        extra,
    }
}
