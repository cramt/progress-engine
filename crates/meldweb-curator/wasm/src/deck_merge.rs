//! A deck merge (CONTEXT.md): another line of a deck's history merged into
//! one of the deck's branches. A branch merged into `main` or `main` into a
//! branch ([`crate::branches`]) and another player's deck taken into a
//! fork's `main` ([`crate::upstream`]) are each this, told what to merge and
//! what to call it.
//!
//! How the two tips stand is `meldweb-git`'s one decision
//! ([`Repo::standing`]), from its one merge base: nothing to merge, a
//! fast-forward, or a three-way merge with each file both changed merged as
//! [`merging::merge_bytes`] merges its kind, the deck's by card. A merge
//! commit has the branch's tip then theirs as parents and the deck's
//! changelog as its message, as every save's is (ADR-0021):
//! `lantern: merge budget: +1 Sol Ring, -1 Mind Stone`. A sync's merge of a
//! deck's line says the same ([`message`]).
//!
//! What does not merge stops as one [`Conflict`], guarded by the two tips;
//! a [`Settle`] made against other tips is refused here, in planning and in
//! merging alike, so the page only shows the plan and confirms it.
//!
//! Their deck's files can be under another stem than ours: a fork of
//! another player's deck kept under a stem of its own. Their side and the
//! merge base are then read with their files named as ours, and their tip
//! is never fast-forwarded to, since it holds no file of ours.

use chip_decklist::changelog;
use chip_decklist::stem::DeckStem;
use facet::Facet;
use meldweb_git::{
    BranchName, CommitId, EntryKind, Error, Repo, RepoPath, Signature, Standing, Store, Tip,
    TreeId, TreeMerge,
};

use crate::merging::{self, Conflict, Guard, Settle, Settling, Stops};

type Out<T> = Result<T, String>;

fn said(e: Error) -> String {
    e.to_string()
}

/// What a deck merge would do. `into` and `from` are the two tips it was
/// worked out from, which the merge itself is guarded by; `before` and
/// `after` are the deck's file on the branch merged into, now and once
/// merged, absent where the branch holds none.
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
    /// read. A [`Settle`] under its guard settles it.
    Conflict { conflict: Conflict },
    /// The two share no history.
    Unrelated,
    /// The settle given was made against another merge: a branch moved
    /// since it was shown. Nothing is settled.
    Stopped { message: String },
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
    /// Either side moved since the plan was read, or the merge does not go
    /// through as planned. Nothing changed.
    Stopped {
        message: String,
    },
}

/// One deck merge: their tip into the deck's branch `into`.
pub(crate) struct DeckMerge<'a> {
    /// The deck merged into.
    pub(crate) stem: &'a DeckStem,
    /// The branch of its line merged into.
    pub(crate) into: BranchName,
    /// How a refusal names the branch merged into: `main`, or the deck.
    pub(crate) into_label: String,
    /// Their tip.
    pub(crate) from: CommitId,
    /// The stem their deck's files are under: `stem`, unless theirs is
    /// another player's deck kept here under another one.
    pub(crate) their_stem: &'a DeckStem,
    /// How the merge names their side: `budget`, `alice's lantern`.
    pub(crate) label: String,
}

/// What merging the trees came to.
enum Planned {
    UpToDate,
    FastForward,
    Merge(TreeId),
    Conflicts(Vec<String>),
    Unrelated,
}

/// The merge worked out at the branch's tip `into`, under `guard`, with why
/// each file that did not merge stopped.
struct Worked {
    into: CommitId,
    guard: Guard,
    planned: Planned,
    stops: Stops,
}

