//! Blame per card (ADR-0041) on real git histories: simple edits, a quantity,
//! a category move, a printing, a card removed and added again, a branch
//! merged with a two-parent merge, a clash the merge settled, and a fork.
//! Where a card's line is a text line and nothing but the card changes it,
//! the card's last change is what `git blame` says of that line.
//!
//! Without `git` on PATH the git checks print `SKIPPED` and the rest runs,
//! unless `MELDWEB_GIT_REQUIRE` is set.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use chip_decklist::stem::DeckStem;
use meldweb_git::{CommitId, FsStore, Repo, RepoPath, Store};
use meldweb_wasm::blame::{blame, Blame, CardBlame, DeckBlame, Memo};
use meldweb_wasm::branches::{self, MergePlan};
use meldweb_wasm::merging::Side;

mod common;
use common::*;

fn have_git() -> bool {
    let found = Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !found && std::env::var_os("MELDWEB_GIT_REQUIRE").is_some() {
        panic!("MELDWEB_GIT_REQUIRE is set and there is no `git` on PATH");
    }
    if !found {
        eprintln!("SKIPPED: no `git` on PATH");
    }
    found
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn blamed<S: Store>(
    repo: &Repo<S>,
    stem: &str,
    at: CommitId,
    unsaved: Option<&str>,
) -> Vec<CardBlame> {
    match blame(
        repo,
        &mut Memo::default(),
        &DeckStem::new(stem).unwrap(),
        at,
        unsaved,
    )
    .unwrap()
    {
        DeckBlame::Blamed { cards, .. } => cards,
        DeckBlame::NoDeck => panic!("no deck at {at}"),
    }
}

/// `(added, last changed)` commits of a card blamed at a commit.
fn commits_of(b: &CardBlame) -> (String, String) {
    match &b.blame {
        Blame::Added { added } => (added.commit.clone(), added.commit.clone()),
        Blame::Changed { added, changed } => (added.commit.clone(), changed.commit.clone()),
        other => panic!("unsaved: {other:?}"),
    }
}

/// What `git blame` names for each line of the deck file at `at`.
fn git_blame(dir: &Path, at: &str, file: &str) -> Vec<String> {
    git(dir, &["blame", "--root", "-l", "-s", at, "--", file])
        .lines()
        .map(|l| {
            l.split_whitespace()
                .next()
                .unwrap()
                .trim_start_matches('^')
                .to_owned()
        })
        .collect()
}

/// The text line of each card line in [`deck`]'s layout.
fn card_line(i: usize) -> usize {
    4 + i
}

#[test]
fn a_card_is_blamed_on_what_last_changed_it_and_added_where_it_came_in() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
    let sol1 = r#"{ name = "Sol Ring", in = ["Ramp"] }"#;
    let sol2 = r#"{ name = "Sol Ring", qty = 2, in = ["Ramp"] }"#;
    let stone_ramp = r#"{ name = "Mind Stone", in = ["Ramp"] }"#;
    let stone_draw = r#"{ name = "Mind Stone", in = ["Draw"] }"#;
    let signet = r#"{ printing = "woc/145", in = ["Ramp"] }  # Arcane Signet"#;
    let signet_cmm = r#"{ printing = "cmm/464", in = ["Ramp"] }  # Arcane Signet"#;
    let island = r#"{ name = "Island", qty = 10, in = ["Land"] }"#;
    let ponder = r#"{ name = "Ponder", in = ["Draw"] }"#;
    let ponder_sb = r#"{ name = "Ponder", in = ["Sideboard"] }"#;
    let opt = r#"{ name = "Opt", in = ["Draw"] }"#;

    let made = save(
        &mut repo,
        "lantern",
        "main",
        &deck(&[sol1, stone_ramp, signet, island, ponder]),
        "lantern: made",
        "Alex",
        T0,
    );
    let qty = save(
        &mut repo,
        "lantern",
        "main",
        &deck(&[sol2, stone_ramp, signet, island, ponder]),
        "lantern: Sol Ring: 1 → 2",
        "Bo",
        T0 + DAY,
    );
    let moved = save(
        &mut repo,
        "lantern",
        "main",
        &deck(&[sol2, stone_draw, signet, island, ponder]),
        "lantern: Mind Stone: ramp → draw",
        "Alex",
        T0 + 2 * DAY,
    );
    let reprinted = save(
        &mut repo,
        "lantern",
        "main",
        &deck(&[sol2, stone_draw, signet_cmm, island, ponder]),
        "lantern: Arcane Signet: woc/145 → cmm/464",
        "Alex",
        T0 + 3 * DAY,
    );
    // Island out, then back in two saves later, with a card added above it.
    save(
        &mut repo,
        "lantern",
        "main",
        &deck(&[sol2, stone_draw, signet_cmm, ponder]),
        "lantern: -10 Island",
        "Alex",
        T0 + 4 * DAY,
    );
    let readded = save(
        &mut repo,
        "lantern",
        "main",
        &deck(&[opt, sol2, stone_draw, signet_cmm, island, ponder_sb]),
        "lantern: +1 Opt, +10 Island, Ponder: draw → sideboard",
        "Bo",
        T0 + 5 * DAY,
    );

    let cards = blamed(&repo, "lantern", readded, None);
    let got: Vec<(String, String)> = cards.iter().map(commits_of).collect();
    assert_eq!(
        got,
        vec![
            (hex(readded), hex(readded)), // Opt
            (hex(made), hex(qty)),        // Sol Ring
            (hex(made), hex(moved)),      // Mind Stone
            (hex(made), hex(reprinted)),  // Arcane Signet, by printing
            (hex(readded), hex(readded)), // Island, removed and added again
            (hex(made), hex(readded)),    // Ponder, to the sideboard
        ]
    );
    let said: Vec<Vec<String>> = cards.iter().map(|c| c.changes.clone()).collect();
    assert_eq!(
        said,
        vec![
            vec!["+1 Opt".to_owned()],
            vec!["Sol Ring: 1 → 2".to_owned()],
            vec!["Mind Stone: ramp → draw".to_owned()],
            vec!["Arcane Signet: woc/145 → cmm/464".to_owned()],
            vec!["+10 Island".to_owned()],
            vec!["Ponder: draw → sideboard".to_owned()],
        ]
    );
    let Blame::Changed { added, changed } = &cards[1].blame else {
        panic!("{:?}", cards[1].blame)
    };
    assert_eq!(
        (
            added.author.as_str(),
            added.authored_at,
            changed.author.as_str()
        ),
        ("Alex", T0, "Bo")
    );
    assert_eq!(changed.message, "lantern: Sol Ring: 1 → 2");

    // At a revision, blame is that commit's.
    let then = blamed(&repo, "lantern", qty, None);
    assert_eq!(commits_of(&then[0]), (hex(made), hex(qty)));
    assert_eq!(commits_of(&then[1]), (hex(made), hex(made)));

    // The editor's unsaved text sits on top: a quantity changed is unsaved,
    // a card added is unsaved, the rest keep their commits.
    let unsaved = deck(&[
        opt,
        r#"{ name = "Sol Ring", qty = 3, in = ["Ramp"] }"#,
        stone_draw,
        signet_cmm,
        island,
        ponder_sb,
        r#"{ name = "Brainstorm", in = ["Draw"] }"#,
    ]);
    let now = blamed(&repo, "lantern", readded, Some(&unsaved));
    assert!(
        matches!(&now[1].blame, Blame::ChangedUnsaved { added } if added.commit == hex(made)),
        "{:?}",
        now[1]
    );
    assert_eq!(now[1].changes, ["Sol Ring: 2 → 3"]);
    assert!(matches!(now[6].blame, Blame::AddedUnsaved));
    assert_eq!(commits_of(&now[2]), (hex(made), hex(moved)));

    // Every card here is one line of text that only its own changes touch,
    // so `git blame` agrees line for line.
    if have_git() {
        let lines = git_blame(dir.path(), &hex(readded), "lantern.deck.toml");
        for (i, card) in cards.iter().enumerate() {
            assert_eq!(
                lines[card_line(i) - 1],
                commits_of(card).1,
                "card {i} against git blame line {}",
                card_line(i)
            );
        }
        println!("git blame agrees on {} cards", cards.len());
    }
}

