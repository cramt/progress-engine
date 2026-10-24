//! A three-way merge of two versions of a deck made from one base, by card
//! rather than by text line.
//!
//! A line merge clashes wherever both sides touched neighbouring lines, and
//! Curator touches nearly every line of a deck it opens when it pins the
//! printing of each card named only by name (ADR-0027), so a fork and the
//! deck it came from clashed on almost any change. Here a card is the card
//! [`crate::diff::pair_lines`] says it is in each version, the pairing commit
//! messages and blame follow, and each of its attributes merges on its own:
//!
//! - **quantity**, **categories**, **printing** and **finish** of a card each
//!   take the side that changed it. Both changing one to the same value is
//!   one change; to different values is a [`CardClash`]. A quantity is how
//!   many copies the deck runs, not a count of copies added, so 2 against 3
//!   is two answers, not 1 + 2 more. The category list is one value, since
//!   its order is the card's (the first is where Archidekt files it).
//! - **a card one side removed** goes if the other left it as it was, and
//!   clashes ([`CardClash::Removed`]) if the other changed it.
//! - **a card both added** is one card when the two pair, and clashes on each
//!   attribute where the two adds differ.
//! - **the deck's own name, format, cover and description**, and **each
//!   declared category**, merge the same way, one value each.
//!
//! The result is OURS' TEXT with theirs' changes applied by the edits that
//! taking a change uses (ADR-0024, [`crate::diff::apply`]), so ours' comments,
//! spacing and order survive, and it is checked to parse once at the end. A
//! category theirs stopped declaring stays while a merged card is still in
//! it, since dropping it would take the card's place with it.

use std::collections::{BTreeSet, HashMap};
use std::num::NonZeroU32;

use thiserror::Error;
use toml_edit::Value;

use crate::changelog::{finish_name, reference};
use crate::deck::{Card, CardRef, CategoryType, Deck, DeckError, Printing};
use crate::diff::{categories, label_of, pair, pair_lines, printing_names, Diff};
use crate::edit::{self, card_comments, EditError};

/// One side of a merge, and the side a user picks for a clash: `ours` is
/// the version merged into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, facet::Facet)]
#[repr(u8)]
#[facet(rename_all = "kebab-case")]
pub enum Side {
    /// The version merged into: the text the result is made from.
    Ours,
    /// The version merged from.
    Theirs,
}

/// A category as one version declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declared {
    Absent,
    /// Declared without a type.
    Label,
    Typed(CategoryType),
}

impl Declared {
    fn of(deck: &Deck, name: &str) -> Declared {
        match deck.category(name) {
            None => Declared::Absent,
            Some(c) => c.kind.map_or(Declared::Label, Declared::Typed),
        }
    }
}

/// One attribute of one card that both sides changed differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardClash {
    Quantity {
        ours: NonZeroU32,
        theirs: NonZeroU32,
    },
    Categories {
        ours: Vec<String>,
        theirs: Vec<String>,
    },
    Printing {
        ours: CardRef,
        theirs: CardRef,
    },
    Finish {
        ours: crate::deck::Finish,
        theirs: crate::deck::Finish,
    },
    /// `by` removed the card and the other side changed it into `kept`.
    /// Picking `by` removes it; picking the other keeps `kept`.
    Removed {
        by: Side,
        kept: Card,
    },
}

/// The deck's own fields that merge as one value each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Format,
    Cover,
    Description,
}

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Field::Name => "name",
            Field::Format => "format",
            Field::Cover => "cover",
            Field::Description => "description",
        }
    }
}

/// Something both sides changed differently, which a [`Side`] settles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clash {
    /// `card` is the card's name, or its printing's.
    Card { card: String, clash: CardClash },
    Field {
        field: Field,
        ours: Option<String>,
        theirs: Option<String>,
    },
    Category {
        name: String,
        ours: Declared,
        theirs: Declared,
    },
}

/// A clash in words, for a person choosing a side: `Sol Ring`, `quantity`,
/// `2`, `3`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    /// The card, `the deck`, or `category ramp`.
    pub subject: String,
    pub attribute: &'static str,
    pub ours: String,
    pub theirs: String,
}

fn card_said(c: &Card) -> String {
    let mut s = format!("{} × {}", c.qty, reference(&c.card));
    if c.finish != crate::deck::Finish::Nonfoil {
        s += &format!(", {}", finish_name(c.finish));
    }
    s + &format!(", {}", categories(&c.categories))
}

