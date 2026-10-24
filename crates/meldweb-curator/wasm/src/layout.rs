//! The Magic repo's layout, version 2 (ADR-0041), and the migration to it
//! from version 1.
//!
//! In version 2, `main` holds what is the player's and not one deck's:
//! `VERSION`, `collection.toml`, `wanted.toml`, `meldweb.toml` and `cards/`.
//! Each deck is a line of its own, the branch `decks/<stem>/main`, whose tree
//! holds `<stem>.deck.toml` and, when it has any, its Gauntlet criteria
//! `<stem>.criteria.toml` and its named criteria sets
//! `<stem>.<name>.criteria.toml`. A snapshot is still the annotated tag
//! `decks/<stem>/<slug>`, now on a commit of the deck's line.
//!
//! A fork's line starts on the commit of the deck it was forked from: its
//! first commit's parent is that commit, so the fork's history is the
//! original's up to the fork, as real ancestors. Which deck a fork came from
//! is therefore read from the history, never written down
//! ([`DeckLines::parents`]).
//!
//! Version 1 kept every deck on `main` as `decks/<stem>.deck.toml`, a
//! variant naming its parent's path with `variant_of`. [`rewrite_v1`] replays
//! `main`'s first-parent history into each deck's own line and writes one
//! commit on `main` that takes `decks/` away. Every new commit is made from
//! what the old ones hold and nothing else (no clock, no author of this
//! browser's), so two browsers migrating the same `main` make byte-identical
//! commits, and a remote still on version 1 can be migrated here to be
//! integrated as if it had been migrated where it lives.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chip_decklist::collection;
use chip_decklist::edit;
use chip_decklist::stem::DeckStem;
use meldweb_git::{
    BlobId, BranchName, Change, CommitId, EntryKind, Error, Identity, Integrated, MergeBranch,
    Repo, RepoPath, Side, Signature, Standing, Store, TagKind, Timestamp, Tip, WriteCommit,
};

/// The repo version this layout is.
pub const VERSION: u32 = 2;

const DECKS_DIR: &str = "decks";
const DECK_SUFFIX: &str = ".deck.toml";
const CRITERIA_SUFFIX: &str = ".criteria.toml";

type Out<T> = Result<T, String>;

fn said(e: Error) -> String {
    e.to_string()
}

fn path(p: &str) -> RepoPath {
    RepoPath::new(p).expect("the layout's own paths are paths")
}

fn branch_name(name: &str) -> BranchName {
    BranchName::new(name).expect("a stem makes a branch name")
}

/// `decks/<stem>/main`.
pub fn main_line(stem: &DeckStem) -> BranchName {
    branch_name(&stem.branch("main"))
}

/// The repo version `VERSION` holds at `at`, or `None` when it holds none.
pub fn version_at<S: Store>(repo: &Repo<S>, at: &CommitId) -> Out<Option<u32>> {
    let Some(bytes) = repo.read_file(at, &path("VERSION")).map_err(said)? else {
        return Ok(None);
    };
    Ok(String::from_utf8_lossy(&bytes).trim().parse().ok())
}

/// The decks of a version-2 repo, as their `main` branches say.
#[derive(Debug, Default)]
pub struct DeckLines {
    /// Each deck's `main` tip, by stem.
    pub tips: BTreeMap<DeckStem, CommitId>,
}

impl DeckLines {
    pub fn read<S: Store>(repo: &Repo<S>) -> Out<Self> {
        let mut tips = BTreeMap::new();
        for (branch, tip) in repo.branches().map_err(said)? {
            let Some(rest) = branch.as_str().strip_prefix("decks/") else {
                continue;
            };
            let Some(stem) = rest.strip_suffix("/main") else {
                continue;
            };
            if let Ok(stem) = DeckStem::new(stem) {
                tips.insert(stem, tip);
            }
        }
        Ok(Self { tips })
    }

    /// The commits of `stem`'s own line, newest first: its `main`'s
    /// first-parent chain for as long as each commit holds its deck file.
    /// The first commit that does not is where the deck was forked from,
    /// the second value.
    fn own_line<S: Store>(
        repo: &Repo<S>,
        stem: &DeckStem,
        tip: CommitId,
    ) -> Out<(Vec<CommitId>, Option<CommitId>)> {
        let file = path(&stem.deck_file());
        let mut line = Vec::new();
        let mut at = Some(tip);
        while let Some(c) = at {
            if repo.entry(&c, &file).map_err(said)?.is_none() {
                return Ok((line, Some(c)));
            }
            line.push(c);
            at = repo.read_commit(&c).map_err(said)?.parents.first().copied();
        }
        Ok((line, None))
    }

