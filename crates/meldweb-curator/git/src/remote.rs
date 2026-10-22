//! One remote, `origin`, over git's smart HTTP protocol (gitprotocol-http(5),
//! gitprotocol-pack(5)), as pure functions over bytes: the caller does the
//! HTTP and hands back what the server said. ADR-0041 keeps push and fetch
//! off any host's REST API, so this is all a remote is.
//!
//! It speaks protocol v0, the one git falls back to. receive-pack has no v2,
//! so pushing needs v0's ref advertisement whatever fetching speaks, and v2's
//! one gain for fetching, filtering the advertised refs by prefix, buys
//! nothing for a repo of a few dozen refs. One advertisement parser serves
//! both directions.
//!
//! A fetch is two requests: `GET info/refs?service=git-upload-pack`, whose
//! body goes to [`Repo::fetch_request`], and, when that wants anything, the
//! `POST git-upload-pack` it returns the body of, whose response goes to
//! [`Repo::fetch_finish`]. A push is the same with `git-receive-pack`,
//! [`Repo::push_request`] and [`Repo::push_finish`]. Each step reads the
//! repo as it is when it runs, so other writes may land between them.
//!
//! The remote's branches are tracked under `refs/remotes/origin/`, as
//! `git fetch` keeps them, and its tags under `refs/remotes/origin/tags/`,
//! so the repo knows what the remote holds without asking it.
//!
//! Every branch and tag syncs, not only `main` (ADR-0041: each deck is a
//! branch, each snapshot a tag). What was tracked before a fetch is what
//! both sides last agreed on, so each ref is settled three ways, as git
//! settles a file: the side that moved from it wins, which is how a branch
//! made, moved or deleted on one side reaches the other. A branch both sides
//! moved merges; a tag both moved takes the remote's.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use gix_hash::ObjectId;
use gix_object::bstr::ByteSlice;
use gix_object::{CommitRef, Kind, TagRef, TreeRef};

use crate::merge::{Side, TreeMerge};
use crate::names::{BranchName, CommitId, Signature, TreeId};
use crate::pack;
use crate::pktline::{self, chomp, Line, Reader};
use crate::repo::{Error, Repo, Result};
use crate::store::Store;

const SHA1: gix_hash::Kind = gix_hash::Kind::Sha1;
/// The one remote a repo has.
pub const ORIGIN: &str = "origin";
const AGENT: &str = "agent=progress-engine";
const TRACKING: &str = "refs/remotes/origin";
const TRACKED_TAGS: &str = "refs/remotes/origin/tags";
/// How many commits a fetch names as haves: enough to reach the last
/// commit both sides had, which the tracked tips name first.
const HAVES: usize = 256;

fn remote_err(detail: impl std::fmt::Display) -> Error {
    Error::Remote(detail.to_string())
}

fn pkt(e: pktline::PktError) -> Error {
    remote_err(e)
}

/// A remote's URL: `https://`, a host, and a path, with no credentials,
/// query or fragment in it, since it is written into the repo's config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteUrl(String);

impl RemoteUrl {
    pub fn new(url: impl Into<String>) -> Result<Self> {
        let url = url.into();
        let refuse = |why: &str| Err(remote_err(format!("{url} is not a remote's URL: {why}")));
        let Some(rest) = url.strip_prefix("https://") else {
            return refuse("it must start with https://");
        };
        let Some((host, path)) = rest.split_once('/') else {
            return refuse("it names no repository");
        };
        let host_ok = !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
        if !host_ok {
            return refuse("its host is not a host name");
        }
        let segments: Vec<&str> = path.split('/').collect();
        let path_ok = !segments.is_empty()
            && segments.iter().all(|s| {
                !s.is_empty()
                    && *s != "."
                    && *s != ".."
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '~'))
            });
        if !path_ok {
            return refuse("its path is not a repository's");
        }
        Ok(Self(url))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a server's `info/refs` named.
#[derive(Debug, Clone, Default)]
struct Advertised {
    refs: BTreeMap<String, ObjectId>,
    caps: Vec<String>,
}

impl Advertised {
    fn has_cap(&self, cap: &str) -> bool {
        self.caps.iter().any(|c| c == cap)
    }

    /// Branches and tags: what this repo tracks of the remote.
    fn tracked(&self) -> impl Iterator<Item = (&String, &ObjectId)> {
        self.refs
            .iter()
            .filter(|(r, _)| r.starts_with("refs/heads/") || r.starts_with("refs/tags/"))
    }
}

fn parse_advertisement(bytes: &[u8], service: &str) -> Result<Advertised> {
    let mut r = Reader::new(bytes);
    let mut out = Advertised::default();
    let mut first = true;
    let mut seen_service = false;
    while let Some(line) = r.next_line().map_err(pkt)? {
        let payload = match line {
            Line::Flush if seen_service && first => {
                // The smart HTTP service header ends in its own flush.
                continue;
            }
            Line::Flush => break,
            Line::Data(p) => chomp(p),
            Line::Delim | Line::ResponseEnd => {
                return Err(remote_err("the server answered in protocol v2"))
            }
        };
        if let Some(name) = payload.strip_prefix(b"# service=") {
            if name != service.as_bytes() {
                return Err(remote_err(format!(
                    "asked for {service}, the server answered for {}",
                    name.as_bstr()
                )));
            }
            seen_service = true;
            continue;
        }
        if let Some(msg) = payload.strip_prefix(b"ERR ") {
            return Err(remote_err(format!("the server said: {}", msg.as_bstr())));
        }
        if payload.starts_with(b"version ") {
            return Err(remote_err(
                "the server answered in a protocol other than v0",
            ));
        }
        let (line, caps) = match payload.find_byte(0) {
            Some(nul) if first => (&payload[..nul], Some(&payload[nul + 1..])),
            _ => (payload, None),
        };
        if let Some(caps) = caps {
            out.caps = caps
                .split(|b| *b == b' ')
                .filter(|c| !c.is_empty())
                .map(|c| c.to_str_lossy().into_owned())
                .collect();
        }
        first = false;
        let (hex, name) = line
            .split_once_str(" ")
            .ok_or_else(|| remote_err(format!("{:?} is not a ref", line.as_bstr())))?;
        let id = ObjectId::from_hex(hex).map_err(|_| remote_err("a ref names no object id"))?;
        let name = name
            .to_str()
            .map_err(|_| remote_err("a ref is not UTF-8"))?;
        if name == "capabilities^{}" || name.ends_with("^{}") {
            continue;
        }
        out.refs.insert(name.to_owned(), id);
    }
    Ok(out)
}

