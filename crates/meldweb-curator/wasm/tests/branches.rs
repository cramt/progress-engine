//! A deck's branches (ADR-0041) against the real `git`: made at a tip or a
//! snapshot, counted against `main` as `git rev-list --count` counts, merged
//! into `main` as a fast-forward or a two-parent merge whose message is the
//! deck's changelog, stopped by a clash or a merge that is no deck until a
//! side is picked, updated from `main`, and deleted. `git fsck --strict`
//! passes on what is left.
//!
//! Without `git` on PATH the git checks print `SKIPPED` and the rest runs,
//! unless `MELDWEB_GIT_REQUIRE` is set.

use std::path::Path;
use std::process::Command;

use meldweb_git::{
    BranchName, Change, CommitId, FsStore, Identity, Repo, RepoPath, Side, Signature, TagName,
    Timestamp, Tip, WriteCommit,
};
use meldweb_wasm::branches::{self, BranchDeleted, BranchMade, MergeDone, MergePlan};

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
        .args(["-c", "gc.auto=0"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim_end().to_owned()
}

fn sig(seconds: i64) -> Signature {
    Signature {
        who: Identity::new("Alex", "alex@example.com").unwrap(),
        when: Timestamp::new(seconds, 120).unwrap(),
    }
}

fn line(name: &str) -> BranchName {
    BranchName::new(format!("decks/lantern/{name}")).unwrap()
}

const FILE: &str = "lantern.deck.toml";

fn deck(cards: &[&str]) -> String {
    let mut s = String::from("name = \"Lantern\"\nformat = \"commander\"\ncards = [\n");
    for c in cards {
        s.push_str(&format!("  {{ name = \"{c}\" }},\n"));
    }
    s.push_str("]\n");
    s
}

const CARDS: [&str; 8] = [
    "Sol Ring",
    "Mind Stone",
    "Arcane Signet",
    "Command Tower",
    "Brainstorm",
    "Ponder",
    "Counterspell",
    "Island",
];

/// One save of `text` on the deck's branch `name`, as the editor makes one.
fn save(repo: &mut Repo<FsStore>, name: &str, text: &str, message: &str, at: i64) -> CommitId {
    let branch = line(name);
    let tip = repo.branch_tip(&branch).unwrap();
    repo.commit(
        &branch,
        tip.map_or(Tip::Unborn, Tip::At),
        &WriteCommit {
            changes: vec![Change::Put {
                path: RepoPath::new(FILE).unwrap(),
                bytes: text.as_bytes().to_vec(),
            }],
            message: format!("{message}\n"),
            author: sig(at),
            committer: sig(at),
        },
    )
    .unwrap()
}

fn text_on(repo: &Repo<FsStore>, name: &str) -> String {
    let tip = repo.branch_tip(&line(name)).unwrap().unwrap();
    String::from_utf8(
        repo.read_file(&tip, &RepoPath::new(FILE).unwrap())
            .unwrap()
            .unwrap(),
    )
    .unwrap()
}

fn tips(plan: &MergePlan) -> (String, String) {
    match plan {
        MergePlan::FastForward { into, from, .. }
        | MergePlan::Merge { into, from, .. }
        | MergePlan::Conflict { into, from, .. } => (into.clone(), from.clone()),
        other => panic!("no tips in {other:?}"),
    }
}

fn merge(
    repo: &mut Repo<FsStore>,
    from: &str,
    into: &str,
    prefer: Option<Side>,
    at: i64,
) -> MergeDone {
    let plan = branches::plan(repo, "lantern", from, into, prefer).unwrap();
    let (i, f) = tips(&plan);
    branches::merge(repo, "lantern", from, into, &i, &f, prefer, &sig(at)).unwrap()
}

fn facts(repo: &Repo<FsStore>) -> Vec<(String, u32, u32)> {
    branches::branches(repo, "lantern")
        .unwrap()
        .branches
        .into_iter()
        .map(|b| (b.name, b.unmerged, b.behind))
        .collect()
}

#[test]
fn a_branch_merges_into_main_and_git_reads_the_two_parent_merge() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
    let t0 = 1_760_000_000;
    let first = save(&mut repo, "main", &deck(&CARDS), "lantern: made", t0);
    repo.create_tag(
        &TagName::new("decks/lantern/fnm-14-sep").unwrap(),
        &first,
        &sig(t0 + 1),
        "FNM 14 Sep\n",
    )
    .unwrap();

    // A branch is a ref at a commit of the deck's line, and no commit.
    let made = branches::create(&mut repo, "lantern", "budget", &first.to_hex()).unwrap();
    assert!(
        matches!(made, BranchMade::Made { ref branch } if branch.unmerged == 0 && branch.behind == 0)
    );
    assert!(matches!(
        branches::create(&mut repo, "lantern", "budget", &first.to_hex()).unwrap(),
        BranchMade::Conflict { .. }
    ));
    assert!(branches::create(&mut repo, "lantern", "a/b", &first.to_hex()).is_err());

    // Two cards changed on the branch: Mind Stone out, Fellwar Stone in.
    let mut budget = CARDS.to_vec();
    budget[1] = "Fellwar Stone";
    budget.insert(6, "Opt");
    save(
        &mut repo,
        "budget",
        &deck(&budget),
        "lantern: try rocks",
        t0 + 10,
    );
    assert_eq!(
        facts(&repo),
        vec![("main".into(), 0, 0), ("budget".into(), 1, 0)]
    );

    // Main has not moved, so merging is a fast-forward; not taken yet.
    let plan = branches::plan(&mut repo, "lantern", "budget", "main", None).unwrap();
    assert!(matches!(plan, MergePlan::FastForward { .. }), "{plan:?}");

    // Main moves on another card, so the merge is a real one.
    let mut on_main = CARDS.to_vec();
    on_main[7] = "Snow-Covered Island";
    save(&mut repo, "main", &deck(&on_main), "lantern: snow", t0 + 20);
    assert_eq!(
        facts(&repo),
        vec![("main".into(), 0, 0), ("budget".into(), 1, 1)]
    );
    let plan = branches::plan(&mut repo, "lantern", "budget", "main", None).unwrap();
    let MergePlan::Merge {
        ref message,
        ref after,
        ..
    } = plan
    else {
        panic!("{plan:?}")
    };
    println!("merge message: {message}");
    assert_eq!(
        message,
        "lantern: merge budget: +1 Fellwar Stone, +1 Opt, -1 Mind Stone"
    );
    let mut both = CARDS.to_vec();
    both[1] = "Fellwar Stone";
    both[7] = "Snow-Covered Island";
    both.insert(6, "Opt");
    assert_eq!(after.as_deref(), Some(deck(&both).as_str()));

    let done = merge(&mut repo, "budget", "main", None, t0 + 30);
    let MergeDone::Merged {
        commit,
        message: Some(_),
    } = done
    else {
        panic!("{done:?}")
    };
    assert_eq!(text_on(&repo, "main"), deck(&both));
    let merged = CommitId::from_hex(&commit).unwrap();
    assert_eq!(repo.read_commit(&merged).unwrap().parents.len(), 2);
    assert_eq!(
        facts(&repo),
        vec![("main".into(), 0, 0), ("budget".into(), 0, 2)]
    );

    // Merging again is nothing; updating the branch from main fast-forwards.
    assert!(matches!(
        branches::plan(&mut repo, "lantern", "budget", "main", None).unwrap(),
        MergePlan::UpToDate
    ));
    assert!(matches!(
        merge(&mut repo, "main", "budget", None, t0 + 40),
        MergeDone::Merged { message: None, .. }
    ));
    assert_eq!(text_on(&repo, "budget"), deck(&both));

    // A plan read before a branch moved is not merged.
    let plan_before = branches::plan(&mut repo, "lantern", "main", "budget", None).unwrap();
    assert!(matches!(plan_before, MergePlan::UpToDate));

    // A branch from the snapshot is behind main and has nothing to merge.
    assert!(matches!(
        branches::create(&mut repo, "lantern", "fnm", &first.to_hex()).unwrap(),
        BranchMade::Made { ref branch } if branch.unmerged == 0 && branch.behind == 3
    ));

    if have_git() {
        let d = dir.path();
        println!("{}", git(d, &["log", "--all", "--graph", "--oneline"]));
        let out = git(d, &["fsck", "--strict", "--full", "--no-dangling"]);
        assert!(out.is_empty(), "fsck: {out}");
        assert_eq!(
            git(
                d,
                &["rev-list", "--parents", "-n", "1", "decks/lantern/main"]
            ),
            format!(
                "{commit} {} {}",
                repo.read_commit(&merged).unwrap().parents[0],
                repo.read_commit(&merged).unwrap().parents[1]
            )
        );
        assert_eq!(
            git(
                d,
                &[
                    "rev-list",
                    "--count",
                    "decks/lantern/main..decks/lantern/fnm"
                ]
            ),
            "0"
        );
        assert_eq!(
            git(
                d,
                &[
                    "rev-list",
                    "--count",
                    "decks/lantern/fnm..decks/lantern/main"
                ]
            ),
            "3"
        );
        assert_eq!(
            git(d, &["log", "-1", "--format=%s", "decks/lantern/main"]),
            "lantern: merge budget: +1 Fellwar Stone, +1 Opt, -1 Mind Stone"
        );
    }

    // Deleting needs the tip it was read at, and main is the deck.
    let fnm = repo.branch_tip(&line("fnm")).unwrap().unwrap();
    assert!(matches!(
        branches::delete(&mut repo, "lantern", "fnm", &merged.to_hex()).unwrap(),
        BranchDeleted::Conflict { .. }
    ));
    assert!(matches!(
        branches::delete(&mut repo, "lantern", "fnm", &fnm.to_hex()).unwrap(),
        BranchDeleted::Deleted
    ));
    assert!(branches::delete(&mut repo, "lantern", "main", &merged.to_hex()).is_err());
    assert_eq!(
        facts(&repo),
        vec![("main".into(), 0, 0), ("budget".into(), 0, 0)]
    );
    if have_git() {
        let out = git(dir.path(), &["fsck", "--strict", "--full", "--no-dangling"]);
        assert!(out.is_empty(), "fsck: {out}");
    }
}

