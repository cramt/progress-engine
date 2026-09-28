//! The Delver X engine's files, fetched at build time and pinned by hash.
//!
//! The origin sends no CORS headers, so a page on any other origin cannot
//! `fetch` the engine from Delver - it has to serve its own copy. This crate is
//! that copy: its build script downloads the pinned build once, checks every
//! byte against [`PINNED`], unpacks the weights, and leaves the lot in [`dir`],
//! named as a page serves them. A web build copies that directory next to its
//! own output and points `gitaxian-probe-engine`'s web host at wherever it ends
//! up.
//!
//! Nothing here is compiled into a binary. The files are ~80 MB; they belong
//! beside the app, where a browser can cache them apart from it.
//!
//! The build downloads unless told otherwise. `GITAXIAN_PROBE_ASSETS_FROM=<dir>`
//! takes the files from a directory instead (still checked against the pin),
//! `GITAXIAN_PROBE_ARCHIVE_TOKEN=<token>` takes them from [`ARCHIVE`]'s release
//! for the pin, which outlives upstream serving it (the engine README, *The archive*), and
//! `GITAXIAN_PROBE_OFFLINE=1` refuses to touch the network.

use std::io;
use std::path::Path;

include!("pin.rs");

/// Where the build left the served files: [`SERVED`], and nothing else.
pub fn dir() -> &'static Path {
    Path::new(env!("GITAXIAN_PROBE_ASSETS_DIR"))
}

/// Copy the served files into `dest`, creating it. For a web build's own build
/// step, which knows where its output goes and this crate does not.
pub fn copy_to(dest: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for name in SERVED {
        std::fs::copy(dir().join(name), dest.join(name))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_served_file_is_there() {
        for name in SERVED {
            let path = dir().join(name);
            assert!(path.is_file(), "{} is missing", path.display());
        }
    }

    #[test]
    fn the_version_served_is_the_version_pinned() {
        let served = std::fs::read_to_string(dir().join("version.txt")).unwrap();
        assert_eq!(served.trim(), VERSION);
    }
}
