//! `insight lint` held to two bars. The pinned trees have no violation at
//! all. The rest of the corpus may have some, but no more of any kind than
//! `tests/lint-baseline.txt` says: a ratchet, lowered as trees are fixed.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use vivisurgeons_insight::corpus::{self, cache};
use vivisurgeons_insight::golden;
use vivisurgeons_insight::lint::{self, Kind};

#[test]
fn pinned_trees_have_no_lint() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/golden");
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).expect("golden/ exists") {
        let path = entry.unwrap().path();
        let text = fs::read_to_string(&path).unwrap();
        let pinned = golden::read(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        entries.extend(pinned.into_iter().map(|(e, _)| e));
    }
    let hits: Vec<String> = lint::faces(&entries)
        .into_iter()
        .map(|(k, card, text)| format!("{k}: {card}: {text}"))
        .collect();
    assert!(
        hits.is_empty(),
        "{} lint violations in pinned trees:\n{}",
        hits.len(),
        hits.join("\n")
    );
}

const BASELINE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/lint-baseline.txt");

fn read_baseline() -> BTreeMap<Kind, usize> {
    let text = fs::read_to_string(BASELINE).expect("tests/lint-baseline.txt exists");
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (k, n) = l
                .split_once(' ')
                .unwrap_or_else(|| panic!("bad line {l:?}"));
            let k = Kind::from_name(k).unwrap_or_else(|| panic!("no lint named {k}"));
            (
                k,
                n.parse().unwrap_or_else(|_| panic!("bad count in {l:?}")),
            )
        })
        .collect()
}

/// Needs the bulk file, which CI does not have, so it says so and passes
/// without one.
#[test]
fn corpus_lint_counts_do_not_rise() {
    let bulk = cache("oracle-cards.jsonl");
    let tags = cache("oracle-tags.jsonl");
    if !Path::new(&bulk).exists() || !Path::new(&tags).exists() {
        eprintln!("no Scryfall bulk file at {bulk}: corpus lint not checked");
        return;
    }
    let excluded = corpus::excluded_ids(&tags).unwrap();
    let (printed, _) = corpus::read(&bulk, &excluded, None).unwrap();
    let faces: Vec<_> = printed.into_iter().flat_map(|p| p.faces).collect();
    let mut now: BTreeMap<Kind, usize> = Kind::ALL.iter().map(|&k| (k, 0)).collect();
    for (k, _, _) in lint::faces(&faces) {
        *now.entry(k).or_default() += 1;
    }
    if std::env::var_os("UPDATE_LINT").is_some() {
        let mut out = String::from(
            "# Lint violations per kind across the corpus; tests/lint.rs fails if one rises.\n\
             # `insight lint <kind>` lists them. Lower with UPDATE_LINT=1 cargo test -p vivisurgeons-insight --test lint\n",
        );
        for (k, n) in &now {
            out.push_str(&format!("{k} {n}\n"));
        }
        fs::write(BASELINE, out).unwrap();
        return;
    }
    let base = read_baseline();
    let mut rose = Vec::new();
    let mut fell = Vec::new();
    for (&k, &n) in &now {
        let was = base.get(&k).copied().unwrap_or(0);
        if n > was {
            rose.push(format!("{k}: {was} -> {n}"));
        } else if n < was {
            fell.push(format!("{k}: {was} -> {n}"));
        }
    }
    if !fell.is_empty() {
        eprintln!(
            "lint counts fell, so lower the baseline with UPDATE_LINT=1 cargo test -p vivisurgeons-insight --test lint:\n{}",
            fell.join("\n")
        );
    }
    assert!(
        rose.is_empty(),
        "lint counts rose; `insight lint <kind>` lists the trees:\n{}",
        rose.join("\n")
    );
}
