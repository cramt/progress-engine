//! What differs between two versions of a deck, one change at a time, and
//! how to take any of those changes from one into the other.
//!
//! A save's commit message ([`crate::changelog`]) is this diff written as a
//! sentence, so the changelog in git and the comparison Curator draws are the
//! same changes, in the same order, saying the same thing.
//!
//! [`apply`] takes a chosen set of a diff's changes into its `before` text.
//! Restoring an old version is taking every change; bringing back one card a
//! deck cut last week, or a variant's swaps into the deck it came from, is
//! taking some. The result is always a deck the format allows: a change that
//! needs a category the `before` deck lacks declares it, and the whole
//! application is checked once, at the end, rather than edit by edit.

use std::collections::HashMap;

use toml_edit::Value;

use crate::changelog::{finish_name, reference};
use crate::deck::{Card, CardRef, CategoryType, Deck, DeckError, Printing};
use crate::edit::{self, card_comments, EditError};

/// One change between two decks. A card line is named by its index in the
/// `before` deck, the `after` deck, or both when the line is in each and
/// differs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Add {
        after: usize,
    },
    Remove {
        before: usize,
    },
    Qty {
        before: usize,
        after: usize,
    },
    /// The line's categories.
    Move {
        before: usize,
        after: usize,
    },
    Printing {
        before: usize,
        after: usize,
    },
    Finish {
        before: usize,
        after: usize,
    },
    Declare(String),
    Undeclare(String),
    Retype(String),
    Rename,
    Format,
    Cover,
    Description,
}

impl Change {
    /// The order a commit message lists changes in.
    fn rank(&self) -> u8 {
        match self {
            Change::Add { .. } => 0,
            Change::Remove { .. } => 1,
            Change::Qty { .. } => 2,
            Change::Move { .. } => 3,
            Change::Printing { .. } => 4,
            Change::Finish { .. } => 5,
            Change::Declare(_) => 6,
            Change::Undeclare(_) => 7,
            Change::Retype(_) => 8,
            Change::Rename => 9,
            Change::Format => 10,
            Change::Cover => 12,
            Change::Description => 13,
        }
    }
}

/// Every change from `before` to `after`, in commit-message order, each with
/// the line a commit message says it with.
pub struct Diff<'a> {
    pub before: &'a Deck,
    pub after: &'a Deck,
    pub changes: Vec<(Change, String)>,
}

