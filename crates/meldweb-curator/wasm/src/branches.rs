//! A deck's branches (ADR-0041): `decks/<stem>/<name>`, the same physical
//! deck with a different list. Each saves by itself like `main`, and only
//! `main` is the built deck. Here is everything git decides about them:
//! which there are and how far each is from `main`, making one at a commit
//! of the deck's line, deleting one, and merging one into another.
//!
//! Merging one into another is a deck merge ([`crate::deck_merge`]), named
//! by the branch merged: `lantern: merge budget: +1 Sol Ring, -1 Mind Stone`.

use chip_decklist::stem::DeckStem;
use facet::Facet;
use meldweb_git::{BranchName, CommitId, Error, Repo, Signature, Store};

use crate::deck_merge::{deck_path, DeckMerge};
pub use crate::deck_merge::{MergeDone, MergePlan};
use crate::merging::Settle;

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

/// The deck merge of its branch `from` into its branch `into`.
fn deck_merge<'a, S: Store>(
    repo: &Repo<S>,
    stem: &'a DeckStem,
    from: &str,
    into: &str,
) -> Out<DeckMerge<'a>> {
    if from == into {
        return Err(format!("{from} cannot be merged into itself"));
    }
    Ok(DeckMerge {
        stem,
        into: branch_of(stem, into)?,
        into_label: into.to_owned(),
        from: tip_of(repo, &branch_of(stem, from)?)?,
        their_stem: stem,
        label: from.to_owned(),
    })
}

/// What merging the deck's branch `from` into its branch `into` would do
/// ([`DeckMerge::plan`]).
pub fn plan<S: Store>(
    repo: &mut Repo<S>,
    stem: &str,
    from: &str,
    into: &str,
    settle: Option<&Settle>,
) -> Out<MergePlan> {
    let stem = stem_of(stem)?;
    deck_merge(repo, &stem, from, into)?.plan(repo, settle)
}

/// Merges the deck's branch `from` into its branch `into` as [`plan`] says,
/// while the two are still at `into_tip` and `from_tip` ([`DeckMerge::merge`]).
#[allow(clippy::too_many_arguments)]
pub fn merge<S: Store>(
    repo: &mut Repo<S>,
    stem: &str,
    from: &str,
    into: &str,
    into_tip: &str,
    from_tip: &str,
    settle: Option<&Settle>,
    who: &Signature,
) -> Out<MergeDone> {
    let stem = stem_of(stem)?;
    deck_merge(repo, &stem, from, into)?.merge(repo, into_tip, from_tip, settle, who)
}
