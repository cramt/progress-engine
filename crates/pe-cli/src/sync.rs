//! `progress-engine sync`: build the card index from Scryfall's bulk data.
//!
//! This used to be somebody else's job. The index was written by a separate
//! `scryfall sync` shell tool, which meant a clone of this repository could
//! parse a decklist and nothing else — half a tool presented as a whole one,
//! and a data ceiling on every query the language could grow.
//!
//! It was also a correctness problem, which is the half that forced the issue.
//! That index keyed tokens and cards under one name and let the last one
//! written win, and nothing here could detect or repair it. Owning the build is
//! what makes the fix expressible at all.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;

use anyhow::{bail, Context, Result};
use facet::Facet;
use pe_scryfall::bulk::BulkCard;
use pe_scryfall::index::{BuildReport, Index};

/// Scryfall's index of what bulk files exist.
const BULK_DATA_API: &str = "https://api.scryfall.com/bulk-data";

/// The bulk file this tool wants.
///
/// One record per Oracle ID, rather than one per printing. A decklist names
/// cards, not printings, so the 500MB `default_cards` file would be 460MB of
/// answers to a question nobody here asks.
const WANTED: &str = "oracle_cards";

/// Scryfall asks for a descriptive user agent and a good citizen gives one.
const USER_AGENT: &str = concat!("progress-engine/", env!("CARGO_PKG_VERSION"));

/// One entry in Scryfall's bulk-data listing.
#[derive(Facet, Debug)]
struct BulkListing {
    data: Vec<BulkFile>,
}

#[derive(Facet, Debug)]
struct BulkFile {
    #[facet(rename = "type")]
    kind: String,
    #[facet(default)]
    updated_at: Option<String>,
    /// Scryfall serves gzipped JSONL, which is why this streams rather than
    /// reading a 500MB array into memory to parse it.
    #[facet(default)]
    jsonl_download_uri: Option<String>,
    #[facet(default)]
    compressed_size: Option<u64>,
}

/// How many structural surprises are tolerated before the result is refused.
///
/// Not zero, because one strange record must not cost you the other 38,625.
/// Not unlimited, because the flattening in `pe-scryfall::bulk` rests on the
/// shape of Scryfall's data, and a bulk format that has moved under us produces
/// an index that is wrong in ways no query can see. A run that trips this says
/// what it saw and installs nothing.
const ANOMALY_CEILING: usize = 25;

/// A download that produced fewer cards than this is not a sync, it is a
/// truncated download that parsed. Scryfall has published upwards of thirty thousand
/// distinct cards for years, and installing a fraction of them over a working
/// index would answer every question about a library that does not exist.
const MINIMUM_PLAUSIBLE_CARDS: usize = 20_000;

pub fn run(index_path: Option<&Path>, from: Option<&Path>, force: bool) -> Result<()> {
    let path = index_path
        .map(Path::to_path_buf)
        .unwrap_or_else(Index::default_path);

    let (records, updated_at) = match from {
        // A local file, so the tests and a no-network machine can exercise
        // everything downstream of the download.
        Some(file) => {
            eprintln!("reading bulk data from {}", file.display());
            (read_local(file)?, None)
        }
        None => {
            let bulk = find_bulk_file()?;
            let updated_at = bulk.updated_at.clone();
            if !force && already_current(&path, updated_at.as_deref()) {
                eprintln!(
                    "index is already current ({}); pass --force to rebuild it",
                    updated_at.as_deref().unwrap_or("no date")
                );
                return Ok(());
            }
            let uri = bulk.jsonl_download_uri.clone().with_context(|| {
                format!("Scryfall listed {WANTED} without a jsonl_download_uri")
            })?;
            eprintln!(
                "downloading {WANTED}, updated {} ({})",
                updated_at.as_deref().unwrap_or("at an unstated time"),
                bulk.compressed_size
                    .map_or("size unstated".into(), megabytes)
            );
            (download(&uri)?, updated_at)
        }
    };

    let (index, report) = Index::build(records, updated_at);
    print_report(&report);
    check(&report, &index, from.is_none())?;
    write_atomically(&path, &index)?;
    eprintln!("wrote {} cards to {}", report.kept, path.display());
    Ok(())
}

/// Whether the index on disk was built from the same bulk file.
///
/// Compared on Scryfall's own timestamp for the file rather than on the local
/// clock: "when we last ran" and "which data we have" are different facts, and
/// only the second one is a reason to skip the work.
fn already_current(path: &Path, updated_at: Option<&str>) -> bool {
    let Some(updated_at) = updated_at else {
        return false;
    };
    match Index::load(path) {
        Ok(existing) => existing.updated_at.as_deref() == Some(updated_at) && !existing.is_stale(),
        Err(_) => false,
    }
}

fn find_bulk_file() -> Result<BulkFile> {
    let body = ureq::get(BULK_DATA_API)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json")
        .call()
        .context("asking Scryfall which bulk files exist")?
        .body_mut()
        .read_to_string()
        .context("reading Scryfall's bulk-data listing")?;

    let listing: BulkListing =
        facet_json::from_str(&body).context("parsing Scryfall's bulk-data listing")?;

    listing
        .data
        .into_iter()
        .find(|f| f.kind == WANTED)
        .with_context(|| format!("Scryfall's bulk-data listing has no {WANTED} file"))
}

