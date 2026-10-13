//! `insight coverage` reads every card in Scryfall's oracle bulk file and
//! reports how many parse with no hole; `insight card <name>` prints one tree.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;

use anyhow::{bail, Context, Result};
use vivisurgeons_insight::golden::{self, Entry};
use vivisurgeons_insight::{read_face, Face, Line};

use vivisurgeons_insight::corpus::{self, cache, excluded_ids, EXCLUDED_TAG};

struct Card {
    name: String,
    edhrec_rank: Option<u64>,
    /// Each face as the bulk file has it.
    faces: Vec<Entry>,
    lines: Vec<Line>,
}

impl Card {
    fn parsed(&self) -> bool {
        self.lines.iter().all(|l| l.parsed.is_ok())
    }
}

fn load(bulk: &str, excluded: &HashSet<String>, only: Option<&str>) -> Result<(Vec<Card>, usize)> {
    let (printed, skipped) = corpus::read(bulk, excluded, only)?;
    let cards = printed
        .into_iter()
        .map(|p| Card {
            lines: p.faces.iter().flat_map(Entry::lines).collect(),
            name: p.name,
            edhrec_rank: p.edhrec_rank,
            faces: p.faces,
        })
        .collect();
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

/// The milestone lists in `milestones/`: one card name per line, `#` lines
/// say where the list came from. Read at run time so a refreshed list needs
/// no rebuild.
fn milestones() -> Result<Vec<(String, Vec<String>)>> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/milestones");
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {dir}"))? {
        let path = entry?.path();
        let text = fs::read_to_string(&path)?;
        let names = text
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_string)
            .collect();
        let label = path.file_stem().unwrap_or_default().to_string_lossy();
        out.push((label.into_owned(), names));
    }
    out.sort();
    Ok(out)
}