#[test]
fn a_clash_stops_the_merge_until_a_side_is_picked_for_the_stretch() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
    let t0 = 1_760_000_000;
    let first = save(&mut repo, "main", &deck(&CARDS), "lantern: made", t0);
    branches::create(&mut repo, "lantern", "clash", &first.to_hex()).unwrap();

    // Both change Ponder; each changes one more card of its own.
    let mut theirs = CARDS.to_vec();
    theirs[5] = "Preordain";
    theirs[0] = "Mana Crypt";
    save(
        &mut repo,
        "clash",
        &deck(&theirs),
        "lantern: preordain",
        t0 + 10,
    );
    let mut ours = CARDS.to_vec();
    ours[5] = "Serum Visions";
    ours[7] = "Snow-Covered Island";
    save(&mut repo, "main", &deck(&ours), "lantern: visions", t0 + 20);

    let plan = branches::plan(&mut repo, "lantern", "clash", "main", None).unwrap();
    let MergePlan::Conflict { ref hunks, .. } = plan else {
        panic!("{plan:?}")
    };
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].path, FILE);
    assert_eq!(hunks[0].base, vec!["  { name = \"Ponder\" },"]);
    assert_eq!(hunks[0].ours, vec!["  { name = \"Serum Visions\" },"]);
    assert_eq!(hunks[0].theirs, vec!["  { name = \"Preordain\" },"]);
    let main_before = repo.branch_tip(&line("main")).unwrap();
    assert!(matches!(
        merge(&mut repo, "clash", "main", None, t0 + 30),
        MergeDone::Stopped { .. }
    ));
    assert_eq!(repo.branch_tip(&line("main")).unwrap(), main_before);

    // Taking the branch's side settles only the clash: main keeps its snow.
    let plan = branches::plan(&mut repo, "lantern", "clash", "main", Some(Side::Theirs)).unwrap();
    let mut settled = CARDS.to_vec();
    settled[0] = "Mana Crypt";
    settled[5] = "Preordain";
    settled[7] = "Snow-Covered Island";
    assert!(
        matches!(plan, MergePlan::Merge { ref after, .. } if after.as_deref() == Some(deck(&settled).as_str())),
        "{plan:?}"
    );
    assert!(matches!(
        merge(&mut repo, "clash", "main", Some(Side::Theirs), t0 + 30),
        MergeDone::Merged {
            message: Some(_),
            ..
        }
    ));
    assert_eq!(text_on(&repo, "main"), deck(&settled));
    if have_git() {
        let out = git(dir.path(), &["fsck", "--strict", "--full", "--no-dangling"]);
        assert!(out.is_empty(), "fsck: {out}");
    }
}

