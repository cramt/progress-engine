//! The browser's repo (ADR-0041): a bare git repo of loose objects, kept in
//! OPFS by the repo's worker and read and written here through
//! `meldweb-git`.
//!
//! `meldweb-git`'s store is synchronous and OPFS is not, so the worker loads
//! the repo's files into a [`Mirror`] and every operation runs on that. The
//! mirror keeps a journal of what an operation wrote, which the worker writes
//! through to OPFS, in order, before it answers. So the worker moves bytes and
//! decides nothing: which blob guards a write, what conflicts, what a history
//! page holds and what a zip may contain are all decided here.
//!
//! Each method answers with JSON of one of the wire types below, which
//! `web/src/deck.gen.ts` carries. An outcome the page acts on, such as a
//! stale blob id, is a variant; a repo that cannot be read is an error.

use std::collections::BTreeSet;
use std::convert::Infallible;
use std::io::{Cursor, Read, Write};

use facet::Facet;

use chip_decklist::stem::DeckStem;
use meldweb_git::{
    BlobId, BranchName, Change, CommitId, EntryKind, Error, FetchRequest, Identity, Integrated,
    MemStore, PushRequest, RemoteUrl, Repo, RepoPath, Side, Signature, Store, TagKind, TagName,
    Timestamp, Tip, Unsupported, WriteCommit,
};

use crate::layout::{self, DeckLines};
use wasm_bindgen::prelude::{wasm_bindgen, JsError};

/// One thing an operation did to the git dir, for the worker to repeat.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Step {
    Write { path: String },
    Remove { path: String },
    Dir { path: String },
}

/// What the worker must write through before it answers, in order: objects
/// before the refs that name them, as `meldweb-git` wrote them.
#[derive(Debug, Default, Facet)]
pub struct Journal {
    pub steps: Vec<Step>,
}

/// The repo's files in memory, with a journal of what changed since the
/// worker last took it.
#[derive(Debug, Default)]
pub struct Mirror {
    files: MemStore,
    journal: Vec<Step>,
}

impl Store for Mirror {
    type Error = Infallible;

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, Infallible> {
        self.files.read(path)
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), Infallible> {
        self.journal.push(Step::Write {
            path: path.to_owned(),
        });
        self.files.write(path, bytes)
    }

    fn remove(&mut self, path: &str) -> Result<bool, Infallible> {
        let removed = self.files.remove(path)?;
        if removed {
            self.journal.push(Step::Remove {
                path: path.to_owned(),
            });
        }
        Ok(removed)
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, Infallible> {
        self.files.list(dir)
    }

    fn create_dir(&mut self, dir: &str) -> Result<(), Infallible> {
        self.journal.push(Step::Dir {
            path: dir.to_owned(),
        });
        self.files.create_dir(dir)
    }
}

/// Who wrote a commit or a tag, and when, as the page knows it: the identity
/// set in this browser (ADR-0041) and its clock.
#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct Who {
    pub name: String,
    pub email: String,
    /// Seconds since the Unix epoch.
    pub seconds: i64,
    /// Minutes east of UTC, the negation of `getTimezoneOffset()`.
    pub offset_minutes: i32,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum FileRead {
    /// `sha` is the blob id, which a later write sends back as its guard.
    File {
        text: String,
        sha: String,
    },
    NoFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
#[facet(rename_all = "lowercase")]
pub enum EntryType {
    File,
    Dir,
    Symlink,
    Submodule,
}

#[derive(Debug, Facet)]
pub struct ListedEntry {
    pub name: String,
    pub path: String,
    pub sha: String,
    #[facet(rename = "type")]
    pub kind: EntryType,
}

#[derive(Debug, Facet)]
pub struct Listing {
    pub entries: Vec<ListedEntry>,
}

/// One commit, as a deck's history lists it.
#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct CommitFacts {
    pub commit: String,
    /// Without the newline git ends a message with.
    pub message: String,
    /// The committer's time, in seconds since the Unix epoch.
    pub committed_at: i64,
    /// The author's name.
    pub author: String,
}

#[derive(Debug, Facet)]
pub struct Revisions {
    pub revisions: Vec<CommitFacts>,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum RevisionRead {
    Found {
        revision: CommitFacts,
    },
    /// No commit by that id, or not an id at all.
    Missing,
}

/// An annotated tag: a snapshot (ADR-0024).
#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct TagFacts {
    pub tag: String,
    /// The tag's message, without the newline git ends it with.
    pub label: String,
    pub commit: String,
    /// The tagger's time in seconds since the Unix epoch, when it has one.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub tagged_at: Option<i64>,
}

#[derive(Debug, Facet)]
pub struct Snapshots {
    pub snapshots: Vec<TagFacts>,
}

/// What a write to the default branch came to.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Wrote {
    /// `sha` is the file's new blob id (absent after a delete), `commit` the
    /// branch's new tip.
    Committed {
        #[facet(default, skip_serializing_if = Option::is_none)]
        sha: Option<String>,
        commit: String,
    },
    /// The file is not the one the write was guarded by: it changed, was
    /// made, or was removed since. Nothing was written.
    Conflict { message: String },
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Tagged {
    Tagged {
        snapshot: TagFacts,
    },
    /// A tag by that name exists.
    Conflict {
        message: String,
    },
}

/// The remote, and what is waiting to go to it or come from it, as the
/// settings page shows them.
#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct RemoteFacts {
    /// `origin`'s URL; absent when the repo has no remote.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub url: Option<String>,
    /// Commits on the branches here that the remote's lack, as last fetched.
    pub ahead: u32,
    /// Commits on the remote's branches, as last fetched, that none here holds.
    pub behind: u32,
    /// Branches (decks, mostly) to make, move or delete on the remote.
    pub branches: u32,
    /// Snapshots to make, move or delete on the remote.
    pub tags: u32,
}

/// What bringing the remote's branches into these came to, all of them
/// together; see `meldweb_git::Integrated`. A path is named as it is on its
/// branch, and a deck's with its branch before it:
/// `decks/lantern/main: lantern.deck.toml`.
#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum Synced {
    RemoteEmpty,
    UpToDate,
    Ahead,
    FastForwarded {
        to: String,
    },
    Adopted {
        to: String,
    },
    Merged {
        commit: String,
        taken: Vec<String>,
    },
    /// Nothing moved; each path changed here and on the remote in the same
    /// lines, or into something that is no longer a file of its kind.
    Conflict {
        paths: Vec<String>,
    },
    Unrelated,
}

#[derive(Debug, Facet)]
pub struct Refused {
    #[facet(rename = "ref")]
    pub reference: String,
    pub reason: String,
}

/// What a push came to, ref by ref.
#[derive(Debug, Facet)]
pub struct PushOutcome {
    pub updated: Vec<String>,
    pub rejected: Vec<Refused>,
}

/// One deck as the deck list reads it: its stem, the text on its `main`,
/// and the deck it was forked from, read from its history (ADR-0041).
#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct DeckLine {
    pub stem: String,
    /// The tip of `decks/<stem>/main`.
    pub commit: String,
    /// `<stem>.deck.toml` there; absent when the line holds none.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub text: Option<String>,
    /// Its blob id, which a later write sends back as its guard.
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub sha: Option<String>,
    #[facet(default, skip_serializing_if = Option::is_none)]
    pub parent: Option<String>,
}

#[derive(Debug, Facet)]
pub struct DeckLineList {
    pub decks: Vec<DeckLine>,
}

#[derive(Debug, Facet)]
#[repr(u8)]
#[facet(tag = "kind", rename_all = "camelCase")]
pub enum DeckDeleted {
    Deleted,
    /// The deck's line moved since it was read, or is gone. Nothing changed.
    Conflict {
        message: String,
    },
}

/// What migrating the repo to the next version came to.
#[derive(Debug, Facet)]
#[facet(rename_all = "camelCase")]
pub struct MigratedFacts {
    pub version: u32,
    pub decks: u32,
    pub snapshots: u32,
}

type Out<T> = Result<T, String>;

/// `meldweb-git`'s refusals, in the words the page shows.
fn said(e: Error) -> String {
    match e {
        Error::Unsupported(Unsupported::Packfile) => "This repo keeps its objects in a packfile \
            (objects/pack/), as git clone and git gc leave them, and Progress Engine reads only loose \
            objects. In a copy of the repo, move each .pack file out of objects/pack/ and run \
            `git unpack-objects < that.pack`, then zip the bare repo again."
            .to_owned(),
        Error::Unsupported(Unsupported::PackedRefs) => "This repo keeps its branches and tags \
            in one packed-refs file, as git clone and git pack-refs leave them, and Progress Engine \
            reads only the files under refs/."
            .to_owned(),
        Error::Unsupported(u) => format!("This repo holds a {u:?}, which Progress Engine does not read."),
        other => other.to_string(),
    }
}

