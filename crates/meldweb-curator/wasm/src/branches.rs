//! A deck's branches (ADR-0041): `decks/<stem>/<name>`, the same physical
//! deck with a different list. Each saves by itself like `main`, and only
//! `main` is the built deck. Here is everything git decides about them:
//! which there are and how far each is from `main`, making one at a commit
//! of the deck's line, deleting one, and merging one into another.
//!
//! A merge is three-way from the merge base, each file both changed merged
//! as [`crate::merging`] merges its kind, the deck's by card, as a refused
//! save and a sync merge it. It is a
//! fast-forward when the branch merged into has not moved since the other
//! left it, and otherwise a merge commit with two parents whose message is
//! the semantic diff of the deck, as every save's is (ADR-0021):
//! `lantern: merge budget: +1 Sol Ring, -1 Mind Stone`. What does not merge,
//! or merges into something that is not a deck, stops and says where, until
//! the user picks a side for each card both changed.

use chip_decklist::changelog;
use chip_decklist::stem::DeckStem;
use facet::Facet;
use meldweb_git::{BranchName, CommitId, Error, Planned, Repo, RepoPath, Signature, Store, Tip};

use crate::merging::{self, FileMerge, MergeClash, Settle};

type Out<T> = Result<T, String>;

/// The branch every deck has, and the only one that is the built deck.
pub const MAIN: &str = "main";

/// One of a deck's branches, as the picker lists it.
#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct BranchFacts {
    /// Its own name: `budget` for `decks/lantern/budget`.
    pub name: String,
    /// Its tip.
    pub commit: String,
    /// When its tip was committed, in seconds since the Unix epoch.
    pub committed_at: i64,
    /// Commits on it that the deck's `main` lacks: what merging it brings.
    /// Deleting a branch with any loses them.
    pub unmerged: u32,
    /// Commits on `main` that it lacks: what updating it from `main` brings.
    pub behind: u32,
}