pub(crate) fn categories(list: &[String]) -> String {
    if list.is_empty() {
        "uncategorized".to_string()
    } else {
        list.iter()
            .map(|c| c.to_lowercase())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn kind_name(k: Option<CategoryType>) -> &'static str {
    k.map_or("untyped", CategoryType::as_str)
}

fn quoted(s: &Option<String>) -> String {
    s.as_ref()
        .map_or_else(|| "none".to_string(), |s| format!("{s:?}"))
}

/// Pairs each card in `after` with the card it was in `before`, most alike
/// first: the same line, then the same card with one thing changed, then the
/// same card name under another printing. What stays unpaired was added or
/// removed. A card's label is asked for only once the first four passes
/// leave it unpaired.
pub(crate) fn pair<'c>(
    before: &[&'c Card],
    after: &[&'c Card],
    label: impl Fn(&Card) -> String,
) -> Vec<(usize, usize)> {
    type Alike = fn(&Card, &Card) -> bool;
    let passes: [Alike; 4] = [
        |a, b| a.card == b.card && a.categories == b.categories && a.finish == b.finish,
        |a, b| a.card == b.card && a.categories == b.categories,
        |a, b| a.card == b.card && a.finish == b.finish,
        |a, b| a.card == b.card,
    ];
    let mut paired_before = vec![false; before.len()];
    let mut paired_after = vec![false; after.len()];
    let mut pairs = Vec::new();
    let pass = |alike: &dyn Fn(usize, usize) -> bool,
                paired_before: &mut [bool],
                paired_after: &mut [bool],
                pairs: &mut Vec<(usize, usize)>| {
        for (i, done) in paired_before.iter_mut().enumerate() {
            if *done {
                continue;
            }
            if let Some(j) = (0..after.len()).find(|&j| !paired_after[j] && alike(i, j)) {
                *done = true;
                paired_after[j] = true;
                pairs.push((i, j));
            }
        }
    };
    for alike in passes {
        pass(
            &|i, j| alike(before[i], after[j]),
            &mut paired_before,
            &mut paired_after,
            &mut pairs,
        );
    }
    let left = |cards: &[&Card], paired: &[bool]| -> Vec<Option<String>> {
        cards
            .iter()
            .zip(paired)
            .map(|(c, p)| (!p).then(|| label(c)))
            .collect()
    };
    let (lb, la) = (left(before, &paired_before), left(after, &paired_after));
    pass(
        &|i, j| lb[i].is_some() && lb[i] == la[j],
        &mut paired_before,
        &mut paired_after,
        &mut pairs,
    );
    pairs
}

/// The name a commit message gives a card: its own, or its printing's.
pub(crate) fn label_of(c: &Card, name_of: &impl Fn(&Printing) -> Option<String>) -> String {
    match &c.card {
        CardRef::Name(n) => n.clone(),
        CardRef::Printing(p) => name_of(p).unwrap_or_else(|| p.to_string()),
    }
}

fn cards_of(deck: &Deck) -> Vec<&Card> {
    deck.cards.iter().collect()
}

/// Which card line of `after` each card line of `before` became, as
/// `(before, after)` indices: the pairing every [`Diff`] is worked out from.
/// A card is the same card across two versions when this pairs it, whatever
/// else changed about its line; a line in neither pair was removed or added.
/// Blame follows a card through history by these pairs.
pub fn pair_lines(
    before: &Deck,
    after: &Deck,
    name_of: impl Fn(&Printing) -> Option<String>,
) -> Vec<(usize, usize)> {
    pair(&cards_of(before), &cards_of(after), |c| {
        label_of(c, &name_of)
    })
}

impl<'a> Diff<'a> {
    /// The changes from `before` to `after`. `name_of` labels a card named by
    /// printing; without a name it is its `set/num`.
    pub fn new(
        before: &'a Deck,
        after: &'a Deck,
        name_of: impl Fn(&Printing) -> Option<String>,
    ) -> Self {
        let labelled = |d: &'a Deck| -> Vec<(&'a Card, String)> {
            d.cards.iter().map(|c| (c, label_of(c, &name_of))).collect()
        };
        let (b, a) = (labelled(before), labelled(after));
        let pairs = pair(&cards_of(before), &cards_of(after), |c| {
            label_of(c, &name_of)
        });

        // Each change with the card or category it is about, for ordering.
        let mut out: Vec<(Change, String, String)> = Vec::new();
        let mut push = |change: Change, subject: &str, text: String| {
            out.push((change, subject.to_string(), text));
        };
        for (i, (card, label)) in b.iter().enumerate() {
            if !pairs.iter().any(|&(x, _)| x == i) {
                push(
                    Change::Remove { before: i },
                    label,
                    format!("-{} {label}", card.qty),
                );
            }
        }
        for (j, (card, label)) in a.iter().enumerate() {
            if !pairs.iter().any(|&(_, y)| y == j) {
                push(
                    Change::Add { after: j },
                    label,
                    format!("+{} {label}", card.qty),
                );
            }
        }
        for &(i, j) in &pairs {
            let (x, y, label) = (b[i].0, a[j].0, &a[j].1);
            let (before, after) = (i, j);
            if x.qty != y.qty {
                push(
                    Change::Qty { before, after },
                    label,
                    format!("{label}: {} → {}", x.qty, y.qty),
                );
            }
            if x.categories != y.categories {
                push(
                    Change::Move { before, after },
                    label,
                    format!(
                        "{label}: {} → {}",
                        categories(&x.categories),
                        categories(&y.categories)
                    ),
                );
            }
            if x.card != y.card {
                push(
                    Change::Printing { before, after },
                    label,
                    format!("{label}: {} → {}", reference(&x.card), reference(&y.card)),
                );
            }
            if x.finish != y.finish {
                push(
                    Change::Finish { before, after },
                    label,
                    format!(
                        "{label}: {} → {}",
                        finish_name(x.finish),
                        finish_name(y.finish)
                    ),
                );
            }
        }

        for c in &after.categories {
            let name = c.name.to_lowercase();
            match before.category(&c.name) {
                None => push(
                    Change::Declare(c.name.clone()),
                    &name,
                    match c.kind {
                        None => format!("+category {name}"),
                        Some(k) => format!("+category {name} ({k})"),
                    },
                ),
                Some(old) if old.kind != c.kind => push(
                    Change::Retype(c.name.clone()),
                    &name,
                    format!(
                        "category {name}: {} → {}",
                        kind_name(old.kind),
                        kind_name(c.kind)
                    ),
                ),
                Some(_) => {}
            }
        }
        for c in &before.categories {
            if after.category(&c.name).is_none() {
                let name = c.name.to_lowercase();
                push(
                    Change::Undeclare(c.name.clone()),
                    &name,
                    format!("-category {name}"),
                );
            }
        }

        if before.name != after.name {
            push(
                Change::Rename,
                "",
                format!("name: {} → {}", quoted(&before.name), quoted(&after.name)),
            );
        }
        if before.format != after.format {
            let f = |s: &Option<String>| s.as_deref().unwrap_or("none").to_lowercase();
            push(
                Change::Format,
                "",
                format!("format: {} → {}", f(&before.format), f(&after.format)),
            );
        }
        if before.cover != after.cover {
            let c = |p: &Option<Printing>| {
                p.as_ref().map_or_else(
                    || "none".to_string(),
                    |p| name_of(p).unwrap_or_else(|| p.to_string()),
                )
            };
            push(
                Change::Cover,
                "",
                format!("cover: {} → {}", c(&before.cover), c(&after.cover)),
            );
        }
        if before.description != after.description {
            let text = match (&before.description, &after.description) {
                (None, Some(_)) => "description: added",
                (Some(_), None) => "description: removed",
                _ => "description: rewritten",
            };
            push(Change::Description, "", text.to_string());
        }

        out.sort_by(|x, y| {
            (x.0.rank(), x.1.to_lowercase(), &x.1, &x.2).cmp(&(
                y.0.rank(),
                y.1.to_lowercase(),
                &y.1,
                &y.2,
            ))
        });
        Diff {
            before,
            after,
            changes: out.into_iter().map(|(c, _, t)| (c, t)).collect(),
        }
    }
}

/// Each printing's name from the comments beside it in `text`, which `deck`
/// was parsed from.
pub fn printing_names(text: &str, deck: &Deck, names: &mut HashMap<Printing, String>) {
    for (card, comment) in deck.cards.iter().zip(card_comments(text)) {
        if let (CardRef::Printing(p), Some(name)) = (&card.card, comment) {
            names.insert(p.clone(), name);
        }
    }
}

/// Both texts parsed, and every printing's name from the comments in either,
/// the newer text's first: what a diff between two texts labels cards with.
pub fn parse_pair(
    before: &str,
    after: &str,
) -> Result<(Deck, Deck, HashMap<Printing, String>), DeckError> {
    let old = Deck::parse(before)?;
    let new = Deck::parse(after)?;
    let mut names = HashMap::new();
    printing_names(before, &old, &mut names);
    printing_names(after, &new, &mut names);
    Ok((old, new, names))
}

/// `before` with the changes at `take` (indices into the diff of `before` to
/// `after`) taken from `after`, and nothing else touched: comments, spacing
/// and the order of the lines that stay all survive. Taking every change
/// gives a deck the diff says is `after`; taking none gives `before` back.
///
/// The edits run in an order that cannot trip over each other (categories
/// first, then lines in place, then removals from the bottom up, then new
/// lines, then what the deck itself says), and only the result is checked.
/// Dropping a category some card is still in is refused then, naming it.
pub fn apply(before_text: &str, after_text: &str, take: &[usize]) -> Result<String, EditError> {
    let (before, after, names) = parse_pair(before_text, after_text)?;
    let diff = Diff::new(&before, &after, |p| names.get(p).cloned());
    let mut chosen = Vec::with_capacity(take.len());
    for &i in take {
        let (change, _) = diff.changes.get(i).ok_or(EditError::NoChange(i))?;
        chosen.push(change);
    }
    let after_comments = card_comments(after_text);
    let unchecked: edit::Check = |_| Ok(());
    let mut text = before_text.to_string();

    // A category any taken line will sit in, or one taken on its own, is
    // declared as `after` declares it. One that `before` types otherwise is
    // only retyped when its retype is taken too.
    let mut needed: Vec<&str> = Vec::new();
    for change in &chosen {
        match change {
            Change::Add { after: j } | Change::Move { after: j, .. } => {
                needed.extend(after.cards[*j].categories.iter().map(String::as_str));
            }
            Change::Declare(name) => needed.push(name),
            _ => {}
        }
    }
    for name in needed {
        if before.category(name).is_none() {
            let kind = after.category(name).and_then(|c| c.kind);
            text = edit::declare_category_with(&text, name, kind, unchecked)?;
        }
    }
    for change in &chosen {
        if let Change::Retype(name) = change {
            let kind = after.category(name).and_then(|c| c.kind);
            text = edit::retype_category(&text, name, kind, unchecked)?;
        }
    }

    for change in &chosen {
        text = match **change {
            Change::Qty {
                before: i,
                after: j,
            } => edit::set_qty(&text, i, after.cards[j].qty.get(), unchecked)?,
            Change::Move {
                before: i,
                after: j,
            } => edit::set_categories_with(&text, i, &after.cards[j].categories, unchecked)?,
            Change::Printing {
                before: i,
                after: j,
            } => match &after.cards[j].card {
                CardRef::Printing(p) => edit::set_printing(&text, i, &p.set, &p.num, unchecked)?,
                CardRef::Name(n) => edit::set_name(&text, i, n, unchecked)?,
            },
            Change::Finish {
                before: i,
                after: j,
            } => edit::set_finish(&text, i, after.cards[j].finish, unchecked)?,
            _ => text,
        };
    }

    let mut removed: Vec<usize> = chosen
        .iter()
        .filter_map(|c| match c {
            Change::Remove { before } => Some(*before),
            _ => None,
        })
        .collect();
    removed.sort_unstable_by(|a, b| b.cmp(a));
    for i in removed {
        text = edit::remove_line(&text, i, unchecked)?;
    }

    for change in &chosen {
        if let Change::Add { after: j } = change {
            let comment = after_comments.get(*j).cloned().flatten();
            text = edit::push_card(&text, &after.cards[*j], comment.as_deref())?;
        }
    }

    for change in &chosen {
        if let Change::Undeclare(name) = change {
            text = edit::undeclare_category(&text, name)?;
        }
    }

    let mut doc = edit::document(&text)?;
    for change in &chosen {
        let string = |s: &Option<String>| s.as_deref().map(Value::from);
        match change {
            Change::Rename => edit::set_meta(&mut doc, "name", string(&after.name)),
            Change::Format => edit::set_meta(&mut doc, "format", string(&after.format)),
            Change::Cover => edit::set_meta(
                &mut doc,
                "cover",
                after.cover.as_ref().map(|p| Value::from(p.to_string())),
            ),
            Change::Description => {
                let value = match &after.description {
                    None => None,
                    Some(d) => Some(
                        crate::deck::quote_multiline(d)
                            .parse::<Value>()
                            .map_err(|e| EditError::Toml(e.to_string()))?,
                    ),
                };
                edit::set_meta(&mut doc, "description", value);
            }
            _ => {}
        }
    }
    let text = doc.to_string();

    // The one check: what every edit above skipped.
    if let Err(e) = Deck::parse(&text) {
        return Err(match e {
            DeckError::Undeclared { category, .. } => EditError::StillUsed(category),
            e => e.into(),
        });
    }
    Ok(text)
}
