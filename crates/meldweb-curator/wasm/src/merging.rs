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
use chip_decklist::stem::DeckStem;
use facet::Facet;
use meldweb_git::{
    merge_lines_with, sync_message, BlobId, BranchMerge, BranchName, CommitId, MergeBranch, Repo,
    RepoPath, Store, TreeId, ORIGIN,
};

use crate::deck_merge;
use crate::saving::{self, Unsaid};

/// `side` as `meldweb-git` merges lines and trees.
pub fn git_side(side: Side) -> meldweb_git::Side {
    match side {
        Side::Ours => meldweb_git::Side::Ours,
        Side::Theirs => meldweb_git::Side::Theirs,
    }
}

/// What a conflict was worked out from. Picks made against it settle only
/// a merge worked out from the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guard {
    /// Merging `theirs` into `branch` while it is at `ours`.
    Tips {
        branch: BranchName,
        ours: CommitId,
        theirs: CommitId,
    },
    /// Saving `ours`, made from `base` (`None`: no file), over `theirs`,
    /// what `path` on `branch` is now: a save's merge, whose picks are for
    /// these three texts and no others.
    Blobs {
        branch: BranchName,
        path: RepoPath,
        base: Option<BlobId>,
        ours: BlobId,
        theirs: BlobId,
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
            Guard::Blobs {
                branch,
                path,
                base,
                ours,
                theirs,
            } => {
                let base = base.map_or_else(|| "-".to_owned(), |b| b.to_hex());
                format!("blobs {branch} {base} {ours} {theirs} {path}")
            }
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

/// How a sync merges each branch both sides moved: each file as
/// [`merge_bytes`] merges its kind, settled by the settle made against that
/// branch's tips, and a deck's line's merge commit saying what it did to
/// the deck, as a deck merge's does (`lantern: merge origin/main: +1 Sol
/// Ring`); any other branch's says `sync: merge origin/<branch>`.
pub struct SyncMerging<'a> {
    pub settle: &'a SyncSettle,
    /// Why each file did not merge, by branch.
    pub stops: BTreeMap<String, Stops>,
}

impl<S: Store> MergeBranch<S> for SyncMerging<'_> {
    fn merge_file(
        &mut self,
        at: &BranchMerge<'_>,
        path: &str,
        base: &[u8],
        ours: &[u8],
        theirs: &[u8],
    ) -> Option<Vec<u8>> {
        let settling = self
            .settle
            .settling(&Guard::tips(at.branch, at.ours, at.theirs));
        let stops = self.stops.entry(at.branch.as_str().to_owned()).or_default();
        merge_bytes(path, base, ours, theirs, &settling, stops)
    }

    fn message(
        &mut self,
        repo: &Repo<S>,
        at: &BranchMerge<'_>,
        tree: TreeId,
        taken: &[String],
    ) -> meldweb_git::Result<String> {
        let line = at
            .branch
            .as_str()
            .strip_prefix("decks/")
            .and_then(|rest| rest.split_once('/'))
            .and_then(|(stem, name)| Some((DeckStem::new(stem).ok()?, name)));
        let Some((stem, name)) = line else {
            return Ok(sync_message(at.branch, taken));
        };
        let deck = deck_merge::deck_path(&stem);
        let before = deck_merge::text(repo.read_file(&at.ours, &deck)?);
        let after = deck_merge::text(repo.read_file_in(tree, &deck)?);
        let said = deck_merge::message(
            &stem,
            &format!("{ORIGIN}/{name}"),
            before.as_deref(),
            after.as_deref(),
        );
        Ok(format!("{}\n", said.trim_end()))
    }
}

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
        // A path no editor writes is only text.
        Ok(merged) => match saving::message_for(path, theirs, &merged) {
            Err(Unsaid::Unreadable(reason)) => Err(unreadable(reason)),
            Ok(_) | Err(Unsaid::NotEdited) => Ok(merged),
        },
    }
}