/// A deck's branches, `main` first and then the newest first.
#[derive(Debug, Facet)]
pub struct DeckBranches {
    pub branches: Vec<BranchFacts>,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum BranchMade {
    Made {
        branch: BranchFacts,
    },
    /// A branch by that name exists. Nothing changed.
    Conflict {
        message: String,
    },
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum BranchDeleted {
    Deleted,
    /// The branch moved since it was read, or is gone. Nothing changed.
    Conflict {
        message: String,
    },
}

/// One stretch of a file that both branches changed, differently.
#[derive(Debug, Facet)]
pub struct ConflictHunk {
    pub path: String,
    /// The lines as the merge base had them.
    pub base: Vec<String>,
    /// As the branch merged into has them.
    pub ours: Vec<String>,
    /// As the branch merged from has them.
    pub theirs: Vec<String>,
}

/// A file whose merge makes something Progress Engine does not read.
#[derive(Debug, Facet)]
pub struct MergeRefusal {
    pub path: String,
    pub reason: String,
}

/// What merging one of a deck's branches into another would do. `into` and
/// `from` are the two tips it was worked out from, which the merge itself
/// is guarded by; `before` and `after` are the deck's file on the branch
/// merged into, now and once merged, absent where the branch holds none.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum MergePlan {
    /// The branch merged into already holds every commit of the other.
    UpToDate,
    /// The branch merged into has not moved since the other left it, so it
    /// moves to the other's tip and no commit is made.
    FastForward {
        into: String,
        from: String,
        #[facet(default, skip_serializing_if = Option::is_none)]
        before: Option<String>,
        #[facet(default, skip_serializing_if = Option::is_none)]
        after: Option<String>,
    },
    /// A merge commit with two parents, and its message.
    Merge {
        into: String,
        from: String,
        #[facet(default, skip_serializing_if = Option::is_none)]
        before: Option<String>,
        #[facet(default, skip_serializing_if = Option::is_none)]
        after: Option<String>,
        message: String,
    },
    /// Both changed the same attribute of a card or of the deck, or the
    /// same lines of another file, or the merge makes a file that does not
    /// read. A side picked for each clash settles it.
    Conflict {
        into: String,
        from: String,
        clashes: Vec<MergeClash>,
        hunks: Vec<ConflictHunk>,
        unreadable: Vec<MergeRefusal>,
    },
    /// The two share no history.
    Unrelated,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum MergeDone {
    /// The branch's new tip; `message` is the merge commit's, absent for a
    /// fast-forward.
    Merged {
        commit: String,
        #[facet(default, skip_serializing_if = Option::is_none)]
        message: Option<String>,
    },
    UpToDate,
    /// Either branch moved since the plan was read, or the merge does not
    /// go through as planned. Nothing changed.
    Stopped {
        message: String,
    },
}

pub(crate) fn said(e: Error) -> String {
    e.to_string()
}

fn stem_of(stem: &str) -> Out<DeckStem> {
    DeckStem::new(stem).map_err(|e| e.to_string())
}

/// `decks/<stem>/<name>`. A branch's own name is one component held to a
/// stem's rules, so it never nests under or over another of the deck's refs.
fn branch_of(stem: &DeckStem, name: &str) -> Out<BranchName> {
    DeckStem::new(name).map_err(|_| {
        format!(
            "{name:?} is not a branch name: one name of ASCII letters, digits, '.', '_' and '-', like \"budget\""
        )
    })?;
    BranchName::new(stem.branch(name)).map_err(|e| e.to_string())
}

fn tip_of<S: Store>(repo: &Repo<S>, branch: &BranchName) -> Out<CommitId> {
    repo.branch_tip(branch)
        .map_err(said)?
        .ok_or_else(|| format!("there is no branch {branch}"))
}

pub(crate) fn deck_path(stem: &DeckStem) -> RepoPath {
    RepoPath::new(stem.deck_file()).expect("a stem makes a file name")
}

pub(crate) fn text(bytes: Option<Vec<u8>>) -> Option<String> {
    bytes.map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn facts<S: Store>(repo: &Repo<S>, name: &str, tip: CommitId, main: CommitId) -> Out<BranchFacts> {
    Ok(BranchFacts {
        name: name.to_owned(),
        commit: tip.to_hex(),
        committed_at: repo
            .read_commit(&tip)
            .map_err(said)?
            .committer
            .when
            .seconds(),
        unmerged: count(repo.count_only_in(&tip, &main).map_err(said)?),
        behind: count(repo.count_only_in(&main, &tip).map_err(said)?),
    })
}

/// The deck's branches, `main` first and then the newest first.
pub fn branches<S: Store>(repo: &Repo<S>, stem: &str) -> Out<DeckBranches> {
    let stem = stem_of(stem)?;
    let main = tip_of(repo, &branch_of(&stem, MAIN)?)?;
    let prefix = stem.branch("");
    let mut out = Vec::new();
    for (branch, tip) in repo.branches().map_err(said)? {
        let Some(name) = branch.as_str().strip_prefix(&prefix) else {
            continue;
        };
        if name.contains('/') {
            continue;
        }
        out.push(facts(repo, name, tip, main)?);
    }
    out.sort_by(|a, b| {
        (b.name == MAIN)
            .cmp(&(a.name == MAIN))
            .then(b.committed_at.cmp(&a.committed_at))
            .then(a.name.cmp(&b.name))
    });
    Ok(DeckBranches { branches: out })
}

/// Makes the branch `name` of the deck at `at`, a commit that holds the
/// deck's file: the tip of the branch it is made from, a snapshot's or a
/// revision's. No commit is made; the branch's history is `at`'s.
pub fn create<S: Store>(repo: &mut Repo<S>, stem: &str, name: &str, at: &str) -> Out<BranchMade> {
    let stem = stem_of(stem)?;
    let branch = branch_of(&stem, name)?;
    let at = CommitId::from_hex(at).map_err(|e| e.to_string())?;
    if repo.entry(&at, &deck_path(&stem)).map_err(said)?.is_none() {
        return Err(format!("{at} holds no {}", stem.deck_file()));
    }
    match repo.create_branch(&branch, &at) {
        Ok(()) => {}
        Err(Error::Conflict { .. } | Error::RefClash { .. }) => {
            return Ok(BranchMade::Conflict {
                message: format!("{} already has a branch {name}", stem),
            })
        }
        Err(e) => return Err(said(e)),
    }
    let main = tip_of(repo, &branch_of(&stem, MAIN)?)?;
    Ok(BranchMade::Made {
        branch: facts(repo, name, at, main)?,
    })
}

/// Deletes the branch `name` while it is still at `commit`. `main` is the
/// deck, and is deleted only with it.
pub fn delete<S: Store>(
    repo: &mut Repo<S>,
    stem: &str,
    name: &str,
    commit: &str,
) -> Out<BranchDeleted> {
    let stem = stem_of(stem)?;
    if name == MAIN {
        return Err(format!(
            "{stem}'s main is the deck itself; delete the deck instead"
        ));
    }
    let branch = branch_of(&stem, name)?;
    let expected = CommitId::from_hex(commit).map_err(|e| e.to_string())?;
    match repo.delete_branch(&branch, &expected) {
        Ok(()) => Ok(BranchDeleted::Deleted),
        Err(Error::Conflict { .. }) => Ok(BranchDeleted::Conflict {
            message: format!("{branch} changed since it was read"),
        }),
        Err(e) => Err(said(e)),
    }
}

/// The merge's tips, its plan, and what stopped it.
pub(crate) struct Worked {
    pub(crate) into: CommitId,
    pub(crate) from: CommitId,
    pub(crate) planned: Planned,
    pub(crate) clashes: Vec<MergeClash>,
    pub(crate) hunks: Vec<ConflictHunk>,
    pub(crate) unreadable: Vec<MergeRefusal>,
}

fn work<S: Store>(
    repo: &mut Repo<S>,
    stem: &DeckStem,
    from: &str,
    into: &str,
    settle: &Settle,
) -> Out<Worked> {
    if from == into {
        return Err(format!("{from} cannot be merged into itself"));
    }
    let into_tip = tip_of(repo, &branch_of(stem, into)?)?;
    let from_tip = tip_of(repo, &branch_of(stem, from)?)?;
    work_tips(repo, into_tip, from_tip, settle, |repo, merge_file| {
        repo.plan_merge(&into_tip, &from_tip, settle.prefer, merge_file)
    })
}

/// The deck's merge, each file both changed merged as its kind merges
/// ([`merging::merge_file`]), around `planning`, which plans the merge of
/// `from` into `into` with it.
pub(crate) fn work_tips<S: Store>(
    repo: &mut Repo<S>,
    into: CommitId,
    from: CommitId,
    settle: &Settle,
    planning: impl FnOnce(&mut Repo<S>, &mut meldweb_git::MergeFile<'_>) -> meldweb_git::Result<Planned>,
) -> Out<Worked> {
    let mut clashes = Vec::new();
    let mut hunks = Vec::new();
    let mut unreadable = Vec::new();
    let mut merge_file = |path: &str, base: &[u8], ours: &[u8], theirs: &[u8]| {
        merging::merge_bytes(
            path,
            base,
            ours,
            theirs,
            settle,
            &mut |stopped| match stopped {
                FileMerge::Clashed(c) => clashes.extend(c),
                FileMerge::Hunks(h) => hunks.extend(h.into_iter().map(|h| ConflictHunk {
                    path: path.to_owned(),
                    base: h.base,
                    ours: h.ours,
                    theirs: h.theirs,
                })),
                FileMerge::Unreadable(reason) => unreadable.push(MergeRefusal {
                    path: path.to_owned(),
                    reason,
                }),
                FileMerge::Merged(_) => {}
            },
        )
    };
    let planned = planning(repo, &mut merge_file).map_err(said)?;
    Ok(Worked {
        into,
        from,
        planned,
        clashes,
        hunks,
        unreadable,
    })
}

/// The merge commit's message: the deck's changelog from `before` to
/// `after`, with what was merged, `lantern: merge budget: +1 Sol Ring`.
pub(crate) fn message(
    stem: &DeckStem,
    from: &str,
    before: Option<&str>,
    after: Option<&str>,
) -> String {
    let path = stem.deck_file();
    let head = format!("{}: merge {from}", stem.as_str().to_lowercase());
    let said = match (before, after) {
        (Some(b), Some(a)) if b != a => changelog::commit_message_for_text(b, a, &path).ok(),
        _ => None,
    };
    let Some(said) = said else {
        return head;
    };
    // The changelog starts with the same stem every save's does.
    let changes = said.split_once(": ").map_or(said.as_str(), |(_, c)| c);
    if changes == "reformat" {
        head
    } else {
        format!("{head}: {changes}")
    }
}

/// What merging the deck's branch `from` into its branch `into` would do,
/// with `settle` settling what both changed once the user has picked a
/// side for it. It writes the merged objects and moves no ref.
pub fn plan<S: Store>(
    repo: &mut Repo<S>,
    stem: &str,
    from: &str,
    into: &str,
    settle: impl Into<Settle>,
) -> Out<MergePlan> {
    let stem = stem_of(stem)?;
    let w = work(repo, &stem, from, into, &settle.into())?;
    let deck = deck_path(&stem);
    let before = text(repo.read_file(&w.into, &deck).map_err(said)?);
    let (into_hex, from_hex) = (w.into.to_hex(), w.from.to_hex());
    Ok(match w.planned {
        Planned::UpToDate => MergePlan::UpToDate,
        Planned::Unrelated => MergePlan::Unrelated,
        Planned::FastForward => MergePlan::FastForward {
            into: into_hex,
            from: from_hex,
            before,
            after: text(repo.read_file(&w.from, &deck).map_err(said)?),
        },
        Planned::Merge { tree, .. } => {
            let after = text(repo.read_file_in(tree, &deck).map_err(said)?);
            MergePlan::Merge {
                into: into_hex,
                from: from_hex,
                message: message(&stem, from, before.as_deref(), after.as_deref()),
                before,
                after,
            }
        }
        Planned::Conflicts(_) => MergePlan::Conflict {
            into: into_hex,
            from: from_hex,
            clashes: w.clashes,
            hunks: w.hunks,
            unreadable: w.unreadable,
        },
    })
}

/// Merges the deck's branch `from` into its branch `into` as [`plan`] says,
/// while the two are still at `into_tip` and `from_tip`: a fast-forward, or
/// a merge commit by `who` with `into`'s tip then `from`'s as parents.
#[allow(clippy::too_many_arguments)]
pub fn merge<S: Store>(
    repo: &mut Repo<S>,
    stem: &str,
    from: &str,
    into: &str,
    into_tip: &str,
    from_tip: &str,
    settle: impl Into<Settle>,
    who: &Signature,
) -> Out<MergeDone> {
    let stem = stem_of(stem)?;
    let w = work(repo, &stem, from, into, &settle.into())?;
    if w.into.to_hex() != into_tip || w.from.to_hex() != from_tip {
        return Ok(MergeDone::Stopped {
            message: format!(
                "{into} or {from} changed since the merge was shown; look at it again"
            ),
        });
    }
    let branch = branch_of(&stem, into)?;
    let deck = deck_path(&stem);
    let moved = |e: Error| match e {
        Error::Conflict { .. } => Ok(MergeDone::Stopped {
            message: format!("{branch} moved while it was being merged"),
        }),
        e => Err(said(e)),
    };
    match w.planned {
        Planned::UpToDate => Ok(MergeDone::UpToDate),
        Planned::Unrelated => Ok(MergeDone::Stopped {
            message: format!("{from} and {into} share no history"),
        }),
        Planned::Conflicts(paths) => Ok(MergeDone::Stopped {
            message: format!("{} do not merge without a side picked", paths.join(", ")),
        }),
        Planned::FastForward => match repo.update_branch(&branch, Tip::At(w.into), &w.from) {
            Ok(()) => Ok(MergeDone::Merged {
                commit: w.from.to_hex(),
                message: None,
            }),
            Err(e) => moved(e),
        },
        Planned::Merge { tree, .. } => {
            let before = text(repo.read_file(&w.into, &deck).map_err(said)?);
            let after = text(repo.read_file_in(tree, &deck).map_err(said)?);
            let said_ = message(&stem, from, before.as_deref(), after.as_deref());
            let commit = repo
                .write_commit_of(
                    tree,
                    &[w.into, w.from],
                    who,
                    who,
                    &format!("{}\n", said_.trim_end()),
                )
                .map_err(said)?;
            match repo.update_branch(&branch, Tip::At(w.into), &commit) {
                Ok(()) => Ok(MergeDone::Merged {
                    commit: commit.to_hex(),
                    message: Some(said_),
                }),
                Err(e) => moved(e),
            }
        }
    }
}
