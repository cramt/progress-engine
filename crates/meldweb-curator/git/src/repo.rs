use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{Read, Write};

use gix_hash::ObjectId;
use gix_object::bstr::{BStr, BString, ByteSlice};
use gix_object::tree::{self, EntryMode};
use gix_object::{CommitRef, Kind, TagRef, TreeRef, WriteTo};

use crate::names::{
    BlobId, BranchName, CommitId, Identity, RepoPath, Signature, TagId, TagName, Timestamp, TreeId,
};
use crate::store::Store;

const SHA1: gix_hash::Kind = gix_hash::Kind::Sha1;

impl BlobId {
    /// The id `bytes` have as a blob, whether or not a repo holds them.
    pub fn of(bytes: &[u8]) -> Result<BlobId> {
        gix_object::compute_hash(SHA1, Kind::Blob, bytes)
            .map(BlobId)
            .map_err(|e| corrupt("a blob", e))
    }
}

/// What a repo in some other git's layout holds that this crate does not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unsupported {
    /// `objects/pack/`: `git gc` or a clone packed the objects.
    Packfile,
    /// `packed-refs`: `git pack-refs` or a clone moved refs into one file.
    PackedRefs,
    /// A branch or tag ref that is a symbolic ref rather than an id.
    SymbolicRef,
    /// An annotated tag of something other than a commit.
    TagOfNonCommit,
    /// Retargeting a lightweight tag, which has no tagger or label to keep.
    LightweightTag,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the store failed: {0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("there is no git repo here: HEAD is missing")]
    NotARepo,
    #[error("there is a git repo here already")]
    AlreadyARepo,
    #[error("object {0} is missing")]
    MissingObject(ObjectId),
    #[error("object {id} is a {found}, not a {expected}")]
    WrongKind {
        id: ObjectId,
        expected: Kind,
        found: Kind,
    },
    #[error("{what} is corrupt: {detail}")]
    Corrupt { what: String, detail: String },
    #[error("the repo uses {0:?}, which this crate does not read")]
    Unsupported(Unsupported),
    #[error("{reference} was expected at {expected:?} but is at {found:?}")]
    Conflict {
        reference: String,
        expected: Option<ObjectId>,
        found: Option<ObjectId>,
    },
    #[error("{reference} cannot exist beside {existing}: one would be a directory of the other")]
    RefClash { reference: String, existing: String },
    #[error("tag {0} does not exist")]
    NoSuchTag(TagName),
    #[error("{0} is a directory, not a file")]
    NotAFile(RepoPath),
    #[error("{0} is not a directory")]
    NotADirectory(String),
    #[error("{0} cannot be deleted: there is no file there")]
    DeleteMissing(RepoPath),
    #[error("{0} is changed twice in one commit")]
    DuplicatePath(RepoPath),
    /// The remote, or what it answered, is not what git's protocol allows.
    #[error("{0}")]
    Remote(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

fn st<T, E: std::error::Error + Send + Sync + 'static>(r: Result<T, E>) -> Result<T> {
    r.map_err(|e| Error::Store(Box::new(e)))
}

fn corrupt(what: impl std::fmt::Display, detail: impl std::fmt::Display) -> Error {
    Error::Corrupt {
        what: what.to_string(),
        detail: detail.to_string(),
    }
}

fn utf8(what: impl std::fmt::Display, bytes: &BStr) -> Result<String> {
    bytes
        .to_str()
        .map(str::to_owned)
        .map_err(|_| corrupt(what, "it is not UTF-8"))
}

/// What a branch is expected to point at when it is moved: the
/// compare-and-swap's expected side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tip {
    /// The branch must not exist.
    Unborn,
    /// The branch must point at this commit.
    At(CommitId),
}

impl Tip {
    fn id(self) -> Option<ObjectId> {
        match self {
            Tip::Unborn => None,
            Tip::At(c) => Some(c.0),
        }
    }
}

/// One path's change in a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Write the file, making the directories it needs.
    Put { path: RepoPath, bytes: Vec<u8> },
    /// Remove the file, and any directory that leaves empty.
    Delete { path: RepoPath },
}

impl Change {
    fn path(&self) -> &RepoPath {
        match self {
            Change::Put { path, .. } | Change::Delete { path } => path,
        }
    }
}

/// A commit's content, less its parents: the tree is the first parent's (or
/// empty, for a root commit) with `changes` applied in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteCommit {
    pub changes: Vec<Change>,
    pub message: String,
    pub author: Signature,
    pub committer: Signature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub id: CommitId,
    pub tree: TreeId,
    pub parents: Vec<CommitId>,
    pub author: Signature,
    pub committer: Signature,
    pub message: String,
}

