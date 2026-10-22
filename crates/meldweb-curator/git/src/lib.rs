//! A bare git repository of loose objects, over a [`Store`] the caller
//! provides: Meldweb Curator's offline repo, which lives in the browser's
//! OPFS and travels as a zip of the bare dir. Every object is a real git
//! object, so `git` reads an exported repo and ids match across repos.
//!
//! It refuses packfiles and packed refs on disk, the index and a working
//! tree. It has one remote, `origin`, spoken to over git's smart HTTP
//! protocol as pure functions over bytes ([`remote`]'s docs), and merges
//! three-way by lines.

mod merge;
mod names;
mod pack;
mod pktline;
mod remote;
mod repo;
mod store;

pub use merge::{merge_lines, merge_lines_with, Hunk, MergeFile, Planned, Side, TreeMerge};
pub use pktline::PktError;
pub use remote::{FetchRequest, Integrated, PushRequest, Pushed, RemoteStatus, RemoteUrl, ORIGIN};

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
