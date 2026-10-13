//! Every pinned tree, read again from its Oracle text: the milestone files
//! in `golden/`, a promise that each of their cards parses, and the corpus
//! in `snapshot/`, a record of how every face reads now, its broken lines
//! included. A changed tree fails here until someone has read it and blessed
//! it with `UPDATE_GOLDEN=1 cargo test -p vivisurgeons-insight --test golden`,
//! so a grammar change's commit carries every tree it moved.

use std::fs;
use std::path::PathBuf;
use std::thread;

use vivisurgeons_insight::golden;

/// Moved trees printed in full; the rest are counted. A rule that moves
/// thousands is read from `git diff` after blessing, not from here.
const SHOWN: usize = 40;

fn files(dir: &str) -> Vec<PathBuf> {
    let dir = format!("{}/{dir}", env!("CARGO_MANIFEST_DIR"));
    let mut v: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{dir}: {e}"))
        .map(|e| e.unwrap().path())
        .collect();
    v.sort();
    v
}

/// The part of two one-line trees that differs, with some context either
/// side: the whole of a long `Debug` line hides a one-word change.
fn changed(a: &str, b: &str) -> String {
    let pre = a
        .char_indices()
        .zip(b.chars())
        .find(|((_, x), y)| x != y)
        .map_or(a.len().min(b.len()), |((i, _), _)| i);
    let suf = a[pre..]
        .chars()
        .rev()
        .zip(b[pre..].chars().rev())
        .take_while(|(x, y)| x == y)
        .map(|(x, _)| x.len_utf8())
        .sum::<usize>();
    let cut = |s: &str| {
        let start = s.floor_char_boundary(pre.saturating_sub(60));
        let end = s.ceil_char_boundary((s.len() - suf + 60).min(s.len()));
        format!(
            "{}{}{}",
            if start > 0 { "…" } else { "" },
            &s[start..end],
            if end < s.len() { "…" } else { "" }
        )
    };
    format!("  pinned {}\n  now    {}", cut(a), cut(b))
}

/// Each moved face in one file, and the file blessed if asked.
fn check(path: &PathBuf, bless: bool) -> Vec<String> {
    let text = fs::read_to_string(path).unwrap();
    let pinned = golden::read(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut moved = Vec::new();
    for (e, trees) in &pinned {
        let now = e.trees();
        if &now == trees {
            continue;
        }
        let lines = (0..now.len().max(trees.len()))
            .filter_map(|i| {
                let (p, n) = (trees.get(i), now.get(i));
                (p != n).then(|| changed(p.map_or("", |s| s), n.map_or("", |s| s)))
            })
            .collect::<Vec<_>>()
            .join("\n");
        moved.push(format!(
            "{} ({}) in {}:\n{lines}",
            e.card,
            e.face,
            path.display()
        ));
    }
    if bless && !moved.is_empty() {
        let entries: Vec<_> = pinned.into_iter().map(|(e, _)| e).collect();
        fs::write(path, golden::render(&entries)).unwrap();
    }
    moved
}

#[test]
fn pinned_trees_have_not_moved() {
    let bless = std::env::var_os("UPDATE_GOLDEN").is_some();
    let all: Vec<PathBuf> = files("golden")
        .into_iter()
        .chain(files("snapshot"))
        .collect();
    let n = thread::available_parallelism().map_or(4, |n| n.get());
    let moved: Vec<String> = thread::scope(|s| {
        let workers: Vec<_> = all
            .chunks(all.len().div_ceil(n))
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .flat_map(|p| check(p, bless))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap())
            .collect()
    });
    assert!(
        bless || moved.is_empty(),
        "{} pinned faces read differently now; read each, then bless with UPDATE_GOLDEN=1:\n\n{}{}",
        moved.len(),
        moved
            .iter()
            .take(SHOWN)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n"),
        if moved.len() > SHOWN {
            format!(
                "\n\n… and {} more; bless, then read them in `git diff`",
                moved.len() - SHOWN
            )
        } else {
            String::new()
        }
    );
}
