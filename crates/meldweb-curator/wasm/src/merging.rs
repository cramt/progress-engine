//! How a file both sides changed merges, chosen by its path, as git chooses
//! a merge driver: a deck's file by card (`chip_decklist::merge`), every
//! other file by lines (`meldweb_git::merge_lines_with`). `meldweb-git`
//! merges trees and knows nothing of Magic; it calls [`merge_bytes`] through
//! its `MergeFile` hook for each file both sides changed.
//!
//! A deck merges by card because a line merge clashed wherever two edits
//! touched neighbouring lines, and opening a deck pins the printing of every
//! card named by name (ADR-0027), so a fork clashed with nearly any change
//! of the deck it came from. The collection, the wanted list, the settings,
//! criteria and custom cards still merge by lines for now.
//!
//! A merge that stops is a [`Conflict`]: every file it stopped on, each by
//! what settles it, and a guard naming what it was worked out from. The
//! page hands the guard back with its picks as a [`Settle`], and only a
//! merge with the same guard takes them, so a pick settles only the clash
//! it was shown for: a branch merge, a take and a sync alike.

use std::collections::BTreeMap;

pub use chip_decklist::merge::Side;
use chip_decklist::merge::{self as by_card, Merged};
use facet::Facet;
use meldweb_git::{merge_lines_with, BranchName, CommitId};

/// `side` as `meldweb-git` merges lines and trees.
pub fn git_side(side: Side) -> meldweb_git::Side {
    match side {
        Side::Ours => meldweb_git::Side::Ours,
        Side::Theirs => meldweb_git::Side::Theirs,
    }
}

/// What a conflict was worked out from. Picks made against it settle only
/// a merge worked out from the same; a refused save's would be the blob
/// the save started from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guard {
    /// Merging `theirs` into `branch` while it is at `ours`.
    Tips {
        branch: BranchName,
        ours: CommitId,
        theirs: CommitId,
    },
}

impl Guard {
    pub fn tips(branch: &BranchName, ours: CommitId, theirs: CommitId) -> Self {
        Guard::Tips {
            branch: branch.clone(),
            ours,
            theirs,
        }
    }

    /// The guard as the page carries it, which it never reads.
    pub fn token(&self) -> String {
        match self {
            Guard::Tips {
                branch,
                ours,
                theirs,
            } => format!("tips {branch} {ours} {theirs}"),
        }
    }
}

/// One thing in a deck both sides changed differently, in words: `Sol Ring`,
/// `quantity`, `2`, `3`. A [`FileSettle::EachClash`] settles it by `index`,
/// its place among its file's clashes.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct MergeClash {
    pub index: u32,
    /// The card, `the deck`, or `category ramp`.
    pub subject: String,
    /// `quantity`, `categories`, `printing`, `finish`, `in the deck` (one
    /// side removed it), or one of the deck's own fields.
    pub attribute: String,
    pub ours: String,
    pub theirs: String,
}

/// One stretch of a file merged by lines that both sides changed, differently.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct ConflictHunk {
    /// The lines as the merge base had them.
    pub base: Vec<String>,
    /// As the side merged into has them.
    pub ours: Vec<String>,
    /// As the side merged from has them.
    pub theirs: Vec<String>,
}

/// Why a file that only a side for the whole of it settles did not merge.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum WholeFile {
    /// Stretches of lines both changed differently.
    Lines { hunks: Vec<ConflictHunk> },
    /// What the merge would make is not a file Progress Engine reads.
    Unreadable { reason: String },
}

/// One file a merge stopped on, by what settles it.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(tag = "needs", rename_all = "camelCase")]
pub enum FileConflict {
    /// A deck's cards or fields both changed differently: a side for each
    /// clash settles it.
    EachClash {
        path: String,
        clashes: Vec<MergeClash>,
    },
    /// A side for the whole file settles it.
    WholeFile { path: String, why: WholeFile },
    /// One side deleted or replaced it whole, or it is not text: only
    /// settling everything one way settles it.
    AllOneWay { path: String },
}

/// A merge that stopped: the files it stopped on, in git's order, and the
/// guard of what it was worked out from, which a [`Settle`] hands back.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct Conflict {
    pub guard: String,
    pub files: Vec<FileConflict>,
}

impl Conflict {
    /// The conflict over `paths`, what git says did not merge, each file's
    /// why as [`merge_bytes`] left it in `stops`.
    pub fn of(guard: &Guard, paths: impl IntoIterator<Item = String>, stops: &mut Stops) -> Self {
        Conflict {
            guard: guard.token(),
            files: paths
                .into_iter()
                .map(|path| {
                    stops
                        .0
                        .remove(&path)
                        .unwrap_or(FileConflict::AllOneWay { path })
                })
                .collect(),
        }
    }
}

/// How the user settled one file of a conflict, as the conflict said it is
/// settled.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(tag = "needs", rename_all = "camelCase")]
pub enum FileSettle {
    /// A side for each of its clashes, by [`MergeClash::index`].
    EachClash { path: String, picks: Vec<Side> },
    /// The whole file one way.
    WholeFile { path: String, side: Side },
}

/// How the user settled a [`Conflict`]: file by file, and `prefer` for what
/// is not, under the guard of the conflict the picks were made against.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct Settle {
    pub guard: String,
    #[facet(default)]
    pub prefer: Option<Side>,
    #[facet(default)]
    pub files: Vec<FileSettle>,
}

/// A settle made against another merge than the one at hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stale;

/// What settles one merge, once its picks are known to be for it: its files
/// one by one, and `prefer` for everything else.
#[derive(Debug, Clone, Copy, Default)]
pub struct Settling<'a> {
    files: &'a [FileSettle],
    pub prefer: Option<Side>,
}

