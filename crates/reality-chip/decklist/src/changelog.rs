//! What changed between two versions of a deck, as the commit message that
//! saves it (ADR-0021): `lantern: +1 Sol Ring, -1 Mind Stone, Sol Ring: ramp →
//! draw`, so the repo's log reads as the deck's changelog.
//!
//! The message is a function of the two decks and the path alone. It is
//! lowercase apart from card and deck names, and ordered by kind of change,
//! then by card. Past [`SUBJECT_CHANGES`] changes the subject ends in
//! `and N more` and the body lists every change, one per line.
//!
//! A card named by printing has no name in the file, only the comment the
//! writer leaves beside it. [`commit_message_for_text`] reads those comments
//! from both versions and labels a printing with its name when either has one,
//! and as `set/num` when neither does.

use std::collections::HashMap;

use crate::deck::{Card, CardRef, CategoryType, Deck, DeckError, Finish, Printing};
use crate::edit::card_comments;

/// Changes the subject names before it says `and N more`.
pub const SUBJECT_CHANGES: usize = 3;

/// The kinds of change, in the order a message lists them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Add,
    Remove,
    Qty,
    Move,
    Printing,
    Finish,
    Declare,
    Undeclare,
    Retype,
    Rename,
    Format,
    Cover,
}

struct Change {
    kind: Kind,
    /// The card or category it is about, for ordering.
    subject: String,
    text: String,
}

fn stem(path: &str) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = file
        .strip_suffix(".deck.toml")
        .or_else(|| {
            file.rsplit_once('.')
                .map(|(s, _)| s)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or(file);
    stem.to_lowercase()
}

fn categories(list: &[String]) -> String {
    if list.is_empty() {
        "uncategorized".to_string()
    } else {
        list.iter()
            .map(|c| c.to_lowercase())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub(crate) fn finish_name(f: Finish) -> &'static str {
    match f {
        Finish::Nonfoil => "nonfoil",
        Finish::Foil => "foil",
        Finish::Etched => "etched",
    }
}

pub(crate) fn reference(r: &CardRef) -> String {
    match r {
        CardRef::Printing(p) => p.to_string(),
        CardRef::Name(_) => "any printing".to_string(),
    }
}

fn kind_name(k: Option<CategoryType>) -> &'static str {
    k.map_or("untyped", CategoryType::as_str)
}

struct Side<'a> {
    card: &'a Card,
    label: String,
}

fn sides<'a>(deck: &'a Deck, label: &dyn Fn(&Card) -> String) -> Vec<Side<'a>> {
    deck.cards
        .iter()
        .map(|card| Side {
            card,
            label: label(card),
        })
        .collect()
}

/// Pairs each card in `after` with the card it was in `before`, most alike
/// first: the same line, then the same card with one thing changed, then the
/// same card name under another printing. What stays unpaired was added or
/// removed.
fn card_changes(before: &[Side], after: &[Side], out: &mut Vec<Change>) {
    type Alike = fn(&Side, &Side) -> bool;
    let passes: [Alike; 5] = [
        |a, b| {
            a.card.card == b.card.card
                && a.card.categories == b.card.categories
                && a.card.finish == b.card.finish
        },
        |a, b| a.card.card == b.card.card && a.card.categories == b.card.categories,
        |a, b| a.card.card == b.card.card && a.card.finish == b.card.finish,
        |a, b| a.card.card == b.card.card,
        |a, b| a.label == b.label,
    ];
    let mut paired_after = vec![false; after.len()];
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let mut paired_before = vec![false; before.len()];
    for alike in passes {
        for (i, a) in before.iter().enumerate() {
            if paired_before[i] {
                continue;
            }
            if let Some(j) = (0..after.len()).find(|&j| !paired_after[j] && alike(a, &after[j])) {
                paired_before[i] = true;
                paired_after[j] = true;
                pairs.push((i, j));
            }
        }
    }

    let mut push = |kind: Kind, label: &str, text: String| {
        out.push(Change {
            kind,
            subject: label.to_string(),
            text,
        })
    };
    for (a, _) in before.iter().zip(&paired_before).filter(|(_, p)| !**p) {
        push(
            Kind::Remove,
            &a.label,
            format!("-{} {}", a.card.qty, a.label),
        );
    }
    for (b, _) in after.iter().zip(&paired_after).filter(|(_, p)| !**p) {
        push(Kind::Add, &b.label, format!("+{} {}", b.card.qty, b.label));
    }
    for (i, j) in pairs {
        let (a, b) = (&before[i], &after[j]);
        let label = &b.label;
        if a.card.qty != b.card.qty {
            push(
                Kind::Qty,
                label,
                format!("{label}: {} → {}", a.card.qty, b.card.qty),
            );
        }
        if a.card.categories != b.card.categories {
            push(
                Kind::Move,
                label,
                format!(
                    "{label}: {} → {}",
                    categories(&a.card.categories),
                    categories(&b.card.categories)
                ),
            );
        }
        if a.card.card != b.card.card {
            push(
                Kind::Printing,
                label,
                format!(
                    "{label}: {} → {}",
                    reference(&a.card.card),
                    reference(&b.card.card)
                ),
            );
        }
        if a.card.finish != b.card.finish {
            push(
                Kind::Finish,
                label,
                format!(
                    "{label}: {} → {}",
                    finish_name(a.card.finish),
                    finish_name(b.card.finish)
                ),
            );
        }
    }
}