fn message_of(text: &str) -> String {
    text.trim_end_matches('\n').to_owned()
}

/// Git ends a commit or tag message with a newline, as `git commit` does.
fn git_message(text: &str) -> String {
    format!("{}\n", text.trim_end_matches('\n'))
}

fn signature(who: &Who) -> Out<Signature> {
    Ok(Signature {
        who: Identity::new(who.name.as_str(), who.email.as_str()).map_err(|e| e.to_string())?,
        when: Timestamp::new(who.seconds, who.offset_minutes).map_err(|e| e.to_string())?,
    })
}

fn repo_path(path: &str) -> Out<RepoPath> {
    RepoPath::new(path).map_err(|e| e.to_string())
}

/// Every branch's outcome as one: what stops the sync first, then what moved.
fn synced(outcomes: Vec<(BranchName, Integrated)>) -> Synced {
    let named = |branch: &BranchName, path: String| {
        if branch.as_str() == "main" {
            path
        } else {
            format!("{branch}: {path}")
        }
    };
    let mut conflicts = Vec::new();
    let mut unrelated = false;
    let mut merged: Option<(String, Vec<String>)> = None;
    let mut adopted = None;
    let mut forwarded = None;
    let mut ahead = false;
    let mut empty = false;
    for (branch, outcome) in outcomes {
        match outcome {
            Integrated::Conflict { paths } => {
                conflicts.extend(paths.into_iter().map(|p| named(&branch, p)));
            }
            Integrated::Unrelated => unrelated = true,
            Integrated::Merged { commit, taken } => {
                let taken: Vec<String> = taken.into_iter().map(|p| named(&branch, p)).collect();
                match &mut merged {
                    Some((_, all)) => all.extend(taken),
                    None => merged = Some((commit.to_hex(), taken)),
                }
            }
            Integrated::Adopted { to } => {
                if branch.as_str() == "main" || adopted.is_none() {
                    adopted = Some(to.to_hex());
                }
            }
            Integrated::FastForwarded { to } => {
                if branch.as_str() == "main" || forwarded.is_none() {
                    forwarded = Some(to.to_hex());
                }
            }
            Integrated::Ahead => ahead = true,
            Integrated::UpToDate => {}
            Integrated::RemoteEmpty => empty = branch.as_str() == "main",
        }
    }
    if !conflicts.is_empty() {
        return Synced::Conflict { paths: conflicts };
    }
    if unrelated {
        return Synced::Unrelated;
    }
    if let Some((commit, taken)) = merged {
        return Synced::Merged { commit, taken };
    }
    if let Some(to) = adopted {
        return Synced::Adopted { to };
    }
    if let Some(to) = forwarded {
        return Synced::FastForwarded { to };
    }
    if empty {
        return Synced::RemoteEmpty;
    }
    if ahead {
        return Synced::Ahead;
    }
    Synced::UpToDate
}

/// The files a bare repo is, and nothing a working tree or a hook adds.
fn is_repo_file(path: &str) -> bool {
    matches!(path, "HEAD" | "config" | "packed-refs")
        || path.starts_with("objects/")
        || path.starts_with("refs/")
}

/// Whether `path` is a ref, which another tab may have moved: everything
/// else in the git dir is an object, which never changes once written.
fn is_ref_file(path: &str) -> bool {
    matches!(path, "HEAD" | "packed-refs") || path.starts_with("refs/")
}

/// The directories `git` needs to see a bare repo, even an empty one.
const REPO_DIRS: [&str; 4] = ["objects/info", "objects/pack", "refs/heads", "refs/tags"];

/// The repo as one tab holds it. `None` until it is made or loaded.
#[derive(Debug, Default)]
pub struct Local {
    repo: Option<Repo<Mirror>>,
    /// Files loaded before the repo opens, which `open` turns into one.
    loading: Mirror,
}

impl Local {
    fn repo(&self) -> Out<&Repo<Mirror>> {
        self.repo
            .as_ref()
            .ok_or_else(|| "the repo is not open".to_owned())
    }

    fn repo_mut(&mut self) -> Out<&mut Repo<Mirror>> {
        self.repo
            .as_mut()
            .ok_or_else(|| "the repo is not open".to_owned())
    }

    fn mirror(&self) -> &Mirror {
        match &self.repo {
            Some(r) => r.store(),
            None => &self.loading,
        }
    }

    /// One file of the git dir, as storage holds it. Not journalled.
    pub fn load(&mut self, path: &str, bytes: &[u8]) {
        self.with_files(|f| {
            let _ = f.write(path, bytes);
        });
    }

    /// A directory of the git dir, as storage holds it. Not journalled.
    pub fn load_dir(&mut self, path: &str) {
        self.with_files(|f| {
            let _ = f.create_dir(path);
        });
    }

    fn with_files(&mut self, f: impl FnOnce(&mut MemStore)) {
        match self.repo.as_mut() {
            Some(repo) => f(&mut repo.store_mut().files),
            None => f(&mut self.loading.files),
        }
    }

    pub fn has(&self, path: &str) -> bool {
        matches!(self.mirror().files.read(path), Ok(Some(_)))
    }