/// What settles one file: a side for each of a deck's clashes by its index,
/// and `whole` for any clash not picked, for every stretch of lines both
/// changed, and for the file itself where it would not merge.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileSettling<'a> {
    pub picks: &'a [Side],
    pub whole: Option<Side>,
}

impl<'a> Settling<'a> {
    /// `settle` for the merge `guard` names, or [`Stale`] when it was made
    /// against another; nothing settles a merge with no settle.
    pub fn checked(settle: Option<&'a Settle>, guard: &Guard) -> Result<Self, Stale> {
        match settle {
            None => Ok(Settling::default()),
            Some(s) if s.guard == guard.token() => Ok(Settling {
                files: &s.files,
                prefer: s.prefer,
            }),
            Some(_) => Err(Stale),
        }
    }

    fn file(&self, path: &str) -> FileSettling<'a> {
        let own = self.files.iter().find(|f| match f {
            FileSettle::EachClash { path: p, .. } | FileSettle::WholeFile { path: p, .. } => {
                p == path
            }
        });
        match own {
            Some(FileSettle::EachClash { picks, .. }) => FileSettling {
                picks,
                whole: self.prefer,
            },
            Some(FileSettle::WholeFile { side, .. }) => FileSettling {
                picks: &[],
                whole: Some(*side),
            },
            None => FileSettling {
                picks: &[],
                whole: self.prefer,
            },
        }
    }
}

/// How the user settled a sync: each branch's merge by the settle made
/// against it, and `prefer` for every branch and file with none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
pub struct SyncSettle {
    #[facet(default)]
    pub prefer: Option<Side>,
    #[facet(default)]
    pub settles: Vec<Settle>,
}

impl SyncSettle {
    /// What settles the merge `guard` names: the settle made against it,
    /// otherwise `prefer` alone. A settle made against other tips of the
    /// branch settles nothing, so the clashes are shown again.
    pub fn settling(&self, guard: &Guard) -> Settling<'_> {
        let token = guard.token();
        match self.settles.iter().find(|s| s.guard == token) {
            Some(s) => Settling {
                files: &s.files,
                prefer: s.prefer.or(self.prefer),
            },
            None => Settling {
                files: &[],
                prefer: self.prefer,
            },
        }
    }
}

/// Why each file of one merge did not merge, by its path.
#[derive(Debug, Default)]
pub struct Stops(BTreeMap<String, FileConflict>);

/// Whether `path` is a deck's file: `<stem>.deck.toml` at the root of its
/// own line (ADR-0041).
pub fn is_deck(path: &str) -> bool {
    path.ends_with(".deck.toml") && !path.contains('/')
}

/// `ours` and `theirs`, both made from `base`, merged as `path`'s kind of
/// file merges, with `how` settling what it can; what is left is the file's
/// [`FileConflict`].
pub fn merge_file(
    path: &str,
    base: &str,
    ours: &str,
    theirs: &str,
    how: FileSettling<'_>,
) -> Result<String, FileConflict> {
    let unreadable = |reason: String| FileConflict::WholeFile {
        path: path.to_owned(),
        why: WholeFile::Unreadable { reason },
    };
    if is_deck(path) {
        return match by_card::merge(base, ours, theirs, |i| {
            how.picks.get(i).copied().or(how.whole)
        }) {
            Ok(Merged::Clean { text, .. }) => Ok(text),
            Ok(Merged::Clashed(clashes)) => Err(FileConflict::EachClash {
                path: path.to_owned(),
                clashes: clashes
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let said = c.said();
                        MergeClash {
                            index: u32::try_from(i).unwrap_or(u32::MAX),
                            subject: said.subject,
                            attribute: said.attribute.to_owned(),
                            ours: said.ours,
                            theirs: said.theirs,
                        }
                    })
                    .collect(),
            }),
            Err(e) => Err(unreadable(e.to_string())),
        };
    }
    match merge_lines_with(base, ours, theirs, how.whole.map(git_side)) {
        Err(hunks) => Err(FileConflict::WholeFile {
            path: path.to_owned(),
            why: WholeFile::Lines {
                hunks: hunks
                    .into_iter()
                    .map(|h| ConflictHunk {
                        base: h.base,
                        ours: h.ours,
                        theirs: h.theirs,
                    })
                    .collect(),
            },
        }),
        Ok(merged) => match crate::unreadable(path, theirs, &merged) {
            None => Ok(merged),
            Some(reason) => Err(unreadable(reason)),
        },
    }
}

/// One file both sides of a merge changed, over bytes as `meldweb_git`'s
/// merge hooks take them, settled by `settling`: `None` when it does not
/// merge, with why in `stops`. A file settled one way whole takes that
/// side's bytes even where merging it would not make a file Progress Engine
/// reads, or it is not text. Every merge Curator makes goes through here: a
/// branch merge, a take and a sync.
pub fn merge_bytes(
    path: &str,
    base: &[u8],
    ours: &[u8],
    theirs: &[u8],
    settling: &Settling<'_>,
    stops: &mut Stops,
) -> Option<Vec<u8>> {
    fn text(b: &[u8]) -> Option<&str> {
        std::str::from_utf8(b).ok()
    }
    let how = settling.file(path);
    let merged = match (text(base), text(ours), text(theirs)) {
        (Some(b), Some(o), Some(t)) => Some(merge_file(path, b, o, t, how)),
        _ => None,
    };
    let stopped = match merged {
        Some(Ok(m)) => return Some(m.into_bytes()),
        Some(Err(why)) => Some(why),
        None => None,
    };
    match how.whole {
        Some(Side::Ours) => Some(ours.to_vec()),
        Some(Side::Theirs) => Some(theirs.to_vec()),
        None => {
            if let Some(why) = stopped {
                stops.0.insert(path.to_owned(), why);
            }
            None
        }
    }
}