    /// The deck each deck was forked from, by stem: the deck whose own line
    /// holds the commit its line started from. A deck forked from one since
    /// deleted, or from another repo's, has none, and stands on its own.
    pub fn parents<S: Store>(&self, repo: &Repo<S>) -> Out<BTreeMap<DeckStem, DeckStem>> {
        let mut owner: HashMap<CommitId, &DeckStem> = HashMap::new();
        let mut forked_at = Vec::new();
        for (stem, tip) in &self.tips {
            let (line, fork) = Self::own_line(repo, stem, *tip)?;
            for c in line {
                owner.insert(c, stem);
            }
            if let Some(f) = fork {
                forked_at.push((stem, f));
            }
        }
        Ok(forked_at
            .into_iter()
            .filter_map(|(stem, at)| {
                let parent = owner.get(&at)?;
                (*parent != stem).then(|| (stem.clone(), (*parent).clone()))
            })
            .collect())
    }
}

/// What [`rewrite_v1`] made: every object written, and no ref moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewritten {
    /// The version-1 `main` it was made from.
    pub from: CommitId,
    /// The one commit on top of it that takes `decks/` away.
    pub main: CommitId,
    /// Each deck at `from`, the tip of its own line.
    pub decks: BTreeMap<DeckStem, CommitId>,
    /// `from`'s first-parent chain, oldest first, by position.
    index: HashMap<CommitId, usize>,
    /// Each deck's rewritten commits, oldest first, beside the position of
    /// the commit on `main` each replays.
    lines: BTreeMap<DeckStem, Vec<(usize, CommitId)>>,
}

impl Rewritten {
    /// The commit of `stem`'s line that stands for `at`, a commit of the
    /// version-1 history: the newest one replaying a commit at or before
    /// it on `main`'s first-parent chain. `None` when the deck did not exist
    /// then, or `at` is not in that history.
    pub fn standing_for<S: Store>(
        &self,
        repo: &Repo<S>,
        stem: &DeckStem,
        at: &CommitId,
    ) -> Out<Option<CommitId>> {
        let line = match self.lines.get(stem) {
            Some(l) if !l.is_empty() => l,
            _ => return Ok(None),
        };
        // A commit off the first-parent chain (the other side of a merge)
        // stands where its own first-parent chain meets it.
        let mut walk = Some(*at);
        let mut position = None;
        let mut steps = 0;
        while let Some(c) = walk {
            if let Some(i) = self.index.get(&c) {
                position = Some(*i);
                break;
            }
            steps += 1;
            if steps > 100_000 {
                break;
            }
            walk = match repo.read_commit(&c) {
                Ok(info) => info.parents.first().copied(),
                Err(Error::MissingObject(_)) => None,
                Err(e) => return Err(said(e)),
            };
        }
        let Some(i) = position else {
            return Ok(None);
        };
        Ok(line
            .iter()
            .rev()
            .find(|(at, _)| *at <= i)
            .map(|(_, commit)| *commit))
    }
}

/// The files of one deck at one commit of version 1's `main`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct DeckFiles {
    deck: Option<BlobId>,
    criteria: Option<BlobId>,
    /// Its named criteria sets, `<stem>.<name>.criteria.toml`, by name.
    named: BTreeMap<String, BlobId>,
}

/// Whose criteria `base`, a file's name less `.criteria.toml`, are: the deck
/// of that stem, or else the longest deck stem it starts with and a dot, as
/// that deck's set named the rest. `None` when no deck is either.
fn criteria_owner(base: &str, is_deck: impl Fn(&str) -> bool) -> Option<(&str, Option<&str>)> {
    if is_deck(base) {
        return Some((base, None));
    }
    base.rmatch_indices('.')
        .map(|(i, _)| (&base[..i], &base[i + 1..]))
        .find(|(stem, name)| !name.is_empty() && is_deck(stem))
        .map(|(stem, name)| (stem, Some(name)))
}