/// What a fetch's ref advertisement came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchRequest {
    /// The repo holds every object the remote named; finish the fetch with
    /// no pack.
    HaveAll,
    /// `POST <url>/git-upload-pack` with this body.
    Want(Vec<u8>),
}

/// What a push's ref advertisement came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushRequest {
    /// The remote has everything this repo would push.
    Nothing,
    /// The remote's `main` has commits this repo has not merged: fetch and
    /// integrate first.
    Behind,
    /// `POST <url>/git-receive-pack` with this body.
    Send(Vec<u8>),
}

/// What a push came to, ref by ref.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pushed {
    pub updated: Vec<String>,
    /// Each ref the remote refused, with its reason, such as the remote ref
    /// having moved since it was advertised (the compare-and-swap failing).
    pub rejected: Vec<(String, String)>,
}

/// What bringing one of the remote's branches into this repo's came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Integrated {
    /// The remote has no such branch yet: a push makes it.
    RemoteEmpty,
    UpToDate,
    /// This repo has commits the remote lacks, and none the other way.
    Ahead,
    FastForwarded {
        to: CommitId,
    },
    /// This repo held only its first `VERSION` commit, so it took the
    /// remote's history, as a clone would; or a branch only the remote had.
    Adopted {
        to: CommitId,
    },
    Merged {
        commit: CommitId,
        taken: Vec<String>,
    },
    /// Paths both sides changed so that they do not merge, between the
    /// branch's tip here (`ours`) and the remote's (`theirs`). Nothing moved.
    Conflict {
        paths: Vec<String>,
        ours: CommitId,
        theirs: CommitId,
    },
    /// Both sides have history and none of it is shared. Nothing moved.
    Unrelated,
}

/// Which branch's merge a file is merged for, and the two tips merged:
/// what a choice the user made about one sync's clashes is keyed by, so it
/// settles that merge and no later one with other clashes at the same path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BranchMerge<'a> {
    pub branch: &'a BranchName,
    pub ours: CommitId,
    pub theirs: CommitId,
}

/// Merges one file both sides of a branch's sync changed: [`crate::MergeFile`]
/// told which branch's merge it is part of.
pub type MergeBranchFile<'a> =
    dyn FnMut(&BranchMerge<'_>, &str, &[u8], &[u8], &[u8]) -> Option<Vec<u8>> + 'a;

/// What is waiting to go to the remote.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteStatus {
    /// Commits on the branches here that no tracked branch holds.
    pub ahead: usize,
    /// Commits on the tracked branches that no branch here holds.
    pub behind: usize,
    /// Branches to make, move or delete on the remote.
    pub branches: usize,
    /// Tags to make, move or delete on the remote.
    pub tags: usize,
}

/// What an object names: a commit its tree and parents, a tree its entries
/// but submodules, a tag its target. Each with whether it may name more in
/// turn, which a tree's blobs never do.
fn named_by(kind: Kind, data: &[u8]) -> Result<Vec<(ObjectId, bool)>> {
    Ok(match kind {
        Kind::Blob => Vec::new(),
        Kind::Commit => {
            let commit = CommitRef::from_bytes(data, SHA1).map_err(remote_err)?;
            std::iter::once(commit.tree())
                .chain(commit.parents())
                .map(|id| (id, true))
                .collect()
        }
        Kind::Tree => TreeRef::from_bytes(data, SHA1)
            .map_err(remote_err)?
            .entries
            .into_iter()
            .filter(|e| !e.mode.is_commit())
            .map(|e| (e.oid.to_owned(), e.mode.is_tree()))
            .collect(),
        Kind::Tag => vec![(
            TagRef::from_bytes(data, SHA1).map_err(remote_err)?.target(),
            true,
        )],
    })
}

/// `objects` with each after every one of them it names. The browser's
/// storage keeps a prefix of a fetch's writes when its tab closes mid-way,
/// and this way that prefix never holds a commit without its history and
/// files, which having it promises (the haves, [`Repo::is_whole`]).
fn names_first(objects: Vec<(ObjectId, Kind, Vec<u8>)>) -> Result<Vec<(ObjectId, Kind, Vec<u8>)>> {
    let at: std::collections::HashMap<ObjectId, usize> = objects
        .iter()
        .enumerate()
        .map(|(i, (id, ..))| (*id, i))
        .collect();
    let mut named = Vec::with_capacity(objects.len());
    for (_, kind, data) in &objects {
        named.push(named_by(*kind, data)?);
    }
    let mut placed = vec![false; objects.len()];
    let mut order = Vec::with_capacity(objects.len());
    // Depth first and by hand: a history thousands of commits long would
    // overflow the wasm's stack recursing.
    for root in 0..objects.len() {
        let mut stack = vec![(root, false)];
        while let Some((i, expanded)) = stack.pop() {
            if placed[i] {
                continue;
            }
            if expanded {
                placed[i] = true;
                order.push(i);
                continue;
            }
            stack.push((i, true));
            stack.extend(
                named[i]
                    .iter()
                    .filter_map(|(id, _)| at.get(id))
                    .filter(|&&j| !placed[j])
                    .map(|&j| (j, false)),
            );
        }
    }
    let mut objects: Vec<Option<_>> = objects.into_iter().map(Some).collect();
    Ok(order
        .into_iter()
        .map(|i| objects[i].take().expect("each placed once"))
        .collect())
}

fn main_branch() -> BranchName {
    BranchName::new("main").expect("main is a branch name")
}

fn tracking_for(remote_ref: &str) -> Option<String> {
    if let Some(b) = remote_ref.strip_prefix("refs/heads/") {
        // A remote branch `tags/x` would land on a tracked tag.
        (!b.starts_with("tags/")).then(|| format!("{TRACKING}/{b}"))
    } else {
        remote_ref
            .strip_prefix("refs/tags/")
            .map(|t| format!("{TRACKED_TAGS}/{t}"))
    }
}

/// The config file with `[remote "origin"]` and `[branch "main"]` taken out.
fn without_origin(config: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
            skipping = compact == "[remote\"origin\"]" || compact == "[branch\"main\"]";
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn url_in(config: &str) -> Option<String> {
    let mut inside = false;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
            inside = compact == "[remote\"origin\"]";
            continue;
        }
        if inside {
            if let Some((k, v)) = t.split_once('=') {
                if k.trim().eq_ignore_ascii_case("url") {
                    return Some(v.trim().trim_matches('"').to_owned());
                }
            }
        }
    }
    None
}

