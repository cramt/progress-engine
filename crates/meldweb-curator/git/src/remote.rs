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

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use gix_hash::ObjectId;
use gix_object::bstr::ByteSlice;
use gix_object::{Kind, TagRef};

use crate::merge::{MergeFile, Side, TreeMerge};
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

/// What bringing the remote's `main` into this repo's came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Integrated {
    /// The remote has no `main` yet: a push makes it.
    RemoteEmpty,
    UpToDate,
    /// This repo has commits the remote lacks, and none the other way.
    Ahead,
    FastForwarded {
        to: CommitId,
    },
    /// This repo held only its first `VERSION` commit, so it took the
    /// remote's history, as a clone would.
    Adopted {
        to: CommitId,
    },
    Merged {
        commit: CommitId,
        taken: Vec<String>,
    },
    /// Paths both sides changed so that they do not merge. Nothing moved.
    Conflict {
        paths: Vec<String>,
    },
    /// Both sides have history and none of it is shared. Nothing moved.
    Unrelated,
}

/// What is waiting to go to the remote.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteStatus {
    /// Commits on `main` that the remote's tracked `main` lacks.
    pub ahead: usize,
    /// Commits on the tracked `main` that `main` lacks.
    pub behind: usize,
    /// Tags here that the remote does not have.
    pub tags: usize,
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
    pub fn set_remote(&mut self, url: &RemoteUrl) -> Result<()> {
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

    fn tracked_main(&self) -> Result<Option<CommitId>> {
        Ok(self.read_ref(&format!("{TRACKING}/main"))?.map(CommitId))
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

    fn is_ancestor(&self, ancestor: &CommitId, of: &CommitId) -> Result<bool> {
        let mut cache = Default::default();
        Ok(self.ancestors(&[*of], &mut cache)?.contains(ancestor))
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
    /// answer, asking for every branch and tag tip this repo lacks.
    pub fn fetch_request(&self, advertised: &[u8]) -> Result<FetchRequest> {
        let ad = parse_advertisement(advertised, "git-upload-pack")?;
        let mut wants = BTreeSet::new();
        for (_, id) in ad.tracked() {
            if !self.has_object(id)? {
                wants.insert(*id);
            }
        }
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
        let mut tips: Vec<CommitId> = Vec::new();
        for (_, id) in self.refs_under(TRACKING)? {
            tips.extend(self.commit_of(&id)?);
        }
        for (_, id) in self.refs_under("refs/heads")? {
            tips.push(CommitId(id));
        }
        for c in self.recent_commits(&tips, HAVES)? {
            pktline::text(&mut body, &format!("have {c}\n"));
        }
        pktline::text(&mut body, "done\n");
        Ok(FetchRequest::Want(body))
    }

    /// Takes in what `POST git-upload-pack` answered (none when
    /// [`FetchRequest::HaveAll`]): writes the pack's objects loose, then
    /// points the tracking refs at what the remote advertised.
    pub fn fetch_finish(&mut self, advertised: &[u8], response: Option<&[u8]>) -> Result<()> {
        let ad = parse_advertisement(advertised, "git-upload-pack")?;
        if let Some(response) = response {
            let pack = unwrap_upload_response(response)?;
            let objects = pack::read(&pack, |id| self.read_any(id))?;
            for (id, kind, data) in objects {
                let wrote = self.write_object(kind, &data)?;
                debug_assert_eq!(wrote, id);
            }
        }
        let mut want: BTreeMap<String, ObjectId> = BTreeMap::new();
        for (remote_ref, id) in ad.tracked() {
            if let Some(local) = tracking_for(remote_ref) {
                if !self.has_object(id)? {
                    return Err(remote_err(format!(
                        "the remote sent no {id}, which its {remote_ref} names"
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

    /// Brings the tracked `main` into `main`: fast-forwards when it can,
    /// takes the remote's history over a fresh repo's, and otherwise merges
    /// three-way from the merge base with `merge_file` (see
    /// [`Repo::merge_trees`]), committing the merge with two parents.
    /// `prefer` settles conflicting paths when the user has chosen a side.
    pub fn integrate(
        &mut self,
        who: &Signature,
        prefer: Option<Side>,
        merge_file: &mut MergeFile<'_>,
    ) -> Result<Integrated> {
        let main = main_branch();
        let Some(remote) = self.tracked_main()? else {
            return Ok(Integrated::RemoteEmpty);
        };
        let local = self.branch_tip(&main)?;
        let outcome = match local {
            Some(l) if l == remote => Integrated::UpToDate,
            Some(l) if self.is_ancestor(&remote, &l)? => Integrated::Ahead,
            Some(l) if self.is_ancestor(&l, &remote)? => {
                self.update_branch(&main, crate::Tip::At(l), &remote)?;
                Integrated::FastForwarded { to: remote }
            }
            _ if self.is_fresh()? => {
                let tip = local.map_or(crate::Tip::Unborn, crate::Tip::At);
                self.update_branch(&main, tip, &remote)?;
                Integrated::Adopted { to: remote }
            }
            None => unreachable!("an unborn main is fresh"),
            Some(l) => {
                let bases = self.merge_base(&l, &remote)?;
                let Some(base) = bases.first() else {
                    return Ok(Integrated::Unrelated);
                };
                match self.merge_trees(Some(base), &l, &remote, prefer, merge_file)? {
                    TreeMerge::Conflicts(paths) => Integrated::Conflict { paths },
                    TreeMerge::Clean { tree, taken } => {
                        let commit = self.write_merge(&l, &remote, tree, &taken, who)?;
                        self.update_branch(&main, crate::Tip::At(l), &commit)?;
                        Integrated::Merged { commit, taken }
                    }
                }
            }
        };
        self.adopt_tags()?;
        Ok(outcome)
    }

    fn write_merge(
        &mut self,
        ours: &CommitId,
        theirs: &CommitId,
        tree: TreeId,
        taken: &[String],
        who: &Signature,
    ) -> Result<CommitId> {
        let mut message = format!("sync: merge {ORIGIN}/main\n");
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

    /// What `main` and the tags hold that the tracked remote does not, and
    /// the other way.
    pub fn remote_status(&self) -> Result<RemoteStatus> {
        let local = self.branch_tip(&main_branch())?;
        let remote = self.tracked_main()?;
        let mut cache = Default::default();
        let mine = self.ancestors(&local.into_iter().collect::<Vec<_>>(), &mut cache)?;
        let theirs = self.ancestors(&remote.into_iter().collect::<Vec<_>>(), &mut cache)?;
        let tracked = self.tracked_tags()?;
        let tags = self
            .local_tags()?
            .keys()
            .filter(|name| !tracked.contains_key(*name))
            .count();
        Ok(RemoteStatus {
            ahead: mine.difference(&theirs).count(),
            behind: theirs.difference(&mine).count(),
            tags,
        })
    }

    // Pushing.

    /// The body of `POST git-receive-pack` for the remote's `info/refs`
    /// answer: `main` when the remote's is behind it, and every tag the
    /// remote lacks, with a pack of what the remote does not have. Each
    /// update names the remote ref's advertised id as its old one, which is
    /// receive-pack's compare-and-swap: a ref that moved since is refused.
    pub fn push_request(&self, advertised: &[u8]) -> Result<PushRequest> {
        let ad = parse_advertisement(advertised, "git-receive-pack")?;
        if !ad.has_cap("report-status") {
            return Err(remote_err("the remote does not report a push's outcome"));
        }
        let zero = ObjectId::null(SHA1);
        let mut commands: Vec<(ObjectId, ObjectId, String)> = Vec::new();
        if let Some(local) = self.branch_tip(&main_branch())? {
            match ad.refs.get("refs/heads/main") {
                Some(r) if *r == local.0 => {}
                None => commands.push((zero, local.0, "refs/heads/main".to_owned())),
                Some(r) => {
                    let ahead = self.has_object(r)? && self.is_ancestor(&CommitId(*r), &local)?;
                    if !ahead {
                        return Ok(PushRequest::Behind);
                    }
                    commands.push((*r, local.0, "refs/heads/main".to_owned()));
                }
            }
        }
        for (name, id) in self.local_tags()? {
            let path = format!("refs/tags/{name}");
            if !ad.refs.contains_key(&path) {
                commands.push((zero, id, path));
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
        let tips: Vec<ObjectId> = commands.iter().map(|(_, new, _)| *new).collect();
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
        body.extend(pack::write(&objects));
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
            let now = self.read_ref(&local)?;
            if now != Some(*new) {
                self.swap_ref(&local, now, Some(*new))?;
            }
        }
        Ok(out)
    }
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
