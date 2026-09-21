//! `gauntlet sync`: build the card index from Scryfall's bulk data.
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
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chip_scryfall::bulk::BulkCard;
use chip_scryfall::index::{BuildReport, Index, IndexFile};
use facet::Facet;

/// Scryfall's index of what bulk files exist.
const BULK_DATA_API: &str = "https://api.scryfall.com/bulk-data";

/// The bulk file this tool wants.
///
/// One record per Oracle ID, rather than one per printing. A decklist names
/// cards, not printings, so the 500MB `default_cards` file would be 460MB of
/// answers to a question nobody here asks.
const WANTED: &str = "oracle_cards";

/// Scryfall asks for a descriptive user agent and a good citizen gives one.
const USER_AGENT: &str = concat!("ichormoon-gauntlet/", env!("CARGO_PKG_VERSION"));

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
/// Not unlimited, because the flattening in `chip-scryfall::bulk` rests on the
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

    let wanted_tags = chip_scryfall::tags::standard_tag_names();

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
            if !force && already_current(&path, updated_at.as_deref(), &wanted_tags) {
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
    let mut missing = Vec::new();
    if from.is_none() {
        eprintln!("fetching {} oracle tags", wanted_tags.len());
        let fetch = fetch_tags(&wanted_tags);
        let tagged = index.attach_tags(fetch.fetched.clone(), now_utc(), &fetch.membership);
        eprintln!(
            "tagged {tagged} cards with {} of {} tags",
            fetch.fetched.len(),
            wanted_tags.len()
        );
        missing = fetch.failed;
    } else {
        eprintln!(
            "skipping oracle tags: --from reads a bulk file, and tags come from the search API.\n\
             This index will carry none, so every otag: query against it is refused rather than\n\
             answered, and the standard effect library — which is keyed entirely on otag: — \
             matches\n\
             nothing. Run `gauntlet sync` without --from to fetch them."
        );
    }
    // Written before the tag phase is judged, always. The download is the
    // expensive half and the tags are the flaky one, so a tag that failed must
    // not cost the 25MB that succeeded (#50). What the header claims is exactly
    // what was fetched, so a query naming a missing tag is refused by name
    // rather than answered with a confident zero.
    index
        .write_atomically(&path)
        .with_context(|| format!("writing the index to {}", path.display()))?;
    eprintln!("wrote {} cards to {}", report.kept, path.display());
    if !missing.is_empty() {
        return Err(partial_sync(&path, wanted_tags.len(), &missing));
    }
    Ok(())
}

/// What an incomplete sync says on its way out.
///
/// Non-zero, because a sync missing tags is not a sync: a run against this
/// index will refuse the questions those tags answer, and a caller scripting
/// `sync && test` would otherwise carry on into a refusal it could have
/// prevented. It is still an index, and it says so — the failure is named, the
/// remedy is one command, and neither costs the download again.
fn partial_sync(path: &Path, wanted: usize, failed: &[FailedTag]) -> anyhow::Error {
    let named: Vec<String> = failed
        .iter()
        .map(|f| format!("  otag:{}: {}", f.name, f.why))
        .collect();
    anyhow::anyhow!(
        "this sync did not finish: {} of {wanted} oracle tags could not be fetched.\n{}\n\
         The index at {} was written with the {} that did succeed, so the download is not \
         lost.\n\
         A query naming a missing tag is refused by name until a later sync fetches it; \
         run\n`gauntlet sync` again to finish the job.",
        failed.len(),
        named.join("\n"),
        path.display(),
        wanted - failed.len(),
    )
}