#[test]
fn lines_that_merge_into_no_deck_stop_with_why() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
    let t0 = 1_760_000_000;
    let base = deck(&CARDS).replace(
        "  { name = \"Sol Ring\" },",
        "  { name = \"Sol Ring\", in = [\"Ramp\"] },",
    ) + "\n[categories]\nRamp = {}\n";
    let first = save(&mut repo, "main", &base, "lantern: made", t0);
    branches::create(&mut repo, "lantern", "notes", &first.to_hex()).unwrap();
    // Main drops the Ramp category; the branch files a new card under it.
    // Each is a deck; both together name a category nothing declares.
    let ours = base
        .replace(
            "  { name = \"Sol Ring\", in = [\"Ramp\"] },",
            "  { name = \"Sol Ring\" },",
        )
        .replace("Ramp = {}\n", "");
    let theirs = base.replace(
        "  { name = \"Counterspell\" },",
        "  { name = \"Counterspell\" },\n  { name = \"Mind Stone\", in = [\"Ramp\"] },",
    );
    save(&mut repo, "notes", &theirs, "lantern: b", t0 + 10);
    save(&mut repo, "main", &ours, "lantern: a", t0 + 20);
    let plan = branches::plan(&mut repo, "lantern", "notes", "main", None).unwrap();
    let MergePlan::Conflict {
        ref hunks,
        ref unreadable,
        ..
    } = plan
    else {
        panic!("{plan:?}")
    };
    assert!(hunks.is_empty());
    assert_eq!(unreadable.len(), 1);
    println!("unreadable: {}", unreadable[0].reason);
    // A side picked still merges into no deck, so the whole file takes it.
    let plan = branches::plan(&mut repo, "lantern", "notes", "main", Some(Side::Ours)).unwrap();
    assert!(
        matches!(plan, MergePlan::Merge { ref after, ref message, .. } if after.as_deref() == Some(ours.as_str()) && message == "lantern: merge notes"),
        "{plan:?}"
    );
}
