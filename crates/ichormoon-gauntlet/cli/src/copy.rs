//! The copy of Scryfall on disk: the native driver for
//! [`chip_scryfall::copy::store`] (ADR-0031).
//!
//! The store decides which slot a new copy goes in, whether the copy kept is
//! current and which file was already refused. This does what it names and
//! nothing else: reads and writes the files, downloads Scryfall's bulk files
//! a few megabytes at a time, and holds a file lock where the browser holds a
//! Web Lock. The layout is the browser's, file for file, in a directory of
//! its own.
//!
//! Natively nothing refreshes on its own (#142, decision 4), so this never
//! asks [`Store::due`]: `gauntlet sync` asks Scryfall because somebody ran it.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use anyhow::{bail, Context, Result};
use chip_scryfall::copy::store::{Build, Meta, Plan, Store, UnixMs, META, UNREADABLE};
use chip_scryfall::copy::{Builder, ScryfallCopy};
use chip_scryfall::index::Index;

use crate::sync::{bulk_listing, iso8601, megabytes, USER_AGENT};

/// Held by the sync that is building, so a second waits for it and then
/// finds the copy current rather than downloading it again.
const LOCK: &str = "scryfall-copy.lock";

/// How much of a bulk file the builder is handed at once: enough that a call
/// per chunk costs nothing, little enough that the 630 MB file is never near
/// whole in memory.
const CHUNK: usize = 4 << 20;

/// Fewer distinct cards than this is a truncated download that happened to
/// end on a line break, not Scryfall's card pool, which has been near forty
/// thousand for years. Only a download is held to it: a file named with
/// `--cards-from` may be deliberately small.
const MINIMUM_PLAUSIBLE_CARDS: usize = 20_000;

/// Where the copy is kept unless `--copy` says otherwise: `scryfall-copy/`
/// in the directory [`Index::default_path`] writes the index to.
pub fn default_dir() -> PathBuf {
    let index = Index::default_path();
    index.parent().map_or_else(
        || PathBuf::from("scryfall-copy"),
        |d| d.join("scryfall-copy"),
    )
}

/// Where a copy is made from.
pub enum Source<'a> {
    /// Default Cards and Oracle Tags off Scryfall's `/bulk-data`.
    Scryfall,
    /// The same two files already on disk, plain or gzipped. Nothing says
    /// when Scryfall wrote them, so the Default Cards file's modification
    /// time stands in for its `updated_at`.
    Local { cards: &'a Path, tags: &'a Path },
}

/// The copy this sync leaves kept: its text, uncompressed, and the Default
/// Cards file it was made from.
pub struct Kept {
    pub text: String,
    pub updated_at: String,
}

/// The lock on `dir`, waited for if another sync holds it. Released when the
/// file is dropped or the process dies, however it dies.
pub fn lock(dir: &Path) -> Result<File> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(LOCK);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            eprintln!(
                "another gauntlet sync holds {}; waiting for it to finish",
                path.display()
            );
            file.lock()
                .with_context(|| format!("waiting for {}", path.display()))?;
        }
        Err(TryLockError::Error(e)) => {
            return Err(e).with_context(|| format!("locking {}", path.display()))
        }
    }
    Ok(file)
}

/// Makes `dir`'s copy current with `source`, or leaves it be if it is. The
/// caller holds [`lock`].
pub fn keep(dir: &Path, source: &Source, force: bool) -> Result<Kept> {
    let kept = read_optional(&dir.join(META))?.and_then(|text| match Meta::parse(&text) {
        Ok(meta) => Some(meta),
        Err(e) => {
            eprintln!("{} names no copy this build reads ({e}); making one", META);
            None
        }
    });
    let mut store = Store::new();
    if let Some(k) = &kept {
        store.hold(k.clone());
    }
    let files = Files::find(source)?;
    let build = if force {
        Build::over(kept.as_ref(), &files.updated_at)
    } else {
        let unreadable = read_optional(&dir.join(UNREADABLE))?;
        match store.plan(
            kept.as_ref(),
            &files.updated_at,
            unreadable.as_deref(),
            now(),
        ) {
            Plan::Current { write } => {
                let kept = kept
                    .as_ref()
                    .context("the store found current a copy storage does not hold")?;
                match open(dir, kept) {
                    Ok(text) => {
                        if let Some(meta) = write {
                            write_atomically(&dir.join(META), meta.to_text().as_bytes())?;
                        }
                        eprintln!(
                            "the copy is already current ({}); pass --force to make it again",
                            kept.updated_at()
                        );
                        return Ok(Kept {
                            text,
                            updated_at: kept.updated_at().to_string(),
                        });
                    }
                    // The meta names a slot that does not read back: a disk
                    // that lost it, or a hand that edited it. Nothing but a
                    // new copy fixes that.
                    Err(e) => {
                        eprintln!("the copy kept does not read back ({e:#}); making it again");
                        Build::over(Some(kept), &files.updated_at)
                    }
                }
            }
            Plan::Refused { updated_at } => bail!(
                "Scryfall's Default Cards of {updated_at} is a file this build could not read \
                 when it last tried ({}).\nThe copy kept, if any, is unchanged. Pass --force \
                 to try it again.",
                dir.join(UNREADABLE).display()
            ),
            Plan::Build(build) => build,
        }
    };
    make(dir, &files, build, &mut store, source)
}

