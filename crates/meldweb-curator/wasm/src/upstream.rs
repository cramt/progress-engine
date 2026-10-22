//! A deck forked from another player's repo, and taking their changes
//! (ADR-0041).
//!
//! Forking fetches their deck's line alone, `decks/<stem>/<branch>`, over
//! git: only its tip is wanted, so only its ancestry comes down, and it is
//! tracked under `refs/remotes/<host>/<path>/decks/<stem>/<branch>`, which a
//! push never sends. The local deck's `main` then starts at their tip: the
//! same commits, the same ids, their authors. Kept under their stem, it *is*
//! their tip; under another one, it is one commit on top that names the
//! files by the new stem, since a deck's tree holds `<stem>.deck.toml`.
//! `meldweb.toml` records what the deck follows (`[upstream.<stem>]`),
//! because ancestry holds their commits but not where they came from.
//!
//! Their repo still at version 1 has no deck lines. Then their `main` comes
//! down, and [`layout::rewrite_v1`] makes the line their own migration will
//! make, commit for commit, so the fork holds the ids they get when they
//! migrate.
//!
//! Taking their changes is a deck branch's merge ([`crate::branches`]):
//! nothing to take when the deck holds their tip, a fast-forward when it has
//! not changed since it last took them, otherwise a merge commit with their
//! tip as the second parent, `lantern: merge alice's lantern: +1 Sol Ring`,
//! and a clash stopping until a side is picked. A deck kept under another
//! stem merges their files as its own: their side is read under its stem.

use chip_decklist::deck::Deck;
use chip_decklist::stem::DeckStem;
use facet::Facet;
use meldweb_git::{
    advertised_branches, BranchName, CommitId, EntryKind, Error, FetchRequest, Other, Planned,
    Repo, RepoPath, Signature, Store, Tip, TreeMerge, WriteCommit,
};

use crate::branches::{self, deck_path, said, text, MergeDone, MergePlan};
use crate::layout::{self, version_at};
use crate::merging::Settle;
use crate::preference::{self, Upstream};

type Out<T> = Result<T, String>;

const SETTINGS: &str = "meldweb.toml";

fn branch(name: &str) -> BranchName {
    BranchName::new(name).expect("a stem makes a branch name")
}

fn main_branch() -> BranchName {
    branch("main")
}

fn other(up: &Upstream) -> Out<Other> {
    Other::new(up.url.clone()).map_err(said)
}

/// Their deck's line in their repo: `decks/<deck>/<branch>`.
fn their_line(up: &Upstream) -> BranchName {
    branch(&up.deck.branch(up.branch.as_str()))
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Which of their branches a fetch asks for: their deck's line, or, from a
/// repo with none, their `main`, which may be at version 1.
fn asked(up: &Upstream, advertised: &[u8]) -> Out<BranchName> {
    let branches = advertised_branches(advertised).map_err(said)?;
    let line = their_line(up);
    if branches.contains_key(&line) {
        return Ok(line);
    }
    if up.branch.as_str() == "main" && branches.contains_key(&main_branch()) {
        return Ok(main_branch());
    }
    Err(no_deck(up))
}

fn no_deck(up: &Upstream) -> String {
    format!(
        "{}'s repo has no deck {} ({} is not a branch there)",
        up.owner(),
        up.deck,
        their_line(up)
    )
}

/// The decks another player's repo offers to fork, as its `info/refs`
/// answer names them.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum TheirDecks {
    /// A repo at version 2: each deck's stem, by its `decks/<stem>/main`.
    Lines { stems: Vec<String> },
    /// A repo with a `main` and no deck lines, which may still be at
    /// version 1, its decks in `main`'s `decks/`: a deck is forked from it
    /// by its stem, and the fetch says whether it is there.
    MainOnly,
    /// Nothing to fork: an empty repo, or not a Magic repo.
    None,
}