#[test]
fn a_change_merged_from_a_branch_is_the_branch_commits_and_a_settled_clash_the_merges() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
    let names = [
        "Sol Ring",
        "Mind Stone",
        "Arcane Signet",
        "Command Tower",
        "Brainstorm",
        "Ponder",
        "Counterspell",
        "Island",
    ];
    let line = |n: &str, q: u32| format!(r#"{{ name = "{n}", qty = {q}, in = ["Ramp"] }}"#);
    let with = |qty: &HashMap<&str, u32>| {
        let lines: Vec<String> = names
            .iter()
            .map(|n| line(n, *qty.get(n).unwrap_or(&1)))
            .collect();
        deck(&lines.iter().map(String::as_str).collect::<Vec<_>>())
    };
    let mut q: HashMap<&str, u32> = HashMap::new();
    let made = save(
        &mut repo,
        "lantern",
        "main",
        &with(&q),
        "lantern: made",
        "Alex",
        T0,
    );
    branches::create(&mut repo, "lantern", "budget", &hex(made)).unwrap();

    // On the branch: Counterspell to 2. On main: Sol Ring to 2.
    let mut b = q.clone();
    b.insert("Counterspell", 2);
    let on_branch = save(
        &mut repo,
        "lantern",
        "budget",
        &with(&b),
        "lantern: Counterspell: 1 → 2",
        "Bo",
        T0 + DAY,
    );
    q.insert("Sol Ring", 2);
    let on_main = save(
        &mut repo,
        "lantern",
        "main",
        &with(&q),
        "lantern: Sol Ring: 1 → 2",
        "Alex",
        T0 + 2 * DAY,
    );
    let merge = plan_and_merge(&mut repo, "budget", "main", None, T0 + 3 * DAY);
    assert_eq!(repo.read_commit(&merge).unwrap().parents.len(), 2);

    let cards = blamed(&repo, "lantern", merge, None);
    assert_eq!(
        commits_of(&cards[6]),
        (hex(made), hex(on_branch)),
        "Counterspell"
    );
    assert_eq!(commits_of(&cards[0]), (hex(made), hex(on_main)), "Sol Ring");
    assert_eq!(commits_of(&cards[7]), (hex(made), hex(made)), "Island");
    let Blame::Changed { changed, .. } = &cards[6].blame else {
        panic!()
    };
    assert_eq!(changed.author, "Bo");

    if have_git() {
        let lines = git_blame(dir.path(), &hex(merge), "lantern.deck.toml");
        for (i, card) in cards.iter().enumerate() {
            assert_eq!(
                lines[card_line(i) - 1],
                commits_of(card).1,
                "card {i} against git blame"
            );
        }
        println!("git blame agrees through the two-parent merge");
    }

    // Both change Ponder: the branch to 3, main to 4, and the merge keeps the
    // branch's. That is the merge's doing. Brainstorm, which only the branch
    // changed, in the same clash's neighbourhood, stays the branch's.
    b.insert("Ponder", 3);
    b.insert("Sol Ring", 2);
    let branch_ponder = save(
        &mut repo,
        "lantern",
        "budget",
        &with(&b),
        "lantern: Ponder: 1 → 3",
        "Bo",
        T0 + 4 * DAY,
    );
    b.insert("Brainstorm", 2);
    let branch_storm = save(
        &mut repo,
        "lantern",
        "budget",
        &{
            let mut t = with(&b);
            t.push('\n');
            t
        },
        "lantern: Brainstorm: 1 → 2",
        "Bo",
        T0 + 5 * DAY,
    );
    q.insert("Ponder", 4);
    q.insert("Counterspell", 2);
    save(
        &mut repo,
        "lantern",
        "main",
        &with(&q),
        "lantern: Ponder: 1 → 4",
        "Alex",
        T0 + 6 * DAY,
    );
    let plan = branches::plan(&mut repo, "lantern", "budget", "main", None).unwrap();
    assert!(matches!(plan, MergePlan::Conflict { .. }), "{plan:?}");
    let settled = plan_and_merge(
        &mut repo,
        "budget",
        "main",
        Some(Side::Theirs),
        T0 + 7 * DAY,
    );

    let cards = blamed(&repo, "lantern", settled, None);
    assert_eq!(
        commits_of(&cards[5]),
        (hex(made), hex(settled)),
        "Ponder, the clash"
    );
    assert_eq!(cards[5].changes, ["Ponder: 4 → 3"]);
    assert_eq!(
        commits_of(&cards[4]),
        (hex(made), hex(branch_storm)),
        "Brainstorm"
    );
    assert_eq!(commits_of(&cards[0]), (hex(made), hex(on_main)), "Sol Ring");
    let _ = branch_ponder;
}