/// One file both sides of a merge changed, over bytes as `meldweb_git`'s
/// merge hooks take them, settled by `settling`: `None` when it does not
/// merge, with why in `stops`. A file settled one way whole takes that
/// side's bytes even where merging it would not make a file Progress Engine
/// reads, or it is not text. Every merge Curator makes goes through here: a
/// branch merge, a take, a sync and a save.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(l: &[&str]) -> String {
        format!("{}\n", l.join("\n"))
    }

    fn deck(cards: &[&str]) -> String {
        let mut l = vec!["name = \"Lantern\"".to_owned(), "cards = [".to_owned()];
        l.extend(cards.iter().map(|c| format!("  {c},")));
        l.push("]".to_owned());
        lines(&l.iter().map(String::as_str).collect::<Vec<_>>())
    }

    const SOL: &str = "{ name = \"Sol Ring\" }";
    const STONE: &str = "{ name = \"Mind Stone\" }";
    const ISLAND: &str = "{ name = \"Island\", qty = 10 }";

    fn merge(path: &str, base: &str, ours: &str, theirs: &str) -> Result<String, FileConflict> {
        merge_file(path, base, ours, theirs, FileSettling::default())
    }

    #[test]
    fn a_deck_keeps_both_sides_changes_to_neighbouring_cards() {
        let base = deck(&[SOL, STONE, ISLAND]);
        let ours = base.replace("\"Sol Ring\" }", "\"Sol Ring\", qty = 2 }");
        let theirs = base.replace("\"Mind Stone\" }", "\"Mind Stone\", qty = 3 }");
        assert_eq!(
            merge("lantern.deck.toml", &base, &ours, &theirs).unwrap(),
            deck(&[
                "{ name = \"Sol Ring\", qty = 2 }",
                "{ name = \"Mind Stone\", qty = 3 }",
                ISLAND
            ])
        );
    }

    #[test]
    fn a_deck_lands_their_quantity_on_our_pinned_printing() {
        let base = deck(&[SOL, STONE, ISLAND]);
        let ours = base.replace(
            "{ name = \"Sol Ring\" },",
            "{ printing = \"sld/2417\" },  # Sol Ring",
        );
        let theirs = base.replace("\"Sol Ring\" }", "\"Sol Ring\", qty = 2 }");
        assert_eq!(
            merge("lantern.deck.toml", &base, &ours, &theirs).unwrap(),
            ours.replace("\"sld/2417\" }", "\"sld/2417\", qty = 2 }")
        );
    }

    #[test]
    fn a_deck_clash_names_the_card_and_what_both_changed() {
        let base = deck(&[SOL, STONE, ISLAND]);
        let ours = base.replace("qty = 10", "qty = 9");
        let theirs = base.replace("qty = 10", "qty = 11");
        assert_eq!(
            merge("lantern.deck.toml", &base, &ours, &theirs),
            Err(FileConflict::EachClash {
                path: "lantern.deck.toml".into(),
                clashes: vec![MergeClash {
                    index: 0,
                    subject: "Island".into(),
                    attribute: "quantity".into(),
                    ours: "9".into(),
                    theirs: "11".into(),
                }],
            })
        );
    }

    #[test]
    fn any_other_file_merges_by_lines_and_says_which_lines_clash() {
        let base = lines(&["cards = [", "  a,", "  b,", "  c,", "  d,", "]"]);
        let ours = lines(&["cards = [", "  A,", "  b,", "  c,", "  d,", "]"]);
        let theirs = lines(&["cards = [", "  a,", "  b,", "  c,", "  D,", "]"]);
        assert_eq!(
            merge("notes.txt", &base, &ours, &theirs).unwrap(),
            lines(&["cards = [", "  A,", "  b,", "  c,", "  D,", "]"])
        );
        let changed = lines(&["cards = [", "  a,", "  B,", "  c,", "  d,", "]"]);
        for (o, t) in [(&base, &changed), (&changed, &base), (&changed, &changed)] {
            assert_eq!(merge("notes.txt", &base, o, t).unwrap(), changed);
        }
        let b1 = lines(&["cards = [", "  a,", "  B1,", "  c,", "  d,", "]"]);
        let b2 = lines(&["cards = [", "  a,", "  B2,", "  c,", "  d,", "]"]);
        assert_eq!(
            merge("notes.txt", &base, &b1, &b2),
            Err(FileConflict::WholeFile {
                path: "notes.txt".into(),
                why: WholeFile::Lines {
                    hunks: vec![ConflictHunk {
                        base: vec!["  b,".into()],
                        ours: vec!["  B1,".into()],
                        theirs: vec!["  B2,".into()],
                    }],
                },
            })
        );
    }
}