/// What a directory entry is, with the id of the object it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File(BlobId),
    Executable(BlobId),
    Symlink(BlobId),
    Dir(TreeId),
    Submodule(CommitId),
}

impl EntryKind {
    fn of(mode: EntryMode, id: ObjectId) -> Self {
        match tree::EntryKind::from(mode) {
            tree::EntryKind::Blob => EntryKind::File(BlobId(id)),
            tree::EntryKind::BlobExecutable => EntryKind::Executable(BlobId(id)),
            tree::EntryKind::Link => EntryKind::Symlink(BlobId(id)),
            tree::EntryKind::Tree => EntryKind::Dir(TreeId(id)),
            tree::EntryKind::Commit => EntryKind::Submodule(CommitId(id)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathChange {
    Added,
    Modified,
    Deleted,
}

/// A commit that changed a path, against its first parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryEntry {
    pub commit: CommitId,
    pub change: PathChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagKind {
    /// A tag object, with its own label and tagger.
    Annotated {
        id: TagId,
        tagger: Option<Signature>,
        message: String,
    },
    /// A ref straight to a commit, as `git tag <name>` without `-a` makes.
    Lightweight,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagInfo {
    pub name: TagName,
    pub commit: CommitId,
    pub kind: TagKind,
}

/// A tree being edited: untouched subtrees stay ids and are never read.
pub(crate) enum Dir {
    Unchanged(TreeId),
    Edited(BTreeMap<BString, Node>),
}

pub(crate) enum Node {
    Leaf { mode: EntryMode, id: ObjectId },
    Dir(Dir),
}

/// A bare repo over `S`. Reads verify every object's hash and kind; writes
/// are loose objects, zlib-compressed, as `git` itself writes them.
///
/// One writer at a time: ref updates compare-and-swap against the caller's
/// expectation, but the check and the write are two store calls.
#[derive(Debug)]
pub struct Repo<S> {
    store: S,
}

impl<S: Store> Repo<S> {
    /// Make an empty bare repo whose HEAD is the unborn `main`.
    pub fn init(mut store: S) -> Result<Self> {
        if st(store.read("HEAD"))?.is_some() {
            return Err(Error::AlreadyARepo);
        }
        for dir in ["objects/info", "objects/pack", "refs/heads", "refs/tags"] {
            st(store.create_dir(dir))?;
        }
        st(store.write(
            "config",
            b"[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n",
        ))?;
        st(store.write("HEAD", b"ref: refs/heads/main\n"))?;
        Ok(Self { store })
    }

    pub fn open(store: S) -> Result<Self> {
        if st(store.read("HEAD"))?.is_none() {
            return Err(Error::NotARepo);
        }
        Ok(Self { store })
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    /// The store itself, for a caller that keeps something of its own
    /// beside the repo's files, such as a journal of what was written.
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    pub fn into_store(self) -> S {
        self.store
    }

    /// The branch HEAD names: the repo's default branch.
    pub fn head_branch(&self) -> Result<BranchName> {
        let head = st(self.store.read("HEAD"))?.ok_or(Error::NotARepo)?;
        let name = head
            .strip_prefix(b"ref: refs/heads/")
            .and_then(|rest| rest.strip_suffix(b"\n"))
            .ok_or_else(|| corrupt("HEAD", "it does not name a branch"))?;
        let name = name
            .to_str()
            .map_err(|_| corrupt("HEAD", "it is not UTF-8"))?;
        BranchName::new(name).map_err(|e| corrupt("HEAD", e))
    }

    /// A file of the git dir itself, such as `config`.
    pub(crate) fn read_file_in_git_dir(&self, path: &str) -> Result<Option<Vec<u8>>> {
        st(self.store.read(path))
    }

    pub(crate) fn write_file_in_git_dir(&mut self, path: &str, bytes: &[u8]) -> Result<()> {
        st(self.store.write(path, bytes))
    }

    // Objects.

    pub(crate) fn object_path(id: &ObjectId) -> String {
        let hex = id.to_string();
        format!("objects/{}/{}", &hex[..2], &hex[2..])
    }

    pub(crate) fn write_object(&mut self, kind: Kind, data: &[u8]) -> Result<ObjectId> {
        let id = gix_object::compute_hash(SHA1, kind, data).map_err(|e| corrupt("an object", e))?;
        let path = Self::object_path(&id);
        if st(self.store.read(&path))?.is_some() {
            return Ok(id);
        }
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&gix_object::encode::loose_header(kind, data.len() as u64))
            .and_then(|()| z.write_all(data))
            .expect("writing to a Vec cannot fail");
        let compressed = z.finish().expect("writing to a Vec cannot fail");
        st(self.store.write(&path, &compressed))?;
        Ok(id)
    }

    fn write_gix(&mut self, object: &dyn WriteTo) -> Result<ObjectId> {
        let mut data = Vec::with_capacity(object.size() as usize);
        object
            .write_to(&mut data)
            .map_err(|e| corrupt("an object being written", e))?;
        self.write_object(object.kind(), &data)
    }

    /// The object's kind and data, its hash checked.
    fn read_raw(&self, id: &ObjectId) -> Result<(Kind, Vec<u8>)> {
        let Some(compressed) = st(self.store.read(&Self::object_path(id)))? else {
            if !st(self.store.list("objects/pack"))?.is_empty() {
                return Err(Error::Unsupported(Unsupported::Packfile));
            }
            return Err(Error::MissingObject(*id));
        };
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(compressed.as_slice())
            .read_to_end(&mut raw)
            .map_err(|e| corrupt(id, e))?;
        let (kind, size, header) =
            gix_object::decode::loose_header(&raw).map_err(|e| corrupt(id, e))?;
        let data = raw.split_off(header);
        if data.len() as u64 != size {
            return Err(corrupt(
                id,
                format!("its header says {size} bytes but it holds {}", data.len()),
            ));
        }
        let actual = gix_object::compute_hash(SHA1, kind, &data).map_err(|e| corrupt(id, e))?;
        if actual != *id {
            return Err(corrupt(id, format!("its content hashes to {actual}")));
        }
        Ok((kind, data))
    }

    pub(crate) fn read_object(&self, id: &ObjectId, expected: Kind) -> Result<Vec<u8>> {
        let (kind, data) = self.read_raw(id)?;
        if kind != expected {
            return Err(Error::WrongKind {
                id: *id,
                expected,
                found: kind,
            });
        }
        Ok(data)
    }

    /// The object's kind and data, or `None` when the repo lacks it.
    pub(crate) fn read_any(&self, id: &ObjectId) -> Result<Option<(Kind, Vec<u8>)>> {
        if !self.has_object(id)? {
            return Ok(None);
        }
        self.read_raw(id).map(Some)
    }

    pub(crate) fn has_object(&self, id: &ObjectId) -> Result<bool> {
        Ok(st(self.store.read(&Self::object_path(id)))?.is_some())
    }

    pub fn read_blob(&self, id: &BlobId) -> Result<Vec<u8>> {
        self.read_object(&id.0, Kind::Blob)
    }

    pub fn read_commit(&self, id: &CommitId) -> Result<CommitInfo> {
        let data = self.read_object(&id.0, Kind::Commit)?;
        let commit = CommitRef::from_bytes(&data, SHA1).map_err(|e| corrupt(id, e))?;
        let signature = |raw: Result<gix_actor::SignatureRef<'_>, _>| -> Result<Signature> {
            let raw = raw.map_err(|e| corrupt(id, e))?;
            let time = raw.time().map_err(|e| corrupt(id, e))?;
            Ok(Signature {
                who: Identity::read(utf8(id, raw.name)?, utf8(id, raw.email)?),
                when: Timestamp::read(time.seconds, time.offset),
            })
        };
        Ok(CommitInfo {
            id: *id,
            tree: TreeId(commit.tree()),
            parents: commit.parents().map(CommitId).collect(),
            author: signature(commit.author())?,
            committer: signature(commit.committer())?,
            message: utf8(id, commit.message)?,
        })
    }

    pub(crate) fn parents(&self, id: &CommitId) -> Result<Vec<CommitId>> {
        let data = self.read_object(&id.0, Kind::Commit)?;
        let commit = CommitRef::from_bytes(&data, SHA1).map_err(|e| corrupt(id, e))?;
        Ok(commit.parents().map(CommitId).collect())
    }

    pub(crate) fn tree_of(&self, id: &CommitId) -> Result<TreeId> {
        let data = self.read_object(&id.0, Kind::Commit)?;
        let commit = CommitRef::from_bytes(&data, SHA1).map_err(|e| corrupt(id, e))?;
        Ok(TreeId(commit.tree()))
    }

    pub(crate) fn tree_entries(&self, id: &TreeId) -> Result<Vec<(BString, EntryMode, ObjectId)>> {
        let data = self.read_object(&id.0, Kind::Tree)?;
        let tree = TreeRef::from_bytes(&data, SHA1).map_err(|e| corrupt(id, e))?;
        Ok(tree
            .entries
            .into_iter()
            .map(|e| (e.filename.to_owned(), e.mode, e.oid.to_owned()))
            .collect())
    }

    /// The entry at `path` in `tree`, or `None` when some component is
    /// missing or is a file.
    fn entry_at(&self, tree: TreeId, path: &RepoPath) -> Result<Option<(EntryMode, ObjectId)>> {
        let mut found = (EntryMode::from(tree::EntryKind::Tree), tree.0);
        for component in path.components() {
            if !found.0.is_tree() {
                return Ok(None);
            }
            let entries = self.tree_entries(&TreeId(found.1))?;
            match entries.into_iter().find(|(name, ..)| name == component) {
                Some((_, mode, id)) => found = (mode, id),
                None => return Ok(None),
            }
        }
        Ok(Some(found))
    }

    // Reading a commit's files.

    /// The file's bytes at `at`, or `None` when there is no file there.
    pub fn read_file(&self, at: &CommitId, path: &RepoPath) -> Result<Option<Vec<u8>>> {
        self.read_file_in(self.tree_of(at)?, path)
    }

    /// The file's bytes in `tree`, such as a merge's before it is committed.
    pub fn read_file_in(&self, tree: TreeId, path: &RepoPath) -> Result<Option<Vec<u8>>> {
        match self.entry_at(tree, path)? {
            None => Ok(None),
            Some((mode, id)) => match EntryKind::of(mode, id) {
                EntryKind::File(blob) | EntryKind::Executable(blob) => {
                    self.read_blob(&blob).map(Some)
                }
                _ => Err(Error::NotAFile(path.clone())),
            },
        }
    }

    /// What is at `path` at `at`, with its object's id; `None` when nothing
    /// is. A file's id is the blob id a caller can guard a later write with.
    pub fn entry(&self, at: &CommitId, path: &RepoPath) -> Result<Option<EntryKind>> {
        Ok(self
            .entry_at(self.tree_of(at)?, path)?
            .map(|(mode, id)| EntryKind::of(mode, id)))
    }

    /// What is at `path` in `tree`, as [`Repo::entry`] says it at a commit,
    /// for a caller that has read the commit already.
    pub fn entry_in(&self, tree: TreeId, path: &RepoPath) -> Result<Option<EntryKind>> {
        Ok(self
            .entry_at(tree, path)?
            .map(|(mode, id)| EntryKind::of(mode, id)))
    }

    /// The entries of the directory at `dir` (the root when `None`) at `at`,
    /// in git's tree order; `None` when there is no directory there.
    pub fn list_dir(&self, at: &CommitId, dir: Option<&RepoPath>) -> Result<Option<Vec<DirEntry>>> {
        let root = self.tree_of(at)?;
        let tree = match dir {
            None => root,
            Some(path) => match self.entry_at(root, path)? {
                None => return Ok(None),
                Some((mode, id)) if mode.is_tree() => TreeId(id),
                Some(_) => return Err(Error::NotADirectory(path.to_string())),
            },
        };
        self.tree_entries(&tree)?
            .into_iter()
            .map(|(name, mode, id)| {
                Ok(DirEntry {
                    name: utf8(format!("tree {}", tree), name.as_bstr())?,
                    kind: EntryKind::of(mode, id),
                })
            })
            .collect::<Result<_>>()
            .map(Some)
    }

    // Writing commits.

    fn edit(&mut self, dir: &mut Dir, components: &[&str], change: &Change) -> Result<()> {
        if let Dir::Unchanged(id) = dir {
            let entries = self.tree_entries(id)?;
            *dir = Dir::Edited(
                entries
                    .into_iter()
                    .map(|(name, mode, id)| {
                        let node = if mode.is_tree() {
                            Node::Dir(Dir::Unchanged(TreeId(id)))
                        } else {
                            Node::Leaf { mode, id }
                        };
                        (name, node)
                    })
                    .collect(),
            );
        }
        let Dir::Edited(entries) = dir else {
            unreachable!("loaded above")
        };
        let (name, rest) = components.split_first().expect("a RepoPath is never empty");
        let key = BString::from(*name);
        match (rest.is_empty(), change, entries.get_mut(&key)) {
            (true, Change::Put { bytes, .. }, None | Some(Node::Leaf { .. })) => {
                // A file that was executable stays so when its bytes change.
                let mode = match entries.get(&key) {
                    Some(Node::Leaf { mode, .. }) if mode.is_executable() => *mode,
                    _ => tree::EntryKind::Blob.into(),
                };
                let id = self.write_object(Kind::Blob, bytes)?;
                entries.insert(key, Node::Leaf { mode, id });
            }
            (true, _, Some(Node::Dir(_))) => return Err(Error::NotAFile(change.path().clone())),
            (true, Change::Delete { .. }, Some(Node::Leaf { .. })) => {
                entries.remove(&key);
            }
            (_, Change::Delete { path }, None) => return Err(Error::DeleteMissing(path.clone())),
            (false, Change::Delete { path }, Some(Node::Leaf { .. })) => {
                return Err(Error::DeleteMissing(path.clone()))
            }
            (false, Change::Put { path, .. }, Some(Node::Leaf { .. })) => {
                let depth = path.components().count() - rest.len();
                return Err(Error::NotADirectory(
                    path.components().take(depth).collect::<Vec<_>>().join("/"),
                ));
            }
            (false, Change::Put { .. }, None) => {
                let mut child = Dir::Edited(BTreeMap::new());
                self.edit(&mut child, rest, change)?;
                entries.insert(key, Node::Dir(child));
            }
            (false, _, Some(Node::Dir(child))) => {
                self.edit(child, rest, change)?;
                if matches!(child, Dir::Edited(e) if e.is_empty()) {
                    entries.remove(&key);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn write_dir(&mut self, dir: Dir) -> Result<TreeId> {
        let entries = match dir {
            Dir::Unchanged(id) => return Ok(id),
            Dir::Edited(entries) => entries,
        };
        let mut tree = gix_object::Tree::empty();
        for (filename, node) in entries {
            let (mode, oid) = match node {
                Node::Leaf { mode, id } => (mode, id),
                Node::Dir(child) => (tree::EntryKind::Tree.into(), self.write_dir(child)?.0),
            };
            tree.entries.push(tree::Entry {
                mode,
                filename,
                oid,
            });
        }
        // git's order sorts a directory as if its name ended in `/`.
        tree.entries.sort();
        self.write_gix(&tree).map(TreeId)
    }

    /// Write a commit with these parents and no ref pointing at it. Its tree
    /// is the first parent's with `commit.changes` applied.
    pub fn write_commit(&mut self, parents: &[CommitId], commit: &WriteCommit) -> Result<CommitId> {
        let mut seen = HashSet::new();
        for change in &commit.changes {
            if !seen.insert(change.path()) {
                return Err(Error::DuplicatePath(change.path().clone()));
            }
        }
        let mut root = match parents.first() {
            Some(first) => Dir::Unchanged(self.tree_of(first)?),
            None => Dir::Edited(BTreeMap::new()),
        };
        for change in &commit.changes {
            let components = change.path().components().collect::<Vec<_>>();
            self.edit(&mut root, &components, change)?;
        }
        let tree = self.write_dir(root)?;
        let object = gix_object::Commit {
            tree: tree.0,
            parents: parents.iter().map(|p| p.0).collect::<Vec<_>>().into(),
            author: commit.author.to_gix(),
            committer: commit.committer.to_gix(),
            encoding: None,
            message: commit.message.as_str().into(),
            extra_headers: Vec::new(),
        };
        self.write_gix(&object).map(CommitId)
    }

    /// Writes `bytes` as a blob, which nothing names until a tree does.
    pub fn write_blob(&mut self, bytes: &[u8]) -> Result<BlobId> {
        self.write_object(Kind::Blob, bytes).map(BlobId)
    }

    /// A tree of exactly these regular files, by path, with no ref or
    /// commit naming it.
    pub fn write_files(&mut self, files: &[(RepoPath, BlobId)]) -> Result<TreeId> {
        let mut flat = BTreeMap::new();
        for (path, blob) in files {
            if flat
                .insert(
                    path.as_str().to_owned(),
                    (EntryMode::from(tree::EntryKind::Blob), blob.0),
                )
                .is_some()
            {
                return Err(Error::DuplicatePath(path.clone()));
            }
        }
        self.write_flat(&flat)
    }

    /// A commit of `tree` with exactly these parents, signatures and
    /// message, and no ref pointing at it: what replaying history into
    /// another line needs, where the tree is not the first parent's edited.
    pub fn write_commit_of(
        &mut self,
        tree: TreeId,
        parents: &[CommitId],
        author: &Signature,
        committer: &Signature,
        message: &str,
    ) -> Result<CommitId> {
        self.read_object(&tree.0, Kind::Tree)?;
        let object = gix_object::Commit {
            tree: tree.0,
            parents: parents.iter().map(|p| p.0).collect::<Vec<_>>().into(),
            author: author.to_gix(),
            committer: committer.to_gix(),
            encoding: None,
            message: message.into(),
            extra_headers: Vec::new(),
        };
        self.write_gix(&object).map(CommitId)
    }

    /// Commit onto `branch`, whose tip must be `tip`; the tip is the new
    /// commit's only parent, and `Tip::Unborn` makes a root commit.
    pub fn commit(
        &mut self,
        branch: &BranchName,
        tip: Tip,
        commit: &WriteCommit,
    ) -> Result<CommitId> {
        self.check_ref(&branch.ref_path(), tip.id())?;
        let parents = match tip {
            Tip::Unborn => vec![],
            Tip::At(c) => vec![c],
        };
        let id = self.write_commit(&parents, commit)?;
        self.update_branch(branch, tip, &id)?;
        Ok(id)
    }

    /// Commit a merge onto `branch`: parents `tip` then `merged`, the tree
    /// `tip`'s with `commit.changes` applied. Content merging is the caller's.
    pub fn commit_merge(
        &mut self,
        branch: &BranchName,
        tip: &CommitId,
        merged: &CommitId,
        commit: &WriteCommit,
    ) -> Result<CommitId> {
        self.check_ref(&branch.ref_path(), Some(tip.0))?;
        let id = self.write_commit(&[*tip, *merged], commit)?;
        self.update_branch(branch, Tip::At(*tip), &id)?;
        Ok(id)
    }

    // Refs.

    fn packed_refs_refused(&self) -> Result<()> {
        if st(self.store.read("packed-refs"))?.is_some() {
            return Err(Error::Unsupported(Unsupported::PackedRefs));
        }
        Ok(())
    }

    pub(crate) fn read_ref(&self, path: &str) -> Result<Option<ObjectId>> {
        self.packed_refs_refused()?;
        let Some(bytes) = st(self.store.read(path))? else {
            return Ok(None);
        };
        if bytes.starts_with(b"ref: ") {
            return Err(Error::Unsupported(Unsupported::SymbolicRef));
        }
        let hex = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
        ObjectId::from_hex(hex)
            .map(Some)
            .map_err(|e| corrupt(path, e))
    }

    pub(crate) fn check_ref(&self, path: &str, expected: Option<ObjectId>) -> Result<()> {
        let found = self.read_ref(path)?;
        if found != expected {
            return Err(Error::Conflict {
                reference: path.to_owned(),
                expected,
                found,
            });
        }
        Ok(())
    }

    /// Point `path` at `new` (or remove it) if it is at `expected` now.
    pub(crate) fn swap_ref(
        &mut self,
        path: &str,
        expected: Option<ObjectId>,
        new: Option<ObjectId>,
    ) -> Result<()> {
        self.check_ref(path, expected)?;
        if expected.is_none() {
            self.refuse_clash(path)?;
        }
        match new {
            Some(id) => st(self.store.write(path, format!("{id}\n").as_bytes())),
            None => st(self.store.remove(path)).map(drop),
        }
    }

    /// `refs/heads/a` and `refs/heads/a/b` cannot both exist: on disk one
    /// would be a file where the other needs a directory.
    fn refuse_clash(&self, path: &str) -> Result<()> {
        let clash = |existing: &str| Error::RefClash {
            reference: path.to_owned(),
            existing: existing.to_owned(),
        };
        if let Some(existing) = st(self.store.list(path))?.first() {
            return Err(clash(existing));
        }
        let parts = path.split('/').collect::<Vec<_>>();
        for end in 3..parts.len() {
            let ancestor = parts[..end].join("/");
            if st(self.store.read(&ancestor))?.is_some() {
                return Err(clash(&ancestor));
            }
        }
        Ok(())
    }

    pub(crate) fn refs_under(&self, dir: &str) -> Result<Vec<(String, ObjectId)>> {
        self.packed_refs_refused()?;
        let prefix = format!("{dir}/");
        st(self.store.list(dir))?
            .into_iter()
            .map(|path| {
                let id = self.read_ref(&path)?.expect("listed, so present");
                let name = path
                    .strip_prefix(&prefix)
                    .expect("listed under dir")
                    .to_owned();
                Ok((name, id))
            })
            .collect()
    }

    pub fn branch_tip(&self, branch: &BranchName) -> Result<Option<CommitId>> {
        Ok(self.read_ref(&branch.ref_path())?.map(CommitId))
    }

    /// Every branch and its tip, by name.
    pub fn branches(&self) -> Result<Vec<(BranchName, CommitId)>> {
        let mut out = self
            .refs_under("refs/heads")?
            .into_iter()
            .map(|(name, id)| {
                let branch = BranchName::new(name).map_err(|e| corrupt("a branch ref", e))?;
                Ok((branch, CommitId(id)))
            })
            .collect::<Result<Vec<_>>>()?;
        out.sort();
        Ok(out)
    }

    /// Move `branch` from `expected` to `new`; `Tip::Unborn` creates it.
    pub fn update_branch(
        &mut self,
        branch: &BranchName,
        expected: Tip,
        new: &CommitId,
    ) -> Result<()> {
        self.read_object(&new.0, Kind::Commit)?;
        self.swap_ref(&branch.ref_path(), expected.id(), Some(new.0))
    }

    pub fn create_branch(&mut self, branch: &BranchName, at: &CommitId) -> Result<()> {
        self.update_branch(branch, Tip::Unborn, at)
    }

    pub fn delete_branch(&mut self, branch: &BranchName, expected: &CommitId) -> Result<()> {
        self.swap_ref(&branch.ref_path(), Some(expected.0), None)
    }

    // Tags.

    /// Write an annotated tag of `target` and its ref; refused with
    /// [`Error::Conflict`] when the tag exists.
    pub fn create_tag(
        &mut self,
        name: &TagName,
        target: &CommitId,
        tagger: &Signature,
        message: &str,
    ) -> Result<TagId> {
        self.check_ref(&name.ref_path(), None)?;
        self.read_object(&target.0, Kind::Commit)?;
        let tag = gix_object::Tag {
            target: target.0,
            target_kind: Kind::Commit,
            name: name.as_str().into(),
            tagger: Some(tagger.to_gix()),
            message: message.into(),
            signature: None,
        };
        let id = self.write_gix(&tag)?;
        self.swap_ref(&name.ref_path(), None, Some(id))?;
        Ok(TagId(id))
    }

    fn tag_info(&self, name: TagName, id: ObjectId) -> Result<TagInfo> {
        let data = match self.read_object(&id, Kind::Tag) {
            Err(Error::WrongKind {
                found: Kind::Commit,
                ..
            }) => {
                return Ok(TagInfo {
                    name,
                    commit: CommitId(id),
                    kind: TagKind::Lightweight,
                });
            }
            other => other?,
        };
        let tag = TagRef::from_bytes(&data, SHA1).map_err(|e| corrupt(id, e))?;
        if tag.target_kind != Kind::Commit {
            return Err(Error::Unsupported(Unsupported::TagOfNonCommit));
        }
        let tagger = match tag.tagger().map_err(|e| corrupt(id, e))? {
            None => None,
            Some(raw) => {
                let time = raw.time().map_err(|e| corrupt(id, e))?;
                Some(Signature {
                    who: Identity::read(utf8(id, raw.name)?, utf8(id, raw.email)?),
                    when: Timestamp::read(time.seconds, time.offset),
                })
            }
        };
        Ok(TagInfo {
            name,
            commit: CommitId(tag.target()),
            kind: TagKind::Annotated {
                id: TagId(id),
                tagger,
                message: utf8(id, tag.message)?,
            },
        })
    }

    /// Every tag whose name starts with `prefix` (`""` for all), by name.
    pub fn tags(&self, prefix: &str) -> Result<Vec<TagInfo>> {
        let mut out = self
            .refs_under("refs/tags")?
            .into_iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .map(|(name, id)| {
                let name = TagName::new(name).map_err(|e| corrupt("a tag ref", e))?;
                self.tag_info(name, id)
            })
            .collect::<Result<Vec<_>>>()?;
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Points the annotated tag `name` at `target` with a new tag object
    /// that keeps its tagger and message, so the same retargeting gives the
    /// same id wherever it is made. The ref moves only from the object it
    /// was read at.
    pub fn retarget_tag(&mut self, name: &TagName, target: &CommitId) -> Result<TagId> {
        let Some(old) = self.read_ref(&name.ref_path())? else {
            return Err(Error::NoSuchTag(name.clone()));
        };
        let info = self.tag_info(name.clone(), old)?;
        let TagKind::Annotated {
            tagger, message, ..
        } = info.kind
        else {
            return Err(Error::Unsupported(Unsupported::LightweightTag));
        };
        self.read_object(&target.0, Kind::Commit)?;
        let tag = gix_object::Tag {
            target: target.0,
            target_kind: Kind::Commit,
            name: name.as_str().into(),
            tagger: tagger.map(|t| t.to_gix()),
            message: message.as_str().into(),
            signature: None,
        };
        let id = self.write_gix(&tag)?;
        if id != old {
            self.swap_ref(&name.ref_path(), Some(old), Some(id))?;
        }
        Ok(TagId(id))
    }

    /// Remove the tag's ref. Its object stays, unreachable, as with `git tag -d`.
    pub fn delete_tag(&mut self, name: &TagName) -> Result<()> {
        let Some(id) = self.read_ref(&name.ref_path())? else {
            return Err(Error::NoSuchTag(name.clone()));
        };
        self.swap_ref(&name.ref_path(), Some(id), None)
    }

    // History.

    /// The commits on `branch`'s first-parent chain whose entry at `path`
    /// differs from their first parent's, newest first. A deletion counts.
    pub fn history(&self, branch: &BranchName, path: &RepoPath) -> Result<Vec<HistoryEntry>> {
        let mut out = Vec::new();
        let mut next = self.branch_tip(branch)?;
        let key = |entry: Option<(EntryMode, ObjectId)>| entry.map(|(mode, id)| (mode.value(), id));
        while let Some(commit) = next {
            let tree = self.tree_of(&commit)?;
            let parent = self.parents(&commit)?.first().copied();
            let parent_tree = parent.map(|p| self.tree_of(&p)).transpose()?;
            if parent_tree != Some(tree) {
                let now = key(self.entry_at(tree, path)?);
                let before = match parent_tree {
                    Some(t) => key(self.entry_at(t, path)?),
                    None => None,
                };
                let change = match (before, now) {
                    (None, Some(_)) => Some(PathChange::Added),
                    (Some(_), None) => Some(PathChange::Deleted),
                    (Some(b), Some(n)) if b != n => Some(PathChange::Modified),
                    _ => None,
                };
                if let Some(change) = change {
                    out.push(HistoryEntry { commit, change });
                }
            }
            next = parent;
        }
        Ok(out)
    }

    pub(crate) fn ancestors(
        &self,
        from: &[CommitId],
        cache: &mut HashMap<CommitId, Vec<CommitId>>,
    ) -> Result<HashSet<CommitId>> {
        let mut seen = HashSet::new();
        let mut queue = from.iter().copied().collect::<VecDeque<_>>();
        while let Some(c) = queue.pop_front() {
            if !seen.insert(c) {
                continue;
            }
            let parents = match cache.entry(c) {
                std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                std::collections::hash_map::Entry::Vacant(e) => e.insert(self.parents(&c)?),
            };
            queue.extend(parents.iter().copied());
        }
        Ok(seen)
    }

    /// The best common ancestors of `a` and `b` over all parents, as
    /// `git merge-base --all` gives them: common ancestors that are no other
    /// common ancestor's ancestor. Empty when the histories are unrelated;
    /// more than one after a criss-cross merge. Sorted by id.
    pub fn merge_bases(&self, a: &CommitId, b: &CommitId) -> Result<Vec<CommitId>> {
        self.merge_bases_in(&mut Parents::default(), a, b)
    }

    fn merge_bases_in(
        &self,
        parents: &mut Parents,
        a: &CommitId,
        b: &CommitId,
    ) -> Result<Vec<CommitId>> {
        let cache = &mut parents.0;
        let of_a = self.ancestors(&[*a], cache)?;
        let of_b = self.ancestors(&[*b], cache)?;
        let common = of_a.intersection(&of_b).copied().collect::<Vec<_>>();
        let parents_of_common = common
            .iter()
            .flat_map(|c| cache[c].iter().copied())
            .collect::<Vec<_>>();
        let below = self.ancestors(&parents_of_common, cache)?;
        let mut best = common
            .into_iter()
            .filter(|c| !below.contains(c))
            .collect::<Vec<_>>();
        best.sort();
        Ok(best)
    }

    /// The one merge base every merge of `a` and `b` starts from, and blame
    /// reads a merge against: of [`Repo::merge_bases`], the newest by
    /// committer time, and of those committed in the same second the lowest
    /// id. After a criss-cross the bases disagree and which one a merge
    /// starts from decides what it makes, so there is exactly one rule, here.
    /// The newest is the one that has seen the most of both sides, as git's
    /// own walk by commit date tends to find first. `None` when the
    /// histories are unrelated.
    pub fn merge_base(&self, a: &CommitId, b: &CommitId) -> Result<Option<CommitId>> {
        self.merge_base_in(&mut Parents::default(), a, b)
    }

    /// [`Repo::merge_base`], reading each commit's parents once across
    /// every call given the same `parents`: blame asks it of every merge it
    /// walks through, over the same history.
    pub fn merge_base_in(
        &self,
        parents: &mut Parents,
        a: &CommitId,
        b: &CommitId,
    ) -> Result<Option<CommitId>> {
        let mut newest: Option<(i64, CommitId)> = None;
        for base in self.merge_bases_in(parents, a, b)? {
            let when = self.read_commit(&base)?.committer.when.seconds();
            // Sorted by id, so the first of a second is kept.
            if newest.is_none_or(|(w, _)| when > w) {
                newest = Some((when, base));
            }
        }
        Ok(newest.map(|(_, base)| base))
    }
}

/// Each commit's parents as read once, for [`Repo::merge_base_in`].
#[derive(Debug, Default)]
pub struct Parents(HashMap<CommitId, Vec<CommitId>>);
