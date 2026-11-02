//! `chancellor-logs`: the corpus of real games (README, "Real games").
//!
//! `fetch` downloads PDBot's Magic Online logs from
//! logs.pennydreadfulmagic.com into `~/.cache/chancellor/pdlogs/<id>.txt`,
//! and `coverage` reads every cached log with `chancellor_of_the_spires::log`
//! and says how many games parse whole and how many Insight reads whole,
//! and `replay` plays those games through the engine and ranks what stops
//! them (`chancellor_of_the_spires::replay`).
//!
//! Native only: a browser build has no network or cache to fetch into.

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    native::main()
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::{BTreeSet, HashMap, HashSet};
    use std::fs;
    use std::path::PathBuf;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use anyhow::{bail, Context, Result};
    use chancellor_of_the_spires::deck::{self, CardText};
    use chancellor_of_the_spires::log;
    use chancellor_of_the_spires::replay::{self as r, End};
    use chip_scryfall::bulk::BulkCard;
    use chip_scryfall::index::Index;
    use vivisurgeons_insight::corpus;

    const SITE: &str = "https://logs.pennydreadfulmagic.com";
    const USER_AGENT: &str = concat!(
        "chancellor-logs/",
        env!("CARGO_PKG_VERSION"),
        " (Chancellor of the Spires' replay corpus; https://github.com/cramt/progress-engine)"
    );
    /// The site is one volunteer's; a request a second is the promise.
    /// The margin keeps it under that whatever the clock does.
    const GAP: Duration = Duration::from_millis(1050);

    const USAGE: &str = "usage: chancellor-logs fetch [--formats Freeform,PennyDreadful] [--max N] | chancellor-logs coverage [--bulk F] [--top N] | chancellor-logs replay [--bulk F] [--top N] [--game ID]";

    pub fn main() -> Result<()> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        match args.first().map(String::as_str) {
            Some("fetch") => fetch(&args[1..]),
            Some("coverage") => coverage(&args[1..]),
            Some("replay") => replay(&args[1..]),
            _ => bail!("{USAGE}"),
        }
    }

    fn cache_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join(".cache/chancellor/pdlogs")
    }

    /// Match ids with a log in the cache. Only `<digits>.txt` counts, so a
    /// half-written `.part` or a stray file is never taken for a match.
    fn cached(dir: &PathBuf) -> Result<BTreeSet<u64>> {
        let mut ids = BTreeSet::new();
        if !dir.exists() {
            return Ok(ids);
        }
        for entry in fs::read_dir(dir)? {
            let name = entry?.file_name();
            let name = name.to_string_lossy();
            if let Some(id) = name.strip_suffix(".txt").and_then(|s| s.parse().ok()) {
                ids.insert(id);
            }
        }
        Ok(ids)
    }

    /// Every request goes through here, so none can start within `GAP` of
    /// the one before.
    struct Polite {
        agent: ureq::Agent,
        last: Option<Instant>,
        requests: usize,
    }

    enum Got {
        Body(Vec<u8>),
        NotFound,
    }

    impl Polite {
        fn get(&mut self, url: &str) -> Result<Got> {
            let mut backoff = Duration::from_secs(30);
            for attempt in 0..4 {
                if let Some(last) = self.last {
                    let since = last.elapsed();
                    if since < GAP {
                        sleep(GAP - since);
                    }
                }
                self.last = Some(Instant::now());
                self.requests += 1;
                let res = self.agent.get(url).header("User-Agent", USER_AGENT).call();
                match res {
                    Ok(mut r) => {
                        return Ok(Got::Body(
                            r.body_mut()
                                .with_config()
                                .limit(64 * 1024 * 1024)
                                .read_to_vec()
                                .with_context(|| format!("reading {url}"))?,
                        ))
                    }
                    Err(ureq::Error::StatusCode(404)) => return Ok(Got::NotFound),
                    Err(e) if attempt < 3 => {
                        eprintln!("{url}: {e}; waiting {}s", backoff.as_secs());
                        sleep(backoff);
                        backoff *= 2;
                    }
                    Err(e) => return Err(e).with_context(|| format!("fetching {url}")),
                }
            }
            unreachable!("the last attempt returns")
        }
    }

    /// Match ids a listing page links, in the page's order (newest first).
    fn match_links(html: &str) -> Vec<u64> {
        let mut seen = HashSet::new();
        html.split("href=\"/match/")
            .skip(1)
            .filter_map(|rest| rest.split_once("/\"").and_then(|(id, _)| id.parse().ok()))
            .filter(|id| seen.insert(*id))
            .collect()
    }

    fn fetch(args: &[String]) -> Result<()> {
        let mut formats = vec!["Freeform".to_string(), "PennyDreadful".to_string()];
        let mut max = 3000;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--formats" => {
                    formats = it
                        .next()
                        .context("--formats needs a list")?
                        .split(',')
                        .map(str::to_string)
                        .collect()
                }
                "--max" => max = it.next().context("--max needs a number")?.parse()?,
                other => bail!("unknown argument {other}\n{USAGE}"),
            }
        }
        let dir = cache_dir();
        fs::create_dir_all(&dir)?;
        let mut have = cached(&dir)?;
        let mut web = Polite {
            agent: ureq::Agent::new_with_defaults(),
            last: None,
            requests: 0,
        };
        let started = Instant::now();
        let mut fetched = 0;
        'formats: for format in &formats {
            for page in 1.. {
                if have.len() >= max {
                    break 'formats;
                }
                let url = format!("{SITE}/formats/{format}/?page={page}");
                let ids = match web.get(&url)? {
                    Got::Body(b) => match_links(&String::from_utf8_lossy(&b)),
                    Got::NotFound => Vec::new(),
                };
                if ids.is_empty() {
                    eprintln!("{format}: no matches on page {page}, done");
                    break;
                }
                for id in ids {
                    if have.len() >= max {
                        break 'formats;
                    }
                    if have.contains(&id) {
                        continue;
                    }
                    let body = match web.get(&format!("{SITE}/export/{id}/"))? {
                        Got::Body(b) => b,
                        Got::NotFound => {
                            eprintln!("{id}: no export");
                            continue;
                        }
                    };
                    let part = dir.join(format!("{id}.txt.part"));
                    fs::write(&part, &body)?;
                    fs::rename(&part, dir.join(format!("{id}.txt")))?;
                    have.insert(id);
                    fetched += 1;
                    eprintln!(
                        "{format} page {page}: {id} ({} cached, {fetched} fetched, {} requests in {:.0}s)",
                        have.len(),
                        web.requests,
                        started.elapsed().as_secs_f64()
                    );
                }
            }
        }
        eprintln!(
            "{fetched} fetched, {} cached, {} requests in {:.0}s",
            have.len(),
            web.requests,
            started.elapsed().as_secs_f64()
        );
        Ok(())
    }

    /// A line's kind: cards, players, numbers, parentheses and quotes
    /// blanked, so "alice plays [Island]." and "bob plays [Forest]." are one.
    fn shape(line: &str, players: &[String]) -> String {
        let mut s = line.to_string();
        let mut names: Vec<&String> = players.iter().collect();
        names.sort_by_key(|n| std::cmp::Reverse(n.len()));
        for n in names {
            s = s.replace(n.as_str(), "\u{1}");
        }
        let mut out = String::new();
        let mut depth = (0, 0, false);
        for ch in s.chars() {
            match ch {
                '[' => {
                    if depth.0 == 0 && depth.1 == 0 {
                        out.push_str("[C]");
                    }
                    depth.0 += 1;
                }
                ']' if depth.0 > 0 => depth.0 -= 1,
                '(' if depth.0 == 0 => {
                    if depth.1 == 0 {
                        out.push_str("(..)");
                    }
                    depth.1 += 1;
                }
                ')' if depth.1 > 0 => depth.1 -= 1,
                _ if depth.0 > 0 || depth.1 > 0 => {}
                '"' => {
                    if !depth.2 {
                        out.push_str("\"..\"");
                    }
                    depth.2 = !depth.2;
                }
                _ if depth.2 => {}
                '\u{1}' => out.push('P'),
                c if c.is_ascii_digit() => {
                    if !out.ends_with('N') {
                        out.push('N');
                    }
                }
                c => out.push(c),
            }
        }
        out
    }

    /// The line with the match's players renamed `P1`, `P2`: the report
    /// never prints a real username.
    fn anonymise(line: &str, players: &[String]) -> String {
        let mut names: Vec<(usize, &String)> = players.iter().enumerate().collect();
        names.sort_by_key(|(_, n)| std::cmp::Reverse(n.len()));
        let mut s = line.to_string();
        for (i, n) in &names {
            s = s.replace(n.as_str(), &format!("\u{1}{i}\u{1}"));
        }
        for (i, _) in &names {
            s = s.replace(&format!("\u{1}{i}\u{1}"), &format!("P{}", i + 1));
        }
        s
    }

    fn pct(a: usize, b: usize) -> String {
        if b == 0 {
            return "-".into();
        }
        format!("{:.1}%", 100.0 * a as f64 / b as f64)
    }

    /// What Insight makes of a card the logs name.
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Read {
        Whole,
        Broken,
        /// Not in the bulk file under that name or any face's.
        Missing,
    }

    /// The bulk file's cards by the names the logs use, and what Insight
    /// makes of each.
    struct Names {
        printed: Vec<corpus::Printed>,
        by_name: HashMap<String, usize>,
        read: HashMap<String, (String, Read)>,
    }

    impl Names {
        fn load(bulk: &str) -> Result<Names> {
            // Every card of the bulk file by its name and each face's: the
            // log names a double-faced card by the face it shows.
            let (printed, _) = corpus::read(bulk, &HashSet::new(), None)?;
            let mut by_name: HashMap<String, usize> = HashMap::new();
            for (i, p) in printed.iter().enumerate() {
                by_name.insert(p.name.clone(), i);
                for f in &p.faces {
                    by_name.entry(f.face.clone()).or_insert(i);
                }
            }
            Ok(Names {
                printed,
                by_name,
                read: HashMap::new(),
            })
        }

        /// The bulk file's name for the card the log names, and what
        /// Insight makes of it.
        fn read(&mut self, name: &str) -> (String, Read) {
            if let Some(r) = self.read.get(name) {
                return r.clone();
            }
            // Magic Online writes a split or double-faced card's two names
            // as "A/B" or "A && B" where Scryfall has "A // B".
            let found = self.by_name.get(name).or_else(|| {
                let joined = if name.contains(" && ") {
                    name.replace(" && ", " // ")
                } else if !name.contains(" // ") {
                    name.replace('/', " // ")
                } else {
                    name.to_string()
                };
                self.by_name.get(&joined)
            });
            let r = match found {
                None => (name.to_string(), Read::Missing),
                Some(&i) => {
                    let whole = self.printed[i]
                        .faces
                        .iter()
                        .all(|f| f.lines().iter().all(|l| l.parsed.is_ok()));
                    let r = if whole { Read::Whole } else { Read::Broken };
                    (self.printed[i].name.clone(), r)
                }
            };
            self.read.insert(name.to_string(), r.clone());
            r
        }

        /// Whether the game parsed into events whole and Insight reads
        /// every card it names, tokens aside: the games `coverage` counts
        /// as both, and the ones `replay` plays.
        fn whole(&mut self, g: &log::Game) -> bool {
            g.unrecognised().next().is_none()
                && g.entries
                    .iter()
                    .flat_map(|e| e.event.cards())
                    .filter(|c| !c.is_token())
                    .all(|c| self.read(c.name()).1 == Read::Whole)
        }
    }

    /// Every card of the bulk file as the engine reads it, by its name.
    fn card_texts(bulk: &str) -> Result<HashMap<String, CardText>> {
        let text = fs::read_to_string(bulk).with_context(|| format!("reading {bulk}"))?;
        let records = text
            .lines()
            .filter_map(|l| facet_json::from_str::<BulkCard>(l).ok());
        let (index, _) = Index::build(records, None);
        Ok(index
            .cards
            .values()
            .map(|c| (c.name.clone(), deck::text(c)))
            .collect())
    }

    /// The log's turn number at entry `i`: each player's own count, as
    /// the log numbers them, and 0 before the first turn.
    fn turn_at(g: &log::Game, i: usize) -> u32 {
        g.entries
            .get(i)
            .or(g.entries.last())
            .and_then(|e| e.turn)
            .map_or(0, |t| t.number)
    }

    fn replay(args: &[String]) -> Result<()> {
        let mut bulk = corpus::cache("oracle-cards.jsonl");
        let mut top = 12;
        let mut only: Option<u64> = None;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--bulk" => bulk = it.next().context("--bulk needs a path")?.clone(),
                "--top" => top = it.next().context("--top needs a number")?.parse()?,
                "--game" => only = Some(it.next().context("--game needs an id")?.parse()?),
                other => bail!("unknown argument {other}\n{USAGE}"),
            }
        }
        let mut names = Names::load(&bulk)?;
        let texts = card_texts(&bulk)?;
        let dir = cache_dir();

        let mut games = 0;
        let mut eligible = 0;
        let mut turnless = 0;
        let mut finished = 0;
        let mut winner_line = 0;
        // Lines before each game's stop (all of a finished game's), and all
        // lines: a smoother measure than games to the end, which moves only
        // when a game's last stop goes.
        let mut followed = 0;
        let mut lines = 0;
        // Reason (or card) -> games, and the first game and line it stopped.
        let mut reasons: HashMap<String, (usize, String)> = HashMap::new();
        let mut by_card: HashMap<String, usize> = HashMap::new();
        let mut reached: std::collections::BTreeMap<u32, (usize, usize)> = Default::default();
        for id in cached(&dir)? {
            let text = fs::read(dir.join(format!("{id}.txt")))?;
            let Ok(m) = log::parse(&String::from_utf8_lossy(&text)) else {
                continue;
            };
            for g in &m.games {
                games += 1;
                if only.is_some_and(|o| o != g.id) || !names.whole(g) {
                    continue;
                }
                // An empty log, or a concession during the mulligans, is
                // nothing to replay.
                if !g
                    .entries
                    .iter()
                    .any(|e| matches!(e.event, log::Event::TurnBegins(_)))
                {
                    turnless += 1;
                    continue;
                }
                eligible += 1;
                let mut cards = |name: &str| texts.get(&names.read(name).0).cloned();
                let replayed = r::replay(&m, g, &mut cards);
                if only.is_some() {
                    print_game(&m, g, &replayed);
                }
                let (stop_at, turn) = match &replayed.end {
                    End::Finished => (None, turn_at(g, g.entries.len())),
                    End::Stopped { entry, .. } => (Some(*entry), turn_at(g, *entry)),
                };
                lines += g.entries.len();
                followed += stop_at.unwrap_or(g.entries.len());
                let row = reached.entry(turn).or_default();
                match (&replayed.end, stop_at) {
                    (End::Stopped { stop, .. }, Some(at)) => {
                        row.1 += 1;
                        let line = g
                            .entries
                            .get(at)
                            .map_or(String::new(), |e| anonymise(&e.text, &m.players));
                        let r = reasons
                            .entry(stop.kind())
                            .or_insert_with(|| (0, format!("game {} line: {line}", g.id)));
                        r.0 += 1;
                        if let Some(c) = stop.card() {
                            *by_card.entry(format!("{}: {c}", stop.kind())).or_default() += 1;
                        }
                    }
                    _ => {
                        row.0 += 1;
                        finished += 1;
                        winner_line += g.winner.is_some() as usize;
                    }
                }
            }
        }
        if only.is_some() {
            return Ok(());
        }
        println!(
            "{eligible} of {games} games replayed: those that parse into events whole, whose every card Insight reads whole, and that reach a first turn ({turnless} more don't: empty logs and concessions during the mulligans)"
        );
        println!(
            "replayed to the end: {finished} ({}), {winner_line} of them to the log's winner line",
            pct(finished, eligible)
        );
        println!(
            "lines followed: {followed} of {lines} ({:.2}%), the lines before each game's stop",
            100.0 * followed as f64 / lines.max(1) as f64
        );
        let mut reasons: Vec<_> = reasons.into_iter().collect();
        reasons.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
        println!("\nstop reasons by games:");
        for (kind, (n, example)) in reasons.iter().take(top) {
            println!("{n:>6}  {kind}\n        e.g. {example}");
        }
        let mut cards: Vec<_> = by_card.into_iter().collect();
        cards.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        println!("\nstops by card:");
        for (card, n) in cards.iter().take(top) {
            println!("{n:>6}  {card}");
        }
        println!(
            "\nturn reached (each player's own count, as the log numbers turns; 0 is the setup):"
        );
        println!("  turn  finished  stopped");
        for (turn, (f, s)) in &reached {
            println!("  {turn:>4}  {f:>8}  {s:>7}");
        }
        Ok(())
    }

    /// One game's replay, line by line, with what the engine did.
    fn print_game(m: &log::Match, g: &log::Game, replayed: &r::Replay) {
        println!("game {} (game {} of the match)", g.id, g.number);
        let last = match &replayed.end {
            End::Stopped { entry, .. } => *entry,
            End::Finished => g.entries.len(),
        };
        for t in replayed
            .trace
            .iter()
            .filter(|t| t.entry == 0 && t.did.starts_with("set up"))
        {
            println!("  {}", anonymise(&t.did, &m.players));
        }
        for (i, e) in g.entries.iter().enumerate().take(last + 1) {
            println!("{i:>4} {}", anonymise(&e.text, &m.players));
            for t in replayed
                .trace
                .iter()
                .filter(|t| t.entry == i && !t.did.starts_with("set up"))
            {
                println!("       -> {}", t.did);
            }
        }
        match &replayed.end {
            End::Finished => println!("replayed to the end"),
            End::Stopped { entry, stop } => println!("stopped at line {entry}: {stop}"),
        }
    }

    fn coverage(args: &[String]) -> Result<()> {
        let mut bulk = corpus::cache("oracle-cards.jsonl");
        let mut top = 25;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--bulk" => bulk = it.next().context("--bulk needs a path")?.clone(),
                "--top" => top = it.next().context("--top needs a number")?.parse()?,
                other => bail!("unknown argument {other}\n{USAGE}"),
            }
        }
        let dir = cache_dir();
        let ids = cached(&dir)?;

        let mut names = Names::load(&bulk)?;
        let mut read_card = |name: &str| names.read(name);

        let mut matches = 0;
        let mut malformed = 0;
        let mut games = 0;
        let mut parsed_whole = 0;
        let mut insight_whole = 0;
        let mut both_whole = 0;
        let mut by_format: HashMap<String, (usize, usize)> = HashMap::new();
        let mut modern_tagged = (0, 0);
        let mut kinds: HashMap<String, (usize, String)> = HashMap::new();
        let mut blocks: HashMap<String, (usize, Read)> = HashMap::new();
        for id in &ids {
            let text = fs::read(dir.join(format!("{id}.txt")))?;
            let m = match log::parse(&String::from_utf8_lossy(&text)) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("{id}: {e}");
                    malformed += 1;
                    continue;
                }
            };
            matches += 1;
            // A freeform match names its real format, if at all, in the
            // comment or the Gatherling event.
            let modern = m.format == "Freeform"
                && std::iter::once(&m.comment)
                    .chain(m.games.iter().flat_map(|g| &g.notes))
                    .any(|t| t.to_lowercase().contains("modern"));
            for g in &m.games {
                games += 1;
                let f = by_format.entry(m.format.clone()).or_default();
                f.0 += 1;
                if m.format == "Freeform" {
                    modern_tagged.1 += 1;
                    modern_tagged.0 += modern as usize;
                }
                let mut whole = true;
                for line in g.unrecognised() {
                    whole = false;
                    let k = kinds
                        .entry(shape(line, &m.players))
                        .or_insert_with(|| (0, anonymise(line, &m.players)));
                    k.0 += 1;
                }
                parsed_whole += whole as usize;
                let names: BTreeSet<&str> = g
                    .entries
                    .iter()
                    .flat_map(|e| e.event.cards())
                    .filter(|c| !c.is_token())
                    .map(|c| c.name())
                    .collect();
                let unread: BTreeSet<(String, Read)> = names
                    .into_iter()
                    .map(&mut read_card)
                    .filter(|(_, r)| *r != Read::Whole)
                    .collect();
                if unread.is_empty() {
                    insight_whole += 1;
                    f.1 += 1;
                    both_whole += whole as usize;
                }
                for (n, r) in unread {
                    blocks.entry(n).or_insert((0, r)).0 += 1;
                }
            }
        }

        println!("{matches} matches ({malformed} unreadable), {games} games");
        println!(
            "parsed into events whole: {parsed_whole}/{games} games ({})",
            pct(parsed_whole, games)
        );
        println!(
            "every card read whole by Insight: {insight_whole}/{games} games ({}); both: {both_whole} ({})",
            pct(insight_whole, games),
            pct(both_whole, games)
        );
        let mut formats: Vec<_> = by_format.into_iter().collect();
        formats.sort();
        for (f, (n, ok)) in formats {
            println!(
                "  {f:<16} {n:>6} games, Insight reads {ok} whole ({})",
                pct(ok, n)
            );
        }
        println!(
            "  Freeform games whose comment or event says Modern: {}/{}",
            modern_tagged.0, modern_tagged.1
        );

        let mut kinds: Vec<_> = kinds.into_iter().collect();
        kinds.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
        println!(
            "\nunrecognised line kinds: {} kinds, {} lines",
            kinds.len(),
            kinds.iter().map(|k| k.1 .0).sum::<usize>()
        );
        for (shape, (n, example)) in kinds.iter().take(top) {
            println!("{n:>6}  {shape}\n        e.g. {example}");
        }

        let mut worklist: Vec<_> = blocks.into_iter().collect();
        worklist.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(&b.0)));
        println!("\nworklist: cards by games they keep Insight from reading whole");
        for (name, (n, r)) in worklist.iter().take(top) {
            let why = match r {
                Read::Missing => "  (not in the bulk file)",
                _ => "",
            };
            println!("{n:>6}  {name}{why}");
        }
        let missing: Vec<_> = worklist
            .iter()
            .filter(|(_, (_, r))| *r == Read::Missing)
            .map(|(name, (n, _))| format!("{name} ({n})"))
            .collect();
        println!(
            "\nnames not in the bulk file ({}): {}",
            missing.len(),
            missing.join(", ")
        );
        Ok(())
    }
}