/// Lists outside Scryfall name a double-faced card by its front face, so a
/// name matches either the whole card or the face before ` // `.
fn by_any_name(cards: &[Card]) -> HashMap<&str, &Card> {
    let mut map = HashMap::new();
    for c in cards {
        map.insert(c.name.as_str(), c);
        if let Some((front, _)) = c.name.split_once(" // ") {
            map.entry(front).or_insert(c);
        }
    }
    map
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
    for n in [1000, 2000, 5000] {
        report(
            &format!("EDHREC top {n}"),
            &cards
                .iter()
                .filter(|c| c.edhrec_rank.is_some_and(|r| r <= n))
                .collect::<Vec<_>>(),
        );
    }
    let by_name = by_any_name(&cards);
    for (label, names) in milestones()? {
        let found: Vec<&Card> = names
            .iter()
            .filter_map(|n| by_name.get(n.as_str()).copied())
            .collect();
        report(&label, &found);
        // Excluded cards land here too, as do cards the bulk file renamed.
        let missing: Vec<&String> = names
            .iter()
            .filter(|n| !by_name.contains_key(n.as_str()))
            .collect();
        if !missing.is_empty() {
            println!("  not matched in the bulk file: {missing:?}");
        }
    }
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

/// Which card `insight card` reads: a printing as Scryfall's URLs name it
/// (`2xm/321`), or the card's own name.
enum Lookup {
    Printing { set: String, number: String },
    Name(String),
}

impl Lookup {
    fn parse(args: &[String]) -> Result<Self> {
        let joined = args.join(" ");
        let joined = joined.trim();
        if joined.is_empty() {
            bail!("which card? a name, or set/number like 2xm/321");
        }
        // A name never has a slash without spaces round it ("Fire // Ice"),
        // so one bare slash is always a printing.
        Ok(match joined.split_once('/') {
            Some((set, number))
                if !joined.contains(char::is_whitespace)
                    && !set.is_empty()
                    && !number.is_empty()
                    && !number.contains('/') =>
            {
                Lookup::Printing {
                    set: set.to_ascii_lowercase(),
                    number: number.to_string(),
                }
            }
            _ => Lookup::Name(joined.to_string()),
        })
    }

    /// The Oracle name, which is what the corpus is keyed by: the oracle bulk
    /// file holds one printing per card, so another printing's set and number
    /// have to be asked of Scryfall.
    fn name(self) -> Result<String> {
        let (set, number) = match self {
            Lookup::Name(n) => return Ok(n),
            Lookup::Printing { set, number } => (set, number),
        };
        let url = format!("https://api.scryfall.com/cards/{set}/{number}");
        let body = ureq::get(&url)
            .header(
                "User-Agent",
                concat!("vivisurgeons-insight/", env!("CARGO_PKG_VERSION")),
            )
            .header("Accept", "application/json")
            .call()
            .with_context(|| format!("asking Scryfall for {set}/{number}"))?
            .body_mut()
            .read_to_string()
            .with_context(|| format!("reading Scryfall's answer for {set}/{number}"))?;
        let card: serde_json::Value = serde_json::from_str(&body)?;
        card["name"]
            .as_str()
            .map(str::to_string)
            .with_context(|| format!("Scryfall's {set}/{number} has no name"))
    }
}

fn card(args: &[String]) -> Result<()> {
    let name = Lookup::parse(args)?.name()?;
    let (cards, _) = load(&cache("oracle-cards.jsonl"), &HashSet::new(), Some(&name))?;
    let c = cards
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(&name))
        .with_context(|| {
            format!("no card named {name} in the corpus (not Vintage-legal, or not synced)")
        })?;
    for l in &c.lines {
        println!("{}", l.text);
        match &l.parsed {
            Ok(a) => {
                println!("  {a:#?}");
                // The round trip beside the tree, so a rule and its rendering
                // are written against the same card.
                let canon = vivisurgeons_insight::unparse::ability(a).text();
                match l.round_trip() {
                    Some(Ok(())) => println!("  round-trips: {canon}"),
                    Some(Err(d)) => println!(
                        "  DOES NOT round-trip at token {}: wanted {}, found {:?}\n  renders {canon}",
                        d.at,
                        d.want.join(" | "),
                        d.found
                    ),
                    None => {}
                }
            }
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

/// The cards a target names: the EDHREC top N when it is a number, else the
/// milestone list of that name.
fn select<'a>(cards: &'a [Card], target: &str) -> Result<Vec<&'a Card>> {
    let mut found: Vec<&Card> = match target.parse::<u64>() {
        Ok(n) => cards
            .iter()
            .filter(|c| c.edhrec_rank.is_some_and(|r| r <= n))
            .collect(),
        Err(_) => {
            let (_, names) = milestones()?
                .into_iter()
                .find(|(label, _)| label == target)
                .with_context(|| format!("no milestone named {target}"))?;
            let by_name = by_any_name(cards);
            names
                .iter()
                .filter_map(|n| by_name.get(n.as_str()).copied())
                .collect()
        }
    };
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found.dedup_by_key(|c| c.name.as_str());
    Ok(found)
}

/// Every failing ability among the EDHREC top N, or on a milestone list, most
/// played first: the worklist for a round aimed at cards people play.
fn failing(args: &[String]) -> Result<()> {
    let target = args.first().map(String::as_str).unwrap_or("1000");
    let excluded = excluded_ids(&cache("oracle-tags.jsonl"))?;
    let (cards, _) = load(&cache("oracle-cards.jsonl"), &excluded, None)?;
    let mut top = select(&cards, target)?;
    top.retain(|c| !c.parsed());
    // Unranked cards go last rather than first.
    top.sort_by_key(|c| c.edhrec_rank.unwrap_or(u64::MAX));
    for c in top {
        for l in c.lines.iter() {
            if let Err(b) = &l.parsed {
                println!(
                    "{:>5} {}\n      {}\n      BROKE at {:?} after: {}",
                    c.edhrec_rank.unwrap_or(0),
                    c.name,
                    l.text.replace('\n', " / "),
                    b.at,
                    b.read
                );
            }
        }
    }
    Ok(())
}

/// Pins the trees of a target's cards in `golden/<target>.txt`, which the
/// golden test then holds the grammar to. Only a target that fully parses can
/// be pinned: a golden file is a promise, not a progress report.
fn pin(args: &[String]) -> Result<()> {
    let target = args
        .first()
        .context("pin needs a target: N or a milestone")?;
    let excluded = excluded_ids(&cache("oracle-tags.jsonl"))?;
    let (cards, _) = load(&cache("oracle-cards.jsonl"), &excluded, None)?;
    let chosen = select(&cards, target)?;
    let broken = chosen.iter().filter(|c| !c.parsed()).count();
    if broken > 0 {
        bail!("{broken} cards in {target} do not parse yet; `insight failing {target}` lists them");
    }
    let entries: Vec<Entry> = chosen.iter().flat_map(|c| c.faces.clone()).collect();
    let label = if target.parse::<u64>().is_ok() {
        format!("edhrec-{target}")
    } else {
        target.clone()
    };
    let path = format!("{}/golden/{label}.txt", env!("CARGO_MANIFEST_DIR"));
    fs::create_dir_all(format!("{}/golden", env!("CARGO_MANIFEST_DIR")))?;
    fs::write(&path, golden::render(&entries))?;
    println!(
        "{} cards, {} faces pinned in {path}",
        chosen.len(),
        entries.len()
    );
    Ok(())
}

/// Writes `snapshot/`: every face of the corpus with the trees it reads into
/// now, the lines that break included. The golden test holds the grammar to
/// it, so a rule that turns a right parse into a complete but wrong one, which
/// moves no coverage number, fails until its moved trees are read. Run it
/// after `scryfall sync` brings new cards or new Oracle text.
///
/// Given a path, it writes the driver's flat file there instead: one line
/// per ability that parses, "card | text | tree", sorted, which the driver
/// diffs between a batch's baseline and its end.
fn snapshot(args: &[String]) -> Result<()> {
    let (cards, _) = load(&cache("oracle-cards.jsonl"), &HashSet::new(), None)?;
    if let Some(out) = args.first() {
        let mut rows: Vec<String> = cards
            .iter()
            .flat_map(|c| {
                c.lines.iter().filter_map(move |l| {
                    l.parsed
                        .as_ref()
                        .ok()
                        .map(|a| format!("{} | {} | {a:?}", c.name, l.text.replace('\n', " / ")))
                })
            })
            .collect();
        rows.sort();
        fs::write(out, rows.join("\n") + "\n")?;
        println!("{} parsed abilities written to {out}", rows.len());
        return Ok(());
    }
    let mut shards: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
    for c in &cards {
        shards
            .entry(golden::shard(&c.name))
            .or_default()
            .extend(c.faces.iter().cloned());
    }
    let dir = format!("{}/snapshot", env!("CARGO_MANIFEST_DIR"));
    // Rewritten whole, so a card Scryfall dropped leaves no stale entry.
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    for (file, entries) in &mut shards {
        entries.sort_by(|a, b| a.card.cmp(&b.card));
        fs::write(format!("{dir}/{file}"), golden::render(entries))?;
    }
    let faces: usize = shards.values().map(Vec::len).sum();
    println!(
        "{} cards, {faces} faces in {} files under {dir}",
        cards.len(),
        shards.len()
    );
    Ok(())
}

/// How many abilities render back into their own words, and the ones that
/// do not, grouped by where they first part from the text, commonest first.
/// `--golden` reads the pinned files instead of the bulk file.
fn roundtrip(args: &[String]) -> Result<()> {
    let mut golden_only = false;
    let mut show_n = 3;
    let mut target = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--golden" => golden_only = true,
            "--examples" => show_n = it.next().context("--examples needs a number")?.parse()?,
            t => target = Some(t),
        }
    }
    let mut rows: Vec<(String, Line)> = Vec::new();
    if golden_only {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/golden");
        for entry in fs::read_dir(dir)? {
            let text = fs::read_to_string(entry?.path())?;
            for (e, _) in golden::read(&text).map_err(anyhow::Error::msg)? {
                let face = Face {
                    name: &e.face,
                    type_line: &e.type_line,
                    text: &e.text,
                };
                rows.extend(
                    read_face(&face, &e.card)
                        .into_iter()
                        .map(|l| (e.card.clone(), l)),
                );
            }
        }
    } else {
        let excluded = excluded_ids(&cache("oracle-tags.jsonl"))?;
        let (cards, _) = load(&cache("oracle-cards.jsonl"), &excluded, None)?;
        let chosen: Option<HashSet<String>> = target
            .map(|t| select(&cards, t).map(|cs| cs.iter().map(|c| c.name.clone()).collect()))
            .transpose()?;
        for c in cards {
            if chosen.as_ref().is_none_or(|names| names.contains(&c.name)) {
                rows.extend(c.lines.into_iter().map(|l| (c.name.clone(), l)));
            }
        }
    }
    let mut ok = 0;
    let mut parsed = 0;
    let mut groups: HashMap<String, Vec<(String, String, String)>> = HashMap::new();
    for (card, l) in &rows {
        let Some(r) = l.round_trip() else { continue };
        parsed += 1;
        match r {
            Ok(()) => ok += 1,
            Err(d) => {
                let toks = vivisurgeons_insight::unparse::line_tokens(&l.text);
                let before =
                    d.at.checked_sub(1)
                        .and_then(|i| toks.get(i))
                        .map(vivisurgeons_insight::show)
                        .unwrap_or_default();
                let key = format!(
                    "after {before:?}: want {} found {:?}",
                    d.want.join(" | "),
                    d.found
                );
                let Ok(a) = &l.parsed else { continue };
                let canon = vivisurgeons_insight::unparse::ability(a).text();
                groups.entry(key).or_default().push((
                    card.clone(),
                    l.text.replace('\n', " / "),
                    canon.replace('\n', " / "),
                ));
            }
        }
    }
    println!(
        "{ok}/{parsed} parsed abilities round-trip ({}); {} of {} lines did not parse",
        pct(ok, parsed),
        rows.len() - parsed,
        rows.len()
    );
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    for (key, v) in &groups {
        println!("\n{:>5}  {key}", v.len());
        for (card, text, canon) in v.iter().take(show_n) {
            println!("       {card}: {text}\n         -> {canon}");
        }
    }
    Ok(())
}

