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

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use anyhow::{bail, Context, Result};
use facet::Facet;
use pe_scryfall::bulk::BulkCard;
use pe_scryfall::index::{BuildReport, Index, IndexFile};

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

    let (mut index, report) = Index::build(records, updated_at);
    print_report(&report);
    check(&report, &index, from.is_none())?;

    // Tags come from the search API, so a sync reading a local bulk file has no
    // way to get them and says so rather than writing an index that looks as
    // though it asked. `--from` exists for tests and offline machines, and an
    // index that quietly carried no tags would make `otag:` match nothing there
    // with no indication why.
    if from.is_none() {
        let names = pe_scryfall::tags::standard_tag_names();
        eprintln!("fetching {} oracle tags", names.len());
        let membership = fetch_tags(&names)?;
        let fetched_at = now_utc();
        let tagged = index.attach_tags(names, fetched_at, &membership);
        eprintln!("tagged {tagged} cards");
    } else {
        eprintln!(
            "skipping oracle tags: --from reads a bulk file, and tags come from the search API"
        );
    }
    index
        .write_atomically(&path)
        .with_context(|| format!("writing the index to {}", path.display()))?;
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
    // Only the header is read, which is one line: asking "is this already the
    // data I have?" must not cost as much as using it.
    match IndexFile::open(path) {
        Ok(existing) => existing.updated_at() == Some(updated_at) && !existing.is_stale(),
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

/// One page of a Scryfall search, reduced to what a tag fetch needs.
#[derive(Facet)]
struct SearchPage {
    #[facet(default)]
    data: Vec<SearchCard>,
    #[facet(default)]
    has_more: bool,
    #[facet(default)]
    next_page: Option<String>,
}

#[derive(Facet)]
struct SearchCard {
    #[facet(default)]
    oracle_id: Option<String>,
}

/// Scryfall asks for 50-100ms between requests. Tag fetching is the only place
/// here that makes many small calls in a row, so it is the only place that has
/// to care.
const REQUEST_GAP: std::time::Duration = std::time::Duration::from_millis(100);

/// Which oracle ids are in each of `tags`, as Scryfall answers today.
///
/// One paginated search per tag. A tag that matches nothing comes back as an
/// empty entry rather than being dropped: the index records that it asked, and
/// "asked, no members" has to survive as a different fact from "never asked".
///
/// A tag Scryfall rejects outright is an error and stops the sync. Writing an
/// index that silently lacks a tag the user asked for would produce exactly the
/// confident empty result this tool exists to prevent — better to fail the
/// sync, while the person is standing there.
fn fetch_tags(tags: &[String]) -> Result<HashMap<String, Vec<String>>> {
    let mut membership: HashMap<String, Vec<String>> = HashMap::new();

    for tag in tags {
        let mut url = format!(
            "https://api.scryfall.com/cards/search?q=otag%3A{}&unique=cards",
            urlencode(tag)
        );
        let mut found = 0usize;
        loop {
            std::thread::sleep(REQUEST_GAP);
            let response = ureq::get(&url)
                .header("User-Agent", USER_AGENT)
                .header("Accept", "application/json")
                .call();

            let body = match response {
                Ok(mut r) => r
                    .body_mut()
                    .read_to_string()
                    .with_context(|| format!("reading Scryfall's answer for otag:{tag}"))?,
                Err(ureq::Error::StatusCode(404)) => {
                    // Scryfall answers an empty search with 404, which is a real
                    // answer: the tag exists and nothing is in it.
                    break;
                }
                Err(e) => return Err(e).with_context(|| format!("asking Scryfall for otag:{tag}")),
            };

            let page: SearchPage = facet_json::from_str(&body)
                .with_context(|| format!("parsing Scryfall's answer for otag:{tag}"))?;

            for card in &page.data {
                if let Some(id) = card.oracle_id.as_deref() {
                    membership
                        .entry(id.to_string())
                        .or_default()
                        .push(tag.clone());
                    found += 1;
                }
            }

            match (page.has_more, page.next_page) {
                (true, Some(next)) => url = next,
                _ => break,
            }
        }
        eprintln!("  otag:{tag}: {found} cards");
    }

    Ok(membership)
}

/// Today's UTC date as `YYYY-MM-DD`, or `None` if the clock is before 1970.
///
/// `None` rather than a fallback string, for the reason [`Header::updated_at`]
/// gives: a date nobody can vouch for is worse than an admitted gap, and every
/// reader of this field already handles the gap.
///
/// The civil-date arithmetic is Howard Hinnant's `civil_from_days`, written out
/// rather than pulled in, because one date format is not worth a dependency
/// and this is the whole of what would be used.
fn now_utc() -> Option<String> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    let days = (secs / 86_400) as i64;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Percent-encode the handful of characters a tag name can contain.
///
/// Tags are lowercase words joined by hyphens, so this is close to a no-op —
/// it exists so that a tag with anything else in it fails as a bad request
/// rather than as a malformed URL that means something else.
fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            other => format!("%{:02X}", other as u32),
        })
        .collect()
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

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}