pub fn their_decks(advertised: &[u8]) -> Out<TheirDecks> {
    let branches = advertised_branches(advertised).map_err(said)?;
    let stems: Vec<String> = branches
        .keys()
        .filter_map(|b| {
            let stem = b.as_str().strip_prefix("decks/")?.strip_suffix("/main")?;
            DeckStem::new(stem).ok().map(|s| s.to_string())
        })
        .collect();
    Ok(if !stems.is_empty() {
        TheirDecks::Lines { stems }
    } else if branches.contains_key(&main_branch()) {
        TheirDecks::MainOnly
    } else {
        TheirDecks::None
    })
}

/// The body of `POST git-upload-pack` to their repo for their deck, or
/// `None` when this repo holds all of it already.
pub fn fetch_request<S: Store>(
    repo: &Repo<S>,
    up: &Upstream,
    advertised: &[u8],
) -> Out<Option<Vec<u8>>> {
    let want = asked(up, advertised)?;
    match repo.fetch_branch_request(advertised, &want).map_err(said)? {
        FetchRequest::HaveAll => Ok(None),
        FetchRequest::Want(body) => Ok(Some(body)),
    }
}

/// What fetching their deck came to.
#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct Fetched {
    /// Their deck's tip, as tracked now.
    pub commit: String,
    /// Their deck's name there, when its file reads as a deck.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub name: Option<String>,
    /// Whether their repo is still at version 1, so the line tracked is the
    /// one their migration will make.
    pub migrated: bool,
}

/// Takes in their answer to [`fetch_request`] and tracks their deck's line;
/// from a version-1 repo, the line its migration makes.
pub fn fetch_finish<S: Store>(
    repo: &mut Repo<S>,
    up: &Upstream,
    advertised: &[u8],
    response: Option<&[u8]>,
) -> Out<Fetched> {
    let want = asked(up, advertised)?;
    let them = other(up)?;
    let tip = repo
        .fetch_branch_finish(&them, advertised, &want, response)
        .map_err(said)?;
    let line = their_line(up);
    let (tip, migrated) = if want == line {
        (tip, false)
    } else {
        if version_at(repo, &tip)? != Some(1) {
            return Err(no_deck(up));
        }
        let rewritten = layout::rewrite_v1(repo, &tip)?;
        let Some(deck_tip) = rewritten.decks.get(&up.deck).copied() else {
            return Err(format!(
                "{}'s repo has no deck {}: decks/{}.deck.toml is not on its main",
                up.owner(),
                up.deck,
                up.deck
            ));
        };
        repo.track_other(&them, &line, &deck_tip).map_err(said)?;
        (deck_tip, true)
    };
    Ok(Fetched {
        commit: tip.to_hex(),
        name: deck_name(repo, &tip, &up.deck)?,
        migrated,
    })
}

fn deck_text<S: Store>(repo: &Repo<S>, at: &CommitId, stem: &DeckStem) -> Out<Option<String>> {
    Ok(text(repo.read_file(at, &deck_path(stem)).map_err(said)?))
}

fn deck_name<S: Store>(repo: &Repo<S>, at: &CommitId, stem: &DeckStem) -> Out<Option<String>> {
    Ok(deck_text(repo, at, stem)?
        .and_then(|t| Deck::parse(&t).ok())
        .and_then(|d| d.name))
}

fn their_tip<S: Store>(repo: &Repo<S>, up: &Upstream) -> Out<CommitId> {
    repo.other_tip(&other(up)?, &their_line(up))
        .map_err(said)?
        .ok_or_else(|| format!("{}'s {} has not been fetched", up.owner(), up.deck))
}

/// `stem`'s files at `at` named as `to`'s, every other file as it is:
/// `lantern.deck.toml` as `mine.deck.toml`, and its criteria with it.
fn renamed_tree<S: Store>(
    repo: &mut Repo<S>,
    at: &CommitId,
    from: &DeckStem,
    to: &DeckStem,
) -> Out<meldweb_git::TreeId> {
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

/// What forking came to.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Forked {
    /// The new deck, and its `main`'s tip.
    Forked { stem: String, commit: String },
    /// A deck has that stem already. Nothing changed.
    Taken { message: String },
}