#[test]
fn a_card_unchanged_since_a_fork_is_the_originals() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
    let sol = r#"{ name = "Sol Ring", in = ["Ramp"] }"#;
    let stone = r#"{ name = "Mind Stone", in = ["Ramp"] }"#;
    let made = save(
        &mut repo,
        "lantern",
        "main",
        &deck(&[sol, stone]),
        "lantern: made",
        "Alex",
        T0,
    );
    let stone2 = r#"{ name = "Mind Stone", qty = 2, in = ["Ramp"] }"#;
    let theirs = save(
        &mut repo,
        "lantern",
        "main",
        &deck(&[sol, stone2]),
        "lantern: Mind Stone: 1 → 2",
        "Alex",
        T0 + DAY,
    );

    // Another player forks it: the fork's first commit has the original's
    // as its one parent, and its tree is the fork's file alone.
    let fellwar = r#"{ name = "Fellwar Stone", in = ["Ramp"] }"#;
    let text = deck(&[sol, stone2, fellwar]);
    let blob = repo.write_blob(text.as_bytes()).unwrap();
    let tree = repo
        .write_files(&[(RepoPath::new("budget.deck.toml").unwrap(), blob)])
        .unwrap();
    let forked = repo
        .write_commit_of(
            tree,
            &[theirs],
            &sig("Bo", T0 + 2 * DAY),
            &sig("Bo", T0 + 2 * DAY),
            "budget: forked from lantern\n",
        )
        .unwrap();
    repo.create_branch(&branch("budget", "main"), &forked)
        .unwrap();
    let mut t = text.clone();
    t = t.replace(
        r#"{ name = "Sol Ring", in = ["Ramp"] }"#,
        r#"{ name = "Sol Ring", in = ["Draw"] }"#,
    );
    let later = save(
        &mut repo,
        "budget",
        "main",
        &t,
        "budget: Sol Ring: ramp → draw",
        "Bo",
        T0 + 3 * DAY,
    );

    let cards = blamed(&repo, "budget", later, None);
    assert_eq!(commits_of(&cards[0]), (hex(made), hex(later)), "Sol Ring");
    assert_eq!(
        commits_of(&cards[1]),
        (hex(made), hex(theirs)),
        "Mind Stone, the original's"
    );
    assert_eq!(
        commits_of(&cards[2]),
        (hex(forked), hex(forked)),
        "Fellwar Stone"
    );
    let Blame::Changed { changed, .. } = &cards[1].blame else {
        panic!()
    };
    // Named with the deck it was in, which the page opens it as.
    assert_eq!(
        (changed.author.as_str(), changed.deck.as_str()),
        ("Alex", "lantern")
    );
    let Blame::Changed { changed, .. } = &cards[0].blame else {
        panic!()
    };
    assert_eq!(changed.deck, "budget");
}