impl<S: Store> Repo<S> {
    fn config_text(&self) -> Result<String> {
        let bytes = self.read_file_in_git_dir("config")?.unwrap_or_default();
        String::from_utf8(bytes).map_err(|_| Error::Corrupt {
            what: "config".to_owned(),
            detail: "it is not UTF-8".to_owned(),
        })
    }

    /// `origin`'s URL, from the repo's config as `git remote` keeps it.
    pub fn remote(&self) -> Result<Option<RemoteUrl>> {
        match url_in(&self.config_text()?) {
            None => Ok(None),
            Some(url) => RemoteUrl::new(url).map(Some),
        }
    }

    /// Makes `url` the repo's `origin`, replacing any other. A `git` that
    /// reads the exported repo fetches it with `git fetch`.
    ///
    /// A new URL forgets what was tracked of the old one, as
    /// [`Repo::remove_remote`] does: the next fetch settles every ref
    /// against a remote that has agreed on nothing yet, so a branch or tag
    /// only the old remote had is not taken for one deleted here.
    pub fn set_remote(&mut self, url: &RemoteUrl) -> Result<()> {
        if self.remote()?.as_ref() != Some(url) {
            for (name, id) in self.refs_under(TRACKING)? {
                self.swap_ref(&format!("{TRACKING}/{name}"), Some(id), None)?;
            }
        }
        let mut config = without_origin(&self.config_text()?);
        config.push_str(&format!(
            "[remote \"{ORIGIN}\"]\n\turl = {}\n\tfetch = +refs/heads/*:{TRACKING}/*\n\tfetch = +refs/tags/*:{TRACKED_TAGS}/*\n[branch \"main\"]\n\tremote = {ORIGIN}\n\tmerge = refs/heads/main\n",
            url.as_str()
        ));
        self.write_file_in_git_dir("config", config.as_bytes())
    }

    /// Forgets `origin`: its config and everything tracked of it. The
    /// repo's own branches and tags stay as they are.
    pub fn remove_remote(&mut self) -> Result<()> {
        let config = without_origin(&self.config_text()?);
        self.write_file_in_git_dir("config", config.as_bytes())?;
        for (name, id) in self.refs_under(TRACKING)? {
            self.swap_ref(&format!("{TRACKING}/{name}"), Some(id), None)?;
        }
        Ok(())
    }

    /// The remote's `branch` as last fetched or pushed.
    pub fn tracked_tip(&self, branch: &BranchName) -> Result<Option<CommitId>> {
        match self.read_ref(&format!("{TRACKING}/{branch}"))? {
            Some(id) => self.commit_of(&id),
            None => Ok(None),
        }
    }

    fn tracked_tags(&self) -> Result<BTreeMap<String, ObjectId>> {
        Ok(self.refs_under(TRACKED_TAGS)?.into_iter().collect())
    }

    fn local_tags(&self) -> Result<BTreeMap<String, ObjectId>> {
        Ok(self.refs_under("refs/tags")?.into_iter().collect())
    }

    // Walking objects.

    fn commit_of(&self, id: &ObjectId) -> Result<Option<CommitId>> {
        let mut at = *id;
        loop {
            match self.read_any(&at)? {
                None => return Ok(None),
                Some((Kind::Commit, _)) => return Ok(Some(CommitId(at))),
                Some((Kind::Tag, data)) => {
                    at = TagRef::from_bytes(&data, SHA1)
                        .map_err(remote_err)?
                        .target();
                }
                Some(_) => return Ok(None),
            }
        }
    }

    /// Whether `id` is here with everything it names, down through every
    /// commit's tree and parents: what a have promises the remote. Holding
    /// a commit is not enough; storage that kept only part of an earlier
    /// fetch can hold one without its history. `whole` holds what earlier
    /// walks found whole, and gains what this one does.
    fn is_whole(&self, id: &ObjectId, whole: &mut HashSet<ObjectId>) -> Result<bool> {
        let mut seen = HashSet::new();
        let mut stack = vec![(*id, true)];
        while let Some((at, names)) = stack.pop() {
            if whole.contains(&at) || !seen.insert(at) {
                continue;
            }
            // A blob is only looked for: inflating every file to learn it
            // names nothing would make this most of a sync's time.
            if !names {
                if !self.has_object(&at)? {
                    return Ok(false);
                }
                continue;
            }
            let Some((kind, data)) = self.read_any(&at)? else {
                return Ok(false);
            };
            stack.extend(named_by(kind, &data)?);
        }
        whole.extend(seen);
        Ok(true)
    }