fn download(uri: &str) -> Result<Vec<BulkCard>> {
    let reader = ureq::get(uri)
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("downloading {uri}"))?
        .into_body()
        .into_reader();
    parse_lines(flate2::read::GzDecoder::new(reader))
}

fn read_local(file: &Path) -> Result<Vec<BulkCard>> {
    let handle =
        std::fs::File::open(file).with_context(|| format!("reading {}", file.display()))?;
    // Both forms, so a fixture can be checked in uncompressed and a downloaded
    // file can be re-read without unpacking it first.
    if file.extension().is_some_and(|e| e == "gz") {
        parse_lines(flate2::read::GzDecoder::new(handle))
    } else {
        parse_lines(handle)
    }
}

/// Parse JSONL a line at a time.
///
/// The whole reason to prefer the JSONL bulk file over the JSON one: a 500MB
/// array has to be resident before its first element can be read, while this
/// holds one record plus the cards kept so far.
fn parse_lines(source: impl Read) -> Result<Vec<BulkCard>> {
    let mut out = Vec::new();
    for (n, line) in BufReader::new(source).lines().enumerate() {
        let line = line.with_context(|| format!("reading bulk record {}", n + 1))?;
        let line = line.trim().trim_end_matches(',');
        // The JSON array form brackets the whole file; tolerated so that a
        // hand-fetched `oracle-cards.json` also works.
        if line.is_empty() || line == "[" || line == "]" {
            continue;
        }
        out.push(
            facet_json::from_str(line).with_context(|| format!("parsing bulk record {}", n + 1))?,
        );
    }
    Ok(out)
}

fn print_report(report: &BuildReport) {
    eprintln!("read {} records", report.read);
    for (reason, count) in &report.skipped {
        eprintln!("  skipped {count}: {}", reason.as_str());
    }
    eprintln!("  kept {} cards", report.kept);

    // Named, not just counted. "We dropped some records" is not a statement
    // anybody can check against the next sync.
    let (collisions, structural): (Vec<_>, Vec<_>) = report
        .anomalies
        .iter()
        .partition(|a| matches!(a, pe_scryfall::bulk::Anomaly::NameCollision { .. }));
    if !collisions.is_empty() {
        // Distinct names, not collision events: six printings of one name are
        // five events and one ambiguous name, and only the second is a fact
        // about the card pool.
        let mut names: Vec<&str> = collisions
            .iter()
            .filter_map(|a| match a {
                pe_scryfall::bulk::Anomaly::NameCollision { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        names.sort_unstable();
        names.dedup();
        eprintln!(
            "  {} names match more than one card; which printing is kept is decided by a \
             rule that gives the same answer every sync",
            names.len()
        );
    }
    for anomaly in structural.iter().take(10) {
        eprintln!("  unexpected: {anomaly}");
    }
    if structural.len() > 10 {
        eprintln!("  ... and {} more", structural.len() - 10);
    }
}

/// Refuse to install a result that does not look like the card pool.
///
/// A sync writes over the file every later run reads, so the failure mode is
/// not "sync did nothing" but "every probability from now on is about a library
/// that does not exist". Both checks below would have caught a real accident:
/// a download truncated mid-stream still parses as valid JSONL, and a bulk
/// format that moved would flatten into cards with no text.
fn check(report: &BuildReport, index: &Index, downloaded: bool) -> Result<()> {
    let structural = report
        .anomalies
        .iter()
        .filter(|a| !matches!(a, pe_scryfall::bulk::Anomaly::NameCollision { .. }))
        .count();
    if structural > ANOMALY_CEILING {
        bail!(
            "{structural} records did not have the shape this tool expects, which is more \
             than the {ANOMALY_CEILING} it tolerates.\n\
             Scryfall's bulk format has probably changed. Nothing was written."
        );
    }
    // Only for a download. A file the caller named is their business — they
    // may well be pointing at a deliberately small one — whereas a truncated
    // download is an accident nobody asked for.
    if downloaded && index.cards.len() < MINIMUM_PLAUSIBLE_CARDS {
        bail!(
            "only {} cards were built, which is far fewer than the card pool.\n\
             The download was probably truncated. Nothing was written.",
            index.cards.len()
        );
    }
    Ok(())
}

/// Write to a neighbouring temporary file and rename over the target.
///
/// A rename is atomic, so an interrupted sync leaves the previous index intact
/// rather than a half-written one. Reading a truncated index is the one failure
/// here that would not announce itself — facet-json would refuse it, and the
/// message would be about JSON rather than about a sync that died.
fn write_atomically(path: &Path, index: &Index) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let temp = path.with_extension("json.partial");
    let json = facet_json::to_string(index).context("serialising the index")?;
    {
        let mut file =
            std::fs::File::create(&temp).with_context(|| format!("creating {}", temp.display()))?;
        file.write_all(json.as_bytes())
            .with_context(|| format!("writing {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("flushing {}", temp.display()))?;
    }
    std::fs::rename(&temp, path)
        .with_context(|| format!("renaming {} to {}", temp.display(), path.display()))?;
    Ok(())
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}