/// Makes the deck `stem` a fork of their deck as last fetched, and records
/// that it follows it in `meldweb.toml`, one commit on `main` by `who`.
pub fn fork<S: Store>(
    repo: &mut Repo<S>,
    up: &Upstream,
    stem: &DeckStem,
    who: &Signature,
) -> Out<Forked> {
    let theirs = their_tip(repo, up)?;
    if repo
        .entry(&theirs, &deck_path(&up.deck))
        .map_err(said)?
        .is_none()
    {
        return Err(format!(
            "{}'s {} holds no {}",
            up.owner(),
            up.deck,
            up.deck.deck_file()
        ));
    }
    let line = layout::main_line(stem);
    let taken = || Forked::Taken {
        message: format!("There is a deck {stem} already; fork it under another name."),
    };
    if repo
        .branches()
        .map_err(said)?
        .iter()
        .any(|(b, _)| b.as_str().starts_with(&format!("decks/{stem}/")))
    {
        return Ok(taken());
    }
    let tip = if *stem == up.deck {
        theirs
    } else {
        let tree = renamed_tree(repo, &theirs, &up.deck, stem)?;
        repo.write_commit_of(
            tree,
            &[theirs],
            who,
            who,
            &format!(
                "{}: fork of {}'s {}\n",
                stem.as_str().to_lowercase(),
                up.owner(),
                up.deck
            ),
        )
        .map_err(said)?
    };
    record(repo, stem, Some(up.clone()), who)?;
    match repo.create_branch(&line, &tip) {
        Ok(()) => {}
        Err(Error::Conflict { .. } | Error::RefClash { .. }) => return Ok(taken()),
        Err(e) => return Err(said(e)),
    }
    Ok(Forked::Forked {
        stem: stem.to_string(),
        commit: tip.to_hex(),
    })
}

/// Commits `meldweb.toml` on `main` with `stem` following `up`, or nothing.
fn record<S: Store>(
    repo: &mut Repo<S>,
    stem: &DeckStem,
    up: Option<Upstream>,
    who: &Signature,
) -> Out<()> {
    let main = main_branch();
    let tip = repo
        .branch_tip(&main)
        .map_err(said)?
        .ok_or("the repo has no main to record the fork on")?;
    let path = RepoPath::new(SETTINGS).expect("a path");
    let before = text(repo.read_file(&tip, &path).map_err(said)?);
    let after = preference::set_upstream(before.as_deref(), stem, up)?;
    if before.as_deref() == Some(after.as_str()) {
        return Ok(());
    }
    let message = preference::commit_message(before.as_deref().unwrap_or(""), &after);
    repo.commit(
        &main,
        Tip::At(tip),
        &WriteCommit {
            changes: vec![meldweb_git::Change::Put {
                path,
                bytes: after.into_bytes(),
            }],
            message,
            author: who.clone(),
            committer: who.clone(),
        },
    )
    .map_err(said)?;
    Ok(())
}

/// Stops `stem` following anything. Its history stays as it is.
pub fn unfollow<S: Store>(repo: &mut Repo<S>, stem: &DeckStem, who: &Signature) -> Out<()> {
    record(repo, stem, None, who)
}

/// The deck `stem` follows, as `main`'s `meldweb.toml` says.
pub fn upstream_of<S: Store>(repo: &Repo<S>, stem: &DeckStem) -> Out<Option<Upstream>> {
    let Some(tip) = repo.branch_tip(&main_branch()).map_err(said)? else {
        return Ok(None);
    };
    let settings = text(
        repo.read_file(&tip, &RepoPath::new(SETTINGS).expect("a path"))
            .map_err(said)?,
    );
    Ok(preference::read(settings.as_deref())?
        .upstreams
        .remove(stem))
}

fn followed<S: Store>(repo: &Repo<S>, stem: &DeckStem) -> Out<Upstream> {
    upstream_of(repo, stem)?.ok_or_else(|| format!("{stem} follows no other player's deck"))
}

fn mine<S: Store>(repo: &Repo<S>, stem: &DeckStem) -> Out<CommitId> {
    repo.branch_tip(&layout::main_line(stem))
        .map_err(said)?
        .ok_or_else(|| format!("there is no deck {stem}"))
}

