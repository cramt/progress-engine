//! A ratchet on silent zeros in the engine's source. `unwrap_or(0)` and its
//! kin turn "the engine doesn't know" into "none": counters on an object it
//! can't see, an amount a lowering never set, a branch whose condition read
//! a missing number as false. The overseer's reviews of batches 15 to 21
//! found three such guesses that no test caught (`counters_on`, `amount()`,
//! `ThatMuch`), each a wrong game waiting rather than a refusal.
//!
//! So a new one either carries `// zero: <why>` on its line or the line
//! above, saying why zero or empty is the rule's answer (cite the CR where
//! it is a rule), or it is a refusal instead. Unmarked ones are counted per
//! file in `tests/silent-zero-baseline.txt`, and this test fails if a count
//! rises. Lower the baseline with
//! `UPDATE_SILENT_ZERO=1 cargo test -p chancellor-of-the-spires --test silent_zero`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const SRC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
const BASELINE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/silent-zero-baseline.txt"
);

const PATTERNS: &[&str] = &[
    "unwrap_or(0",
    "map_or(0,",
    "_ => 0",
    "unwrap_or_default()",
    "unwrap_or(false)",
    "map_or(false,",
];

const MARK: &str = "// zero:";

fn unmarked(text: &str) -> usize {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(i, line)| {
            PATTERNS.iter().any(|p| line.contains(p))
                && !line.contains(MARK)
                && !(*i > 0 && lines[i - 1].trim_start().starts_with(MARK))
        })
        .count()
}

fn count_tree(dir: &Path, root: &Path, out: &mut BTreeMap<String, usize>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            count_tree(&path, root, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.strip_prefix(root).unwrap().display().to_string();
            out.insert(name, unmarked(&fs::read_to_string(&path).unwrap()));
        }
    }
}

#[test]
fn silent_zeros_never_rise() {
    let root = Path::new(SRC);
    let mut now = BTreeMap::new();
    count_tree(root, root, &mut now);
    if std::env::var_os("UPDATE_SILENT_ZERO").is_some() {
        let mut out = String::from(
            "# Unmarked silent zeros per file of src/; tests/silent_zero.rs fails if one rises.\n\
             # Mark a deliberate one with `// zero: <why>`. Lower with\n\
             # UPDATE_SILENT_ZERO=1 cargo test -p chancellor-of-the-spires --test silent_zero\n",
        );
        for (file, n) in now.iter().filter(|(_, n)| **n > 0) {
            out.push_str(&format!("{file} {n}\n"));
        }
        fs::write(BASELINE, out).unwrap();
        return;
    }
    let base: BTreeMap<String, usize> = fs::read_to_string(BASELINE)
        .expect("tests/silent-zero-baseline.txt exists")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let (file, n) = l.rsplit_once(' ').expect("`file count` lines");
            (file.to_string(), n.parse().expect("a count"))
        })
        .collect();
    let mut rose = Vec::new();
    let mut fell = Vec::new();
    for (file, &n) in &now {
        let was = base.get(file).copied().unwrap_or(0); // zero: a file absent from the baseline had none
        if n > was {
            rose.push(format!("{file}: {was} -> {n}"));
        } else if n < was {
            fell.push(format!("{file}: {was} -> {n}"));
        }
    }
    if !fell.is_empty() {
        eprintln!(
            "silent zeros fell, so lower the baseline with UPDATE_SILENT_ZERO=1 cargo test -p chancellor-of-the-spires --test silent_zero:\n{}",
            fell.join("\n")
        );
    }
    assert!(
        rose.is_empty(),
        "a silent zero was added without `// zero: <why>`. Mark it with the rule that makes zero right, \
         or refuse by name instead:\n{}",
        rose.join("\n")
    );
}