fn declared_said(d: Declared) -> String {
    match d {
        Declared::Absent => "not declared".into(),
        Declared::Label => "a label".into(),
        Declared::Typed(t) => format!("type {t}"),
    }
}

impl Clash {
    pub fn said(&self) -> Said {
        let text = |s: &Option<String>| s.clone().unwrap_or_else(|| "none".into());
        match self {
            Clash::Card { card, clash } => {
                let (attribute, ours, theirs) = match clash {
                    CardClash::Quantity { ours, theirs } => {
                        ("quantity", ours.to_string(), theirs.to_string())
                    }
                    CardClash::Categories { ours, theirs } => {
                        ("categories", categories(ours), categories(theirs))
                    }
                    CardClash::Printing { ours, theirs } => {
                        ("printing", reference(ours), reference(theirs))
                    }
                    CardClash::Finish { ours, theirs } => (
                        "finish",
                        finish_name(*ours).into(),
                        finish_name(*theirs).into(),
                    ),
                    CardClash::Removed { by, kept } => {
                        let (gone, kept) = ("removed".to_string(), card_said(kept));
                        match by {
                            Side::Ours => ("in the deck", gone, kept),
                            Side::Theirs => ("in the deck", kept, gone),
                        }
                    }
                };
                Said {
                    subject: card.clone(),
                    attribute,
                    ours,
                    theirs,
                }
            }
            Clash::Field {
                field,
                ours,
                theirs,
            } => Said {
                subject: "the deck".into(),
                attribute: field.as_str(),
                ours: text(ours),
                theirs: text(theirs),
            },
            Clash::Category { name, ours, theirs } => Said {
                subject: format!("category {}", name.to_lowercase()),
                attribute: "declared",
                ours: declared_said(*ours),
                theirs: declared_said(*theirs),
            },
        }
    }
}

/// What merging came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merged {
    /// Ours' text with theirs' changes in it, and those changes as a commit
    /// message lists them, from ours to the result.
    Clean { text: String, taken: Vec<String> },
    /// Every clash, in a fixed order; the `pick` passed in left at least one
    /// of them unsettled. Merging again with a side for each index settles
    /// them.
    Clashed(Vec<Clash>),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MergeError {
    #[error("{side} is not a deck: {error}")]
    Unreadable {
        side: &'static str,
        error: Box<DeckError>,
    },
    /// The merge itself made something the format refuses, such as a card a
    /// retyped category puts in two places at once.
    #[error("merged, it would not be a deck: {0}")]
    Result(Box<EditError>),
}

/// How each version's value of one thing compares.
enum Three {
    /// Ours' value stands.
    Keep,
    /// Theirs changed it and ours did not.
    Take,
    Clash,
}

fn three<T: PartialEq + ?Sized>(base: &T, ours: &T, theirs: &T) -> Three {
    if theirs == base || ours == theirs {
        Three::Keep
    } else if ours == base {
        Three::Take
    } else {
        Three::Clash
    }
}

/// One edit to a line of ours.
enum LineEdit {
    Qty(NonZeroU32),
    Categories(Vec<String>),
    Card(CardRef),
    Finish(crate::deck::Finish),
}

fn same(a: &Card, b: &Card) -> bool {
    a.qty == b.qty && a.categories == b.categories && a.card == b.card && a.finish == b.finish
}

/// Clashes as they are found, each settled by `pick` of its index or left
/// open.
struct Clashes<P> {
    pick: P,
    all: Vec<Clash>,
    open: bool,
}

impl<P: Fn(usize) -> Option<Side>> Clashes<P> {
    /// Whether theirs' value is taken.
    fn theirs(&mut self, clash: Clash) -> bool {
        let side = (self.pick)(self.all.len());
        self.all.push(clash);
        match side {
            Some(Side::Theirs) => true,
            Some(Side::Ours) => false,
            None => {
                self.open = true;
                false
            }
        }
    }
}