impl DeckMerge<'_> {
    fn stale(&self) -> String {
        format!(
            "{} or {} changed since the merge was shown; look at it again",
            self.into_label, self.label
        )
    }

    fn deck(&self) -> RepoPath {
        deck_path(self.stem)
    }

    /// The merge from the branch's tip now, or `None` when `settle` was
    /// made against another merge than this one.
    fn work<S: Store>(&self, repo: &mut Repo<S>, settle: Option<&Settle>) -> Out<Option<Worked>> {
        let into = repo
            .branch_tip(&self.into)
            .map_err(said)?
            .ok_or_else(|| format!("there is no branch {}", self.into))?;
        let guard = Guard::tips(&self.into, into, self.from);
        let Ok(settling) = Settling::checked(settle, &guard) else {
            return Ok(None);
        };
        let same = self.their_stem == self.stem;
        let worked = |planned| Worked {
            into,
            guard: guard.clone(),
            planned,
            stops: Stops::default(),
        };
        let base = match repo.standing(&into, &self.from).map_err(said)? {
            Standing::UpToDate => return Ok(Some(worked(Planned::UpToDate))),
            Standing::Unrelated => return Ok(Some(worked(Planned::Unrelated))),
            Standing::FastForward if same => return Ok(Some(worked(Planned::FastForward))),
            // Their tip holds none of the deck's files by its stem, so the
            // deck merges their changes in from where it is.
            Standing::FastForward => into,
            Standing::Diverged { base } => base,
        };
        let (base, theirs) = if same {
            (base, self.from)
        } else {
            (
                as_ours(repo, base, self.their_stem, self.stem)?,
                as_ours(repo, self.from, self.their_stem, self.stem)?,
            )
        };
        let mut w = worked(Planned::UpToDate);
        let stops = &mut w.stops;
        let mut merge_file = |path: &str, base: &[u8], ours: &[u8], theirs: &[u8]| {
            merging::merge_bytes(path, base, ours, theirs, &settling, stops)
        };
        let prefer = settling.prefer.map(merging::git_side);
        w.planned = match repo
            .merge_trees(Some(&base), &into, &theirs, prefer, &mut merge_file)
            .map_err(said)?
        {
            TreeMerge::Conflicts(paths) => Planned::Conflicts(paths),
            TreeMerge::Clean { tree, .. } => Planned::Merge(tree),
        };
        Ok(Some(w))
    }

    /// The merge commit's message for `tree` merged onto `into`.
    fn message_for<S: Store>(&self, repo: &Repo<S>, into: &CommitId, tree: TreeId) -> Out<String> {
        let before = text(repo.read_file(into, &self.deck()).map_err(said)?);
        let after = text(repo.read_file_in(tree, &self.deck()).map_err(said)?);
        Ok(message(
            self.stem,
            &self.label,
            before.as_deref(),
            after.as_deref(),
        ))
    }

    /// What the merge would do, with `settle` settling what both changed
    /// once the user has picked a side for it, when it was made against
    /// this merge. It writes the merged objects and moves no ref.
    pub(crate) fn plan<S: Store>(
        &self,
        repo: &mut Repo<S>,
        settle: Option<&Settle>,
    ) -> Out<MergePlan> {
        let Some(mut w) = self.work(repo, settle)? else {
            return Ok(MergePlan::Stopped {
                message: self.stale(),
            });
        };
        let deck = self.deck();
        let before = text(repo.read_file(&w.into, &deck).map_err(said)?);
        let (into, from) = (w.into.to_hex(), self.from.to_hex());
        Ok(match w.planned {
            Planned::UpToDate => MergePlan::UpToDate,
            Planned::Unrelated => MergePlan::Unrelated,
            Planned::FastForward => MergePlan::FastForward {
                into,
                from,
                before,
                after: text(repo.read_file(&self.from, &deck).map_err(said)?),
            },
            Planned::Merge(tree) => MergePlan::Merge {
                into,
                from,
                message: self.message_for(repo, &w.into, tree)?,
                before,
                after: text(repo.read_file_in(tree, &deck).map_err(said)?),
            },
            Planned::Conflicts(paths) => MergePlan::Conflict {
                conflict: Conflict::of(&w.guard, paths, &mut w.stops),
            },
        })
    }

    /// Merges as [`DeckMerge::plan`] said, while the branch and theirs are
    /// still at `into_tip` and `from_tip` and `settle` was made against
    /// this merge: a fast-forward, or a merge commit by `who`. Anything
    /// else stops, and nothing moves.
    pub(crate) fn merge<S: Store>(
        &self,
        repo: &mut Repo<S>,
        into_tip: &str,
        from_tip: &str,
        settle: Option<&Settle>,
        who: &Signature,
    ) -> Out<MergeDone> {
        let stopped = |message: String| Ok(MergeDone::Stopped { message });
        let w = match self.work(repo, settle)? {
            Some(w) if w.into.to_hex() == into_tip && self.from.to_hex() == from_tip => w,
            _ => return stopped(self.stale()),
        };
        let moved = |e: Error| match e {
            Error::Conflict { .. } => {
                stopped(format!("{} moved while it was being merged", self.into))
            }
            e => Err(said(e)),
        };
        let (to, message) = match w.planned {
            Planned::UpToDate => return Ok(MergeDone::UpToDate),
            Planned::Unrelated => {
                return stopped(format!(
                    "{} and {} share no history",
                    self.into_label, self.label
                ))
            }
            Planned::Conflicts(paths) => {
                return stopped(format!(
                    "{} do not merge without a side picked",
                    paths.join(", ")
                ))
            }
            Planned::FastForward => (self.from, None),
            Planned::Merge(tree) => {
                let message = self.message_for(repo, &w.into, tree)?;
                let commit = repo
                    .write_commit_of(
                        tree,
                        &[w.into, self.from],
                        who,
                        who,
                        &format!("{}\n", message.trim_end()),
                    )
                    .map_err(said)?;
                (commit, Some(message))
            }
        };
        match repo.update_branch(&self.into, Tip::At(w.into), &to) {
            Ok(()) => Ok(MergeDone::Merged {
                commit: to.to_hex(),
                message,
            }),
            Err(e) => moved(e),
        }
    }
}