fn category_changes(before: &Deck, after: &Deck, out: &mut Vec<Change>) {
    for c in &after.categories {
        let name = c.name.to_lowercase();
        match before.category(&c.name) {
            None => out.push(Change {
                kind: Kind::Declare,
                text: match c.kind {
                    None => format!("+category {name}"),
                    Some(k) => format!("+category {name} ({k})"),
                },
                subject: name,
            }),
            Some(old) if old.kind != c.kind => out.push(Change {
                kind: Kind::Retype,
                text: format!(
                    "category {name}: {} → {}",
                    kind_name(old.kind),
                    kind_name(c.kind)
                ),
                subject: name,
            }),
            Some(_) => {}
        }
    }
    for c in &before.categories {
        if after.category(&c.name).is_none() {
            let name = c.name.to_lowercase();
            out.push(Change {
                kind: Kind::Undeclare,
                text: format!("-category {name}"),
                subject: name,
            });
        }
    }
}

fn quoted(s: &Option<String>) -> String {
    s.as_ref()
        .map_or_else(|| "none".to_string(), |s| format!("{s:?}"))
}

/// The commit message for saving `before` as `after` at `path`. `name_of`
/// labels a card named by printing; without a name it is its `set/num`.
pub fn commit_message(
    before: &Deck,
    after: &Deck,
    path: &str,
    name_of: impl Fn(&Printing) -> Option<String>,
) -> String {
    let label = |c: &Card| match &c.card {
        CardRef::Name(n) => n.clone(),
        CardRef::Printing(p) => name_of(p).unwrap_or_else(|| p.to_string()),
    };
    let mut changes = Vec::new();
    card_changes(&sides(before, &label), &sides(after, &label), &mut changes);
    category_changes(before, after, &mut changes);
    if before.name != after.name {
        changes.push(Change {
            kind: Kind::Rename,
            subject: String::new(),
            text: format!("name: {} → {}", quoted(&before.name), quoted(&after.name)),
        });
    }
    if before.format != after.format {
        let f = |s: &Option<String>| s.as_deref().unwrap_or("none").to_lowercase();
        changes.push(Change {
            kind: Kind::Format,
            subject: String::new(),
            text: format!("format: {} → {}", f(&before.format), f(&after.format)),
        });
    }
    if before.cover != after.cover {
        let c = |p: &Option<Printing>| {
            p.as_ref().map_or_else(
                || "none".to_string(),
                |p| name_of(p).unwrap_or_else(|| p.to_string()),
            )
        };
        changes.push(Change {
            kind: Kind::Cover,
            subject: String::new(),
            text: format!("cover: {} → {}", c(&before.cover), c(&after.cover)),
        });
    }
    changes.sort_by(|x, y| {
        (&x.kind, x.subject.to_lowercase(), &x.subject, &x.text).cmp(&(
            &y.kind,
            y.subject.to_lowercase(),
            &y.subject,
            &y.text,
        ))
    });

    let lines: Vec<&str> = changes.iter().map(|c| c.text.as_str()).collect();
    message(path, &lines)
}

/// The message for `lines`, already in order, saving the file at `path`:
/// the file's stem, then the first [`SUBJECT_CHANGES`] of them, and past that
/// every line in the body.
pub(crate) fn message(path: &str, lines: &[&str]) -> String {
    let stem = stem(path);
    if lines.is_empty() {
        return format!("{stem}: reformat");
    }
    if lines.len() <= SUBJECT_CHANGES {
        return format!("{stem}: {}", lines.join(", "));
    }
    format!(
        "{stem}: {}, and {} more\n\n{}\n",
        lines[..SUBJECT_CHANGES].join(", "),
        lines.len() - SUBJECT_CHANGES,
        lines.join("\n")
    )
}

/// Each printing's name from the comments beside it in `text`.
fn printing_names(text: &str, deck: &Deck, names: &mut HashMap<Printing, String>) {
    for (card, comment) in deck.cards.iter().zip(card_comments(text)) {
        if let (CardRef::Printing(p), Some(name)) = (&card.card, comment) {
            names.insert(p.clone(), name);
        }
    }
}

/// [`commit_message`] between two deck texts, labelling each printing with
/// the name commented beside it in either, the newer text's first. An empty
/// `before` is the empty deck, for a deck's first save.
pub fn commit_message_for_text(before: &str, after: &str, path: &str) -> Result<String, DeckError> {
    let old = Deck::parse(before)?;
    let new = Deck::parse(after)?;
    let mut names = HashMap::new();
    printing_names(before, &old, &mut names);
    printing_names(after, &new, &mut names);
    Ok(commit_message(&old, &new, path, |p| names.get(p).cloned()))
}