/// `decks/`'s deck and criteria files at `at`, by stem. Names that are
/// neither are left out; [`check_tip`] refuses them where it matters.
fn deck_files_at<S: Store>(repo: &Repo<S>, at: &CommitId) -> Out<BTreeMap<String, DeckFiles>> {
    let mut out: BTreeMap<String, DeckFiles> = BTreeMap::new();
    let entries = match repo.list_dir(at, Some(&path(DECKS_DIR))) {
        Ok(Some(e)) => e,
        Ok(None) | Err(Error::NotADirectory(_)) => return Ok(out),
        Err(e) => return Err(said(e)),
    };
    let files: Vec<(String, BlobId)> = entries
        .into_iter()
        .filter_map(|e| match e.kind {
            EntryKind::File(b) | EntryKind::Executable(b) => Some((e.name, b)),
            _ => None,
        })
        .collect();
    for (name, blob) in &files {
        if let Some(stem) = name.strip_suffix(DECK_SUFFIX) {
            out.entry(stem.to_owned()).or_default().deck = Some(*blob);
        }
    }
    let decks: BTreeSet<String> = out.keys().cloned().collect();
    for (name, blob) in &files {
        let Some(base) = name.strip_suffix(CRITERIA_SUFFIX) else {
            continue;
        };
        // Criteria of no deck here go under their own name, deckless, and
        // are never replayed.
        let (stem, set) = criteria_owner(base, |s| decks.contains(s)).unwrap_or((base, None));
        let files = out.entry(stem.to_owned()).or_default();
        match set {
            None => files.criteria = Some(*blob),
            Some(set) => {
                files.named.insert(set.to_owned(), *blob);
            }
        }
    }
    Ok(out)
}

/// The decks at the tip, refusing what version 2 has no place for: a
/// directory or another file in `decks/`, criteria without their deck, or a
/// name that cannot be a branch.
fn check_tip<S: Store>(repo: &Repo<S>, tip: &CommitId) -> Out<BTreeSet<DeckStem>> {
    let entries = match repo.list_dir(tip, Some(&path(DECKS_DIR))) {
        Ok(Some(e)) => e,
        Ok(None) | Err(Error::NotADirectory(_)) => return Ok(BTreeSet::new()),
        Err(e) => return Err(said(e)),
    };
    let mut decks = BTreeSet::new();
    let mut criteria = Vec::new();
    for e in entries {
        let refuse = |why: &str| {
            Err(format!(
                "decks/{} {why}, and repo version 2 keeps only each deck's .deck.toml and \
                 its criteria, on the deck's own line. Move it out of decks/ first.",
                e.name
            ))
        };
        if !matches!(e.kind, EntryKind::File(_) | EntryKind::Executable(_)) {
            return refuse("is not a file");
        }
        if let Some(stem) = e.name.strip_suffix(DECK_SUFFIX) {
            decks.insert(
                DeckStem::new(stem).map_err(|e| format!("decks/{stem}{DECK_SUFFIX}: {e}"))?,
            );
        } else if let Some(base) = e.name.strip_suffix(CRITERIA_SUFFIX) {
            criteria.push(base.to_owned());
        } else {
            return refuse("is neither a deck nor its criteria");
        }
    }
    if let Some(lone) = criteria
        .iter()
        .find(|base| criteria_owner(base, |s| decks.iter().any(|d| d.as_str() == s)).is_none())
    {
        return Err(format!(
            "decks/{lone}{CRITERIA_SUFFIX} has no deck beside it, and criteria live on their \
             deck's line in repo version 2. Remove it or add its deck first."
        ));
    }
    Ok(decks)
}

/// Who the migration commit is by: the same in every browser, so two that
/// migrate the same `main` make the same commit.
fn curator_at(when: Timestamp) -> Signature {
    Signature {
        who: Identity::new("Progress Engine", "curator@localhost").expect("a valid identity"),
        when,
    }
}

/// The text of a version-1 deck as its own line keeps it: without
/// `variant_of`, which its ancestry says now.
fn deck_text_v2<S: Store>(repo: &mut Repo<S>, blob: BlobId) -> Out<BlobId> {
    let bytes = repo.read_blob(&blob).map_err(said)?;
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Ok(blob);
    };
    match edit::without_variant_of(text) {
        Some(stripped) => repo.write_blob(stripped.as_bytes()).map_err(said),
        None => Ok(blob),
    }
}

