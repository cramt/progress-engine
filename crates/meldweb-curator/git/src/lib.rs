//! A bare git repository of loose objects, over a [`Store`] the caller
//! provides: Meldweb Curator's offline repo, which lives in the browser's
//! OPFS and travels as a zip of the bare dir. Every object is a real git
//! object, so `git` reads an exported repo and ids match across repos.
//!
//! It refuses packfiles, packed refs, the index, a working tree and network
//! transport, and does no content merge.

mod names;
mod repo;
mod store;

pub use names::{
    BlobId, BranchName, CommitId, Identity, Invalid, RepoPath, Signature, TagId, TagName,
    Timestamp, TreeId,
};
pub use repo::{
    Change, CommitInfo, DirEntry, EntryKind, Error, HistoryEntry, PathChange, Repo, Result,
    TagInfo, TagKind, Tip, Unsupported, WriteCommit,
};
#[cfg(not(target_arch = "wasm32"))]
pub use store::FsStore;
pub use store::{MemStore, Store};