/// Every parsed tree in the corpus through `lint`: the count of each kind,
/// and the first cards of each. `--counts` prints only the counts, in the
/// form `tests/lint-baseline.txt` holds.
fn lint(args: &[String]) -> Result<()> {
    use vivisurgeons_insight::lint::{self, Kind};
    let mut show_n = 5;
    let mut counts_only = false;
    let mut only: Option<Kind> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--examples" => show_n = it.next().context("--examples needs a number")?.parse()?,
            "--counts" => counts_only = true,
            k => only = Some(Kind::from_name(k).with_context(|| format!("no lint named {k}"))?),
        }
    }
    let excluded = excluded_ids(&cache("oracle-tags.jsonl"))?;
    let (printed, _) = corpus::read(&cache("oracle-cards.jsonl"), &excluded, None)?;
    let faces: Vec<Entry> = printed.into_iter().flat_map(|p| p.faces).collect();
    let mut hits: HashMap<Kind, Vec<(String, String)>> = HashMap::new();
    for (k, card, text) in lint::faces(&faces) {
        hits.entry(k).or_default().push((card, text));
    }
    for k in Kind::ALL {
        if only.is_some_and(|o| o != k) {
            continue;
        }
        let v = hits.get(&k).map(Vec::as_slice).unwrap_or_default();
        println!("{k} {}", v.len());
        if counts_only {
            continue;
        }
        for (card, text) in v.iter().take(show_n) {
            println!("    {card}: {}", text.replace('\n', " / "));
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("coverage") => coverage(&args[1..]),
        Some("card") => card(&args[1..]),
        Some("sample") => sample(&args[1..]),
        Some("failing") => failing(&args[1..]),
        Some("snapshot") => snapshot(&args[1..]),
        Some("pin") => pin(&args[1..]),
        Some("roundtrip") => roundtrip(&args[1..]),
        Some("lint") => lint(&args[1..]),
        // Anything else is a card, so `cargo run -- 2xm/321` reads one.
        Some(_) => card(&args),
        None => bail!("usage: insight [card] <name | set/number> | insight coverage [--bulk F] [--tags F] [--deck F]... [--top N] |insight sample N | insight failing [N | milestone] | insight snapshot [file] | insight pin <N | milestone> | insight roundtrip [N | milestone] [--golden] [--examples N] | insight lint [kind] [--counts] [--examples N]"),
    }
}

#[cfg(test)]
mod tests {
    use super::Lookup;

    fn parse(s: &str) -> Lookup {
        Lookup::parse(&s.split(' ').map(str::to_string).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn a_bare_slash_is_a_printing_and_a_split_card_is_a_name() {
        assert!(matches!(
            parse("2XM/321"),
            Lookup::Printing { set, number } if set == "2xm" && number == "321"
        ));
        assert!(matches!(parse("Fire // Ice"), Lookup::Name(n) if n == "Fire // Ice"));
        assert!(matches!(parse("Oblivion Ring"), Lookup::Name(_)));
        assert!(matches!(parse("Fire//Ice"), Lookup::Name(_)));
        assert!(Lookup::parse(&[]).is_err());
    }
}