/// The deck a version-1 deck was a variant of, from the `variant_of` its
/// file at `blob` names, when that is another deck at the tip.
fn variant_parent<S: Store>(
    repo: &Repo<S>,
    blob: BlobId,
    stem: &DeckStem,
    decks: &BTreeSet<DeckStem>,
) -> Out<Option<DeckStem>> {
    let bytes = repo.read_blob(&blob).map_err(said)?;
    let parent = std::str::from_utf8(&bytes)
        .ok()
        .and_then(edit::v1_variant_of)
        .and_then(|p| DeckStem::from_v1_path(&p));
    Ok(parent.filter(|p| p != stem && decks.contains(p)))
}

/// Writes version 2 of the version-1 `main` at `from`, and moves no ref:
///
/// - Each deck at `from` gets its own line: every commit on `from`'s
///   first-parent chain that changed its deck or criteria file since it was
///   last made, replayed with the same message, author and committer
///   (dates and zones included) onto a tree of those two files alone, the
///   deck without `variant_of`. A replay that changes nothing is left out.
/// - A variant's first commit has as its parent the commit of its parent's
///   line that stood at that point, so the variant is a fork.
/// - One commit on `from` deletes `decks/`, writes `VERSION` 2, names each
///   collection deck place and each deck in the deck order by its stem, by
///   "Progress Engine <curator@localhost>" at `from`'s committer time.
pub fn rewrite_v1<S: Store>(repo: &mut Repo<S>, from: &CommitId) -> Out<Rewritten> {
    if version_at(repo, from)? != Some(1) {
        return Err("only a repo at version 1 migrates to version 2".to_owned());
    }
    let decks = check_tip(repo, from)?;
    let tip_files = deck_files_at(repo, from)?;
    let parent_of: BTreeMap<DeckStem, DeckStem> = decks
        .iter()
        .filter_map(|stem| {
            let blob = tip_files.get(stem.as_str())?.deck?;
            match variant_parent(repo, blob, stem, &decks) {
                Ok(Some(p)) => Some(Ok((stem.clone(), p))),
                Ok(None) => None,
                Err(e) => Some(Err(e)),
            }
        })
        .collect::<Out<_>>()?;
    // A deck is replayed after the deck it is a variant of, at each commit,
    // so a variant made in the same commit as its parent's change forks
    // from it. A loop of variants is cut where it closes.
    let depth = |stem: &DeckStem| {
        let mut d = 0;
        let mut at = stem;
        let mut seen = vec![stem];
        while let Some(p) = parent_of.get(at) {
            if seen.contains(&p) {
                break;
            }
            seen.push(p);
            at = p;
            d += 1;
        }
        d
    };
    let mut order: Vec<&DeckStem> = decks.iter().collect();
    order.sort_by_key(|s| (depth(s), (*s).clone()));

    let mut chain = Vec::new();
    let mut at = Some(*from);
    while let Some(c) = at {
        chain.push(c);
        at = repo.read_commit(&c).map_err(said)?.parents.first().copied();
    }
    chain.reverse();
    let index: HashMap<CommitId, usize> = chain.iter().enumerate().map(|(i, c)| (*c, i)).collect();

    let mut lines: BTreeMap<DeckStem, Vec<(usize, CommitId)>> = BTreeMap::new();
    let mut trees: BTreeMap<DeckStem, meldweb_git::TreeId> = BTreeMap::new();
    let mut before: BTreeMap<String, DeckFiles> = BTreeMap::new();
    for (i, commit) in chain.iter().enumerate() {
        let now = deck_files_at(repo, commit)?;
        let info = repo.read_commit(commit).map_err(said)?;
        for stem in &order {
            let files = now.get(stem.as_str()).cloned().unwrap_or_default();
            if before.get(stem.as_str()).cloned().unwrap_or_default() == files {
                continue;
            }
            let Some(deck) = files.deck else {
                // Deleted: a deck made again later starts a line of its own.
                lines.remove(*stem);
                trees.remove(*stem);
                continue;
            };
            let mut tree_files = vec![(path(&stem.deck_file()), deck_text_v2(repo, deck)?)];
            if let Some(c) = files.criteria {
                tree_files.push((path(&stem.criteria_file()), c));
            }
            for (name, c) in &files.named {
                tree_files.push((path(&format!("{stem}.{name}{CRITERIA_SUFFIX}")), *c));
            }
            let tree = repo.write_files(&tree_files).map_err(said)?;
            let line = lines.entry((*stem).clone()).or_default();
            if trees.get(*stem) == Some(&tree) {
                continue;
            }
            let parents: Vec<CommitId> = match line.last() {
                Some((_, last)) => vec![*last],
                None => parent_of
                    .get(*stem)
                    .and_then(|p| lines.get(p))
                    .and_then(|l| l.last())
                    .map(|(_, c)| *c)
                    .into_iter()
                    .collect(),
            };
            let replayed = repo
                .write_commit_of(tree, &parents, &info.author, &info.committer, &info.message)
                .map_err(said)?;
            lines
                .get_mut(*stem)
                .expect("made above")
                .push((i, replayed));
            trees.insert((*stem).clone(), tree);
        }
        before = now;
    }

    let tip_info = repo.read_commit(from).map_err(said)?;
    let mut changes: Vec<Change> = tip_files
        .iter()
        .flat_map(|(stem, f)| {
            let mut out = Vec::new();
            if f.deck.is_some() {
                out.push(Change::Delete {
                    path: path(&format!("{DECKS_DIR}/{stem}{DECK_SUFFIX}")),
                });
            }
            if f.criteria.is_some() {
                out.push(Change::Delete {
                    path: path(&format!("{DECKS_DIR}/{stem}{CRITERIA_SUFFIX}")),
                });
            }
            for name in f.named.keys() {
                out.push(Change::Delete {
                    path: path(&format!("{DECKS_DIR}/{stem}.{name}{CRITERIA_SUFFIX}")),
                });
            }
            out
        })
        .collect();
    changes.push(Change::Put {
        path: path("VERSION"),
        bytes: format!("{VERSION}\n").into_bytes(),
    });
    let text_at = |repo: &Repo<S>, p: &str| -> Out<Option<String>> {
        Ok(repo
            .read_file(from, &path(p))
            .map_err(said)?
            .and_then(|b| String::from_utf8(b).ok()))
    };
    if let Some(text) = text_at(repo, "collection.toml")? {
        if let Some(stems) = collection::v1_places_to_stems(&text) {
            changes.push(Change::Put {
                path: path("collection.toml"),
                bytes: stems.into_bytes(),
            });
        }
    }
    if let Some(text) = text_at(repo, "meldweb.toml")? {
        if let Some(stems) = v1_order_to_stems(&text) {
            changes.push(Change::Put {
                path: path("meldweb.toml"),
                bytes: stems.into_bytes(),
            });
        }
    }
    let curator = curator_at(tip_info.committer.when);
    let main = repo
        .write_commit(
            &[*from],
            &WriteCommit {
                changes,
                message: format!(
                    "curator: this repo is at version {VERSION}, each deck a line of its own\n\n\
                     Each deck's commits are replayed onto decks/<stem>/main, a variant's as a \
                     fork of its parent's,\nand each snapshot moved onto its deck's line \
                     (ADR-0041).\n"
                ),
                author: curator.clone(),
                committer: curator,
            },
        )
        .map_err(said)?;
    let decks = lines
        .iter()
        .filter_map(|(stem, l)| l.last().map(|(_, c)| (stem.clone(), *c)))
        .collect();
    Ok(Rewritten {
        from: *from,
        main,
        decks,
        index,
        lines,
    })
}

