//! What changed between two versions of a deck, as the commit message that
//! saves it (ADR-0021): `lantern: +1 Sol Ring, -1 Mind Stone, Sol Ring: ramp →
//! draw`, so the repo's log reads as the deck's changelog.
//!
//! The message is a function of the two decks and the path alone. It is
//! lowercase apart from card and deck names, and ordered by kind of change,
//! then by card. Past [`SUBJECT_CHANGES`] changes the subject ends in
//! `and N more` and the body lists every change, one per line. The changes
//! themselves are [`crate::diff`]'s, so a message and a comparison in Curator
//! never disagree.
//!
//! A card named by printing has no name in the file, only the comment the
//! writer leaves beside it. [`commit_message_for_text`] reads those comments
//! from both versions and labels a printing with its name when either has one,
//! and as `set/num` when neither does.

use crate::deck::{CardRef, Deck, DeckError, Finish, Printing};
use crate::diff::{parse_pair, Diff};

/// Changes the subject names before it says `and N more`.
pub const SUBJECT_CHANGES: usize = 3;

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

/// The commit message for saving `before` as `after` at `path`. `name_of`
/// labels a card named by printing; without a name it is its `set/num`.
pub fn commit_message(
    before: &Deck,
    after: &Deck,
    path: &str,
    name_of: impl Fn(&Printing) -> Option<String>,
) -> String {
    let diff = Diff::new(before, after, name_of);
    let lines: Vec<&str> = diff.changes.iter().map(|(_, t)| t.as_str()).collect();
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

/// [`commit_message`] between two deck texts, labelling each printing with
/// the name commented beside it in either, the newer text's first. An empty
/// `before` is the empty deck, for a deck's first save.
pub fn commit_message_for_text(before: &str, after: &str, path: &str) -> Result<String, DeckError> {
    let (old, new, names) = parse_pair(before, after)?;
    Ok(commit_message(&old, &new, path, |p| names.get(p).cloned()))
}
