//! `cr check`, `cr diff` and `cr pin`: the Comprehensive Rules the repo is
//! built against, and what to recheck when Wizards publishes new ones.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use anyhow::{bail, Context, Result};
use chancellor_cr::{citations, diff, Change, Citation, Cited, RuleId, Rules};
use facet::Facet;
use sha2::{Digest, Sha256};

const USAGE: &str =
    "usage: cr check [--cr FILE] [ROOT] | cr diff <URL | FILE> [ROOT] | cr pin <URL>";
const PIN: &str = include_str!("../pin.json");
const USER_AGENT: &str = "progress-engine/chancellor-cr";

#[derive(Facet)]
struct Pin {
    url: String,
    /// As `YYYY-MM-DD`, from the text's own "effective as of".
    effective: String,
    sha256: String,
}

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("cr: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<ExitCode> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["check", "--cr", file, rest @ ..] => {
            check(&read_pinned_file(Path::new(file))?, root(rest)?)
        }
        ["check", rest @ ..] => check(&pinned()?, root(rest)?),
        ["diff", new, rest @ ..] => {
            let new = if new.starts_with("https://") {
                download(new)?
            } else {
                fs::read_to_string(new).with_context(|| format!("reading {new}"))?
            };
            print!("{}", worklist(&pinned()?, &new, &root(rest)?)?);
            Ok(ExitCode::SUCCESS)
        }
        ["pin", url] => pin(url),
        _ => bail!("{USAGE}"),
    }
}

fn pin_now() -> Result<Pin> {
    facet_json::from_str(PIN).map_err(|e| anyhow::anyhow!("pin.json: {e}"))
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn read_pinned_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let want = pin_now()?.sha256;
    let got = sha256(&bytes);
    if got != want {
        bail!(
            "{} has sha256 {got}, and pin.json pins {want}",
            path.display()
        );
    }
    Ok(String::from_utf8(bytes)?)
}

/// The pinned text: `CHANCELLOR_CR` if it is set, else the cache, filled
/// from the pinned URL on a miss. Always checked against the hash.
fn pinned() -> Result<String> {
    if let Ok(path) = env::var("CHANCELLOR_CR") {
        return read_pinned_file(Path::new(&path));
    }
    let pin = pin_now()?;
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .context("neither XDG_CACHE_HOME nor HOME is set")?
        .join("chancellor");
    let file = cache.join(format!("cr-{}.txt", pin.sha256));
    if !file.exists() {
        let text = download(&pin.url)?;
        fs::create_dir_all(&cache)?;
        fs::write(&file, text)?;
    }
    read_pinned_file(&file)
}

fn download(url: &str) -> Result<String> {
    eprintln!("cr: fetching {url}");
    ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("fetching {url}"))?
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_to_string()
        .with_context(|| format!("reading {url}"))
}

fn root(rest: &[&str]) -> Result<PathBuf> {
    match rest {
        [] => Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()?),
        [root] => Ok(PathBuf::from(root)),
        _ => bail!("{USAGE}"),
    }
}

/// Every text file a citation could be in. Build output and dependencies
/// aren't the repo's own words, and this tool's own tests and docs cite
/// rules only as examples of how a citation is written.
fn files(root: &Path) -> Result<Vec<PathBuf>> {
    // Relative to the root: the manifest dir baked in at build time is a
    // sandbox path where the flake builds this.
    let own = Path::new("crates/chancellor-of-the-spires/cr");
    const SKIP: &[&str] = &["target", "node_modules", ".git", "dist", "pkg", ".direnv"];
    const TEXT: &[&str] = &["rs", "md", "toml", "txt", "py", "ts", "tsx", "nix"];
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))? {
            let path = entry?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if !SKIP.contains(&name)
                    && !name.starts_with("result")
                    && path.strip_prefix(root).ok() != Some(own)
                {
                    stack.push(path);
                }
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| TEXT.contains(&e))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

struct Cites {
    file: PathBuf,
    line_text: String,
    citation: Citation,
}

fn every_citation(root: &Path) -> Result<Vec<Cites>> {
    let mut out = Vec::new();
    for file in files(root)? {
        // The pinned text itself, if someone dropped it in the tree, is not
        // a citation of anything.
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for citation in citations(&text) {
            out.push(Cites {
                file: file.strip_prefix(root).unwrap_or(&file).to_path_buf(),
                line_text: lines[citation.line - 1].trim().to_string(),
                citation,
            });
        }
    }
    Ok(out)
}

