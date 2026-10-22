//! Blame on a deck with a few hundred commits, timed natively, and the same
//! repo written as a zip that Settings imports, to time blame in the browser.
//!
//! `cargo run --release -p meldweb-wasm --example blame_bench -- [out.zip] [saves]`
//!
//! On 2026-10-22, on an i7-4770K: 100 cards over 413 commits (13 merges)
//! blamed natively in 220 ms, and 0.5 ms again from what the first one
//! remembered. In Chromium with the shipped `wasm` build, the zip blamed
//! on the page's thread took 593 to 927 ms the first time and 9 ms again;
//! imported into the site, the Blame view's first ask took 904 ms in the
//! repo's worker beside the deck page's other reads, and one with an unsaved
//! edit on top 17 ms.

#[path = "../tests/common/mod.rs"]
mod common;

use std::time::Instant;

use meldweb_git::{Change, FsStore, Repo, RepoPath, Tip, WriteCommit};
use meldweb_wasm::blame::DeckBlame;
use meldweb_wasm::repo::Local;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next();
    let saves: usize = args.next().map_or(400, |n| n.parse().expect("a count"));

    let dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
    let main = meldweb_git::BranchName::new("main").unwrap();
    repo.commit(
        &main,
        Tip::Unborn,
        &WriteCommit {
            changes: vec![Change::Put {
                path: RepoPath::new("VERSION").unwrap(),
                bytes: b"2\n".to_vec(),
            }],
            message: "version 2\n".to_owned(),
            author: common::sig("Alex", common::T0 - 60),
            committer: common::sig("Alex", common::T0 - 60),
        },
    )
    .unwrap();
    let made = common::fixture(&mut repo, saves);
    drop(repo);

    // The repo as the browser holds it: every file of the git dir, loaded.
    let mut local = Local::default();
    for entry in walk(dir.path()) {
        let rel = entry.strip_prefix(dir.path()).unwrap().to_str().unwrap();
        if entry.is_dir() {
            local.load_dir(rel);
        } else {
            local.load(rel, &std::fs::read(&entry).unwrap());
        }
    }
    assert!(local.open(), "the fixture opens");

    let mut times = Vec::new();
    let mut cards = 0;
    for _ in 0..5 {
        let start = Instant::now();
        let DeckBlame::Blamed { cards: c, .. } =
            local.blame("lantern", "decks/lantern/main", None).unwrap()
        else {
            panic!("no deck")
        };
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        cards = c.len();
    }
    println!("blamed {cards} cards over {made} commits: {times:.1?} ms");

    if let Some(out) = out {
        std::fs::write(&out, local.export_zip().unwrap()).unwrap();
        println!("wrote {out}");
    }
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        out.push(p.clone());
        if p.is_dir() {
            out.extend(walk(&p));
        }
    }
    out
}