/// Points each snapshot of a deck `rewritten` holds at the commit of the
/// deck's line standing for the version-1 commit it named. A snapshot already
/// on its deck's line, or naming a commit from before its deck, stays.
/// Returns how many moved.
pub fn move_snapshots<S: Store>(repo: &mut Repo<S>, rewritten: &Rewritten) -> Out<usize> {
    let mut moved = 0;
    for tag in repo.tags("decks/").map_err(said)? {
        let TagKind::Annotated { .. } = tag.kind else {
            continue;
        };
        let Some(stem) = tag
            .name
            .as_str()
            .strip_prefix("decks/")
            .and_then(|r| r.split_once('/'))
            .and_then(|(s, _)| DeckStem::new(s).ok())
        else {
            continue;
        };
        if !rewritten.decks.contains_key(&stem) {
            continue;
        }
        if let Some(target) = rewritten.standing_for(repo, &stem, &tag.commit)? {
            if target != tag.commit {
                repo.retarget_tag(&tag.name, &target).map_err(said)?;
                moved += 1;
            }
        }
    }
    Ok(moved)
}

/// What migrating a repo came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migrated {
    pub main: CommitId,
    pub decks: usize,
    pub snapshots: usize,
}

/// Migrates `main` from version 1 to 2 in one go: every object is written
/// first, then each deck's branch is made, each snapshot moved, and `main`
/// moved last, so a failure before that leaves the repo at version 1, and
/// migrating again makes the same objects and finds the same branches.
pub fn migrate<S: Store>(repo: &mut Repo<S>) -> Out<Migrated> {
    let main = branch_name("main");
    let Some(tip) = repo.branch_tip(&main).map_err(said)? else {
        return Err("the repo has no main to migrate".to_owned());
    };
    let rewritten = rewrite_v1(repo, &tip)?;
    for (stem, line_tip) in &rewritten.decks {
        let branch = main_line(stem);
        match repo.branch_tip(&branch).map_err(said)? {
            None => repo.create_branch(&branch, line_tip).map_err(said)?,
            Some(existing) if existing == *line_tip => {}
            Some(existing) => {
                // A branch the remote made (fetched before this migrated):
                // take whichever line holds the other.
                match repo.standing(&existing, line_tip).map_err(said)? {
                    Standing::UpToDate => {}
                    Standing::FastForward => repo
                        .update_branch(&branch, Tip::At(existing), line_tip)
                        .map_err(said)?,
                    Standing::Diverged { .. } | Standing::Unrelated => {
                        return Err(format!(
                            "{branch} is here already with history this migration did not make; \
                             nothing was changed"
                        ))
                    }
                }
            }
        }
    }
    let snapshots = move_snapshots(repo, &rewritten)?;
    repo.update_branch(&main, Tip::At(tip), &rewritten.main)
        .map_err(said)?;
    Ok(Migrated {
        main: rewritten.main,
        decks: rewritten.decks.len(),
        snapshots,
    })
}