pub(crate) fn deck_path(stem: &DeckStem) -> RepoPath {
    RepoPath::new(stem.deck_file()).expect("a stem makes a file name")
}

pub(crate) fn text(bytes: Option<Vec<u8>>) -> Option<String> {
    bytes.map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// A deck merge's commit message: the deck's changelog from `before` to
/// `after`, with what was merged, `lantern: merge budget: +1 Sol Ring`; the
/// head alone when the deck's cards did not change.
pub(crate) fn message(
    stem: &DeckStem,
    label: &str,
    before: Option<&str>,
    after: Option<&str>,
) -> String {
    let path = stem.deck_file();
    let head = format!("{}: merge {label}", stem.as_str().to_lowercase());
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

/// `from`'s files at `at` named as `to`'s, every other file as it is:
/// `lantern.deck.toml` as `mine.deck.toml`, and its criteria with it.
pub(crate) fn renamed_tree<S: Store>(
    repo: &mut Repo<S>,
    at: &CommitId,
    from: &DeckStem,
    to: &DeckStem,
) -> Out<TreeId> {
    let entries = repo.list_dir(at, None).map_err(said)?.unwrap_or_default();
    let prefix = format!("{from}.");
    let mut files = Vec::new();
    for e in entries {
        let (EntryKind::File(blob) | EntryKind::Executable(blob)) = e.kind else {
            return Err(format!(
                "{} holds {}, which is not a file, and a deck's line holds only its files",
                at, e.name
            ));
        };
        let name = match e.name.strip_prefix(&prefix) {
            Some(rest) if rest == "deck.toml" || rest.ends_with("criteria.toml") => {
                format!("{to}.{rest}")
            }
            _ => e.name.clone(),
        };
        files.push((RepoPath::new(name).map_err(|e| e.to_string())?, blob));
    }
    repo.write_files(&files).map_err(said)
}

/// `at` with `from`'s files named as `stem`'s, as a commit nothing names,
/// when `at` holds no file of `stem`'s own: their side of a merge, read as
/// the deck kept here under another stem.
fn as_ours<S: Store>(
    repo: &mut Repo<S>,
    at: CommitId,
    from: &DeckStem,
    stem: &DeckStem,
) -> Out<CommitId> {
    if repo.entry(&at, &deck_path(stem)).map_err(said)?.is_some() {
        return Ok(at);
    }
    let tree = renamed_tree(repo, &at, from, stem)?;
    let info = repo.read_commit(&at).map_err(said)?;
    repo.write_commit_of(tree, &[], &info.author, &info.committer, "")
        .map_err(said)
}