/// Builds `build`'s copy into its slot, then writes the meta naming it.
fn make(
    dir: &Path,
    files: &Files,
    build: Build,
    store: &mut Store,
    source: &Source,
) -> Result<Kept> {
    eprintln!(
        "making the copy of Scryfall's Default Cards of {} in {}",
        build.updated_at(),
        dir.join(build.slot().file()).display()
    );
    let started = Instant::now();
    let mut builder = Builder::new(build.updated_at());
    feed(files.cards.open()?, &files.cards.what, |lines| {
        builder.cards(lines)
    })?;
    feed(files.tags.open()?, &files.tags.what, |lines| {
        builder.tags(lines)
    })?;
    let refused = |store: &mut Store, build: Build, why: String| -> Result<Kept> {
        let updated_at = build.updated_at().to_string();
        let text = store.unreadable(build);
        write_atomically(&dir.join(UNREADABLE), format!("{text}\n").as_bytes())?;
        bail!(
            "this build cannot read Scryfall's Default Cards of {updated_at}: {why}\n\
             Nothing was replaced, and it is not downloaded again until Scryfall writes a \
             newer file or --force asks."
        )
    };
    if let Some(first) = builder.unreadable().first() {
        let why = format!(
            "{} lines were not cards or tags, the first: {first}",
            builder.unreadable().len()
        );
        return refused(store, build, why);
    }
    let stored = builder.finish();
    let (cards, printings, tags) = (
        stored.cards.len(),
        stored.printings.len(),
        stored.tags.len(),
    );
    if matches!(source, Source::Scryfall) && cards < MINIMUM_PLAUSIBLE_CARDS {
        bail!(
            "only {cards} cards were built, which is far fewer than the card pool.\n\
             The download was probably truncated. Nothing was replaced."
        );
    }
    let text = match stored.to_text() {
        Ok(text) => text,
        Err(why) => return refused(store, build, why),
    };
    drop(stored);
    if let Err(why) = ScryfallCopy::load(&text) {
        return refused(store, build, why);
    }
    eprintln!(
        "  {printings} printings of {cards} cards, {tags} oracle tags, built in {:.1}s",
        started.elapsed().as_secs_f64()
    );

    let slot = dir.join(build.slot().file());
    let written = Instant::now();
    write_gzipped(&slot, &text)?;
    // The meta last: until it is written it names the copy before, and a
    // sync killed before here leaves that one answering.
    let meta = store.built(build, now());
    write_atomically(&dir.join(META), meta.to_text().as_bytes())?;
    eprintln!(
        "  wrote {} ({} of text, {} gzipped) in {:.1}s",
        slot.display(),
        megabytes(text.len() as u64),
        megabytes(std::fs::metadata(&slot).map_or(0, |m| m.len())),
        written.elapsed().as_secs_f64()
    );
    Ok(Kept {
        text,
        updated_at: meta.updated_at().to_string(),
    })
}

/// The text of the copy `kept` names, read back as a copy would be, so a
/// copy reported current is one that loads.
fn open(dir: &Path, kept: &Meta) -> Result<String> {
    let path = dir.join(kept.slot().file());
    let mut text = String::new();
    flate2::read::GzDecoder::new(
        File::open(&path).with_context(|| format!("opening {}", path.display()))?,
    )
    .read_to_string(&mut text)
    .with_context(|| format!("reading {}", path.display()))?;
    let copy = ScryfallCopy::load(&text).map_err(anyhow::Error::msg)?;
    if copy.updated_at() != kept.updated_at() {
        bail!(
            "{} holds the copy of {}, and the meta says {}",
            path.display(),
            copy.updated_at(),
            kept.updated_at()
        );
    }
    eprintln!(
        "{} reads back: {} printings of {} cards",
        path.display(),
        copy.printing_count(),
        copy.card_count()
    );
    Ok(text)
}

/// The two bulk files a copy is made from, and the `updated_at` it is
/// known by.
struct Files {
    updated_at: String,
    cards: BulkSource,
    tags: BulkSource,
}

struct BulkSource {
    /// How it is named on stderr.
    what: String,
    from: Origin,
}

