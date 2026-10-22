//! How a file both sides changed merges, chosen by its path, as git chooses
//! a merge driver: a deck's file by card (`chip_decklist::merge`), every
//! other file by lines (`meldweb_git::merge_lines_with`). `meldweb-git`
//! merges trees and knows nothing of Magic; it calls [`merge_file`] through
//! its `MergeFile` hook for each file both sides changed.
//!
//! A deck merges by card because a line merge clashed wherever two edits
//! touched neighbouring lines, and opening a deck pins the printing of every
//! card named by name (ADR-0027), so a fork clashed with nearly any change
//! of the deck it came from. The collection, the wanted list, the settings,
//! criteria and custom cards still merge by lines for now.

use chip_decklist::merge::{self as by_card, Merged};
use facet::Facet;
use meldweb_git::{merge_lines_with, BranchMerge, Hunk, Side};

/// How the user settled a merge's clashes: `picks[i]` for a deck's clash
/// `i`, and `prefer` for any clash not picked and for every stretch of
/// lines both changed in a file merged by lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settle {
    pub prefer: Option<Side>,
    pub picks: Vec<Side>,
}

impl From<Option<Side>> for Settle {
    fn from(prefer: Option<Side>) -> Self {
        Settle {
            prefer,
            picks: Vec::new(),
        }
    }
}

impl Settle {
    fn deck(&self, i: usize) -> Option<by_card::Side> {
        let side = self.picks.get(i).copied().or(self.prefer)?;
        Some(match side {
            Side::Ours => by_card::Side::Ours,
            Side::Theirs => by_card::Side::Theirs,
        })
    }
}

/// A side, as the page sends a pick: `ours` is the branch merged into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(rename_all = "kebab-case")]
pub enum Pick {
    Ours,
    Theirs,
}

impl From<Pick> for Side {
    fn from(p: Pick) -> Side {
        match p {
            Pick::Ours => Side::Ours,
            Pick::Theirs => Side::Theirs,
        }
    }
}

/// How the user settled one file of one branch's sync merge: its deck's
/// clashes one by one, or the whole file one way. Keyed by the branch and
/// the path, and guarded by the two tips the clashes were shown from, so a
/// pick made against one merge never settles a later one whose clash `i`
/// may be another card.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct FileSettle {
    pub branch: String,
    pub path: String,
    /// The branch's tip here, as the clashes were shown from.
    pub ours: String,
    /// The remote's tip of the branch, as the clashes were shown from.
    pub theirs: String,
    #[facet(default)]
    pub prefer: Option<Pick>,
    #[facet(default)]
    pub picks: Vec<Pick>,
}

/// How the user settled a sync's clashes: file by file, and `prefer` for
/// every file not settled on its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncSettle {
    pub prefer: Option<Side>,
    pub files: Vec<FileSettle>,
}

impl SyncSettle {
    /// What settles `path` in `at`'s merge: its own picks when they were
    /// made against these two tips, otherwise `prefer`.
    pub fn of(&self, at: &BranchMerge<'_>, path: &str) -> Settle {
        let own = self.files.iter().find(|f| {
            f.branch == at.branch.as_str()
                && f.path == path
                && f.ours == at.ours.to_hex()
                && f.theirs == at.theirs.to_hex()
        });
        match own {
            Some(f) => Settle {
                prefer: f.prefer.map(Side::from),
                picks: f.picks.iter().copied().map(Side::from).collect(),
            },
            None => Settle::from(self.prefer),
        }
    }
}

/// [`merge_bytes`] for one file of one branch's sync merge, settled by its
/// own picks when it has them. A file the user settled one way whole takes
/// that side's bytes even where merging it would not make a file Progress
/// Engine reads, as `prefer` does for the whole sync.
pub fn merge_sync_bytes(
    at: &BranchMerge<'_>,
    path: &str,
    base: &[u8],
    ours: &[u8],
    theirs: &[u8],
    settle: &SyncSettle,
    stop: &mut impl FnMut(FileMerge),
) -> Option<Vec<u8>> {
    let own = settle.of(at, path);
    let mut stopped = Vec::new();
    let merged = merge_bytes(path, base, ours, theirs, &own, &mut |s| stopped.push(s));
    if merged.is_some() {
        return merged;
    }
    match own.prefer {
        Some(Side::Ours) => Some(ours.to_vec()),
        Some(Side::Theirs) => Some(theirs.to_vec()),
        None => {
            stopped.into_iter().for_each(stop);
            None
        }
    }
}

/// One thing in a deck both sides changed differently, in words: `Sol Ring`,
/// `quantity`, `2`, `3`. A merge's `picks` settle it by its `index`.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct MergeClash {
    pub path: String,
    pub index: u32,
    /// The card, `the deck`, or `category ramp`.
    pub subject: String,
    /// `quantity`, `categories`, `printing`, `finish`, `in the deck` (one
    /// side removed it), or one of the deck's own fields.
    pub attribute: String,
    pub ours: String,
    pub theirs: String,
}

/// What merging one file came to.
#[derive(Debug)]
pub enum FileMerge {
    Merged(String),
    /// A deck's cards or fields both changed differently.
    Clashed(Vec<MergeClash>),
    /// Stretches of lines both changed differently.
    Hunks(Vec<Hunk>),
    /// What the merge would make is not a file Progress Engine reads.
    Unreadable(String),
}

/// Whether `path` is a deck's file: `<stem>.deck.toml` at the root of its
/// own line (ADR-0041).
pub fn is_deck(path: &str) -> bool {
    path.ends_with(".deck.toml") && !path.contains('/')
}

/// `ours` and `theirs`, both made from `base`, merged as `path`'s kind of
/// file merges, with `settle` settling what it can.
pub fn merge_file(path: &str, base: &str, ours: &str, theirs: &str, settle: &Settle) -> FileMerge {
    if is_deck(path) {
        return match by_card::merge(base, ours, theirs, |i| settle.deck(i)) {
            Ok(Merged::Clean { text, .. }) => FileMerge::Merged(text),
            Ok(Merged::Clashed(clashes)) => FileMerge::Clashed(
                clashes
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let said = c.said();
                        MergeClash {
                            path: path.to_owned(),
                            index: u32::try_from(i).unwrap_or(u32::MAX),
                            subject: said.subject,
                            attribute: said.attribute.to_owned(),
                            ours: said.ours,
                            theirs: said.theirs,
                        }
                    })
                    .collect(),
            ),
            Err(e) => FileMerge::Unreadable(e.to_string()),
        };
    }
    match merge_lines_with(base, ours, theirs, settle.prefer) {
        Err(hunks) => FileMerge::Hunks(hunks),
        Ok(merged) => match crate::unreadable(path, theirs, &merged) {
            None => FileMerge::Merged(merged),
            Some(reason) => FileMerge::Unreadable(reason),
        },
    }
}

/// [`merge_file`] over bytes, as `meldweb_git`'s `MergeFile` hook takes
/// them: `None` for a file that is not text or does not merge, with `stop`
/// told why.
pub fn merge_bytes(
    path: &str,
    base: &[u8],
    ours: &[u8],
    theirs: &[u8],
    settle: &Settle,
    stop: &mut impl FnMut(FileMerge),
) -> Option<Vec<u8>> {
    fn text(b: &[u8]) -> Option<&str> {
        std::str::from_utf8(b).ok()
    }
    match merge_file(path, text(base)?, text(ours)?, text(theirs)?, settle) {
        FileMerge::Merged(m) => Some(m.into_bytes()),
        other => {
            stop(other);
            None
        }
    }
}