fn check(cr: &str, root: PathBuf) -> Result<ExitCode> {
    let rules = Rules::parse(cr);
    let all = every_citation(&root)?;
    let mut bad = 0;
    for c in &all {
        for id in c.citation.cites.ends() {
            if !rules.rules.contains_key(&id) {
                bad += 1;
                println!(
                    "{}:{}: CR {id} is not a rule in the pinned CR ({})",
                    c.file.display(),
                    c.citation.line,
                    rules.effective.as_deref().unwrap_or("undated"),
                );
            }
        }
    }
    let distinct: std::collections::BTreeSet<RuleId> =
        all.iter().flat_map(|c| c.citation.cites.ends()).collect();
    eprintln!(
        "cr: {} citations of {} distinct rules, against {} rules effective {}; {bad} don't resolve",
        all.len(),
        distinct.len(),
        rules.rules.len(),
        rules.effective.as_deref().unwrap_or("(undated)"),
    );
    Ok(if bad == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn quote(text: &str) -> String {
    text.lines().map(|l| format!("> {l}\n")).collect()
}

/// A Markdown worklist, written for an agent that has not seen this
/// conversation: what changed among the rules the repo cites, where each
/// is cited, and what to do.
fn worklist(old: &str, new: &str, root: &Path) -> Result<String> {
    let old = Rules::parse(old);
    let new = Rules::parse(new);
    let changes = diff(&old, &new);
    let cites = every_citation(root)?;
    let date = |r: &Rules| r.effective.clone().unwrap_or_else(|| "(undated)".into());

    let mut out = String::new();
    writeln!(
        out,
        "# Comprehensive Rules: {} → {}\n",
        date(&old),
        date(&new)
    )?;
    let (cited, uncited): (Vec<&Change>, Vec<&Change>) = changes
        .iter()
        .filter(|c| !matches!(c, Change::Added(_)))
        .partition(|c| cites.iter().any(|x| x.citation.cites.covers(c.id())));
    let added: Vec<RuleId> = changes
        .iter()
        .filter_map(|c| match c {
            Change::Added(id) => Some(*id),
            _ => None,
        })
        .collect();
    writeln!(
        out,
        "{} rules changed or removed and {} added. {} of the changed or removed are cited in the repo.\n",
        cited.len() + uncited.len(),
        added.len(),
        cited.len()
    )?;
    writeln!(
        out,
        "## What to do\n\n\
         For each cited rule below, read the old and the new text, then every place that cites it. \
         Where the code or the doc relied on what changed, fix the behaviour and its tests, or the claim. \
         Where a rule was renumbered, update the citation. Where nothing relied on it, leave it. \
         Then run `cargo run -p chancellor-cr -- pin <new URL>`, check `cargo run -p chancellor-cr -- check` is clean, \
         and commit it all together, saying in the PR which cited rules changed meaning and what you did about each.\n"
    )?;
    writeln!(out, "## Cited rules that changed\n")?;
    for c in &cited {
        let id = c.id();
        match c {
            Change::Changed(_) => {
                writeln!(
                    out,
                    "### {id}: changed\n\nWas:\n\n{}\nNow:\n\n{}",
                    quote(&old.rules[&id]),
                    quote(&new.rules[&id])
                )?;
            }
            Change::Removed {
                moved_to: Some(to), ..
            } => {
                writeln!(out, "### {id}: renumbered to {to}, text unchanged\n")?;
            }
            Change::Removed { moved_to: None, .. } => {
                writeln!(
                    out,
                    "### {id}: removed\n\nWas:\n\n{}",
                    quote(&old.rules[&id])
                )?;
            }
            Change::Added(_) => unreachable!("added rules were filtered out"),
        }
        // "CR 613" covers 613.4c but says less about it than "CR 613.4c"
        // does. A precise citation is the one likely to have relied on the
        // changed words, so it is quoted and the broad ones only located.
        let mut precise: Vec<(String, &str)> = Vec::new();
        let mut broad: Vec<String> = Vec::new();
        for x in cites.iter().filter(|x| x.citation.cites.covers(id)) {
            let at = format!("{}:{}", x.file.display(), x.citation.line);
            match x.citation.cites {
                Cited::One(c) if c != id => {
                    if !broad.contains(&at) {
                        broad.push(at);
                    }
                }
                _ => {
                    if !precise.iter().any(|(a, _)| *a == at) {
                        precise.push((at, &x.line_text));
                    }
                }
            }
        }
        if !precise.is_empty() {
            writeln!(out, "Cited at:\n")?;
            for (at, text) in &precise {
                writeln!(out, "- `{at}` {text}")?;
            }
            writeln!(out)?;
        }
        if !broad.is_empty() {
            let at: Vec<String> = broad.iter().map(|a| format!("`{a}`")).collect();
            writeln!(
                out,
                "Also under a citation of a rule or section above it: {}\n",
                at.join(", ")
            )?;
        }
    }
    let list = |ids: &mut dyn Iterator<Item = RuleId>| {
        ids.map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
    };
    writeln!(
        out,
        "## Changed or removed, cited nowhere\n\n{}\n",
        list(&mut uncited.iter().map(|c| c.id()))
    )?;
    writeln!(
        out,
        "## Added\n\nNew rules can mean new mechanics Vivisurgeon's Insight doesn't read yet.\n\n{}",
        list(&mut added.into_iter())
    )?;
    Ok(out)
}

fn iso_date(effective: &str) -> Result<String> {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let mut words = effective.split([' ', ',']).filter(|w| !w.is_empty());
    let (Some(month), Some(day), Some(year)) = (words.next(), words.next(), words.next()) else {
        bail!("can't read the date {effective:?}");
    };
    let month = MONTHS
        .iter()
        .position(|m| *m == month)
        .context("unknown month")?
        + 1;
    Ok(format!("{year}-{month:02}-{:02}", day.parse::<u8>()?))
}

fn pin(url: &str) -> Result<ExitCode> {
    let text = download(url)?;
    let rules = Rules::parse(&text);
    let effective = rules
        .effective
        .as_deref()
        .context("the text says no \"effective as of\" date; is this the CR?")?;
    let pin = Pin {
        url: url.to_string(),
        effective: iso_date(effective)?,
        sha256: sha256(text.as_bytes()),
    };
    let json = facet_json::to_string_pretty(&pin).map_err(|e| anyhow::anyhow!("{e}"))?;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pin.json");
    fs::write(&path, json + "\n")?;
    eprintln!(
        "cr: pinned {} rules effective {}, sha256 {}, in {}",
        rules.rules.len(),
        pin.effective,
        pin.sha256,
        path.display()
    );
    Ok(ExitCode::SUCCESS)
}