/// The edits that bring one card of ours to theirs where it should.
fn card_edits<P: Fn(usize) -> Option<Side>>(
    label: &str,
    base: Option<&Card>,
    ours: &Card,
    theirs: &Card,
    clashes: &mut Clashes<P>,
) -> Vec<LineEdit> {
    let mut out = Vec::new();
    let mut attribute = |three: Three, clash: CardClash, edit: LineEdit| {
        let take = match three {
            Three::Keep => false,
            Three::Take => true,
            Three::Clash => clashes.theirs(Clash::Card {
                card: label.to_owned(),
                clash,
            }),
        };
        if take {
            out.push(edit);
        }
    };
    // A card both added has no base: any difference is a clash.
    let field = |b: Option<&Card>, f: &dyn Fn(&Card, &Card) -> bool| -> Three {
        if f(ours, theirs) || b.is_some_and(|b| f(b, theirs)) {
            Three::Keep
        } else if b.is_some_and(|b| f(b, ours)) {
            Three::Take
        } else {
            Three::Clash
        }
    };
    attribute(
        field(base, &|a, b| a.qty == b.qty),
        CardClash::Quantity {
            ours: ours.qty,
            theirs: theirs.qty,
        },
        LineEdit::Qty(theirs.qty),
    );
    attribute(
        field(base, &|a, b| a.categories == b.categories),
        CardClash::Categories {
            ours: ours.categories.clone(),
            theirs: theirs.categories.clone(),
        },
        LineEdit::Categories(theirs.categories.clone()),
    );
    attribute(
        field(base, &|a, b| a.card == b.card),
        CardClash::Printing {
            ours: ours.card.clone(),
            theirs: theirs.card.clone(),
        },
        LineEdit::Card(theirs.card.clone()),
    );
    attribute(
        field(base, &|a, b| a.finish == b.finish),
        CardClash::Finish {
            ours: ours.finish,
            theirs: theirs.finish,
        },
        LineEdit::Finish(theirs.finish),
    );
    out
}

fn parse(side: &'static str, text: &str) -> Result<Deck, MergeError> {
    Deck::parse(text).map_err(|error| MergeError::Unreadable {
        side,
        error: Box::new(error),
    })
}