    /// The first `limit` commits reachable from `tips`, breadth first, so
    /// each tip's own commits come before anything far behind it.
    fn recent_commits(&self, tips: &[CommitId], limit: usize) -> Result<Vec<CommitId>> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        let mut queue: VecDeque<CommitId> = tips.iter().copied().collect();
        while let Some(c) = queue.pop_front() {
            if out.len() >= limit {
                break;
            }
            if !seen.insert(c) {
                continue;
            }
            out.push(c);
            queue.extend(self.parents(&c)?);
        }
        Ok(out)
    }

    /// Every tree and blob under `tree`, skipping trees already in `seen`.
    fn tree_objects(
        &self,
        tree: TreeId,
        seen: &mut HashSet<ObjectId>,
        out: &mut Vec<ObjectId>,
    ) -> Result<()> {
        if !seen.insert(tree.0) {
            return Ok(());
        }
        out.push(tree.0);
        for (_, mode, id) in self.tree_entries(&tree)? {
            if mode.is_tree() {
                self.tree_objects(TreeId(id), seen, out)?;
            } else if !mode.is_commit() && seen.insert(id) {
                out.push(id);
            }
        }
        Ok(())
    }

    /// The objects reachable from `tips` that are not reachable from
    /// `known`, which the other side has: the commits behind the known
    /// tips, and the trees and blobs of the commits where the walk stops.
    fn objects_between(&self, tips: &[ObjectId], known: &[ObjectId]) -> Result<Vec<ObjectId>> {
        let known_commits: Vec<CommitId> = known
            .iter()
            .filter_map(|id| self.commit_of(id).transpose())
            .collect::<Result<_>>()?;
        let mut cache = Default::default();
        let behind = self.ancestors(&known_commits, &mut cache)?;
        let mut seen: HashSet<ObjectId> = known.iter().copied().collect();
        for c in &known_commits {
            let mut ignored = Vec::new();
            self.tree_objects(self.tree_of(c)?, &mut seen, &mut ignored)?;
        }
        let mut out = Vec::new();
        let mut commits: VecDeque<ObjectId> = VecDeque::new();
        for tip in tips {
            let mut at = *tip;
            // A tag's object, and any tag it is of, before its commit.
            while let Some((Kind::Tag, data)) = self.read_any(&at)? {
                if !seen.insert(at) {
                    break;
                }
                out.push(at);
                at = TagRef::from_bytes(&data, SHA1)
                    .map_err(remote_err)?
                    .target();
            }
            commits.push_back(at);
        }
        let mut walked = HashSet::new();
        while let Some(c) = commits.pop_front() {
            let commit = CommitId(c);
            if behind.contains(&commit) || !walked.insert(c) {
                continue;
            }
            if seen.insert(c) {
                out.push(c);
            }
            for parent in self.parents(&commit)? {
                if behind.contains(&parent) {
                    // Where the walk stops, the other side has the parent's
                    // tree; leave its files out of the pack.
                    let mut ignored = Vec::new();
                    self.tree_objects(self.tree_of(&parent)?, &mut seen, &mut ignored)?;
                } else {
                    commits.push_back(parent.0);
                }
            }
            self.tree_objects(self.tree_of(&commit)?, &mut seen, &mut out)?;
        }
        Ok(out)
    }

    // Fetching.

    /// The body of `POST git-upload-pack` for the remote's `info/refs`
    /// answer, asking for every branch and tag tip this repo lacks, or holds
    /// without all of its history.
    pub fn fetch_request(&self, advertised: &[u8]) -> Result<FetchRequest> {
        let ad = parse_advertisement(advertised, "git-upload-pack")?;
        let mut whole = HashSet::new();
        let mut wants = BTreeSet::new();
        for (_, id) in ad.tracked() {
            if !self.is_whole(id, &mut whole)? {
                wants.insert(*id);
            }
        }
        let mut tips = self.refs_under(TRACKING)?;
        tips.extend(self.refs_under("refs/heads")?);
        self.upload_request(&ad, &wants, &tips, &mut whole)
    }

    /// An upload-pack request for `wants`, naming as haves the newest
    /// commits behind `tips` that this repo holds whole.
    fn upload_request(
        &self,
        ad: &Advertised,
        wants: &BTreeSet<ObjectId>,
        tips: &[(String, ObjectId)],
        whole: &mut HashSet<ObjectId>,
    ) -> Result<FetchRequest> {
        if wants.is_empty() {
            return Ok(FetchRequest::HaveAll);
        }
        let band = if ad.has_cap("side-band-64k") {
            Some("side-band-64k")
        } else if ad.has_cap("side-band") {
            Some("side-band")
        } else {
            None
        };
        let caps: Vec<&str> = band
            .into_iter()
            .chain(
                ["ofs-delta", "no-progress"]
                    .into_iter()
                    .filter(|c| ad.has_cap(c)),
            )
            .chain(
                ad.caps
                    .iter()
                    .any(|c| c.starts_with("agent="))
                    .then_some(AGENT),
            )
            .collect();
        let mut body = Vec::new();
        for (i, id) in wants.iter().enumerate() {
            if i == 0 {
                pktline::text(&mut body, &format!("want {id} {}\n", caps.join(" ")));
            } else {
                pktline::text(&mut body, &format!("want {id}\n"));
            }
        }
        pktline::flush(&mut body);
        let mut commits: Vec<CommitId> = Vec::new();
        for (_, id) in tips {
            if self.is_whole(id, whole)? {
                commits.extend(self.commit_of(id)?);
            }
        }
        for c in self.recent_commits(&commits, HAVES)? {
            pktline::text(&mut body, &format!("have {c}\n"));
        }
        pktline::text(&mut body, "done\n");
        Ok(FetchRequest::Want(body))
    }

    /// Writes the objects of upload-pack's answer loose, each after what it
    /// names.
    fn take_pack(&mut self, response: &[u8]) -> Result<()> {
        let pack = unwrap_upload_response(response)?;
        let objects = names_first(pack::read(&pack, |id| self.read_any(id))?)?;
        for (id, kind, data) in objects {
            let wrote = self.write_object(kind, &data)?;
            debug_assert_eq!(wrote, id);
        }
        Ok(())
    }

    /// Takes in what `POST git-upload-pack` answered (none when
    /// [`FetchRequest::HaveAll`]): writes the pack's objects loose, then
    /// points the tracking refs at what the remote advertised.
    pub fn fetch_finish(&mut self, advertised: &[u8], response: Option<&[u8]>) -> Result<()> {
        let ad = parse_advertisement(advertised, "git-upload-pack")?;
        if let Some(response) = response {
            self.take_pack(response)?;
        }
        let mut whole = HashSet::new();
        let mut want: BTreeMap<String, ObjectId> = BTreeMap::new();
        for (remote_ref, id) in ad.tracked() {
            if let Some(local) = tracking_for(remote_ref) {
                if !self.is_whole(id, &mut whole)? {
                    return Err(remote_err(format!(
                        "the remote did not send all of {id}, which its {remote_ref} names"
                    )));
                }
                want.insert(local, *id);
            }
        }
        let held: BTreeMap<String, ObjectId> = self
            .refs_under(TRACKING)?
            .into_iter()
            .map(|(name, id)| (format!("{TRACKING}/{name}"), id))
            .collect();
        self.settle_refs(&held, &want)?;
        // Pruned first, so a tracked branch `a` can follow a gone `a/b`.
        for (path, id) in &held {
            if !want.contains_key(path) {
                self.swap_ref(path, Some(*id), None)?;
            }
        }
        for (path, id) in &want {
            let now = held.get(path).copied();
            if now != Some(*id) {
                self.swap_ref(path, now, Some(*id))?;
            }
        }
        Ok(())
    }

    /// Brings in what the remote made, moved or deleted since the last
    /// fetch (`held`, what was tracked then) and this repo did not: each
    /// branch but `main`, and each tag. A branch both sides moved is left
    /// for [`Repo::integrate`] to merge; one this repo deleted and the
    /// remote did not touch stays deleted, for the push to delete there.
    fn settle_refs(
        &mut self,
        held: &BTreeMap<String, ObjectId>,
        want: &BTreeMap<String, ObjectId>,
    ) -> Result<()> {
        let names: BTreeSet<&String> = held.keys().chain(want.keys()).collect();
        for tracked in names {
            let base = held.get(tracked).copied();
            let remote = want.get(tracked).copied();
            if remote == base {
                continue;
            }
            let rest = &tracked[TRACKING.len() + 1..];
            let (local_path, is_tag) = match rest.strip_prefix("tags/") {
                Some(tag) => (format!("refs/tags/{tag}"), true),
                None if rest == "main" => continue,
                None => (format!("refs/heads/{rest}"), false),
            };
            let local = self.read_ref(&local_path)?;
            let take = if local == base {
                true
            } else if remote.is_none() {
                // Moved here, deleted there: keep it, and the push makes it again.
                false
            } else if is_tag {
                // A snapshot both sides moved: the remote's is what others see.
                true
            } else {
                // Deleted here, moved there, it comes back; moved both ways, it merges.
                local.is_none()
            };
            if !take || local == remote {
                continue;
            }
            if let Some(id) = remote {
                // A snapshot is a tag of a commit; anything else is not taken.
                if is_tag && self.commit_of(&id)?.is_none() {
                    continue;
                }
            }
            match self.swap_ref(&local_path, local, remote) {
                // A local ref `a` where the remote made `a/b`: keep the local one.
                Err(Error::RefClash { .. }) => {}
                other => other?,
            }
        }
        Ok(())
    }

    // Integrating.

    /// Whether `main` is only the first `VERSION` commit an empty repo
    /// gets, or nothing at all: nothing a clone over it would lose.
    pub fn is_fresh(&self) -> Result<bool> {
        let Some(tip) = self.branch_tip(&main_branch())? else {
            return Ok(true);
        };
        if !self.parents(&tip)?.is_empty() {
            return Ok(false);
        }
        let entries = self.tree_entries(&self.tree_of(&tip)?)?;
        Ok(entries.len() == 1 && entries[0].0 == "VERSION" && !entries[0].1.is_tree())
    }

    /// Makes each tag the remote has and this repo lacks, when it is of a
    /// commit. A tag this repo has under the same name stays as it is.
    fn adopt_tags(&mut self) -> Result<()> {
        let local = self.local_tags()?;
        for (name, id) in self.tracked_tags()? {
            if local.contains_key(&name) || self.commit_of(&id)?.is_none() {
                continue;
            }
            let path = format!("refs/tags/{name}");
            match self.swap_ref(&path, None, Some(id)) {
                // A local tag `a` where the remote has `a/b`: keep the local one.
                Err(Error::RefClash { .. }) => {}
                other => other?,
            }
        }
        Ok(())
    }

    /// Brings each tracked branch into the branch here of the same name:
    /// [`Repo::integrate_onto`] with [`Repo::integration_targets`].
    pub fn integrate(
        &mut self,
        who: &Signature,
        prefer: Option<Side>,
        merge_file: &mut MergeBranchFile<'_>,
    ) -> Result<Vec<(BranchName, Integrated)>> {
        let theirs = self.integration_targets()?;
        self.integrate_onto(&theirs, who, prefer, merge_file)
    }

    /// The tracked branches, each as the commit to bring into the branch
    /// here of its name. A branch tracked but deleted here (other than
    /// `main`) was deleted since the last sync, since the fetch would have
    /// made it again otherwise, and stays deleted.
    pub fn integration_targets(&self) -> Result<BTreeMap<BranchName, CommitId>> {
        let mut theirs = BTreeMap::new();
        for (name, id) in self.refs_under(TRACKING)? {
            if name.starts_with("tags/") {
                continue;
            }
            let Ok(branch) = BranchName::new(name) else {
                continue;
            };
            if branch != main_branch() && self.branch_tip(&branch)?.is_none() {
                continue;
            }
            if let Some(c) = self.commit_of(&id)? {
                theirs.insert(branch, c);
            }
        }
        Ok(theirs)
    }

    /// Brings `theirs`, the remote's branches as this repo should take them,
    /// into the branches here, one by one: a fast-forward when it can, the
    /// remote's history over a `main` that is only its first `VERSION`
    /// commit, a branch only the remote has made here, and otherwise a merge
    /// three-way from the merge base with `merge_file`, told the branch and
    /// the two tips (see [`Repo::merge_trees`]), committed with two parents. `prefer` settles
    /// conflicting paths when the user has chosen a side. `main` always has
    /// an outcome, [`Integrated::RemoteEmpty`] when `theirs` lacks it.
    pub fn integrate_onto(
        &mut self,
        theirs: &BTreeMap<BranchName, CommitId>,
        who: &Signature,
        prefer: Option<Side>,
        merge_file: &mut MergeBranchFile<'_>,
    ) -> Result<Vec<(BranchName, Integrated)>> {
        let main = main_branch();
        let mut out = Vec::new();
        if !theirs.contains_key(&main) {
            out.push((main.clone(), Integrated::RemoteEmpty));
        }
        for (branch, remote) in theirs {
            let outcome = self.integrate_branch(branch, *remote, who, prefer, merge_file)?;
            out.push((branch.clone(), outcome));
        }
        Ok(out)
    }

    fn integrate_branch(
        &mut self,
        branch: &BranchName,
        remote: CommitId,
        who: &Signature,
        prefer: Option<Side>,
        merge_file: &mut MergeBranchFile<'_>,
    ) -> Result<Integrated> {
        let is_main = *branch == main_branch();
        let local = self.branch_tip(branch)?;
        Ok(match local {
            Some(l) if l == remote => Integrated::UpToDate,
            Some(l) if self.is_ancestor(&remote, &l)? => Integrated::Ahead,
            Some(l) if self.is_ancestor(&l, &remote)? => {
                self.update_branch(branch, crate::Tip::At(l), &remote)?;
                Integrated::FastForwarded { to: remote }
            }
            None if !is_main => {
                self.update_branch(branch, crate::Tip::Unborn, &remote)?;
                Integrated::Adopted { to: remote }
            }
            _ if is_main && self.is_fresh()? => {
                let tip = local.map_or(crate::Tip::Unborn, crate::Tip::At);
                self.update_branch(branch, tip, &remote)?;
                Integrated::Adopted { to: remote }
            }
            None => unreachable!("an unborn main is fresh"),
            Some(l) => {
                let bases = self.merge_base(&l, &remote)?;
                let Some(base) = bases.first() else {
                    return Ok(Integrated::Unrelated);
                };
                let at = BranchMerge {
                    branch,
                    ours: l,
                    theirs: remote,
                };
                let mut file =
                    |path: &str, b: &[u8], o: &[u8], t: &[u8]| merge_file(&at, path, b, o, t);
                match self.merge_trees(Some(base), &l, &remote, prefer, &mut file)? {
                    TreeMerge::Conflicts(paths) => Integrated::Conflict {
                        paths,
                        ours: l,
                        theirs: remote,
                    },
                    TreeMerge::Clean { tree, taken } => {
                        let commit = self.write_merge(branch, &l, &remote, tree, &taken, who)?;
                        self.update_branch(branch, crate::Tip::At(l), &commit)?;
                        Integrated::Merged { commit, taken }
                    }
                }
            }
        })
    }

    fn write_merge(
        &mut self,
        branch: &BranchName,
        ours: &CommitId,
        theirs: &CommitId,
        tree: TreeId,
        taken: &[String],
        who: &Signature,
    ) -> Result<CommitId> {
        let mut message = format!("sync: merge {ORIGIN}/{branch}\n");
        if !taken.is_empty() {
            message.push('\n');
            for path in taken {
                message.push_str(path);
                message.push('\n');
            }
        }
        let object = gix_object::Commit {
            tree: tree.0,
            parents: vec![ours.0, theirs.0].into(),
            author: who.to_gix(),
            committer: who.to_gix(),
            encoding: None,
            message: message.as_str().into(),
            extra_headers: Vec::new(),
        };
        let mut data = Vec::new();
        gix_object::WriteTo::write_to(&object, &mut data).map_err(remote_err)?;
        self.write_object(Kind::Commit, &data).map(CommitId)
    }

    /// Replaces this repo's branches and tags with the remote's tracked
    /// ones, for a remote whose history is unrelated to this one's and
    /// which the user chose over it. Objects stay, unreachable.
    pub fn adopt_remote(&mut self) -> Result<()> {
        let tracked: BTreeMap<String, ObjectId> = self.refs_under(TRACKING)?.into_iter().collect();
        for (name, id) in self.refs_under("refs/heads")? {
            if !tracked.contains_key(&name) {
                self.swap_ref(&format!("refs/heads/{name}"), Some(id), None)?;
            }
        }
        for (name, id) in &tracked {
            if name.starts_with("tags/") || self.commit_of(id)?.is_none() {
                continue;
            }
            let path = format!("refs/heads/{name}");
            let now = self.read_ref(&path)?;
            if now != Some(*id) {
                self.swap_ref(&path, now, Some(*id))?;
            }
        }
        let remote_tags = self.tracked_tags()?;
        for (name, id) in self.local_tags()? {
            if remote_tags.get(&name) != Some(&id) {
                self.swap_ref(&format!("refs/tags/{name}"), Some(id), None)?;
            }
        }
        self.adopt_tags()
    }

    /// What the branches and tags here hold that the tracked remote does
    /// not, and the other way.
    pub fn remote_status(&self) -> Result<RemoteStatus> {
        let local: BTreeMap<String, ObjectId> =
            self.refs_under("refs/heads")?.into_iter().collect();
        let tracked: BTreeMap<String, ObjectId> = self
            .refs_under(TRACKING)?
            .into_iter()
            .filter(|(n, _)| !n.starts_with("tags/"))
            .collect();
        let mut cache = Default::default();
        // A tip storage lost history under counts as nothing until the next
        // fetch makes it whole (see `is_whole`). Only its commits are walked
        // here, so only a missing commit stops the walk: this runs on every
        // save, and reading every tree too would cost a tenth of a second.
        let mut history = |m: &BTreeMap<String, ObjectId>| -> Result<HashSet<CommitId>> {
            let mut out = HashSet::new();
            for id in m.values() {
                let Some(tip) = self.commit_of(id)? else {
                    continue;
                };
                match self.ancestors(&[tip], &mut cache) {
                    Ok(walked) => out.extend(walked),
                    Err(Error::MissingObject(_)) => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(out)
        };
        let mine = history(&local)?;
        let theirs = history(&tracked)?;
        let differing = |a: &BTreeMap<String, ObjectId>, b: &BTreeMap<String, ObjectId>| {
            a.iter().filter(|(n, id)| b.get(*n) != Some(id)).count()
                + b.keys().filter(|n| !a.contains_key(*n)).count()
        };
        let tracked_tags = self.tracked_tags()?;
        Ok(RemoteStatus {
            ahead: mine.difference(&theirs).count(),
            behind: theirs.difference(&mine).count(),
            branches: differing(&local, &tracked),
            tags: differing(&self.local_tags()?, &tracked_tags),
        })
    }

    // Pushing.

    /// The body of `POST git-receive-pack` for the remote's `info/refs`
    /// answer: every branch here the remote's is behind, and every tag here
    /// it lacks or has elsewhere, with a pack of what the remote does not
    /// have; and the deletion of each branch and tag deleted here since the
    /// last sync. Each update names the remote ref's advertised id as its
    /// old one, which is receive-pack's compare-and-swap: a ref that moved
    /// since is refused. A remote branch this repo has not merged, or a ref
    /// the remote moved since the last fetch, is [`PushRequest::Behind`].
    pub fn push_request(&self, advertised: &[u8]) -> Result<PushRequest> {
        let ad = parse_advertisement(advertised, "git-receive-pack")?;
        if !ad.has_cap("report-status") {
            return Err(remote_err("the remote does not report a push's outcome"));
        }
        let zero = ObjectId::null(SHA1);
        let tracked: BTreeMap<String, ObjectId> = self.refs_under(TRACKING)?.into_iter().collect();
        let mut commands: Vec<(ObjectId, ObjectId, String)> = Vec::new();
        let mut local_refs: Vec<(String, ObjectId)> = Vec::new();
        for (name, id) in self.refs_under("refs/heads")? {
            local_refs.push((format!("refs/heads/{name}"), id));
        }
        for (name, id) in self.local_tags()? {
            local_refs.push((format!("refs/tags/{name}"), id));
        }
        for (path, local) in &local_refs {
            let remote = ad.refs.get(path).copied();
            if remote == Some(*local) {
                continue;
            }
            let base =
                tracking_for(path).and_then(|t| tracked.get(&t[TRACKING.len() + 1..]).copied());
            if path.starts_with("refs/tags/") {
                // The remote moved the tag since the fetch: take that in first.
                if remote != base {
                    return Ok(PushRequest::Behind);
                }
            } else if let Some(r) = remote {
                let ahead =
                    self.has_object(&r)? && self.is_ancestor(&CommitId(r), &CommitId(*local))?;
                if !ahead {
                    return Ok(PushRequest::Behind);
                }
            }
            commands.push((remote.unwrap_or(zero), *local, path.clone()));
        }
        // Tracked refs gone here, deleted since the last sync.
        for (name, base) in &tracked {
            let path = match name.strip_prefix("tags/") {
                Some(tag) => format!("refs/tags/{tag}"),
                None if name == "main" => continue,
                None => format!("refs/heads/{name}"),
            };
            if local_refs.iter().any(|(p, _)| *p == path) {
                continue;
            }
            match ad.refs.get(&path) {
                None => {}
                Some(r) if r == base => {
                    if !ad.has_cap("delete-refs") {
                        return Err(remote_err("the remote does not let a push delete a ref"));
                    }
                    commands.push((*r, zero, path));
                }
                Some(_) => return Ok(PushRequest::Behind),
            }
        }
        if commands.is_empty() {
            return Ok(PushRequest::Nothing);
        }
        let known: Vec<ObjectId> = ad
            .refs
            .values()
            .copied()
            .filter(|id| self.has_object(id).unwrap_or(false))
            .collect();
        let tips: Vec<ObjectId> = commands
            .iter()
            .map(|(_, new, _)| *new)
            .filter(|new| !new.is_null())
            .collect();
        let objects = self
            .objects_between(&tips, &known)?
            .into_iter()
            .map(|id| self.read_any(&id)?.ok_or(Error::MissingObject(id)))
            .collect::<Result<Vec<_>>>()?;
        let mut body = Vec::new();
        for (i, (old, new, name)) in commands.iter().enumerate() {
            if i == 0 {
                let caps = [
                    Some("report-status"),
                    commands
                        .iter()
                        .any(|(_, n, _)| n.is_null())
                        .then_some("delete-refs"),
                    ad.caps
                        .iter()
                        .any(|c| c.starts_with("agent="))
                        .then_some(AGENT),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
                let mut line = format!("{old} {new} {name}").into_bytes();
                line.push(0);
                line.extend_from_slice(caps.as_bytes());
                line.push(b'\n');
                pktline::data(&mut body, &line);
            } else {
                pktline::text(&mut body, &format!("{old} {new} {name}\n"));
            }
        }
        pktline::flush(&mut body);
        // A push of deletions alone sends no pack (gitprotocol-pack(5)).
        if !tips.is_empty() {
            body.extend(pack::write(&objects));
        }
        Ok(PushRequest::Send(body))
    }

    /// Takes in what `POST git-receive-pack` answered to `sent`, the body
    /// [`Repo::push_request`] made: each ref the remote took is tracked at
    /// its new id.
    pub fn push_finish(&mut self, sent: &[u8], response: &[u8]) -> Result<Pushed> {
        let mut commands: BTreeMap<String, ObjectId> = BTreeMap::new();
        let mut r = Reader::new(sent);
        while let Some(Line::Data(line)) = r.next_line().map_err(pkt)? {
            let line = chomp(line);
            let line = line.split(|b| *b == 0).next().unwrap_or(line);
            let mut parts = line.splitn(3, |b| *b == b' ');
            let (_, new, name) = (parts.next(), parts.next(), parts.next());
            let (Some(new), Some(name)) = (new, name) else {
                return Err(remote_err("the push's own request is not a push"));
            };
            let new = ObjectId::from_hex(new).map_err(|_| remote_err("a pushed id is not hex"))?;
            commands.insert(name.to_str_lossy().into_owned(), new);
        }
        let mut out = Pushed::default();
        let mut r = Reader::new(response);
        let mut unpacked = false;
        while let Some(line) = r.next_line().map_err(pkt)? {
            let Line::Data(line) = line else { break };
            let line = chomp(line).to_str_lossy().into_owned();
            if let Some(status) = line.strip_prefix("unpack ") {
                if status != "ok" {
                    return Err(remote_err(format!(
                        "the remote could not unpack the push: {status}"
                    )));
                }
                unpacked = true;
            } else if let Some(name) = line.strip_prefix("ok ") {
                out.updated.push(name.to_owned());
            } else if let Some(rest) = line.strip_prefix("ng ") {
                let (name, why) = rest.split_once(' ').unwrap_or((rest, ""));
                out.rejected.push((name.to_owned(), why.to_owned()));
            } else if let Some(msg) = line.strip_prefix("ERR ") {
                return Err(remote_err(format!("the server said: {msg}")));
            }
        }
        if !unpacked {
            return Err(remote_err(
                "the remote did not say whether it took the push",
            ));
        }
        for name in &out.updated {
            let (Some(new), Some(local)) = (commands.get(name), tracking_for(name)) else {
                continue;
            };
            let new = (!new.is_null()).then_some(*new);
            let now = self.read_ref(&local)?;
            if now != new {
                self.swap_ref(&local, now, new)?;
            }
        }
        Ok(out)
    }
}

/// Another repo than `origin`, whose branches this one fetches one at a
/// time: a deck forked from another player's (ADR-0041). Nothing of it is in
/// the config. What was fetched is tracked under `refs/remotes/<name>/`,
/// `<name>` its URL's host and path, so two players' repos never share a
/// tracking ref, and a push, which sends only `refs/heads/` and
/// `refs/tags/`, never sends them on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Other {
    url: RemoteUrl,
    name: String,
}

impl Other {
    pub fn new(url: RemoteUrl) -> Result<Self> {
        let rest = url.as_str().trim_start_matches("https://");
        let rest = rest.strip_suffix(".git").unwrap_or(rest);
        // A port's colon cannot be in a ref.
        let name = rest.replace(':', "_");
        let first = name.split('/').next().unwrap_or_default();
        if first == ORIGIN {
            return Err(remote_err(format!(
                "{} cannot be tracked beside origin",
                url.as_str()
            )));
        }
        crate::names::refname("refs/remotes/", &name)
            .map_err(|why| remote_err(format!("{} makes no tracking ref: {why}", url.as_str())))?;
        Ok(Self { url, name })
    }

    pub fn url(&self) -> &RemoteUrl {
        &self.url
    }

    fn tracking(&self, branch: &BranchName) -> String {
        format!("refs/remotes/{}/{branch}", self.name)
    }
}

/// The branches an `info/refs?service=git-upload-pack` answer names, by
/// name, with the commit each is at.
pub fn advertised_branches(advertised: &[u8]) -> Result<BTreeMap<BranchName, CommitId>> {
    let ad = parse_advertisement(advertised, "git-upload-pack")?;
    Ok(ad
        .refs
        .iter()
        .filter_map(|(r, id)| {
            let branch = BranchName::new(r.strip_prefix("refs/heads/")?).ok()?;
            Some((branch, CommitId(*id)))
        })
        .collect())
}

impl<S: Store> Repo<S> {
    /// The body of `POST git-upload-pack` asking `other` (whose `info/refs`
    /// answer is `advertised`) for its `branch` alone: only that tip and
    /// what it stands on come down, less what this repo holds already.
    pub fn fetch_branch_request(
        &self,
        advertised: &[u8],
        branch: &BranchName,
    ) -> Result<FetchRequest> {
        let ad = parse_advertisement(advertised, "git-upload-pack")?;
        let tip = advertised_tip(&ad, branch)?;
        let mut whole = HashSet::new();
        let mut wants = BTreeSet::new();
        if !self.is_whole(&tip, &mut whole)? {
            wants.insert(tip);
        }
        let mut tips = self.refs_under("refs/heads")?;
        tips.extend(self.refs_under("refs/remotes")?);
        self.upload_request(&ad, &wants, &tips, &mut whole)
    }

    /// Takes in what `POST git-upload-pack` answered to
    /// [`Repo::fetch_branch_request`], and tracks `other`'s `branch` at the
    /// tip it advertised, which it returns.
    pub fn fetch_branch_finish(
        &mut self,
        other: &Other,
        advertised: &[u8],
        branch: &BranchName,
        response: Option<&[u8]>,
    ) -> Result<CommitId> {
        let ad = parse_advertisement(advertised, "git-upload-pack")?;
        let tip = advertised_tip(&ad, branch)?;
        if let Some(response) = response {
            self.take_pack(response)?;
        }
        if !self.is_whole(&tip, &mut HashSet::new())? {
            return Err(remote_err(format!(
                "{} did not send all of {tip}, which its {branch} names",
                other.url.as_str()
            )));
        }
        let tip = self
            .commit_of(&tip)?
            .ok_or_else(|| remote_err(format!("{branch} is not a commit")))?;
        self.track_other(other, branch, &tip)?;
        Ok(tip)
    }

    /// Tracks `other`'s `branch` at `tip`, a commit here: what was fetched
    /// of it, or what it will be once it migrates, which is the same.
    pub fn track_other(
        &mut self,
        other: &Other,
        branch: &BranchName,
        tip: &CommitId,
    ) -> Result<()> {
        let path = other.tracking(branch);
        let now = self.read_ref(&path)?;
        if now != Some(tip.0) {
            self.swap_ref(&path, now, Some(tip.0))?;
        }
        Ok(())
    }

    /// `other`'s `branch` as last fetched.
    pub fn other_tip(&self, other: &Other, branch: &BranchName) -> Result<Option<CommitId>> {
        match self.read_ref(&other.tracking(branch))? {
            Some(id) => self.commit_of(&id),
            None => Ok(None),
        }
    }
}

fn advertised_tip(ad: &Advertised, branch: &BranchName) -> Result<ObjectId> {
    ad.refs
        .get(&branch.ref_path())
        .copied()
        .ok_or_else(|| remote_err(format!("the remote has no branch {branch}")))
}

/// The pack inside upload-pack's answer: past its ACK and NAK lines, out
/// of side-band framing when it came in it.
fn unwrap_upload_response(response: &[u8]) -> Result<Vec<u8>> {
    let mut r = Reader::new(response);
    loop {
        if !r.at_pkt_line() {
            // No side-band: the pack follows the last ACK or NAK raw.
            return Ok(r.rest().to_vec());
        }
        let before = r.rest();
        match r.next_line().map_err(pkt)? {
            None => return Err(remote_err("the remote sent no pack")),
            Some(Line::Data(p)) if p.starts_with(b"NAK") || p.starts_with(b"ACK ") => continue,
            Some(Line::Data(p)) if p.starts_with(b"ERR ") => {
                return Err(remote_err(format!(
                    "the server said: {}",
                    chomp(&p[4..]).as_bstr()
                )))
            }
            Some(Line::Data(p)) if p.starts_with(b"shallow ") || p.starts_with(b"unshallow ") => {
                continue
            }
            Some(_) => {
                r = Reader::new(before);
                break;
            }
        }
    }
    let mut pack = Vec::new();
    while let Some(line) = r.next_line().map_err(pkt)? {
        match line {
            Line::Flush => break,
            Line::Data([1, data @ ..]) => pack.extend_from_slice(data),
            Line::Data([2, ..]) => {}
            Line::Data([3, msg @ ..]) => {
                return Err(remote_err(format!(
                    "the server said: {}",
                    chomp(msg).as_bstr()
                )))
            }
            _ => return Err(remote_err("the remote's side-band holds something else")),
        }
    }
    Ok(pack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_is_https_with_a_plain_path() {
        assert!(RemoteUrl::new("https://github.com/cramt/mtg.git").is_ok());
        assert!(RemoteUrl::new("https://gitlab.com/group/sub/mtg.git").is_ok());
        for bad in [
            "http://github.com/a/b.git",
            "https://user:pw@github.com/a/b.git",
            "https://github.com/a/../b.git",
            "https://github.com/a/b.git?x=1",
            "https://github.com",
        ] {
            assert!(RemoteUrl::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_config_keeps_its_other_sections() {
        let config = "[core]\n\tbare = true\n[remote \"origin\"]\n\turl = https://x/a\n[branch \"main\"]\n\tremote = origin\n[user]\n\tname = x\n";
        assert_eq!(url_in(config).as_deref(), Some("https://x/a"));
        assert_eq!(
            without_origin(config),
            "[core]\n\tbare = true\n[user]\n\tname = x\n"
        );
    }

    #[test]
    fn an_advertisement_names_its_refs_and_caps() {
        let mut body = Vec::new();
        pktline::text(&mut body, "# service=git-upload-pack\n");
        pktline::flush(&mut body);
        let mut first = b"1111111111111111111111111111111111111111 HEAD".to_vec();
        first.push(0);
        first.extend_from_slice(b"side-band-64k ofs-delta symref=HEAD:refs/heads/main\n");
        pktline::data(&mut body, &first);
        pktline::text(
            &mut body,
            "1111111111111111111111111111111111111111 refs/heads/main\n",
        );
        pktline::text(
            &mut body,
            "2222222222222222222222222222222222222222 refs/tags/v1\n",
        );
        pktline::text(
            &mut body,
            "1111111111111111111111111111111111111111 refs/tags/v1^{}\n",
        );
        pktline::flush(&mut body);
        let ad = parse_advertisement(&body, "git-upload-pack").unwrap();
        assert_eq!(ad.refs.len(), 3);
        assert!(ad.has_cap("ofs-delta"));
        assert_eq!(ad.tracked().count(), 2);
        assert!(parse_advertisement(&body, "git-receive-pack").is_err());
    }
}
