//! `insight coverage` reads every card in Scryfall's oracle bulk file and
//! reports how many parse with no hole; `insight card <name>` prints one tree.

use std::collections::{HashMap, HashSet};
use std::fs;

use anyhow::{bail, Context, Result};
use serde_json::Value;
use vivisurgeons_insight::{read_face, Face, Line};

/// Layouts that are not cards a deck can hold.
const NOT_CARDS: &[&str] = &[
    "token",
    "double_faced_token",
    "emblem",
    "art_series",
    "scheme",
    "planar",
    "vanguard",
    "reversible_card",
];

/// Cards with a rule in the Comprehensive Rules written for them alone. They
/// are the tail a grammar should not chase: each is its own special case in
/// any engine, so the headline leaves them out and says how many.
const EXCLUDED_TAG: &str = "unique-cr-reference";

struct Card {
    name: String,
    edhrec_rank: Option<u64>,
    lines: Vec<Line>,
}

impl Card {
    fn parsed(&self) -> bool {
        self.lines.iter().all(|l| l.parsed.is_ok())
    }
}

fn cache(file: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/.cache/scryfall/{file}")
}

fn excluded_ids(tags_path: &str) -> Result<HashSet<String>> {
    let text = fs::read_to_string(tags_path).with_context(|| format!("reading {tags_path}"))?;
    for line in text.lines() {
        let v: Value = serde_json::from_str(line)?;
        if v["slug"] == EXCLUDED_TAG {
            return Ok(v["taggings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t["oracle_id"].as_str().map(str::to_string))
                .collect());
        }
    }
    bail!("{tags_path} has no {EXCLUDED_TAG} tag")
}

fn load(bulk: &str, excluded: &HashSet<String>, only: Option<&str>) -> Result<(Vec<Card>, usize)> {
    let text = fs::read_to_string(bulk).with_context(|| format!("reading {bulk}"))?;
    let mut cards = Vec::new();
    let mut skipped = 0;
    for line in text.lines() {
        let c: Value = serde_json::from_str(line)?;
        let layout = c["layout"].as_str().unwrap_or("");
        if NOT_CARDS.contains(&layout) || c["set_type"] == "funny" {
            continue;
        }
        if c["oracle_id"]
            .as_str()
            .is_some_and(|id| excluded.contains(id))
        {
            skipped += 1;
            continue;
        }
        let name = c["name"].as_str().unwrap_or("").to_string();
        if only.is_some_and(|o| !name.eq_ignore_ascii_case(o)) {
            continue;
        }
        let faces: Vec<(String, String, String)> = match c["card_faces"].as_array() {
            Some(fs) if c["oracle_text"].is_null() => fs
                .iter()
                .map(|f| {
                    (
                        f["name"].as_str().unwrap_or("").to_string(),
                        f["type_line"].as_str().unwrap_or("").to_string(),
                        f["oracle_text"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect(),
            _ => vec![(
                name.clone(),
                c["type_line"].as_str().unwrap_or("").to_string(),
                c["oracle_text"].as_str().unwrap_or("").to_string(),
            )],
        };
        let lines = faces
            .iter()
            .flat_map(|(n, t, o)| {
                read_face(
                    &Face {
                        name: n,
                        type_line: t,
                        text: o,
                    },
                    &name,
                )
            })
            .collect();
        cards.push(Card {
            name,
            edhrec_rank: c["edhrec_rank"].as_u64(),
            lines,
        });
    }
    Ok((cards, skipped))
}

/// Card names from a `.deck.toml`, read off the comment Curator writes beside
/// each printing. Good enough for a spike; a real reader goes through the index.
fn deck_names(path: &str) -> Result<Vec<String>> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    Ok(text
        .lines()
        .filter(|l| l.contains("printing"))
        .filter_map(|l| l.rsplit_once("# ").map(|(_, n)| n.trim().to_string()))
        .collect())
}

fn pct(a: usize, b: usize) -> String {
    if b == 0 {
        return "-".into();
    }
    format!("{:.1}%", 100.0 * a as f64 / b as f64)
}

fn report(label: &str, cards: &[&Card]) {
    let ok = cards.iter().filter(|c| c.parsed()).count();
    let lines: Vec<&Line> = cards.iter().flat_map(|c| &c.lines).collect();
    let lines_ok = lines.iter().filter(|l| l.parsed.is_ok()).count();
    println!(
        "{label:<28} cards {ok:>6}/{:<6} {:>6}   abilities {lines_ok:>6}/{:<6} {:>6}",
        cards.len(),
        pct(ok, cards.len()),
        lines.len(),
        pct(lines_ok, lines.len()),
    );
}

/// Digits become N, so "draw two cards" and "draw three cards" stay apart
/// (their words differ) but "{2}" and "{3}" costs do not.
fn shape(text: &str) -> String {
    let mut out = String::new();
    let mut in_digits = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('N');
            }
            in_digits = true;
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out
}

fn coverage(args: &[String]) -> Result<()> {
    let mut bulk = cache("oracle-cards.jsonl");
    let mut tags = cache("oracle-tags.jsonl");
    let mut decks = Vec::new();
    let mut top = 30;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--bulk" => bulk = it.next().context("--bulk needs a path")?.clone(),
            "--tags" => tags = it.next().context("--tags needs a path")?.clone(),
            "--deck" => decks.push(it.next().context("--deck needs a path")?.clone()),
            "--top" => top = it.next().context("--top needs a number")?.parse()?,
            other => bail!("unknown argument {other}"),
        }
    }
    let excluded = excluded_ids(&tags)?;
    let (cards, skipped) = load(&bulk, &excluded, None)?;
    println!("excluded {skipped} cards tagged otag:{EXCLUDED_TAG}\n");
    report("all cards", &cards.iter().collect::<Vec<_>>());
    report(
        "EDHREC top 1000",
        &cards
            .iter()
            .filter(|c| c.edhrec_rank.is_some_and(|r| r <= 1000))
            .collect::<Vec<_>>(),
    );
    let by_name: HashMap<&str, &Card> = cards.iter().map(|c| (c.name.as_str(), c)).collect();
    for d in &decks {
        let names = deck_names(d)?;
        let found: Vec<&Card> = names
            .iter()
            .filter_map(|n| by_name.get(n.as_str()).copied())
            .collect();
        let label = d.rsplit('/').next().unwrap_or(d);
        report(label, &found);
        let missing: Vec<&String> = names
            .iter()
            .filter(|n| !by_name.contains_key(n.as_str()))
            .collect();
        if !missing.is_empty() {
            println!("  not in the bulk file: {missing:?}");
        }
        for c in found.iter().filter(|c| !c.parsed()) {
            for l in c.lines.iter().filter(|l| l.parsed.is_err()) {
                println!("  {:<32} {}", c.name, l.text.replace('\n', " "));
            }
        }
    }

    let mut shapes: HashMap<String, usize> = HashMap::new();
    let mut breaks: HashMap<String, usize> = HashMap::new();
    for c in &cards {
        for l in &c.lines {
            if let Err(b) = &l.parsed {
                *shapes.entry(shape(&l.text.replace('\n', " "))).or_default() += 1;
                *breaks.entry(b.at.clone()).or_default() += 1;
            }
        }
    }
    let mut shapes: Vec<_> = shapes.into_iter().collect();
    shapes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    println!(
        "\n{} distinct failing ability shapes; the commonest:",
        shapes.len()
    );
    for (s, n) in shapes.iter().take(top) {
        println!("{n:>6}  {s}");
    }
    let mut breaks: Vec<_> = breaks.into_iter().collect();
    breaks.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    println!("\nthe token each failing ability broke at:");
    for (s, n) in breaks.iter().take(top) {
        println!("{n:>6}  {s}");
    }
    Ok(())
}

fn card(args: &[String]) -> Result<()> {
    let name = args.join(" ");
    let (cards, _) = load(&cache("oracle-cards.jsonl"), &HashSet::new(), Some(&name))?;
    let c = cards
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(&name))
        .with_context(|| format!("no card named {name}"))?;
    for l in &c.lines {
        println!("{}", l.text);
        match &l.parsed {
            Ok(a) => println!("  {a:#?}"),
            Err(b) => println!("  BROKE at {:?} after: {}", b.at, b.read),
        }
    }
    Ok(())
}