/// `ours` and `theirs`, both made from `base`, merged by card: ours' text
/// with theirs' changes applied. A clash is settled by `pick` of its index
/// in [`Merged::Clashed`]'s list, which is the same list for the same three
/// texts; one `pick` leaves open is returned with every other clash.
pub fn merge(
    base: &str,
    ours: &str,
    theirs: &str,
    pick: impl Fn(usize) -> Option<Side>,
) -> Result<Merged, MergeError> {
    let b = parse("the merge base", base)?;
    let o = parse("ours", ours)?;
    let t = parse("theirs", theirs)?;
    let mut names = HashMap::new();
    printing_names(base, &b, &mut names);
    printing_names(theirs, &t, &mut names);
    printing_names(ours, &o, &mut names);
    let name_of = |p: &Printing| names.get(p).cloned();
    let taken = |text: String, merged: &Deck| Merged::Clean {
        taken: Diff::new(&o, merged, name_of)
            .changes
            .into_iter()
            .map(|(_, t)| t)
            .collect(),
        text,
    };
    if theirs == base || theirs == ours {
        return Ok(taken(ours.to_owned(), &o));
    }
    if ours == base {
        return Ok(taken(theirs.to_owned(), &t));
    }

    let mut clashes = Clashes {
        pick,
        all: Vec::new(),
        open: false,
    };
    let to_ours: HashMap<usize, usize> = pair_lines(&b, &o, name_of).into_iter().collect();
    let to_theirs: HashMap<usize, usize> = pair_lines(&b, &t, name_of).into_iter().collect();
    let label = |c: &Card| label_of(c, &name_of);

    let mut edits: Vec<(usize, LineEdit)> = Vec::new();
    let mut removed: Vec<usize> = Vec::new();
    let mut pushed: Vec<usize> = Vec::new();
    // Theirs' line each line of ours is: where theirs' new lines go.
    let mut theirs_in_ours: HashMap<usize, usize> = HashMap::new();
    for (i, was) in b.cards.iter().enumerate() {
        match (to_ours.get(&i), to_theirs.get(&i)) {
            (Some(&j), Some(&k)) => {
                theirs_in_ours.insert(k, j);
                let lines = card_edits(
                    &label(was),
                    Some(was),
                    &o.cards[j],
                    &t.cards[k],
                    &mut clashes,
                );
                edits.extend(lines.into_iter().map(|e| (j, e)));
            }
            (Some(&j), None) => {
                let kept = &o.cards[j];
                if same(was, kept)
                    || clashes.theirs(Clash::Card {
                        card: label(was),
                        clash: CardClash::Removed {
                            by: Side::Theirs,
                            kept: kept.clone(),
                        },
                    })
                {
                    removed.push(j);
                }
            }
            (None, Some(&k)) => {
                let kept = &t.cards[k];
                if !same(was, kept)
                    && clashes.theirs(Clash::Card {
                        card: label(was),
                        clash: CardClash::Removed {
                            by: Side::Ours,
                            kept: kept.clone(),
                        },
                    })
                {
                    pushed.push(k);
                }
            }
            (None, None) => {}
        }
    }

    // What each side added, paired with the other's adds as a diff pairs.
    let ours_new: Vec<usize> = (0..o.cards.len())
        .filter(|j| !to_ours.values().any(|v| v == j))
        .collect();
    let theirs_new: Vec<usize> = (0..t.cards.len())
        .filter(|k| !to_theirs.values().any(|v| v == k))
        .collect();
    let both = pair(
        &ours_new.iter().map(|&j| &o.cards[j]).collect::<Vec<_>>(),
        &theirs_new.iter().map(|&k| &t.cards[k]).collect::<Vec<_>>(),
        label,
    );
    for &k in &theirs_new {
        match both.iter().find(|&&(_, y)| theirs_new[y] == k) {
            Some(&(x, _)) => {
                let j = ours_new[x];
                theirs_in_ours.insert(k, j);
                let lines = card_edits(
                    &label(&o.cards[j]),
                    None,
                    &o.cards[j],
                    &t.cards[k],
                    &mut clashes,
                );
                edits.extend(lines.into_iter().map(|e| (j, e)));
            }
            None => pushed.push(k),
        }
    }

    // Declared categories, one value each.
    let names_declared: BTreeSet<&str> = [&b, &o, &t]
        .iter()
        .flat_map(|d| d.categories.iter().map(|c| c.name.as_str()))
        .collect();
    let mut declares: Vec<(String, Declared)> = Vec::new();
    for name in names_declared {
        let (db, d_o, dt) = (
            Declared::of(&b, name),
            Declared::of(&o, name),
            Declared::of(&t, name),
        );
        let take = match three(&db, &d_o, &dt) {
            Three::Keep => false,
            Three::Take => true,
            Three::Clash => clashes.theirs(Clash::Category {
                name: name.to_owned(),
                ours: d_o,
                theirs: dt,
            }),
        };
        if take {
            declares.push((name.to_owned(), dt));
        }
    }

    // The deck's own fields.
    let mut fields: Vec<(Field, Option<String>)> = Vec::new();
    let cover = |d: &Deck| d.cover.as_ref().map(Printing::to_string);
    for (field, vb, vo, vt) in [
        (Field::Name, b.name.clone(), o.name.clone(), t.name.clone()),
        (
            Field::Format,
            b.format.clone(),
            o.format.clone(),
            t.format.clone(),
        ),
        (Field::Cover, cover(&b), cover(&o), cover(&t)),
        (
            Field::Description,
            b.description.clone(),
            o.description.clone(),
            t.description.clone(),
        ),
    ] {
        let take = match three(&vb, &vo, &vt) {
            Three::Keep => false,
            Three::Take => true,
            Three::Clash => clashes.theirs(Clash::Field {
                field,
                ours: vo,
                theirs: vt.clone(),
            }),
        };
        if take {
            fields.push((field, vt));
        }
    }

    if clashes.open {
        return Ok(Merged::Clashed(clashes.all));
    }

    pushed.sort_unstable();
    let lines = Lines {
        edits,
        removed,
        pushed,
        theirs_in_ours,
    };
    let text = write(ours, &o, &t, theirs, lines, &declares, &fields)
        .map_err(|e| MergeError::Result(Box::new(e)))?;
    let merged = Deck::parse(&text).map_err(|e| {
        MergeError::Result(Box::new(match e {
            DeckError::Undeclared { category, .. } => EditError::StillUsed(category),
            e => e.into(),
        }))
    })?;
    Ok(taken(text, &merged))
}

/// What the merge does to ours' card lines.
struct Lines {
    edits: Vec<(usize, LineEdit)>,
    /// Ours' lines that go.
    removed: Vec<usize>,
    /// Theirs' lines that come in, in theirs' order.
    pushed: Vec<usize>,
    /// For theirs' lines that are also ours', which line of ours.
    theirs_in_ours: HashMap<usize, usize>,
}