enum Origin {
    /// Scryfall's `jsonl_download_uri`, gzipped.
    Url(String),
    File(PathBuf),
}

impl Files {
    fn find(source: &Source) -> Result<Files> {
        match source {
            Source::Scryfall => {
                let listing = bulk_listing()?;
                let file = |kind: &str| {
                    listing
                        .iter()
                        .find(|f| f.kind == kind)
                        .with_context(|| format!("Scryfall's bulk-data listing has no {kind}"))
                };
                let (cards, tags) = (file("default_cards")?, file("oracle_tags")?);
                let updated_at = cards
                    .updated_at
                    .clone()
                    .context("Scryfall listed default_cards without an updated_at")?;
                let remote = |f: &crate::sync::BulkFile| -> Result<BulkSource> {
                    Ok(BulkSource {
                        what: format!(
                            "{} of {} ({})",
                            f.kind,
                            f.updated_at.as_deref().unwrap_or("an unstated time"),
                            f.compressed_size.map_or("size unstated".into(), megabytes)
                        ),
                        from: Origin::Url(f.jsonl_download_uri.clone().with_context(|| {
                            format!("Scryfall listed {} without a jsonl_download_uri", f.kind)
                        })?),
                    })
                };
                Ok(Files {
                    updated_at,
                    cards: remote(cards)?,
                    tags: remote(tags)?,
                })
            }
            Source::Local { cards, tags } => {
                let modified = std::fs::metadata(cards)
                    .and_then(|m| m.modified())
                    .with_context(|| format!("reading {}", cards.display()))?;
                let local = |p: &Path| BulkSource {
                    what: p.display().to_string(),
                    from: Origin::File(p.to_path_buf()),
                };
                Ok(Files {
                    updated_at: iso8601(modified)
                        .context("the Default Cards file was modified before 1970")?,
                    cards: local(cards),
                    tags: local(tags),
                })
            }
        }
    }
}

impl BulkSource {
    /// The file's lines, ungzipped.
    fn open(&self) -> Result<Box<dyn Read>> {
        match &self.from {
            Origin::Url(url) => {
                let body = ureq::get(url)
                    .header("User-Agent", USER_AGENT)
                    .call()
                    .with_context(|| format!("downloading {url}"))?
                    .into_body()
                    .into_reader();
                Ok(Box::new(flate2::read::GzDecoder::new(body)))
            }
            Origin::File(path) => {
                let file =
                    File::open(path).with_context(|| format!("reading {}", path.display()))?;
                Ok(if path.extension().is_some_and(|e| e == "gz") {
                    Box::new(flate2::read::GzDecoder::new(file))
                } else {
                    Box::new(file)
                })
            }
        }
    }
}

/// Hands `eat` whole lines of `source`, about [`CHUNK`] at a time.
fn feed(source: impl Read, what: &str, mut eat: impl FnMut(&str)) -> Result<()> {
    eprintln!("  reading {what}");
    let started = Instant::now();
    let mut reader = BufReader::with_capacity(1 << 20, source);
    let mut chunk = String::with_capacity(CHUNK + (64 << 10));
    let mut total = 0u64;
    loop {
        let n = reader
            .read_line(&mut chunk)
            .with_context(|| format!("reading {what} after {}", megabytes(total)))?;
        total += n as u64;
        if n == 0 || chunk.len() >= CHUNK {
            eat(&chunk);
            chunk.clear();
        }
        if n == 0 {
            break;
        }
    }
    eprintln!(
        "  read {} in {:.1}s",
        megabytes(total),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}

fn now() -> UnixMs {
    UnixMs(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
    )
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// A slot's gzipped text, on disk before the meta naming it is written.
fn write_gzipped(path: &Path, text: &str) -> Result<()> {
    let io = || format!("writing {}", path.display());
    let file = File::create(path).with_context(io)?;
    let mut gz =
        flate2::write::GzEncoder::new(BufWriter::new(file), flate2::Compression::default());
    gz.write_all(text.as_bytes()).with_context(io)?;
    let file = gz
        .finish()
        .with_context(io)?
        .into_inner()
        .map_err(|e| e.into_error())
        .with_context(io)?;
    file.sync_all().with_context(io)
}

/// Written beside `path` and renamed over it, so `path` is never half a file.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let io = || format!("writing {}", path.display());
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(io)?;
    }
    let temp = {
        let mut name = path.as_os_str().to_owned();
        name.push(".partial");
        PathBuf::from(name)
    };
    {
        let mut file = File::create(&temp).with_context(io)?;
        file.write_all(bytes).with_context(io)?;
        file.sync_all().with_context(io)?;
    }
    std::fs::rename(&temp, path).with_context(io)
}