/// Evenly spaced abilities that parsed, with their trees, for reading by eye:
/// a complete parse can still be a wrong one, and only a reader catches that.
fn sample(args: &[String]) -> Result<()> {
    let n: usize = args.first().map(|a| a.parse()).transpose()?.unwrap_or(20);
    let (cards, _) = load(&cache("oracle-cards.jsonl"), &HashSet::new(), None)?;
    let parsed: Vec<(&str, &Line)> = cards
        .iter()
        .flat_map(|c| c.lines.iter().map(move |l| (c.name.as_str(), l)))
        .filter(|(_, l)| l.parsed.is_ok())
        // Bare keyword lines are the least interesting thing to check.
        .filter(|(_, l)| {
            !matches!(
                l.parsed,
                Ok(vivisurgeons_insight::ast::Ability::Keywords(_))
            )
        })
        .collect();
    let step = (parsed.len() / n.max(1)).max(1);
    for (name, l) in parsed.iter().step_by(step).take(n) {
        println!("{name}: {}\n  {:?}\n", l.text, l.parsed.as_ref().ok());
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("coverage") => coverage(&args[1..]),
        Some("card") => card(&args[1..]),
        Some("sample") => sample(&args[1..]),
        _ => bail!("usage: insight coverage [--bulk F] [--tags F] [--deck F]... [--top N] | insight card <name>"),
    }
}