/// Ours' text with the merge's edits made, in an order that cannot trip
/// over itself: categories declared and retyped, lines changed in place,
/// lines removed from the bottom up, theirs' new lines put in below the
/// line theirs has above them (or first, with none), categories
/// no card is in any longer dropped, then the deck's own fields.
#[allow(clippy::too_many_arguments)]
fn write(
    ours: &str,
    o: &Deck,
    t: &Deck,
    theirs: &str,
    lines: Lines,
    declares: &[(String, Declared)],
    fields: &[(Field, Option<String>)],
) -> Result<String, EditError> {
    let unchecked: edit::Check = |_| Ok(());
    let mut text = ours.to_owned();
    let kind = |d: Declared| match d {
        Declared::Typed(k) => Some(k),
        _ => None,
    };
    let mut declared: Vec<&str> = o.categories.iter().map(|c| c.name.as_str()).collect();
    for (name, d) in declares {
        if *d == Declared::Absent {
            continue;
        }
        if declared.contains(&name.as_str()) {
            text = edit::retype_category(&text, name, kind(*d), unchecked)?;
        } else {
            text = edit::declare_category_with(&text, name, kind(*d), unchecked)?;
            declared.push(name);
        }
    }
    let Lines {
        edits,
        mut removed,
        pushed,
        theirs_in_ours,
    } = lines;
    // A category a card of theirs brings in, declared as theirs declares it.
    let brought = edits
        .iter()
        .filter_map(|(_, e)| match e {
            LineEdit::Categories(c) => Some(c),
            _ => None,
        })
        .chain(pushed.iter().map(|&k| &t.cards[k].categories))
        .flatten();
    for name in brought {
        if !declared.contains(&name.as_str()) {
            let k = t.category(name).and_then(|c| c.kind);
            text = edit::declare_category_with(&text, name, k, unchecked)?;
            declared.push(name);
        }
    }

    for (j, e) in &edits {
        text = match e {
            LineEdit::Qty(q) => edit::set_qty(&text, *j, q.get(), unchecked)?,
            LineEdit::Categories(c) => edit::set_categories_with(&text, *j, c, unchecked)?,
            LineEdit::Card(CardRef::Printing(p)) => {
                edit::set_printing(&text, *j, &p.set, &p.num, unchecked)?
            }
            LineEdit::Card(CardRef::Name(n)) => edit::set_name(&text, *j, n, unchecked)?,
            LineEdit::Finish(f) => edit::set_finish(&text, *j, *f, unchecked)?,
        };
    }
    // Where each line of ours is now, as lines go and come.
    let mut at: Vec<Option<usize>> = (0..o.cards.len()).map(Some).collect();
    removed.sort_unstable_by(|a, b| b.cmp(a));
    removed.dedup();
    for j in removed {
        text = edit::remove_line(&text, j, unchecked)?;
        at[j] = None;
        for p in at.iter_mut().flatten().filter(|p| **p > j) {
            *p -= 1;
        }
    }
    let comments = card_comments(theirs);
    let mut came: HashMap<usize, usize> = HashMap::new();
    for &k in &pushed {
        let above = (0..k).rev().find_map(|above| {
            came.get(&above)
                .copied()
                .or_else(|| theirs_in_ours.get(&above).and_then(|&j| at[j]))
        });
        let line = above.map_or(0, |a| a + 1);
        let comment = comments.get(k).cloned().flatten();
        text = edit::insert_card(&text, line, &t.cards[k], comment.as_deref())?;
        for p in at.iter_mut().flatten().chain(came.values_mut()) {
            if *p >= line {
                *p += 1;
            }
        }
        came.insert(k, line);
    }

    // Dropped only where no card is left in it.
    let undeclared: Vec<&String> = declares
        .iter()
        .filter(|(_, d)| *d == Declared::Absent)
        .map(|(n, _)| n)
        .collect();
    if !undeclared.is_empty() {
        let in_use: BTreeSet<String> = Deck::parse(&text)?
            .cards
            .into_iter()
            .flat_map(|c| c.categories)
            .collect();
        for name in undeclared {
            if !in_use.contains(name) {
                text = edit::undeclare_category(&text, name)?;
            }
        }
    }

    if fields.is_empty() {
        return Ok(text);
    }
    let mut doc = edit::document(&text)?;
    for (field, value) in fields {
        let value = match (field, value) {
            (_, None) => None,
            (Field::Description, Some(d)) => Some(
                crate::deck::quote_multiline(d)
                    .parse::<Value>()
                    .map_err(|e| EditError::Toml(e.to_string()))?,
            ),
            (_, Some(v)) => Some(Value::from(v.as_str())),
        };
        edit::set_meta(&mut doc, field.as_str(), value);
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests;