/// A deck with a hundred and fifty commits: saves on main, a branch merged
/// back every so often, and a hundred cards. What a blame remembers answers
/// the next one as digging all of it again does. `examples/blame_bench.rs`
/// times four hundred.
#[test]
fn a_remembered_blame_answers_as_digging_again_does() {
    let mut repo = Repo::init(meldweb_git::MemStore::default()).unwrap();
    let commits = fixture(&mut repo, 150);
    let stem = DeckStem::new("lantern").unwrap();
    let tip = |repo: &Repo<_>| {
        repo.branch_tip(&branch("lantern", "main"))
            .unwrap()
            .unwrap()
    };
    let cards = |b: DeckBlame| match b {
        DeckBlame::Blamed { cards, .. } => cards,
        DeckBlame::NoDeck => panic!("no deck"),
    };
    let mut memo = Memo::default();
    let start = Instant::now();
    let first = cards(blame(&repo, &mut memo, &stem, tip(&repo), None).unwrap());
    let cold = start.elapsed();
    assert_eq!(first.len(), 100);

    // One more save of a card, and one to the deck's name: blame again.
    let text = String::from_utf8(
        repo.read_file(&tip(&repo), &RepoPath::new("lantern.deck.toml").unwrap())
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    let more = text.replacen("\"Card 7\", qty = ", "\"Card 7\", qty = 1", 1);
    let saved = save(
        &mut repo,
        "lantern",
        "main",
        &more,
        "lantern: Card 7",
        "Bo",
        T0 + 400 * DAY,
    );
    let start = Instant::now();
    let warm = cards(blame(&repo, &mut memo, &stem, saved, None).unwrap());
    let took = start.elapsed();
    let again = cards(blame(&repo, &mut Memo::default(), &stem, saved, None).unwrap());
    assert_eq!(warm, again);
    assert_eq!(commits_of(&warm[7]).1, hex(saved));
    assert_eq!(
        warm.iter()
            .filter(|c| commits_of(c).1 == hex(saved))
            .count(),
        1
    );
    println!(
        "blamed {} cards over {commits} commits in {:.1} ms, and after one more save in {:.2} ms",
        first.len(),
        cold.as_secs_f64() * 1000.0,
        took.as_secs_f64() * 1000.0
    );
}

/// After a criss-cross (main and the branch each merged the other's same
/// two saves, settling the clash between them each its own way) the next
/// merge has two merge bases, and which it starts from decides what it
/// makes: from main's save it takes the branch's quantity, from the
/// branch's it keeps main's. The merge and blame start from the same one,
/// the newest, so blame says what the merge did rather than blaming the
/// merge for dropping a side it never saw.
#[test]
fn after_a_criss_cross_the_merge_and_blame_start_from_the_same_merge_base() {
    let sol = |q: u32| format!(r#"{{ name = "Sol Ring", qty = {q}, in = ["Ramp"] }}"#);
    let island = r#"{ name = "Island", qty = 10, in = ["Land"] }"#;
    let with = |q: u32| deck(&[sol(q).as_str(), island]);
    // The old code took the merge base with the lowest id, so the fixture
    // is made until main's save, the older base, has the lower id: there it
    // and the newest base disagree.
    for salt in 0.. {
        let dir = tempfile::tempdir().unwrap();
        let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
        let made = save(
            &mut repo,
            "lantern",
            "main",
            &with(1),
            "lantern: made",
            "Alex",
            T0,
        );
        branches::create(&mut repo, "lantern", "budget", &hex(made)).unwrap();
        let on_main = save(
            &mut repo,
            "lantern",
            "main",
            &with(2),
            &format!("lantern: Sol Ring: 1 → 2 ({salt})"),
            "Alex",
            T0 + DAY,
        );
        let on_branch = save(
            &mut repo,
            "lantern",
            "budget",
            &with(3),
            "lantern: Sol Ring: 1 → 3",
            "Bo",
            T0 + 2 * DAY,
        );
        if on_main > on_branch {
            continue;
        }
        // main merges the branch keeping its 2; the branch merges main's
        // same save keeping its 3.
        branches::create(&mut repo, "lantern", "was-main", &hex(on_main)).unwrap();
        let main_merged =
            plan_and_merge(&mut repo, "budget", "main", Some(Side::Ours), T0 + 3 * DAY);
        let branch_merged = plan_and_merge(
            &mut repo,
            "was-main",
            "budget",
            Some(Side::Ours),
            T0 + 4 * DAY,
        );
        if have_git() {
            let mut bases: Vec<String> = git(
                dir.path(),
                &[
                    "merge-base",
                    "--all",
                    &hex(main_merged),
                    &hex(branch_merged),
                ],
            )
            .lines()
            .map(str::to_owned)
            .collect();
            bases.sort();
            let mut want = vec![hex(on_main), hex(on_branch)];
            want.sort();
            assert_eq!(bases, want, "git sees two merge bases");
        }

        let plan = branches::plan(&mut repo, "lantern", "budget", "main", None).unwrap();
        let MergePlan::Merge { after, .. } = &plan else {
            panic!("not a clean merge: {plan:?}");
        };
        // From the branch's save, the newest base, main changed Sol Ring to 2
        // and the branch did not.
        assert_eq!(
            after.as_deref(),
            Some(with(2).as_str()),
            "the merge keeps main's 2"
        );
        let merged = plan_and_merge(&mut repo, "budget", "main", None, T0 + 5 * DAY);
        let cards = blamed(&repo, "lantern", merged, None);
        assert_eq!(
            commits_of(&cards[0]),
            (hex(made), hex(main_merged)),
            "Sol Ring is main's merge's, which kept the 2, not this merge's"
        );
        return;
    }
}
