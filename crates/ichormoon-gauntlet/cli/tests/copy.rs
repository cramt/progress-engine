//! `gauntlet sync` keeping the copy of Scryfall, from bulk files on disk so
//! CI proves the whole path without Scryfall.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chip_scryfall::copy::store::{Meta, Slot, META, UNREADABLE};
use chip_scryfall::copy::ScryfallCopy;

fn cards() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reality-chip/scryfall/tests/fixtures/scryfall-copy.jsonl")
}

fn tags() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/oracle-tags.jsonl")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pe-copy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sync(dir: &Path, cards: &Path, extra: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gauntlet"));
    command
        .arg("sync")
        .arg("--copy")
        .arg(dir)
        .arg("--cards-from")
        .arg(cards)
        .arg("--tags-from")
        .arg(tags())
        .args(extra)
        // Should the copy dir ever be ignored, the default is a scratch one
        // rather than the cache of whoever runs the tests.
        .env("SCRYFALL_CACHE", dir.join("cache"));
    command
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn meta(dir: &Path) -> Meta {
    Meta::parse(&std::fs::read_to_string(dir.join(META)).unwrap()).unwrap()
}

/// The copy is the slot the meta names, gzipped, and the snapshot is the
/// same text uncompressed: a header line, then a line per card with its tags
/// as indexes into the header's, a tag's ancestors included.
#[test]
fn sync_keeps_a_copy_and_writes_its_snapshot() {
    let dir = scratch("keep");
    let snapshot = dir.join("snapshot.txt");
    let out = sync(&dir, &cards(), &["--snapshot", snapshot.to_str().unwrap()])
        .output()
        .unwrap();
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert!(err.contains("skipping the index"), "{err}");
    assert!(err.contains("making the copy"), "{err}");

    let kept = meta(&dir);
    assert_eq!(kept.slot(), Slot::A);
    let mut text = String::new();
    std::io::Read::read_to_string(
        &mut flate2::read::GzDecoder::new(std::fs::File::open(dir.join(Slot::A.file())).unwrap()),
        &mut text,
    )
    .unwrap();
    assert_eq!(std::fs::read_to_string(&snapshot).unwrap(), text);
    let copy = ScryfallCopy::load(&text).unwrap();
    assert_eq!(copy.updated_at(), kept.updated_at());

    let (head, body) = text.split_once('\n').unwrap();
    let head: serde_json::Value = serde_json::from_str(head).unwrap();
    let names: Vec<&str> = head["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    let cards = head["cards"].as_u64().unwrap() as usize;
    let sol_ring = body
        .lines()
        .take(cards)
        .find(|l| l.starts_with("Sol Ring\t"))
        .expect("a Sol Ring card line");
    let mut on: Vec<&str> = sol_ring
        .split('\t')
        .nth(4)
        .unwrap()
        .split(',')
        .map(|i| names[i.parse::<usize>().unwrap()])
        .collect();
    on.sort_unstable();
    assert_eq!(
        on,
        ["adds-multiple-mana", "mana-producer", "mana-rock", "ramp"],
        "tagged mana-rock alone of the three in a line, and carries all three"
    );
}

#[test]
fn a_current_copy_is_left_be_and_force_builds_beside_it() {
    let dir = scratch("current");
    assert!(sync(&dir, &cards(), &[]).output().unwrap().status.success());

    let out = sync(&dir, &cards(), &[]).output().unwrap();
    let err = stderr(&out);
    assert!(out.status.success(), "{err}");
    assert!(err.contains("already current"), "{err}");
    assert!(err.contains("reads back"), "{err}");
    assert!(!err.contains("making the copy"), "{err}");
    assert_eq!(meta(&dir).slot(), Slot::A);

    let out = sync(&dir, &cards(), &["--force"]).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(meta(&dir).slot(), Slot::B, "the old copy is not built over");
}

/// The second waits on the first's lock, then finds its copy current.
#[test]
fn two_syncs_at_once_build_one_copy() {
    let dir = scratch("race");
    let children: Vec<_> = (0..2)
        .map(|_| {
            sync(&dir, &cards(), &[])
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let errs: Vec<String> = children
        .into_iter()
        .map(|c| {
            let out = c.wait_with_output().unwrap();
            assert!(out.status.success(), "{}", stderr(&out));
            stderr(&out)
        })
        .collect();
    let built = errs
        .iter()
        .filter(|e| e.contains("making the copy"))
        .count();
    assert_eq!(built, 1, "{errs:#?}");
    assert!(
        errs.iter().any(|e| e.contains("already current")),
        "{errs:#?}"
    );
}

/// A file the builder refuses costs nothing kept, and is remembered, so the
/// next sync does not read it again to fail the same way.
#[test]
fn a_file_the_builder_refuses_is_remembered_and_the_copy_kept_stands() {
    let dir = scratch("refused");
    assert!(sync(&dir, &cards(), &[]).output().unwrap().status.success());
    let before = std::fs::read_to_string(dir.join(META)).unwrap();

    let bad = dir.join("default-cards.jsonl");
    let mut text = std::fs::read_to_string(cards()).unwrap();
    text.push_str("{\"object\":\"card\",\"name\":\n");
    std::fs::write(&bad, text).unwrap();

    let out = sync(&dir, &bad, &[]).output().unwrap();
    let err = stderr(&out);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("cannot read"), "{err}");
    assert_eq!(std::fs::read_to_string(dir.join(META)).unwrap(), before);
    assert!(dir.join(UNREADABLE).exists());

    let out = sync(&dir, &bad, &[]).output().unwrap();
    let err = stderr(&out);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("could not read when it last tried"), "{err}");
    assert!(!err.contains("making the copy"), "{err}");
}

#[test]
fn a_copy_needs_both_bulk_files() {
    let dir = scratch("half");
    let out = Command::new(env!("CARGO_BIN_EXE_gauntlet"))
        .arg("sync")
        .arg("--copy")
        .arg(&dir)
        .arg("--cards-from")
        .arg(cards())
        .output()
        .unwrap();
    let err = stderr(&out);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("--tags-from"), "{err}");
    assert!(!dir.join(META).exists());
}
