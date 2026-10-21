//! Where a bare repo's files live. The crate reads and writes paths inside the
//! git dir (`HEAD`, `objects/ab/cdef…`, `refs/heads/main`) and nothing else, so
//! the browser's OPFS, an in-memory map and a directory on disk are the same
//! repo to it.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;

/// A bare git dir's files, addressed by `/`-separated paths relative to it.
///
/// Synchronous on purpose: OPFS's sync access handles, in a worker, are the
/// browser store this is shaped for.
pub trait Store {
    type Error: std::error::Error + Send + Sync + 'static;

    /// The file's bytes, or `None` when there is no file at `path`.
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Replace the file at `path` with `bytes`, creating parent directories.
    /// A reader sees the old bytes or the new ones, never a torn write.
    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), Self::Error>;

    /// Remove the file at `path`; `false` when there was none.
    fn remove(&mut self, path: &str) -> Result<bool, Self::Error>;

    /// Every file under `dir`, at any depth, as paths relative to the git dir
    /// (`refs/heads/a/b`, not `a/b`). Empty when `dir` does not exist.
    fn list(&self, dir: &str) -> Result<Vec<String>, Self::Error>;

    /// Make sure `dir` exists even with nothing in it. `git` only recognises
    /// a bare repo whose `objects/` and `refs/` exist, so an exported repo
    /// needs them before it has a single object.
    fn create_dir(&mut self, dir: &str) -> Result<(), Self::Error>;
}

/// A repo held in memory; the unit tests' store, and a scratch repo's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemStore {
    files: BTreeMap<String, Vec<u8>>,
    dirs: BTreeSet<String>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every file and its bytes, in path order: what an export would zip.
    pub fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files.iter().map(|(p, b)| (p.as_str(), b.as_slice()))
    }

    /// Every directory made with [`Store::create_dir`].
    pub fn dirs(&self) -> impl Iterator<Item = &str> {
        self.dirs.iter().map(String::as_str)
    }
}

fn under<'a>(dir: &'a str) -> impl Fn(&&String) -> bool + 'a {
    move |path: &&String| {
        dir.is_empty()
            || path
                .strip_prefix(dir.trim_end_matches('/'))
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

impl Store for MemStore {
    type Error = Infallible;

    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, Infallible> {
        Ok(self.files.get(path).cloned())
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), Infallible> {
        self.files.insert(path.to_owned(), bytes.to_vec());
        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<bool, Infallible> {
        Ok(self.files.remove(path).is_some())
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, Infallible> {
        Ok(self.files.keys().filter(under(dir)).cloned().collect())
    }

    fn create_dir(&mut self, dir: &str) -> Result<(), Infallible> {
        self.dirs.insert(dir.trim_end_matches('/').to_owned());
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use fs::FsStore;

#[cfg(not(target_arch = "wasm32"))]
mod fs {
    use std::io::{self, ErrorKind};
    use std::path::{Path, PathBuf};

    use super::Store;

    /// A bare git dir on disk, the one `git -C <dir>` reads.
    #[derive(Debug, Clone)]
    pub struct FsStore {
        root: PathBuf,
    }

    impl FsStore {
        /// The git dir at `root`, which need not exist yet.
        pub fn new(root: impl Into<PathBuf>) -> Self {
            Self { root: root.into() }
        }

        pub fn root(&self) -> &Path {
            &self.root
        }

        fn at(&self, path: &str) -> PathBuf {
            self.root.join(path)
        }
    }

    fn holds_no_files(dir: &Path) -> io::Result<bool> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() || !holds_no_files(&entry.path())? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(root, &path, out)?;
            } else if path.extension().is_none_or(|e| e != "lock") {
                let rel = path.strip_prefix(root).expect("walked from root");
                let rel = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>();
                out.push(rel.join("/"));
            }
        }
        Ok(())
    }

    impl Store for FsStore {
        type Error = io::Error;

        fn read(&self, path: &str) -> io::Result<Option<Vec<u8>>> {
            match std::fs::read(self.at(path)) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }

        fn write(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
            let target = self.at(path);
            // Deleting refs/heads/a/b leaves refs/heads/a/ behind; a later
            // branch `a` has to replace that empty directory, as git's own
            // ref pruning would have.
            if target.is_dir() && holds_no_files(&target)? {
                std::fs::remove_dir_all(&target)?;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // git's own lock-file-then-rename, so a crash never leaves half a
            // ref, and `list` skips the `.lock` if one is left behind.
            let mut lock = target.clone().into_os_string();
            lock.push(".lock");
            std::fs::write(&lock, bytes)?;
            std::fs::rename(&lock, &target)
        }

        fn remove(&mut self, path: &str) -> io::Result<bool> {
            match std::fs::remove_file(self.at(path)) {
                Ok(()) => Ok(true),
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e),
            }
        }

        fn list(&self, dir: &str) -> io::Result<Vec<String>> {
            let mut out = Vec::new();
            walk(&self.root, &self.at(dir), &mut out)?;
            out.sort();
            Ok(out)
        }

        fn create_dir(&mut self, dir: &str) -> io::Result<()> {
            std::fs::create_dir_all(self.at(dir))
        }
    }
}