/// Brings the fetched branches in (`meldweb_git`'s `integrate_onto`). A
/// remote whose `main` is still at repo version 1, while this repo's is at 2,
/// is migrated here first, as it would be where it lives: [`rewrite_v1`]
/// makes the same commits from the same history, so what is brought in is
/// the remote's `main` with its migration on top and each deck's line, and
/// the snapshots fetched from it move onto those lines. A push then takes all
/// of it there, `main` as a fast-forward.
pub fn integrate<S: Store>(
    repo: &mut Repo<S>,
    who: &Signature,
    prefer: Option<Side>,
    merge: &mut dyn MergeBranch<S>,
) -> Out<Vec<(BranchName, Integrated)>> {
    let mut theirs = repo.integration_targets().map_err(said)?;
    let main = branch_name("main");
    if let (Some(remote), Some(local)) = (
        theirs.get(&main).copied(),
        repo.branch_tip(&main).map_err(said)?,
    ) {
        if version_at(repo, &remote)? == Some(1) && version_at(repo, &local)? == Some(VERSION) {
            let rewritten = rewrite_v1(repo, &remote)?;
            theirs.insert(main.clone(), rewritten.main);
            for (stem, tip) in &rewritten.decks {
                theirs.insert(main_line(stem), *tip);
            }
            move_snapshots(repo, &rewritten)?;
        }
    }
    repo.integrate_onto(&theirs, who, prefer, merge)
        .map_err(said)
}

/// `meldweb.toml` with each deck in `[decks] order` that names a
/// repo-version-1 path named by its stem instead, and every other byte as it
/// was; `None` when none does (or the file is not TOML). An edit in place,
/// not the settings written out again, so what the migration makes does not
/// hang on how this version of Curator lays the file out.
fn v1_order_to_stems(text: &str) -> Option<String> {
    let mut doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    let order = doc
        .get_mut("decks")?
        .as_table_like_mut()?
        .get_mut("order")?
        .as_array_mut()?;
    let mut changed = false;
    for value in order.iter_mut() {
        let Some(stem) = value.as_str().and_then(DeckStem::from_v1_path) else {
            continue;
        };
        let decor = value.decor().clone();
        *value = toml_edit::Value::from(stem.as_str());
        *value.decor_mut() = decor;
        changed = true;
    }
    changed.then(|| doc.to_string())
}