/// Whether the index on disk was built from the same bulk file.
///
/// Compared on Scryfall's own timestamp for the file rather than on the local
/// clock: "when we last ran" and "which data we have" are different facts, and
/// only the second one is a reason to skip the work.
///
/// Current also means *complete*. A sync whose tag phase was cut short writes
/// the index it managed and asks to be run again; skipping that re-run because
/// the bulk data had not moved would answer the request with "already current"
/// and leave the tags missing forever.
fn already_current(path: &Path, updated_at: Option<&str>, wanted_tags: &[String]) -> bool {
    let Some(updated_at) = updated_at else {
        return false;
    };
    // Only the header is read, which is one line: asking "is this already the
    // data I have?" must not cost as much as using it.
    match IndexFile::open(path) {
        Ok(existing) => {
            let tags = existing.tag_vocabulary();
            existing.updated_at() == Some(updated_at)
                && !existing.is_stale()
                && wanted_tags.iter().all(|t| tags.contains(t))
        }
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
const REQUEST_GAP: Duration = Duration::from_millis(100);

/// The gap for the rest of the run once Scryfall has said, once, that it was
/// not enough.
///
/// 100ms is inside the stated ask and was still rate-limited 23 pages into the
/// sixth tag (#50), so the ask is a floor rather than a guarantee. Widening
/// after the first 429 costs a few seconds on a sync that already takes a
/// minute, and the alternative is walking back into the same wall on the next
/// page having learnt nothing from it.
const SLOWED_GAP: Duration = Duration::from_millis(250);

/// How many times one page is asked for before its tag is given up on.
const MAX_ATTEMPTS: u32 = 5;

/// The wait after the first rate limit, doubled on each one after it.
///
/// Doubling rather than repeating, because a fixed retry against a limiter that
/// is still counting is the same request arriving again: 1, 2, 4 and 8 seconds
/// gives the window time to move, and totals 15 seconds against a download that
/// took longer than that.
const FIRST_BACKOFF: Duration = Duration::from_secs(1);

/// The longest this will wait before giving up on a tag.
///
/// Two minutes rather than one, measured rather than picked: the 429 this was
/// written against sent `Retry-After: 60`, and a ceiling at exactly the number
/// Scryfall sends would fail a whole tag the first time it rounded up. Past
/// this it stops rather than ignoring the number — coming back sooner than
/// asked is the rudeness the backoff exists to avoid, and a sync that blocks
/// silently for ten minutes is not a better answer than one that writes the
/// tags it has and names the ones it does not.
const MAX_BACKOFF: Duration = Duration::from_secs(120);

/// What a tag fetch produced, including what it failed to produce.
///
/// The three fields are one thought: `membership` is only meaningful beside the
/// list of tags it is complete for, and `failed` is what stops the caller from
/// reading that list as *all of them*. A partial fetch is an ordinary outcome
/// here rather than an error, because the expensive half of the sync has
/// already succeeded by the time this runs.
struct TagFetch {
    /// Oracle id to the tags it is in. Only tags in `fetched` appear: a tag
    /// that failed halfway would otherwise leave cards carrying a membership
    /// the header does not vouch for.
    membership: HashMap<String, Vec<String>>,
    fetched: Vec<String>,
    failed: Vec<FailedTag>,
}

/// A tag this sync asked for and did not get, with the reason it did not.
struct FailedTag {
    name: String,
    why: String,
}

/// Which oracle ids are in each of `tags`, as Scryfall answers today.
///
/// One paginated search per tag. A tag that matches nothing comes back as an
/// empty entry rather than being dropped: the index records that it asked, and
/// "asked, no members" has to survive as a different fact from "never asked".
///
/// A tag that cannot be fetched at all stops that tag and nothing else. It used
/// to stop the sync, which meant one transient 429 threw away a 25MB download
/// that had already parsed — the whole cost of the run paid for the cheapest
/// thing in it (#50). What a failed tag costs now is the tag: it is left out of
/// the index and out of the header, so `otag:` on it is refused by name instead
/// of answered with a confident zero.
fn fetch_tags(tags: &[String]) -> TagFetch {
    let mut membership: HashMap<String, Vec<String>> = HashMap::new();
    let mut fetched = Vec::new();
    let mut failed = Vec::new();
    let mut pace = Pace::new();

    for tag in tags {
        match fetch_tag(tag, &mut pace) {
            Ok(ids) => {
                eprintln!("  otag:{tag}: {} cards", ids.len());
                for id in ids {
                    membership.entry(id).or_default().push(tag.clone());
                }
                fetched.push(tag.clone());
            }
            // Named where it happened, so the summary at the end is a reminder
            // rather than the first anybody hears of it.
            Err(e) => {
                eprintln!("  otag:{tag}: FAILED: {e:#}");
                failed.push(FailedTag {
                    name: tag.clone(),
                    why: format!("{e:#}"),
                });
            }
        }
    }

    TagFetch {
        membership,
        fetched,
        failed,
    }
}

/// Every oracle id in one tag, across as many pages as Scryfall has.
///
/// Collected before anything is recorded: a tag is either fetched whole or not
/// at all, because half a tag written into the index would be a membership list
/// that is wrong rather than missing.
fn fetch_tag(tag: &str, pace: &mut Pace) -> Result<Vec<String>> {
    let mut url = format!(
        "https://api.scryfall.com/cards/search?q=otag%3A{}&unique=cards",
        urlencode(tag)
    );
    let mut ids = Vec::new();
    loop {
        // Scryfall answers an empty search with 404, which is a real answer:
        // the tag exists and nothing is in it.
        let Some(body) = fetch_page(&url, tag, pace)? else {
            break;
        };
        let page: SearchPage = facet_json::from_str(&body)
            .with_context(|| format!("parsing Scryfall's answer for otag:{tag}"))?;
        ids.extend(page.data.iter().filter_map(|c| c.oracle_id.clone()));
        match (page.has_more, page.next_page) {
            (true, Some(next)) => url = next,
            _ => break,
        }
    }
    Ok(ids)
}

/// One page, asked for again while Scryfall says to come back later.
///
/// `Ok(None)` is the 404 for a search that matched nothing.
fn fetch_page(url: &str, tag: &str, pace: &mut Pace) -> Result<Option<String>> {
    for attempt in 1..=MAX_ATTEMPTS {
        pace.wait();
        let mut response = ureq::get(url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            // A status is read here rather than raised as an error, because
            // ureq's `StatusCode` error carries the number and nothing else —
            // and the header that says how long to wait is one of the things it
            // has dropped by then.
            .config()
            .http_status_as_error(false)
            .build()
            .call()
            .with_context(|| format!("asking Scryfall for otag:{tag}"))?;

        let status = response.status().as_u16();
        if status == 404 {
            return Ok(None);
        }
        if response.status().is_success() {
            return Ok(Some(response.body_mut().read_to_string().with_context(
                || format!("reading Scryfall's answer for otag:{tag}"),
            )?));
        }
        if !worth_retrying(status) {
            bail!("Scryfall answered with HTTP {status}, which is an answer rather than a delay");
        }
        pace.slow_down();
        let wait = retry_after(response.headers()).unwrap_or_else(|| backoff(attempt));
        if wait > MAX_BACKOFF {
            bail!(
                "Scryfall answered with HTTP {status} and asked for {}s, which is longer \
                 than this sync will hold the line for",
                wait.as_secs()
            );
        }
        if attempt < MAX_ATTEMPTS {
            eprintln!(
                "    HTTP {status}; waiting {:.1}s and asking again ({attempt} of {MAX_ATTEMPTS})",
                wait.as_secs_f64()
            );
            std::thread::sleep(wait);
        }
    }
    bail!("Scryfall was still rate-limiting after {MAX_ATTEMPTS} attempts")
}

/// Whether a status says *later* rather than *no*.
///
/// 429 is the one #50 was reported against. The 5xx family is here for the same
/// reason and on the same evidence: a gateway having a bad minute is a delay,
/// and treating it as a verdict would throw the download away over something
/// that fixes itself.
fn worth_retrying(status: u16) -> bool {
    status == 429 || (500..600).contains(&status)
}

/// How long to wait before attempt `attempt` + 1, when nobody said.
fn backoff(attempt: u32) -> Duration {
    FIRST_BACKOFF
        .saturating_mul(1u32 << (attempt - 1))
        .min(MAX_BACKOFF)
}

/// How long Scryfall asked us to wait, when it said.
///
/// Seconds only. `Retry-After` may also carry an HTTP date, which is not worth
/// a parser here: the backoff it falls through to waits a comparable time, and
/// a date parser that is subtly wrong would wait the wrong one while looking
/// authoritative.
fn retry_after(headers: &ureq::http::HeaderMap) -> Option<Duration> {
    let value = headers.get("retry-after")?.to_str().ok()?;
    Some(Duration::from_secs(value.trim().parse().ok()?))
}

/// The wait between requests, for the run rather than for one call.
///
/// A struct because the gap is not a constant any more: it widens the first
/// time Scryfall pushes back and stays widened, which is a fact about this run
/// that every later request has to see.
struct Pace {
    gap: Duration,
}

impl Pace {
    fn new() -> Self {
        Pace { gap: REQUEST_GAP }
    }

    fn wait(&self) {
        std::thread::sleep(self.gap);
    }

    fn slow_down(&mut self) {
        self.gap = self.gap.max(SLOWED_GAP);
    }
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
        .partition(|a| matches!(a, chip_scryfall::bulk::Anomaly::NameCollision { .. }));
    if !collisions.is_empty() {
        // Distinct names, not collision events: six printings of one name are
        // five events and one ambiguous name, and only the second is a fact
        // about the card pool.
        let mut names: Vec<&str> = collisions
            .iter()
            .filter_map(|a| match a {
                chip_scryfall::bulk::Anomaly::NameCollision { name, .. } => Some(name.as_str()),
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
        .filter(|a| !matches!(a, chip_scryfall::bulk::Anomaly::NameCollision { .. }))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The waits, in order, against a limiter that keeps saying no.
    ///
    /// Written out rather than computed, because the property that matters is
    /// not "it doubles" — it is that four retries fit inside a wait a person
    /// standing at the terminal will sit through, and a formula asserted
    /// against itself would not notice that changing.
    #[test]
    fn the_backoff_doubles_and_stops() {
        let waits: Vec<u64> = (1..MAX_ATTEMPTS).map(|a| backoff(a).as_secs()).collect();
        assert_eq!(waits, vec![1, 2, 4, 8]);
        assert_eq!(waits.iter().sum::<u64>(), 15);
        assert!(backoff(30) <= MAX_BACKOFF, "and it never runs away");
    }

    #[test]
    fn a_delay_is_retried_and_a_verdict_is_not() {
        assert!(worth_retrying(429), "the one #50 was reported against");
        assert!(worth_retrying(503), "a gateway having a bad minute");
        // 404 never reaches this: it is an empty tag, which is an answer.
        assert!(!worth_retrying(400), "a bad request is not a delay");
        assert!(!worth_retrying(403), "nor is being refused");
    }

    #[test]
    fn scryfalls_own_wait_beats_our_guess() {
        let mut headers = ureq::http::HeaderMap::new();
        headers.insert("retry-after", "7".parse().unwrap());
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(7)));

        // The HTTP-date form is not parsed, and says so by falling through to
        // the backoff rather than by waiting zero seconds.
        headers.insert(
            "retry-after",
            "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap(),
        );
        assert_eq!(retry_after(&headers), None);
        assert_eq!(retry_after(&ureq::http::HeaderMap::new()), None);
    }

    /// A tag that failed must cost the tag and nothing else.
    ///
    /// The message is the whole feature: it has to name what is missing, say
    /// that the index on disk is usable, and not read as a complete sync.
    #[test]
    fn a_partial_sync_names_what_it_lacks_and_keeps_the_rest() {
        let failed = vec![FailedTag {
            name: "tutor".into(),
            why: "Scryfall was still rate-limiting after 5 attempts".into(),
        }];
        let message = format!(
            "{:#}",
            partial_sync(Path::new("/tmp/index.jsonl"), 6, &failed)
        );
        assert!(message.contains("1 of 6"), "{message}");
        assert!(message.contains("otag:tutor"), "{message}");
        assert!(message.contains("rate-limiting"), "{message}");
        assert!(message.contains("/tmp/index.jsonl"), "{message}");
        assert!(message.contains("the 5 that did succeed"), "{message}");
        assert!(message.contains("gauntlet sync"), "{message}");
    }
}
