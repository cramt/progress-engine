//! Upstream's LZMA2 archives, which both hosts unpack: the native one into its
//! artefact cache, the web one in the page, since the files a page serves are
//! the archive's as upstream shipped them.

use std::io::Cursor;

use anyhow::{anyhow, Context, Result};

/// Pull the single named file out of one of upstream's LZMA2 archives.
pub(crate) fn unpack(packed: &[u8], want: &str) -> Result<Vec<u8>> {
    let mut reader =
        sevenz_rust2::ArchiveReader::new(Cursor::new(packed), sevenz_rust2::Password::empty())
            .map_err(|e| anyhow!("{e}"))
            .context("reading the archive header")?;

    let mut found = None;
    let mut failed = None;
    reader
        .for_each_entries(|entry, contents| {
            if entry.name() != want {
                return Ok(true);
            }
            let mut out = Vec::with_capacity(entry.size() as usize);
            match contents.read_to_end(&mut out) {
                Ok(_) => found = Some(out),
                Err(e) => failed = Some(e),
            }
            Ok(false)
        })
        .map_err(|e| anyhow!("{e}"))?;

    if let Some(e) = failed {
        return Err(e).context("decompressing the entry");
    }
    found.ok_or_else(|| anyhow!("the archive holds no {want}"))
}
