//! Every reference back in the corpus, as the resolver reads it
//! (`vivisurgeons_insight::backref`, [backref] in SEMANTICS.md).
//!
//! Reads every face in `snapshot/`, parses it again, and counts each
//! reference by what it may mean: one introducer (DETERMINED), several
//! (AMBIGUOUS, the nearest that fits is picked), none (NONE).
//!
//! `cargo run --release -p vivisurgeons-insight --bin backref -- [--cards]
//! [--examples Antecedent:Ambiguous] [--card NAME]`

use std::collections::BTreeMap;

use vivisurgeons_insight::ast::Ability;
use vivisurgeons_insight::backref::{analyse, Class, Kind, Target};
use vivisurgeons_insight::golden::{self, Entry};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let examples_of = arg("--examples");
    let card = arg("--card");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/snapshot");
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .expect("snapshot/")
        .map(|e| e.expect("an entry").path())
        .collect();
    paths.sort();
    let mut faces: Vec<Entry> = Vec::new();
    for p in paths {
        let text = std::fs::read_to_string(&p).expect("a snapshot file");
        faces.extend(
            golden::read(&text)
                .expect("golden format")
                .into_iter()
                .map(|(e, _)| e),
        );
    }
    let mut table: BTreeMap<(Kind, Class), usize> = BTreeMap::new();
    let mut none_why: BTreeMap<String, usize> = BTreeMap::new();
    let mut amb_n: BTreeMap<usize, usize> = BTreeMap::new();
    let mut amb_self = 0;
    let mut cross = 0;
    let mut examples: Vec<String> = Vec::new();
    // The resolved trees, checked from themselves.
    let mut pointers: BTreeMap<Kind, usize> = BTreeMap::new();
    let mut problems: BTreeMap<String, usize> = BTreeMap::new();
    let mut problem_lines: Vec<String> = Vec::new();
    let show_problems = args.iter().any(|a| a == "--problems");
    for e in &faces {
        let done = e.lines_pointers();
        let abs: Vec<(Option<&Ability>, bool)> = done
            .iter()
            .map(|l| (l.parsed.as_ref().ok(), l.continues))
            .collect();
        let an = analyse(&abs, &e.type_line);
        for r in &an.resolved {
            *pointers.entry(r.kind).or_default() += 1;
            if show_problems && matches!(r.kind, Kind::Text(_)) && r.class != Class::None {
                problem_lines.push(format!("{}: a reference left unresolved resolves on another reading: line {} {} -> {:?}", e.card, r.line, an.reference_text(r), r.pick.as_ref().map(|t| an.describe(t))));
            }
        }
        for (k, p) in an.problems() {
            let what = match k {
                Kind::Text(h) => format!("unresolved {h:?}"),
                k => format!("{k:?} that misses"),
            };
            *problems.entry(what).or_default() += 1;
            if show_problems {
                problem_lines.push(format!("{}: {p}", e.card));
            }
        }
        let lines = e.lines_unresolved();
        let abs: Vec<(Option<&Ability>, bool)> = lines
            .iter()
            .map(|l| (l.parsed.as_ref().ok(), l.continues))
            .collect();
        let an = analyse(&abs, &e.type_line);
        for r in &an.resolved {
            *table.entry((r.kind, r.class)).or_default() += 1;
            cross += usize::from(r.cross_line);
            match r.class {
                Class::None => {
                    let ctx = match an.verb(r) {
                        "@granted" => "in a granted/quoted ability".to_string(),
                        v => format!("verb {v}"),
                    };
                    *none_why
                        .entry(format!("{:?} | {ctx} | kind {:?}", r.kind, an.kind(r)))
                        .or_default() += 1;
                }
                Class::Ambiguous => {
                    *amb_n.entry(r.cands.len().min(5)).or_default() += 1;
                    if r.cands.len() == 2 && r.cands.contains(&Target::SelfCard) {
                        amb_self += 1;
                    }
                }
                Class::Determined => {}
            }
            let tag = format!("{:?}:{:?}", r.kind, r.class);
            let wanted = examples_of.as_deref() == Some(tag.as_str())
                || examples_of.as_deref() == Some("unpointed")
                    && r.pick.is_some()
                    && r.pointer.is_none()
                    && r.kind != Kind::Itself
                || card.as_deref().is_some_and(|c| e.card.starts_with(c));
            if wanted && examples.len() < 80 {
                let text = lines[r.line].text.replace('\n', " ");
                examples.push(format!(
                    "{} | {} | {tag} ref {} | pick: {} | cands: {}{}",
                    e.card,
                    text.chars().take(200).collect::<String>(),
                    an.reference_text(r),
                    r.pick.as_ref().map(|t| an.describe(t)).unwrap_or_default(),
                    r.cands
                        .iter()
                        .map(|t| an.describe(t))
                        .collect::<Vec<_>>()
                        .join(" ; "),
                    if r.note.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", r.note)
                    }
                ));
            }
        }
    }
    println!("{} faces", faces.len());
    println!(
        "{:<18} {:>8} {:>8} {:>8} {:>8} {:>7}",
        "hint", "DETERM", "AMBIG", "NONE", "total", "%det"
    );
    let hints: std::collections::BTreeSet<Kind> = table.keys().map(|k| k.0).collect();
    let (mut td, mut ta, mut tn) = (0, 0, 0);
    for k in hints {
        let get = |c| table.get(&(k, c)).copied().unwrap_or(0);
        let (d, a, n) = (
            get(Class::Determined),
            get(Class::Ambiguous),
            get(Class::None),
        );
        (td, ta, tn) = (td + d, ta + a, tn + n);
        println!(
            "{:<18} {d:>8} {a:>8} {n:>8} {:>8} {:>6.1}%",
            format!("{k:?}"),
            d + a + n,
            100.0 * d as f64 / (d + a + n) as f64
        );
    }
    let t = td + ta + tn;
    println!(
        "{:<18} {td:>8} {ta:>8} {tn:>8} {t:>8} {:>6.1}%",
        "ALL",
        100.0 * td as f64 / t as f64
    );
    println!(
        "ambiguous by candidate count (5 = 5+): {amb_n:?}; of the 2-way, this card vs one object: {amb_self}"
    );
    println!("picks in an earlier paragraph: {cross}");
    let mut why: Vec<_> = none_why.into_iter().collect();
    why.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("NONE by hint/context (top 30):");
    for (k, n) in why.iter().take(30) {
        println!("  {n:>5} {k}");
    }
    println!("\nresolved trees: references by kind {pointers:?}");
    println!("problems (pointers that miss, references with no introducer): {problems:?}");
    for l in &problem_lines {
        println!("  ! {l}");
    }
    for l in &examples {
        println!("  ex: {l}");
    }
}