    pub fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.mirror().files.read(path).ok().flatten()
    }

    /// Opens what was loaded: `true` when it is a repo, `false` when storage
    /// held none, in which case [`Local::init`] makes one.
    pub fn open(&mut self) -> bool {
        if self.repo.is_some() {
            return true;
        }
        if !self.has("HEAD") {
            return false;
        }
        match Repo::open(std::mem::take(&mut self.loading)) {
            Ok(repo) => {
                self.repo = Some(repo);
                true
            }
            Err(_) => unreachable!("HEAD is there, which is all opening reads"),
        }
    }

    /// Makes an empty repo whose HEAD is the unborn `main`.
    pub fn init(&mut self) -> Out<()> {
        if self.repo.is_some() {
            return Err("there is a repo here already".to_owned());
        }
        let mirror = std::mem::take(&mut self.loading);
        self.repo = Some(Repo::init(mirror).map_err(said)?);
        Ok(())
    }

    /// Every ref file (HEAD, packed-refs and refs/…) as another tab may have
    /// left them, replacing this tab's. `true` when any differed, after which
    /// the worker loads the objects this tab lacks.
    pub fn replace_refs(&mut self, refs: &[(String, Vec<u8>)]) -> bool {
        let mut changed = false;
        self.with_files(|files| {
            let given: BTreeSet<&str> = refs.iter().map(|(p, _)| p.as_str()).collect();
            let held: Vec<String> = files
                .files()
                .map(|(p, _)| p.to_owned())
                .filter(|p| is_ref_file(p))
                .collect();
            for path in held {
                if !given.contains(path.as_str()) {
                    let _ = files.remove(&path);
                    changed = true;
                }
            }
            for (path, bytes) in refs {
                if files.read(path).ok().flatten().as_deref() != Some(bytes.as_slice()) {
                    let _ = files.write(path, bytes);
                    changed = true;
                }
            }
        });
        changed
    }

    pub fn take_journal(&mut self) -> Journal {
        let mirror = match self.repo.as_mut() {
            Some(repo) => repo.store_mut(),
            None => &mut self.loading,
        };
        Journal {
            steps: std::mem::take(&mut mirror.journal),
        }
    }

    fn head(&self) -> Out<(BranchName, Option<CommitId>)> {
        let repo = self.repo()?;
        let branch = repo.head_branch().map_err(said)?;
        let tip = repo.branch_tip(&branch).map_err(said)?;
        Ok((branch, tip))
    }

    /// The tip of `branch`, or the default branch's when it is `None`.
    fn tip(&self, branch: Option<&str>) -> Out<(BranchName, Option<CommitId>)> {
        match branch {
            None => self.head(),
            Some(name) => {
                let branch = BranchName::new(name).map_err(|e| e.to_string())?;
                let tip = self.repo()?.branch_tip(&branch).map_err(said)?;
                Ok((branch, tip))
            }
        }
    }

    /// The commit `at` names, a commit's id or a branch's name, or the
    /// default branch's tip; `None` when the branch is unborn or `at` is no
    /// commit here.
    fn commit_at(&self, at: Option<&str>) -> Out<Option<CommitId>> {
        match at {
            None => Ok(self.head()?.1),
            Some(hex) => {
                let Ok(id) = CommitId::from_hex(hex) else {
                    return match BranchName::new(hex) {
                        Ok(_) => Ok(self.tip(Some(hex))?.1),
                        Err(_) => Ok(None),
                    };
                };
                match self.repo()?.read_commit(&id) {
                    Ok(_) => Ok(Some(id)),
                    Err(Error::MissingObject(_) | Error::WrongKind { .. }) => Ok(None),
                    Err(e) => Err(said(e)),
                }
            }
        }
    }

    fn blob_at(&self, at: &CommitId, path: &RepoPath) -> Out<Option<BlobId>> {
        match self.repo()?.entry(at, path).map_err(said)? {
            Some(EntryKind::File(b) | EntryKind::Executable(b)) => Ok(Some(b)),
            Some(_) => Err(format!("{path} is not a file")),
            None => Ok(None),
        }
    }

    pub fn get_file(&self, path: &str, at: Option<&str>) -> Out<FileRead> {
        let path = repo_path(path)?;
        let Some(commit) = self.commit_at(at)? else {
            return Ok(FileRead::NoFile);
        };
        let Some(blob) = self.blob_at(&commit, &path)? else {
            return Ok(FileRead::NoFile);
        };
        let bytes = self.repo()?.read_blob(&blob).map_err(said)?;
        let text = String::from_utf8(bytes).map_err(|_| format!("{path} is not UTF-8 text"))?;
        Ok(FileRead::File {
            text,
            sha: blob.to_hex(),
        })
    }

    pub fn get_bytes(&self, path: &str) -> Out<Option<Vec<u8>>> {
        let path = repo_path(path)?;
        let Some(commit) = self.head()?.1 else {
            return Ok(None);
        };
        match self.blob_at(&commit, &path)? {
            None => Ok(None),
            Some(blob) => self.repo()?.read_blob(&blob).map(Some).map_err(said),
        }
    }

    pub fn list_dir(&self, path: &str) -> Out<Listing> {
        let dir = if path.is_empty() {
            None
        } else {
            Some(repo_path(path)?)
        };
        let Some(commit) = self.head()?.1 else {
            return Ok(Listing { entries: vec![] });
        };
        let entries = match self.repo()?.list_dir(&commit, dir.as_ref()) {
            Ok(Some(entries)) => entries,
            Ok(None) | Err(Error::NotADirectory(_)) => vec![],
            Err(e) => return Err(said(e)),
        };
        let prefix = dir.map(|d| format!("{d}/")).unwrap_or_default();
        Ok(Listing {
            entries: entries
                .into_iter()
                .map(|e| {
                    let (kind, sha) = match e.kind {
                        EntryKind::File(b) | EntryKind::Executable(b) => {
                            (EntryType::File, b.to_hex())
                        }
                        EntryKind::Symlink(b) => (EntryType::Symlink, b.to_hex()),
                        EntryKind::Dir(t) => (EntryType::Dir, t.to_hex()),
                        EntryKind::Submodule(c) => (EntryType::Submodule, c.to_hex()),
                    };
                    ListedEntry {
                        path: format!("{prefix}{}", e.name),
                        name: e.name,
                        sha,
                        kind,
                    }
                })
                .collect(),
        })
    }

    fn facts(&self, id: &CommitId) -> Out<CommitFacts> {
        let info = self.repo()?.read_commit(id).map_err(said)?;
        Ok(CommitFacts {
            commit: id.to_hex(),
            message: message_of(&info.message),
            committed_at: info.committer.when.seconds(),
            author: info.author.who.name().to_owned(),
        })
    }

    /// The commits on `branch`'s (default: the default branch's)
    /// first-parent line that changed `path`, newest first: page `page`
    /// (from 1) of `per_page`, none committed after `until`. A fork's line
    /// goes on into the deck it was forked from, where its file is absent,
    /// so its history ends at the commit that made it.
    pub fn history(
        &self,
        path: &str,
        branch: Option<&str>,
        until: Option<i64>,
        page: u32,
        per_page: u32,
    ) -> Out<Revisions> {
        let path = repo_path(path)?;
        let (branch, _) = self.tip(branch)?;
        let mut revisions = Vec::new();
        let skip = (page.max(1) as usize - 1) * per_page as usize;
        let mut matched = 0;
        for entry in self.repo()?.history(&branch, &path).map_err(said)? {
            let facts = self.facts(&entry.commit)?;
            if until.is_some_and(|u| facts.committed_at > u) {
                continue;
            }
            matched += 1;
            if matched > skip {
                revisions.push(facts);
                if revisions.len() >= per_page as usize {
                    break;
                }
            }
        }
        Ok(Revisions { revisions })
    }

    pub fn revision(&self, commit: &str) -> Out<RevisionRead> {
        match self.commit_at(Some(commit))? {
            None => Ok(RevisionRead::Missing),
            Some(id) => Ok(RevisionRead::Found {
                revision: self.facts(&id)?,
            }),
        }
    }

    pub fn snapshots(&self, prefix: &str) -> Out<Snapshots> {
        let tags = self.repo()?.tags(prefix).map_err(said)?;
        Ok(Snapshots {
            snapshots: tags
                .into_iter()
                .filter_map(|t| match t.kind {
                    // A lightweight tag has no label or date, so it is not a
                    // snapshot, as on GitHub.
                    TagKind::Lightweight => None,
                    TagKind::Annotated {
                        tagger, message, ..
                    } => Some(TagFacts {
                        tag: t.name.as_str().to_owned(),
                        label: message_of(&message),
                        commit: t.commit.to_hex(),
                        tagged_at: tagger.map(|s| s.when.seconds()),
                    }),
                })
                .collect(),
        })
    }

    pub fn take_snapshot(
        &mut self,
        tag: &str,
        label: &str,
        commit: &str,
        who: &Who,
    ) -> Out<Tagged> {
        let name = TagName::new(tag).map_err(|e| e.to_string())?;
        let target = CommitId::from_hex(commit).map_err(|e| e.to_string())?;
        let tagger = signature(who)?;
        match self
            .repo_mut()?
            .create_tag(&name, &target, &tagger, &git_message(label))
        {
            Ok(_) => Ok(Tagged::Tagged {
                snapshot: TagFacts {
                    tag: tag.to_owned(),
                    label: message_of(label),
                    commit: target.to_hex(),
                    tagged_at: Some(who.seconds),
                },
            }),
            Err(Error::Conflict { .. } | Error::RefClash { .. }) => Ok(Tagged::Conflict {
                message: format!("there is already a snapshot {tag}"),
            }),
            Err(e) => Err(said(e)),
        }
    }

    /// Removes the tag; one that is already gone is what was asked.
    pub fn drop_snapshot(&mut self, tag: &str) -> Out<()> {
        let name = TagName::new(tag).map_err(|e| e.to_string())?;
        match self.repo_mut()?.delete_tag(&name) {
            Ok(()) | Err(Error::NoSuchTag(_)) => Ok(()),
            Err(e) => Err(said(e)),
        }
    }

    /// One commit on the default branch writing `text` to `path`, guarded as
    /// GitHub's Contents API guards a `PUT`: `sha` must be the file's blob id
    /// at the tip, and `None` means the file must not exist. The tip may have
    /// moved for other files; it is the file that must not have.
    ///
    /// On `branch` when given, the default branch otherwise. A branch that
    /// does not exist yet is made by a write that creates its file: a root
    /// commit, or with `from`, a commit whose one parent is `from` and whose
    /// tree is the file alone, which is how a deck is forked (ADR-0041).
    #[allow(clippy::too_many_arguments)]
    pub fn put_file(
        &mut self,
        path: &str,
        text: &str,
        sha: Option<&str>,
        message: &str,
        who: &Who,
        branch: Option<&str>,
        from: Option<&str>,
    ) -> Out<Wrote> {
        let path = repo_path(path)?;
        let (branch_name, tip) = self.tip(branch)?;
        if let (None, Some(from)) = (tip, from) {
            if sha.is_some() {
                return Ok(Wrote::Conflict {
                    message: format!("{branch_name} does not exist"),
                });
            }
            let from = match self.commit_at(Some(from))? {
                Some(c) if CommitId::from_hex(from).is_ok() => c,
                _ => return Err(format!("{from} is no commit here")),
            };
            let signed = signature(who)?;
            let repo = self.repo_mut()?;
            let blob = repo.write_blob(text.as_bytes()).map_err(said)?;
            let tree = repo.write_files(&[(path.clone(), blob)]).map_err(said)?;
            let id = repo
                .write_commit_of(tree, &[from], &signed, &signed, &git_message(message))
                .map_err(said)?;
            return match repo.update_branch(&branch_name, Tip::Unborn, &id) {
                Ok(()) => Ok(Wrote::Committed {
                    sha: Some(blob.to_hex()),
                    commit: id.to_hex(),
                }),
                Err(Error::Conflict { .. } | Error::RefClash { .. }) => Ok(Wrote::Conflict {
                    message: format!("{branch_name} was made meanwhile"),
                }),
                Err(e) => Err(said(e)),
            };
        }
        let change = Change::Put {
            path: path.clone(),
            bytes: text.as_bytes().to_vec(),
        };
        self.guarded(&path, sha, change, message, who, branch)
    }

    /// One commit removing `path`, whose blob id must be `sha`.
    pub fn delete_file(&mut self, path: &str, sha: &str, message: &str, who: &Who) -> Out<Wrote> {
        let path = repo_path(path)?;
        let change = Change::Delete { path: path.clone() };
        self.guarded(&path, Some(sha), change, message, who, None)
    }

    fn guarded(
        &mut self,
        path: &RepoPath,
        sha: Option<&str>,
        change: Change,
        message: &str,
        who: &Who,
        branch: Option<&str>,
    ) -> Out<Wrote> {
        let (branch, tip) = self.tip(branch)?;
        let now = match &tip {
            Some(t) => self.blob_at(t, path)?,
            None => None,
        };
        let deleting = matches!(change, Change::Delete { .. });
        let expected = sha.map(BlobId::from_hex).transpose().ok().flatten();
        let stale = match (sha, now) {
            (None, None) => false,
            (None, Some(_)) => true,
            (Some(_), now) => now.is_none() || now != expected,
        };
        if stale {
            return Ok(Wrote::Conflict {
                message: format!("{path} changed in the repo since it was read"),
            });
        }
        let signed = signature(who)?;
        let commit = WriteCommit {
            changes: vec![change],
            message: git_message(message),
            author: signed.clone(),
            committer: signed,
        };
        let at = tip.map_or(Tip::Unborn, Tip::At);
        let id = match self.repo_mut()?.commit(&branch, at, &commit) {
            Ok(id) => id,
            Err(Error::Conflict { .. } | Error::RefClash { .. }) => {
                return Ok(Wrote::Conflict {
                    message: format!("{branch} moved while {path} was being committed"),
                })
            }
            Err(e) => return Err(said(e)),
        };
        let sha = if deleting {
            None
        } else {
            self.blob_at(&id, path)?.map(|b| b.to_hex())
        };
        Ok(Wrote::Committed {
            sha,
            commit: id.to_hex(),
        })
    }

    /// Every deck, `decks/*/main`, by stem: its text and the deck it was
    /// forked from.
    pub fn decks(&self) -> Out<DeckLineList> {
        let repo = self.repo()?;
        let lines = DeckLines::read(repo)?;
        let parents = lines.parents(repo)?;
        let mut decks = Vec::new();
        for (stem, tip) in &lines.tips {
            let file = repo_path(&stem.deck_file())?;
            let (text, sha) = match self.blob_at(tip, &file)? {
                Some(blob) => {
                    let bytes = repo.read_blob(&blob).map_err(said)?;
                    (
                        Some(String::from_utf8_lossy(&bytes).into_owned()),
                        Some(blob.to_hex()),
                    )
                }
                None => (None, None),
            };
            decks.push(DeckLine {
                stem: stem.to_string(),
                commit: tip.to_hex(),
                text,
                sha,
                parent: parents.get(stem).map(|p| p.to_string()),
            });
        }
        Ok(DeckLineList { decks })
    }

    /// Deletes the deck `stem`: every branch of it, and its snapshots, as
    /// long as its `main` is still at `commit`. Its commits stay, as git
    /// keeps what nothing names, and a remote loses them at the next sync.
    pub fn delete_deck(&mut self, stem: &str, commit: &str) -> Out<DeckDeleted> {
        let stem = DeckStem::new(stem).map_err(|e| e.to_string())?;
        let main = layout::main_line(&stem);
        let expected = CommitId::from_hex(commit).map_err(|e| e.to_string())?;
        let repo = self.repo_mut()?;
        if repo.branch_tip(&main).map_err(said)? != Some(expected) {
            return Ok(DeckDeleted::Conflict {
                message: format!("{stem} changed since it was read"),
            });
        }
        let prefix = format!("decks/{stem}/");
        for (branch, tip) in repo.branches().map_err(said)? {
            if branch.as_str().starts_with(&prefix) {
                repo.delete_branch(&branch, &tip).map_err(said)?;
            }
        }
        for tag in repo.tags(&prefix).map_err(said)? {
            repo.delete_tag(&tag.name).map_err(said)?;
        }
        Ok(DeckDeleted::Deleted)
    }

    /// Migrates the repo from version `from` to the next, in one operation
    /// that leaves it at `from` if it fails; see [`layout::migrate`].
    pub fn migrate(&mut self, from: u32) -> Out<MigratedFacts> {
        if from != 1 {
            return Err(format!("there is no migration from version {from}"));
        }
        let done = layout::migrate(self.repo_mut()?)?;
        let n = |x: usize| u32::try_from(x).unwrap_or(u32::MAX);
        Ok(MigratedFacts {
            version: layout::VERSION,
            decks: n(done.decks),
            snapshots: n(done.snapshots),
        })
    }

    pub fn remote(&self) -> Out<RemoteFacts> {
        let repo = self.repo()?;
        let url = repo.remote().map_err(said)?;
        let status = match url {
            Some(_) => repo.remote_status().map_err(said)?,
            None => Default::default(),
        };
        let n = |x: usize| u32::try_from(x).unwrap_or(u32::MAX);
        Ok(RemoteFacts {
            url: url.map(|u| u.as_str().to_owned()),
            ahead: n(status.ahead),
            behind: n(status.behind),
            branches: n(status.branches),
            tags: n(status.tags),
        })
    }

    pub fn set_remote(&mut self, url: &str) -> Out<()> {
        let url = RemoteUrl::new(url).map_err(said)?;
        self.repo_mut()?.set_remote(&url).map_err(said)
    }

    pub fn remove_remote(&mut self) -> Out<()> {
        self.repo_mut()?.remove_remote().map_err(said)
    }

    /// The body of `POST git-upload-pack`, or `None` when this repo holds
    /// everything the advertisement names.
    pub fn fetch_request(&self, advertised: &[u8]) -> Out<Option<Vec<u8>>> {
        match self.repo()?.fetch_request(advertised).map_err(said)? {
            FetchRequest::HaveAll => Ok(None),
            FetchRequest::Want(body) => Ok(Some(body)),
        }
    }

    pub fn fetch_finish(&mut self, advertised: &[u8], response: Option<&[u8]>) -> Out<()> {
        self.repo_mut()?
            .fetch_finish(advertised, response)
            .map_err(said)
    }

    /// Brings the fetched branches in, a remote still at repo version 1
    /// migrated first ([`layout::integrate`]). A path both sides changed
    /// merges by lines, and only when what that makes is still a file
    /// Progress Engine reads, as a refused save merges (ADR-0021).
    pub fn integrate(&mut self, who: &Who, prefer: Option<Side>) -> Out<Synced> {
        let signed = signature(who)?;
        let mut merge_file = |path: &str, base: &[u8], ours: &[u8], theirs: &[u8]| {
            fn text(b: &[u8]) -> Option<&str> {
                std::str::from_utf8(b).ok()
            }
            let merged = meldweb_git::merge_lines(text(base)?, text(ours)?, text(theirs)?)?;
            crate::still_reads(path, text(theirs)?, &merged).then(|| merged.into_bytes())
        };
        let outcomes = layout::integrate(self.repo_mut()?, &signed, prefer, &mut merge_file)?;
        Ok(synced(outcomes))
    }

    pub fn adopt_remote(&mut self) -> Out<()> {
        self.repo_mut()?.adopt_remote().map_err(said)
    }

    pub fn push_request(&self, advertised: &[u8]) -> Out<Outgoing> {
        Ok(match self.repo()?.push_request(advertised).map_err(said)? {
            PushRequest::Nothing => Outgoing::new("nothing", vec![]),
            PushRequest::Behind => Outgoing::new("behind", vec![]),
            PushRequest::Send(body) => Outgoing::new("send", body),
        })
    }

    pub fn push_finish(&mut self, sent: &[u8], response: &[u8]) -> Out<PushOutcome> {
        let pushed = self.repo_mut()?.push_finish(sent, response).map_err(said)?;
        Ok(PushOutcome {
            updated: pushed.updated,
            rejected: pushed
                .rejected
                .into_iter()
                .map(|(reference, reason)| Refused { reference, reason })
                .collect(),
        })
    }

    /// The bare repo as a zip that `unzip` and then `git` read: every file,
    /// and the directories git needs even when they are empty.
    pub fn export_zip(&self) -> Out<Vec<u8>> {
        let repo = self.repo()?;
        let mirror = repo.store();
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        let mut dirs: BTreeSet<String> = REPO_DIRS.iter().map(|d| (*d).to_owned()).collect();
        dirs.extend(mirror.files.dirs().map(str::to_owned));
        for dir in &dirs {
            // A directory without x cannot be entered, and git would read
            // nothing under it.
            zip.add_directory(format!("{dir}/"), options.unix_permissions(0o755))
                .map_err(|e| e.to_string())?;
        }
        for (path, bytes) in mirror.files.files() {
            // Loose objects are already zlib, so deflating them again only
            // costs time.
            let stored = if path.starts_with("objects/") {
                options.compression_method(zip::CompressionMethod::Stored)
            } else {
                options
            };
            zip.start_file(path, stored).map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
        }
        Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
    }

    /// A zip of a bare repo, as [`Local::export_zip`] makes it or as `zip -r`
    /// makes one of a bare repo's directory, as a new `Local` whose journal
    /// holds every file, for the worker to write to fresh storage. Refuses a
    /// zip that is no repo, and a repo `meldweb-git` cannot read.
    pub fn from_zip(bytes: &[u8]) -> Out<Local> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| format!("That is not a zip file: {e}"))?;
        let mut entries: Vec<(String, Option<Vec<u8>>)> = Vec::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
            let Some(name) = file.enclosed_name() else {
                return Err(format!(
                    "The zip names a path outside itself: {}",
                    String::from_utf8_lossy(file.name_raw())
                ));
            };
            let name = name
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if file.is_dir() {
                entries.push((name, None));
            } else {
                let mut data = Vec::new();
                file.read_to_end(&mut data).map_err(|e| e.to_string())?;
                entries.push((name, Some(data)));
            }
        }
        // The git dir is where HEAD sits beside both objects/ and refs/, as
        // `git` itself requires: the zip's root, the one directory
        // `zip -r x.zip repo.git` puts it in, or a clone's `.git/`. Its
        // `logs/HEAD` sits beside `logs/refs/` but no objects.
        let roots: Vec<String> = entries
            .iter()
            .filter_map(|(name, data)| {
                data.as_ref()?;
                let root = name.strip_suffix("HEAD")?;
                (root.is_empty() || root.ends_with('/')).then(|| root.to_owned())
            })
            .filter(|root| {
                let has = |dir: &str| {
                    let dir = format!("{root}{dir}");
                    entries.iter().any(|(n, _)| {
                        n.strip_prefix(&dir)
                            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
                    })
                };
                has("objects") && has("refs")
            })
            .collect();
        let root = match roots.as_slice() {
            [root] => root.clone(),
            [] => {
                return Err(
                    "That zip holds no git repo: there is no HEAD beside objects/ and refs/."
                        .to_owned(),
                )
            }
            _ => return Err("That zip holds more than one git repo.".to_owned()),
        };
        let mut store = Mirror::default();
        for (name, data) in entries {
            let Some(path) = name.strip_prefix(&root) else {
                continue;
            };
            let path = path.trim_end_matches('/');
            match data {
                None if path.starts_with("objects/") || path.starts_with("refs/") => {
                    let _ = store.create_dir(path);
                }
                Some(data) if is_repo_file(path) => {
                    let data = if path == "config" {
                        bare_config(&data)
                    } else {
                        data
                    };
                    let _ = store.write(path, &data);
                }
                _ => {}
            }
        }
        for dir in REPO_DIRS {
            let _ = store.create_dir(dir);
        }
        if store.read("packed-refs").ok().flatten().is_some() {
            return Err(said(Error::Unsupported(Unsupported::PackedRefs)));
        }
        if !store.list("objects/pack").unwrap_or_default().is_empty() {
            return Err(said(Error::Unsupported(Unsupported::Packfile)));
        }
        let repo = Repo::open(store).map_err(said)?;
        // Every ref and the commit it names must read, so a repo this crate
        // cannot use is refused here rather than on the first page.
        let head = repo.head_branch().map_err(said)?;
        for (_, tip) in repo.branches().map_err(said)? {
            repo.read_commit(&tip).map_err(said)?;
        }
        repo.tags("").map_err(said)?;
        if let Some(tip) = repo.branch_tip(&head).map_err(said)? {
            repo.read_commit(&tip).map_err(said)?;
        }
        Ok(Local {
            repo: Some(repo),
            loading: Mirror::default(),
        })
    }
}