/// A forked deck against the deck it follows, as last fetched.
#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct Following {
    pub upstream: crate::UpstreamFacts,
    /// Their deck, once fetched.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub theirs: Option<TheirSide>,
}

#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct TheirSide {
    pub commit: String,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub name: Option<String>,
    /// Their deck's file at their tip.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub text: Option<String>,
    /// Commits of theirs the deck's `main` lacks: what taking them brings.
    pub new: u32,
    /// Commits on the deck's `main` that theirs lacks.
    pub ahead: u32,
    /// The deck as the two last agreed, the merge base: what "since I
    /// forked" and "what they changed" are both measured from.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub base: Option<String>,
    /// The deck's file at the merge base, under whichever stem it had there.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub base_text: Option<String>,
    /// The deck's own file on its `main`.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub mine_text: Option<String>,
}

/// The deck's file at `at` under `stem`, or else under `other`.
fn either_text<S: Store>(
    repo: &Repo<S>,
    at: &CommitId,
    stem: &DeckStem,
    other: &DeckStem,
) -> Out<Option<String>> {
    match deck_text(repo, at, stem)? {
        Some(t) => Ok(Some(t)),
        None => deck_text(repo, at, other),
    }
}

/// Where `stem` stands against the deck it follows, as last fetched; `None`
/// when it follows nothing.
pub fn following<S: Store>(repo: &Repo<S>, stem: &DeckStem) -> Out<Option<Following>> {
    let Some(up) = upstream_of(repo, stem)? else {
        return Ok(None);
    };
    let facts = crate::UpstreamFacts::of(stem, &up);
    let Some(theirs) = repo
        .other_tip(&other(&up)?, &their_line(&up))
        .map_err(said)?
    else {
        return Ok(Some(Following {
            upstream: facts,
            theirs: None,
        }));
    };
    let mine = mine(repo, stem)?;
    let base = repo
        .merge_base(&mine, &theirs)
        .map_err(said)?
        .first()
        .copied();
    let base_text = match &base {
        Some(b) => either_text(repo, b, &up.deck, stem)?,
        None => None,
    };
    Ok(Some(Following {
        upstream: facts,
        theirs: Some(TheirSide {
            commit: theirs.to_hex(),
            name: deck_name(repo, &theirs, &up.deck)?,
            text: deck_text(repo, &theirs, &up.deck)?,
            new: count(repo.count_only_in(&theirs, &mine).map_err(said)?),
            ahead: count(repo.count_only_in(&mine, &theirs).map_err(said)?),
            base: base.map(|b| b.to_hex()),
            base_text,
            mine_text: deck_text(repo, &mine, stem)?,
        }),
    }))
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

fn work<S: Store>(
    repo: &mut Repo<S>,
    stem: &DeckStem,
    up: &Upstream,
    settle: &Settle,
) -> Out<branches::Worked> {
    let into = mine(repo, stem)?;
    let from = their_tip(repo, up)?;
    let same = *stem == up.deck;
    branches::work_tips(repo, into, from, settle, |repo, merge_file| {
        if repo.is_ancestor(&from, &into)? {
            return Ok(Planned::UpToDate);
        }
        if same && repo.is_ancestor(&into, &from)? {
            return Ok(Planned::FastForward);
        }
        let Some(base) = repo.merge_base(&into, &from)?.first().copied() else {
            return Ok(Planned::Unrelated);
        };
        let (base, theirs) = if same {
            (base, from)
        } else {
            let renamed = |repo: &mut Repo<S>, at| {
                as_ours(repo, at, &up.deck, stem).map_err(meldweb_git::Error::Remote)
            };
            (renamed(repo, base)?, renamed(repo, from)?)
        };
        Ok(
            match repo.merge_trees(Some(&base), &into, &theirs, settle.prefer, merge_file)? {
                TreeMerge::Conflicts(paths) => Planned::Conflicts(paths),
                TreeMerge::Clean { tree, taken } => Planned::Merge { tree, taken },
            },
        )
    })
}

/// `alice's lantern`, as a merge of theirs is named.
fn label(up: &Upstream) -> String {
    let deck = up.deck.as_str();
    if up.branch.as_str() == "main" {
        format!("{}'s {deck}", up.owner())
    } else {
        format!("{}'s {deck} {}", up.owner(), up.branch)
    }
}

/// What taking their changes into `stem`'s `main` would do; nothing moves.
pub fn plan<S: Store>(
    repo: &mut Repo<S>,
    stem: &DeckStem,
    settle: impl Into<Settle>,
) -> Out<MergePlan> {
    let up = followed(repo, stem)?;
    let w = work(repo, stem, &up, &settle.into())?;
    let deck = deck_path(stem);
    let before = text(repo.read_file(&w.into, &deck).map_err(said)?);
    let (into, from) = (w.into.to_hex(), w.from.to_hex());
    Ok(match w.planned {
        Planned::UpToDate => MergePlan::UpToDate,
        Planned::Unrelated => MergePlan::Unrelated,
        Planned::FastForward => MergePlan::FastForward {
            into,
            from,
            before,
            after: text(repo.read_file(&w.from, &deck).map_err(said)?),
        },
        Planned::Merge { tree, .. } => {
            let after = text(repo.read_file_in(tree, &deck).map_err(said)?);
            MergePlan::Merge {
                into,
                from,
                message: branches::message(stem, &label(&up), before.as_deref(), after.as_deref()),
                before,
                after,
            }
        }
        Planned::Conflicts(_) => MergePlan::Conflict {
            into,
            from,
            clashes: w.clashes,
            hunks: w.hunks,
            unreadable: w.unreadable,
        },
    })
}

/// Takes their changes into `stem`'s `main` as [`plan`] said, while the
/// deck and theirs are still at `into_tip` and `from_tip`.
pub fn take<S: Store>(
    repo: &mut Repo<S>,
    stem: &DeckStem,
    into_tip: &str,
    from_tip: &str,
    settle: impl Into<Settle>,
    who: &Signature,
) -> Out<MergeDone> {
    let up = followed(repo, stem)?;
    let w = work(repo, stem, &up, &settle.into())?;
    if w.into.to_hex() != into_tip || w.from.to_hex() != from_tip {
        return Ok(MergeDone::Stopped {
            message: format!(
                "{stem} or {} changed since the merge was shown; look at it again",
                label(&up)
            ),
        });
    }
    let line = layout::main_line(stem);
    let deck = deck_path(stem);
    let moved = |e: Error| match e {
        Error::Conflict { .. } => Ok(MergeDone::Stopped {
            message: format!("{line} moved while it was being merged"),
        }),
        e => Err(said(e)),
    };
    match w.planned {
        Planned::UpToDate => Ok(MergeDone::UpToDate),
        Planned::Unrelated => Ok(MergeDone::Stopped {
            message: format!("{stem} and {} share no history", label(&up)),
        }),
        Planned::Conflicts(paths) => Ok(MergeDone::Stopped {
            message: format!("{} do not merge without a side picked", paths.join(", ")),
        }),
        Planned::FastForward => match repo.update_branch(&line, Tip::At(w.into), &w.from) {
            Ok(()) => Ok(MergeDone::Merged {
                commit: w.from.to_hex(),
                message: None,
            }),
            Err(e) => moved(e),
        },
        Planned::Merge { tree, .. } => {
            let before = text(repo.read_file(&w.into, &deck).map_err(said)?);
            let after = text(repo.read_file_in(tree, &deck).map_err(said)?);
            let message = branches::message(stem, &label(&up), before.as_deref(), after.as_deref());
            let commit = repo
                .write_commit_of(
                    tree,
                    &[w.into, w.from],
                    who,
                    who,
                    &format!("{}\n", message.trim_end()),
                )
                .map_err(said)?;
            match repo.update_branch(&line, Tip::At(w.into), &commit) {
                Ok(()) => Ok(MergeDone::Merged {
                    commit: commit.to_hex(),
                    message: Some(message),
                }),
                Err(e) => moved(e),
            }
        }
    }
}