/// A clone's config says `bare = false`, which would make `git` look for a
/// working tree beside the exported repo; this repo is bare.
fn bare_config(data: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(data) {
        Ok(text) => text
            .lines()
            .map(|l| {
                if l.trim().replace(' ', "") == "bare=false" {
                    "\tbare = true".to_owned()
                } else {
                    l.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes()
            .into_iter()
            .chain(*b"\n")
            .collect(),
        Err(_) => data.to_vec(),
    }
}

fn json<T: for<'a> Facet<'a>>(value: &T) -> String {
    facet_json::to_string(value).expect("the wire types serialise")
}

fn who_of(who: &str) -> Result<Who, JsError> {
    facet_json::from_str(who).map_err(|e| JsError::new(&format!("who is not a Who: {e}")))
}

fn js<T>(r: Out<T>) -> Result<T, JsError> {
    r.map_err(|e| JsError::new(&e))
}

/// The repo as one tab's worker holds it. See the module's docs.
#[wasm_bindgen]
#[derive(Default)]
pub struct GitRepo(Local);

#[wasm_bindgen]
impl GitRepo {
    #[wasm_bindgen(constructor)]
    pub fn new() -> GitRepo {
        GitRepo::default()
    }

    /// A zip of a bare repo, as a `GitRepo` whose journal holds every file.
    pub fn from_zip(bytes: &[u8]) -> Result<GitRepo, JsError> {
        js(Local::from_zip(bytes)).map(GitRepo)
    }

    pub fn load(&mut self, path: &str, bytes: &[u8]) {
        self.0.load(path, bytes);
    }

    pub fn load_dir(&mut self, path: &str) {
        self.0.load_dir(path);
    }

    pub fn open(&mut self) -> bool {
        self.0.open()
    }

    pub fn init(&mut self) -> Result<(), JsError> {
        js(self.0.init())
    }

    pub fn has(&self, path: &str) -> bool {
        self.0.has(path)
    }

    pub fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.0.bytes(path)
    }

    /// `paths[i]`'s bytes are `contents[i]`, every ref file storage holds.
    pub fn replace_refs(&mut self, paths: Vec<String>, contents: Vec<String>) -> bool {
        let refs: Vec<(String, Vec<u8>)> = paths
            .into_iter()
            .zip(contents)
            .map(|(p, c)| (p, c.into_bytes()))
            .collect();
        self.0.replace_refs(&refs)
    }

    /// [`Journal`].
    pub fn take_journal(&mut self) -> String {
        json(&self.0.take_journal())
    }

    /// [`FileRead`].
    pub fn get_file(&self, path: &str, at: Option<String>) -> Result<String, JsError> {
        js(self.0.get_file(path, at.as_deref())).map(|r| json(&r))
    }

    pub fn get_bytes(&self, path: &str) -> Result<Option<Vec<u8>>, JsError> {
        js(self.0.get_bytes(path))
    }

    /// [`Listing`].
    pub fn list_dir(&self, path: &str) -> Result<String, JsError> {
        js(self.0.list_dir(path)).map(|r| json(&r))
    }

    /// [`Revisions`] on `branch` (default: `main`). `until` is seconds
    /// since the Unix epoch.
    pub fn history(
        &self,
        path: &str,
        branch: Option<String>,
        until: Option<f64>,
        page: u32,
        per_page: u32,
    ) -> Result<String, JsError> {
        js(self.0.history(
            path,
            branch.as_deref(),
            until.map(|u| u as i64),
            page,
            per_page,
        ))
        .map(|r| json(&r))
    }

    /// [`RevisionRead`].
    pub fn revision(&self, commit: &str) -> Result<String, JsError> {
        js(self.0.revision(commit)).map(|r| json(&r))
    }

    /// [`Snapshots`].
    pub fn snapshots(&self, prefix: &str) -> Result<String, JsError> {
        js(self.0.snapshots(prefix)).map(|r| json(&r))
    }

    /// [`Tagged`]; `who` is a [`Who`].
    pub fn take_snapshot(
        &mut self,
        tag: &str,
        label: &str,
        commit: &str,
        who: &str,
    ) -> Result<String, JsError> {
        let who = who_of(who)?;
        js(self.0.take_snapshot(tag, label, commit, &who)).map(|r| json(&r))
    }

    pub fn drop_snapshot(&mut self, tag: &str) -> Result<(), JsError> {
        js(self.0.drop_snapshot(tag))
    }

    /// [`Wrote`]; `who` is a [`Who`]. On `branch` (default: `main`); a
    /// branch made by this write starts from the commit `from` when given.
    #[allow(clippy::too_many_arguments)]
    pub fn put_file(
        &mut self,
        path: &str,
        text: &str,
        sha: Option<String>,
        message: &str,
        who: &str,
        branch: Option<String>,
        from: Option<String>,
    ) -> Result<String, JsError> {
        let who = who_of(who)?;
        js(self.0.put_file(
            path,
            text,
            sha.as_deref(),
            message,
            &who,
            branch.as_deref(),
            from.as_deref(),
        ))
        .map(|r| json(&r))
    }

    /// [`DeckLineList`].
    pub fn decks(&self) -> Result<String, JsError> {
        js(self.0.decks()).map(|r| json(&r))
    }

    /// [`DeckDeleted`].
    pub fn delete_deck(&mut self, stem: &str, commit: &str) -> Result<String, JsError> {
        js(self.0.delete_deck(stem, commit)).map(|r| json(&r))
    }

    /// [`MigratedFacts`].
    pub fn migrate(&mut self, from: u32) -> Result<String, JsError> {
        js(self.0.migrate(from)).map(|r| json(&r))
    }

    /// [`Wrote`]; `who` is a [`Who`].
    pub fn delete_file(
        &mut self,
        path: &str,
        sha: &str,
        message: &str,
        who: &str,
    ) -> Result<String, JsError> {
        let who = who_of(who)?;
        js(self.0.delete_file(path, sha, message, &who)).map(|r| json(&r))
    }

    pub fn export_zip(&self) -> Result<Vec<u8>, JsError> {
        js(self.0.export_zip())
    }

    /// [`RemoteFacts`].
    pub fn remote(&self) -> Result<String, JsError> {
        js(self.0.remote()).map(|r| json(&r))
    }

    pub fn set_remote(&mut self, url: &str) -> Result<(), JsError> {
        js(self.0.set_remote(url))
    }

    pub fn remove_remote(&mut self) -> Result<(), JsError> {
        js(self.0.remove_remote())
    }

    /// The body to POST to `git-upload-pack`, or nothing when no POST is
    /// needed; either way [`GitRepo::fetch_finish`] follows.
    pub fn fetch_request(&self, advertised: &[u8]) -> Result<Option<Vec<u8>>, JsError> {
        js(self.0.fetch_request(advertised))
    }

    pub fn fetch_finish(
        &mut self,
        advertised: &[u8],
        response: Option<Vec<u8>>,
    ) -> Result<(), JsError> {
        js(self.0.fetch_finish(advertised, response.as_deref()))
    }

    /// [`Synced`]; `who` is a [`Who`], `prefer` `"ours"` or `"theirs"` for a
    /// conflict the user settled.
    pub fn integrate(&mut self, who: &str, prefer: Option<String>) -> Result<String, JsError> {
        let who = who_of(who)?;
        let prefer = match prefer.as_deref() {
            None => None,
            Some("ours") => Some(Side::Ours),
            Some("theirs") => Some(Side::Theirs),
            Some(other) => return Err(JsError::new(&format!("{other} is not a side"))),
        };
        js(self.0.integrate(&who, prefer)).map(|r| json(&r))
    }

    pub fn adopt_remote(&mut self) -> Result<(), JsError> {
        js(self.0.adopt_remote())
    }

    pub fn push_request(&self, advertised: &[u8]) -> Result<Outgoing, JsError> {
        js(self.0.push_request(advertised))
    }

    /// [`PushOutcome`].
    pub fn push_finish(&mut self, sent: &[u8], response: &[u8]) -> Result<String, JsError> {
        js(self.0.push_finish(sent, response)).map(|r| json(&r))
    }
}

/// What a push's ref advertisement came to: `kind` is `"nothing"`,
/// `"behind"` (fetch and integrate first) or `"send"`, with `body` the
/// request to POST to `git-receive-pack`.
#[wasm_bindgen]
#[derive(Debug)]
pub struct Outgoing {
    kind: String,
    body: Vec<u8>,
}

impl Outgoing {
    fn new(kind: &str, body: Vec<u8>) -> Self {
        Self {
            kind: kind.to_owned(),
            body,
        }
    }
}

#[wasm_bindgen]
impl Outgoing {
    #[wasm_bindgen(getter)]
    pub fn kind(&self) -> String {
        self.kind.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn body(&self) -> Vec<u8> {
        self.body.clone()
    }
}

/// Why git would not take `name` and `email` as a commit's author, or `None`
/// when it would: the settings page asks before keeping them.
#[wasm_bindgen]
pub fn identity_problem(name: &str, email: &str) -> Option<String> {
    Identity::new(name, email).err().map(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who(seconds: i64) -> Who {
        Who {
            name: "Alex".to_owned(),
            email: "alex@example.com".to_owned(),
            seconds,
            offset_minutes: 120,
        }
    }

    fn fresh() -> Local {
        let mut local = Local::default();
        assert!(!local.open(), "nothing loaded is no repo");
        local.init().unwrap();
        local
    }

    fn committed(w: Wrote) -> (Option<String>, String) {
        match w {
            Wrote::Committed { sha, commit } => (sha, commit),
            Wrote::Conflict { message } => panic!("conflict: {message}"),
        }
    }

    fn file(local: &Local, path: &str) -> Option<(String, String)> {
        match local.get_file(path, None).unwrap() {
            FileRead::File { text, sha } => Some((text, sha)),
            FileRead::NoFile => None,
        }
    }

    /// Plays the worker: the journal's files copied into a second tab's
    /// storage, in order.
    fn write_through(from: &mut Local, to: &mut Vec<(String, Vec<u8>)>) {
        for step in from.take_journal().steps {
            match step {
                Step::Write { path } => {
                    let bytes = from.bytes(&path).unwrap();
                    to.retain(|(p, _)| *p != path);
                    to.push((path, bytes));
                }
                Step::Remove { path } => to.retain(|(p, _)| *p != path),
                Step::Dir { .. } => {}
            }
        }
    }

    #[test]
    fn a_write_is_guarded_by_the_files_blob_and_not_the_tip() {
        let mut local = fresh();
        assert_eq!(file(&local, "VERSION"), None);
        let (sha, _) = committed(
            local
                .put_file("VERSION", "1\n", None, "v1", &who(1), None, None)
                .unwrap(),
        );
        // `printf '1\n' | git hash-object --stdin`
        assert_eq!(
            sha.as_deref(),
            Some("d00491fd7e5bb6fa28c517a0bb32b8b506539d4d")
        );
        assert!(matches!(
            local
                .put_file("VERSION", "1\n", None, "again", &who(2), None, None)
                .unwrap(),
            Wrote::Conflict { .. }
        ));

        let (deck, _) = committed(
            local
                .put_file(
                    "decks/a.deck.toml",
                    "name = \"A\"\n",
                    None,
                    "a: made",
                    &who(3),
                    None,
                    None,
                )
                .unwrap(),
        );
        // The tip moved for another file; VERSION's blob still guards it.
        committed(
            local
                .put_file(
                    "VERSION",
                    "1\n",
                    sha.as_deref(),
                    "same",
                    &who(4),
                    None,
                    None,
                )
                .unwrap(),
        );
        let (deck2, _) = committed(
            local
                .put_file(
                    "decks/a.deck.toml",
                    "name = \"B\"\n",
                    deck.as_deref(),
                    "a: B",
                    &who(5),
                    None,
                    None,
                )
                .unwrap(),
        );
        // A stale blob is a conflict, and nothing moves.
        let before = local.head().unwrap().1;
        assert!(matches!(
            local
                .put_file(
                    "decks/a.deck.toml",
                    "x",
                    deck.as_deref(),
                    "stale",
                    &who(6),
                    None,
                    None
                )
                .unwrap(),
            Wrote::Conflict { .. }
        ));
        assert_eq!(local.head().unwrap().1, before);
        assert!(matches!(
            local
                .delete_file(
                    "decks/a.deck.toml",
                    deck.as_deref().unwrap(),
                    "gone",
                    &who(7)
                )
                .unwrap(),
            Wrote::Conflict { .. }
        ));
        let (none, _) = committed(
            local
                .delete_file(
                    "decks/a.deck.toml",
                    deck2.as_deref().unwrap(),
                    "a: deleted",
                    &who(8),
                )
                .unwrap(),
        );
        assert_eq!(none, None);
        assert_eq!(file(&local, "decks/a.deck.toml"), None);
        // A missing file deletes as a conflict, as GitHub's 404 does.
        assert!(matches!(
            local
                .delete_file(
                    "decks/a.deck.toml",
                    deck2.as_deref().unwrap(),
                    "again",
                    &who(9)
                )
                .unwrap(),
            Wrote::Conflict { .. }
        ));
    }

    #[test]
    fn history_pages_newest_first_and_stops_at_until() {
        let mut local = fresh();
        let mut sha = None;
        for i in 1..=5 {
            let (s, _) = committed(
                local
                    .put_file(
                        "d.deck.toml",
                        &format!("{i}\n"),
                        sha.as_deref(),
                        &format!("edit {i}"),
                        &who(i * 100),
                        None,
                        None,
                    )
                    .unwrap(),
            );
            sha = s;
            local
                .put_file(
                    &format!("other{i}"),
                    "x",
                    None,
                    "other",
                    &who(i * 100 + 1),
                    None,
                    None,
                )
                .unwrap();
        }
        let all = local
            .history("d.deck.toml", None, None, 1, 100)
            .unwrap()
            .revisions;
        let messages: Vec<_> = all.iter().map(|r| r.message.as_str()).collect();
        assert_eq!(messages, ["edit 5", "edit 4", "edit 3", "edit 2", "edit 1"]);
        assert_eq!(all[0].author, "Alex");
        assert_eq!(all[0].committed_at, 500);
        let page2 = local
            .history("d.deck.toml", None, None, 2, 2)
            .unwrap()
            .revisions;
        assert_eq!(
            page2.iter().map(|r| r.message.as_str()).collect::<Vec<_>>(),
            ["edit 3", "edit 2"]
        );
        let until = local
            .history("d.deck.toml", None, Some(350), 1, 1)
            .unwrap()
            .revisions;
        assert_eq!(until[0].message, "edit 3");

        let RevisionRead::Found { revision } = local.revision(&all[2].commit).unwrap() else {
            panic!("the commit is there")
        };
        assert_eq!(revision.message, "edit 3");
        assert!(matches!(
            local.revision("nope").unwrap(),
            RevisionRead::Missing
        ));
        assert!(matches!(
            local
                .revision("0000000000000000000000000000000000000000")
                .unwrap(),
            RevisionRead::Missing
        ));
        let FileRead::File { text, .. } =
            local.get_file("d.deck.toml", Some(&all[2].commit)).unwrap()
        else {
            panic!("the file was there then")
        };
        assert_eq!(text, "3\n");
    }

    #[test]
    fn a_snapshot_is_an_annotated_tag_and_a_taken_name_conflicts() {
        let mut local = fresh();
        let (_, commit) = committed(
            local
                .put_file("d", "1", None, "one", &who(10), None, None)
                .unwrap(),
        );
        let Tagged::Tagged { snapshot } = local
            .take_snapshot("decks/d/fnm", "FNM 14 Sep", &commit, &who(20))
            .unwrap()
        else {
            panic!("refused")
        };
        assert_eq!(snapshot.label, "FNM 14 Sep");
        assert!(matches!(
            local
                .take_snapshot("decks/d/fnm", "again", &commit, &who(21))
                .unwrap(),
            Tagged::Conflict { .. }
        ));
        let listed = local.snapshots("decks/d/").unwrap().snapshots;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].label, "FNM 14 Sep");
        assert_eq!(listed[0].tagged_at, Some(20));
        assert_eq!(listed[0].commit, commit);
        assert!(local.snapshots("decks/e/").unwrap().snapshots.is_empty());
        local.drop_snapshot("decks/d/fnm").unwrap();
        local.drop_snapshot("decks/d/fnm").unwrap();
        assert!(local.snapshots("").unwrap().snapshots.is_empty());
    }

    #[test]
    fn listing_names_each_entry_by_its_path_and_kind() {
        let mut local = fresh();
        assert!(local.list_dir("decks").unwrap().entries.is_empty());
        local
            .put_file("decks/a.deck.toml", "a", None, "a", &who(1), None, None)
            .unwrap();
        local
            .put_file("decks/sub/b", "b", None, "b", &who(2), None, None)
            .unwrap();
        let entries = local.list_dir("decks").unwrap().entries;
        let seen: Vec<_> = entries.iter().map(|e| (e.path.as_str(), e.kind)).collect();
        assert_eq!(
            seen,
            [
                ("decks/a.deck.toml", EntryType::File),
                ("decks/sub", EntryType::Dir)
            ]
        );
        assert!(local
            .list_dir("decks/a.deck.toml")
            .unwrap()
            .entries
            .is_empty());
        assert_eq!(local.list_dir("").unwrap().entries.len(), 1);
    }

    /// Two tabs: each holds a mirror, and storage is what the journals wrote.
    #[test]
    fn a_tab_that_takes_the_other_tabs_refs_sees_its_commit_and_conflicts_on_its_file() {
        let mut storage = Vec::new();
        let mut a = fresh();
        let (sha, _) = committed(
            a.put_file("d", "1\n", None, "one", &who(1), None, None)
                .unwrap(),
        );
        write_through(&mut a, &mut storage);

        let mut b = Local::default();
        for (p, bytes) in &storage {
            b.load(p, bytes);
        }
        assert!(b.open());
        assert_eq!(file(&b, "d").unwrap().1, sha.clone().unwrap());

        committed(
            a.put_file("d", "2\n", sha.as_deref(), "two", &who(2), None, None)
                .unwrap(),
        );
        write_through(&mut a, &mut storage);

        // b re-reads refs, as the worker does inside the lock, then loads
        // the objects it lacks.
        let refs: Vec<_> = storage
            .iter()
            .filter(|(p, _)| is_ref_file(p))
            .cloned()
            .collect();
        assert!(b.replace_refs(&refs));
        assert!(!b.replace_refs(&refs), "the same refs again change nothing");
        for (p, bytes) in &storage {
            if !b.has(p) {
                b.load(p, bytes);
            }
        }
        assert!(b.take_journal().steps.is_empty(), "loading is not writing");
        assert!(matches!(
            b.put_file("d", "3\n", sha.as_deref(), "three", &who(3), None, None)
                .unwrap(),
            Wrote::Conflict { .. }
        ));
        assert_eq!(file(&b, "d").unwrap().0, "2\n");
    }

    #[test]
    fn the_journal_writes_objects_before_the_ref_that_names_them() {
        let mut local = fresh();
        local.take_journal();
        local
            .put_file("d", "1", None, "one", &who(1), None, None)
            .unwrap();
        let steps = local.take_journal().steps;
        let last = steps.last().unwrap();
        assert_eq!(
            *last,
            Step::Write {
                path: "refs/heads/main".to_owned()
            }
        );
        assert!(steps[..steps.len() - 1]
            .iter()
            .all(|s| matches!(s, Step::Write { path } if path.starts_with("objects/"))));
    }

    #[test]
    fn an_exported_zip_imports_as_the_same_repo() {
        let mut local = fresh();
        let (_, commit) = committed(
            local
                .put_file("VERSION", "1\n", None, "v1", &who(1), None, None)
                .unwrap(),
        );
        local
            .take_snapshot("decks/d/x", "X", &commit, &who(2))
            .unwrap();
        let zip = local.export_zip().unwrap();
        let back = Local::from_zip(&zip).unwrap();
        assert_eq!(file(&back, "VERSION"), file(&local, "VERSION"));
        assert_eq!(back.snapshots("").unwrap().snapshots.len(), 1);
        for (path, bytes) in local.mirror().files.files() {
            assert_eq!(back.bytes(path).as_deref(), Some(bytes), "{path}");
        }
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, bytes) in files {
            if path.ends_with('/') {
                zip.add_directory(*path, zip::write::SimpleFileOptions::default())
                    .unwrap();
            } else {
                zip.start_file(*path, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(bytes).unwrap();
            }
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn an_import_finds_the_repo_in_one_directory_and_refuses_packs() {
        let mut local = fresh();
        local
            .put_file("VERSION", "1\n", None, "v1", &who(1), None, None)
            .unwrap();
        let mut files: Vec<(String, Vec<u8>)> = local
            .mirror()
            .files
            .files()
            .map(|(p, b)| (format!("mtg.git/{p}"), b.to_vec()))
            .collect();
        files.retain(|(p, _)| p != "mtg.git/config");
        files.push(("mtg.git/logs/HEAD".to_owned(), b"0000 1111 x\n".to_vec()));
        files.push((
            "mtg.git/logs/refs/heads/main".to_owned(),
            b"0000 1111 x\n".to_vec(),
        ));
        files.push(("mtg.git/hooks/pre-commit".to_owned(), b"#!/bin/sh".to_vec()));
        files.push((
            "mtg.git/config".to_owned(),
            b"[core]\n\tbare = false\n".to_vec(),
        ));
        let borrowed: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(p, b)| (p.as_str(), b.as_slice()))
            .collect();
        let back = Local::from_zip(&zip_of(&borrowed)).unwrap();
        assert!(file(&back, "VERSION").is_some());
        assert!(
            !back.has("hooks/pre-commit"),
            "only the repo's own files come in"
        );
        assert_eq!(back.bytes("config").unwrap(), b"[core]\n\tbare = true\n");

        let mut packed = borrowed.clone();
        packed.push(("mtg.git/packed-refs", b"# pack-refs with: peeled\n"));
        assert!(Local::from_zip(&zip_of(&packed))
            .unwrap_err()
            .contains("packed-refs"));
        let mut pack = borrowed.clone();
        pack.push(("mtg.git/objects/pack/pack-1.pack", b"PACK"));
        assert!(Local::from_zip(&zip_of(&pack))
            .unwrap_err()
            .contains("git unpack-objects"));
        assert!(Local::from_zip(b"not a zip")
            .unwrap_err()
            .contains("not a zip"));
        assert!(Local::from_zip(&zip_of(&[("readme", b"hi")]))
            .unwrap_err()
            .contains("no git repo"));
    }

    /// What the settings page promises: unzip the export, and `git` reads it.
    #[test]
    fn git_reads_an_unzipped_export_and_clones_it() {
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("SKIPPED: no git on PATH");
            return;
        }
        let mut local = fresh();
        let (sha, commit) = committed(
            local
                .put_file("VERSION", "1\n", None, "v1", &who(1), None, None)
                .unwrap(),
        );
        local
            .put_file("VERSION", "2\n", sha.as_deref(), "v2", &who(2), None, None)
            .unwrap();
        local
            .take_snapshot("decks/d/x", "X", &commit, &who(3))
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("r");
        zip::ZipArchive::new(Cursor::new(local.export_zip().unwrap()))
            .unwrap()
            .extract(&repo)
            .unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        };
        assert_eq!(git(&["-C", "r", "fsck", "--strict", "--no-dangling"]), "");
        assert_eq!(git(&["-C", "r", "log", "--format=%s", "main"]), "v2\nv1\n");
        git(&["clone", "-q", "r", "c"]);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("c/VERSION")).unwrap(),
            "2\n"
        );
    }

    #[test]
    fn a_line_merge_that_is_no_longer_a_deck_does_not_count() {
        let deck = "name = \"Lantern\"\ncards = []\n";
        assert!(crate::still_reads("lantern.deck.toml", deck, deck));
        assert!(!crate::still_reads(
            "lantern.deck.toml",
            deck,
            "name = \"Lantern\"\ncards = [\n"
        ));
        // A file no editor writes is only text.
        assert!(crate::still_reads("notes.txt", "a", "cards = ["));
    }

    /// Version 2's deck lines through the page's calls (ADR-0041): a deck is
    /// made on its own branch, forked from another's commit, saved, read at
    /// its branch or a commit, listed with its parent, and deleted whole.
    #[test]
    fn a_deck_is_a_line_its_fork_starts_on_its_commit_and_both_list_by_stem() {
        let mut local = fresh();
        committed(
            local
                .put_file("VERSION", "2\n", None, "v2", &who(1), None, None)
                .unwrap(),
        );
        let lantern = Some("decks/lantern/main");
        let (sha, made) = committed(
            local
                .put_file(
                    "lantern.deck.toml",
                    "name = \"Lantern\"\n",
                    None,
                    "lantern: made",
                    &who(2),
                    lantern,
                    None,
                )
                .unwrap(),
        );
        committed(
            local
                .put_file(
                    "lantern.deck.toml",
                    "name = \"Lantern\"\ncards = []\n",
                    sha.as_deref(),
                    "lantern: cards",
                    &who(3),
                    lantern,
                    None,
                )
                .unwrap(),
        );
        // A stale guard on a deck's line conflicts, as on main.
        assert!(matches!(
            local
                .put_file(
                    "lantern.deck.toml",
                    "x",
                    None,
                    "again",
                    &who(4),
                    lantern,
                    None
                )
                .unwrap(),
            Wrote::Conflict { .. }
        ));
        // Criteria live beside the deck, on its line.
        committed(
            local
                .put_file(
                    "lantern.criteria.toml",
                    "[[question]]\n",
                    None,
                    "lantern: criteria",
                    &who(5),
                    lantern,
                    None,
                )
                .unwrap(),
        );
        let budget = Some("decks/budget/main");
        let (_, forked) = committed(
            local
                .put_file(
                    "budget.deck.toml",
                    "name = \"Budget\"\n",
                    None,
                    "budget: forked from lantern",
                    &who(6),
                    budget,
                    Some(&made),
                )
                .unwrap(),
        );
        let decks = local.decks().unwrap().decks;
        let seen: Vec<_> = decks
            .iter()
            .map(|d| (d.stem.as_str(), d.parent.as_deref(), d.text.as_deref()))
            .collect();
        assert_eq!(
            seen,
            [
                ("budget", Some("lantern"), Some("name = \"Budget\"\n")),
                ("lantern", None, Some("name = \"Lantern\"\ncards = []\n")),
            ]
        );
        let FileRead::File { text, .. } = local.get_file("lantern.deck.toml", Some(&made)).unwrap()
        else {
            panic!("there then")
        };
        assert_eq!(text, "name = \"Lantern\"\n");
        let FileRead::File { text, .. } = local.get_file("lantern.criteria.toml", lantern).unwrap()
        else {
            panic!("criteria on the deck's line")
        };
        assert_eq!(text, "[[question]]\n");
        assert!(matches!(
            local.get_file("lantern.deck.toml", None).unwrap(),
            FileRead::NoFile
        ));
        let history = local
            .history("budget.deck.toml", budget, None, 1, 100)
            .unwrap()
            .revisions;
        assert_eq!(
            history
                .iter()
                .map(|r| r.message.as_str())
                .collect::<Vec<_>>(),
            ["budget: forked from lantern"]
        );
        let RevisionRead::Found { revision } = local.revision(&forked).unwrap() else {
            panic!("a commit")
        };
        assert_eq!(revision.message, "budget: forked from lantern");

        let lantern_tip = decks[1].commit.clone();
        local
            .take_snapshot("decks/lantern/fnm", "FNM", &lantern_tip, &who(7))
            .unwrap();
        assert!(matches!(
            local.delete_deck("lantern", &made).unwrap(),
            DeckDeleted::Conflict { .. }
        ));
        assert!(matches!(
            local.delete_deck("lantern", &lantern_tip).unwrap(),
            DeckDeleted::Deleted
        ));
        assert!(local
            .snapshots("decks/lantern/")
            .unwrap()
            .snapshots
            .is_empty());
        // The fork outlives its parent, and stands alone.
        let decks = local.decks().unwrap().decks;
        assert_eq!(decks.len(), 1);
        assert_eq!(
            (decks[0].stem.as_str(), decks[0].parent.as_deref()),
            ("budget", None)
        );
    }

    #[test]
    fn git_refuses_an_identity_that_would_not_survive_its_header() {
        assert_eq!(identity_problem("Curator", "curator@localhost"), None);
        assert!(identity_problem("A <b>", "x").is_some());
        assert!(identity_problem("", "x").is_some());
    }
}
