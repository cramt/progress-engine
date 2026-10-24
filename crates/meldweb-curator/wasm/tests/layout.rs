//! Repo version 2 (ADR-0041) against the real `git`: a version-1 repo built
//! the way Curator wrote one (decks edited over months, a variant, snapshots,
//! a deleted deck, collection deck places and a deck order) migrates into a
//! line per deck; and a browser syncs every branch with a remote on either
//! version, through `git upload-pack` and `git receive-pack
//! --stateless-rpc` as a smart HTTP host answers.
//!
//! Without `git` on PATH these print `SKIPPED` and pass, unless
//! `MELDWEB_GIT_REQUIRE` is set.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chip_decklist::stem::DeckStem;
use meldweb_git::{
    BranchName, Change, CommitId, FetchRequest, FsStore, Identity, Integrated, PushRequest,
    RemoteUrl, Repo, RepoPath, Signature, TagName, Timestamp, Tip, WriteCommit,
};
use meldweb_wasm::layout::{self, DeckLines};
use meldweb_wasm::merging::{
    self, FileConflict, FileSettle, FileSettling, Settle, Settling, Side, Stops, SyncSettle,
};
use meldweb_wasm::repo::SyncConflict;
use meldweb_wasm::repo::Synced;

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

fn git_cmd(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(dir)
        .args(["-c", "gc.auto=0", "-c", "commit.gpgsign=false"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_PROTOCOL");
    c
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_cmd(dir).args(args).output().expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim_end().to_owned()
}

fn git_stdin(dir: &Path, args: &[&str], input: &[u8]) -> Vec<u8> {
    let mut child = git_cmd(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git runs");
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn fsck(dir: &Path) {
    let out = git(dir, &["fsck", "--strict", "--full", "--no-dangling"]);
    assert!(out.is_empty(), "fsck: {out}");
}

/// A bare repo answering as a smart HTTP host would.
struct Server(PathBuf);

impl Server {
    fn new(dir: &Path) -> Self {
        let bare = dir.join("server.git");
        git(
            dir,
            &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
        );
        Server(bare)
    }

    fn advertise(&self, service: &str) -> Vec<u8> {
        let out = git_cmd(&self.0)
            .args([service, "--stateless-rpc", "--advertise-refs", "."])
            .output()
            .unwrap();
        assert!(out.status.success());
        let line = format!("# service=git-{service}\n");
        let mut body = format!("{:04x}{line}0000", line.len() + 4).into_bytes();
        body.extend(out.stdout);
        body
    }

    fn post(&self, service: &str, body: &[u8]) -> Vec<u8> {
        git_stdin(&self.0, &[service, "--stateless-rpc", "."], body)
    }

    fn refs(&self) -> String {
        git(
            &self.0,
            &["for-each-ref", "--format=%(objectname) %(refname)"],
        )
    }
}

fn sig(name: &str, seconds: i64, offset: i32) -> Signature {
    Signature {
        who: Identity::new(name, format!("{}@example.com", name.to_lowercase())).unwrap(),
        when: Timestamp::new(seconds, offset).unwrap(),
    }
}

fn p(path: &str) -> RepoPath {
    RepoPath::new(path).unwrap()
}

fn main_branch() -> BranchName {
    BranchName::new("main").unwrap()
}

fn branch(name: &str) -> BranchName {
    BranchName::new(name).unwrap()
}

/// One commit on `main` by `who` at `at`, as a v1 Curator save made it.
fn save(
    repo: &mut Repo<FsStore>,
    puts: &[(&str, &str)],
    deletes: &[&str],
    message: &str,
    who: &Signature,
) -> CommitId {
    let tip = repo.branch_tip(&main_branch()).unwrap();
    let mut changes: Vec<Change> = puts
        .iter()
        .map(|(path, text)| Change::Put {
            path: p(path),
            bytes: text.as_bytes().to_vec(),
        })
        .collect();
    changes.extend(deletes.iter().map(|path| Change::Delete { path: p(path) }));
    repo.commit(
        &main_branch(),
        tip.map_or(Tip::Unborn, Tip::At),
        &WriteCommit {
            changes,
            message: format!("{message}\n"),
            author: who.clone(),
            committer: who.clone(),
        },
    )
    .unwrap()
}

fn deck(name: &str, cards: &[&str]) -> String {
    let mut s = format!("name = \"{name}\"\nformat = \"commander\"\ncards = [\n");
    for c in cards {
        s.push_str(&format!("  {{ name = \"{c}\" }},\n"));
    }
    s.push_str("]\n");
    s
}

fn variant(of: &str, name: &str, cards: &[&str]) -> String {
    deck(name, cards).replacen(
        "format = \"commander\"\n",
        &format!("format = \"commander\"\nvariant_of = \"decks/{of}.deck.toml\"\n"),
        1,
    )
}

const COLLECTION: &str = "cards = [\n  { name = \"Sol Ring\", at = \"Lantern\" },\n  { name = \"Mind Stone\", at = \"Bulk\" },\n]\n\n[places]\nBulk = {}\nLantern = { deck = \"decks/lantern.deck.toml\" }  # sleeved\n";

const SETTINGS: &str = "[decks]\norder = [\n  \"decks/loam.deck.toml\",\n  \"decks/lantern.deck.toml\",\n  \"decks/lantern-budget.deck.toml\",\n]\n";

/// A version-1 repo as months of Curator saves leave one. Returns the
/// commits by what they did.
fn v1_repo(dir: &Path) -> (Repo<FsStore>, BTreeMap<&'static str, CommitId>) {
    let mut repo = Repo::init(FsStore::new(dir)).unwrap();
    let alex = |t| sig("Alex", t, 120);
    let sam = |t| sig("Sam", t, -300);
    let mut at = BTreeMap::new();
    at.insert(
        "version",
        save(
            &mut repo,
            &[("VERSION", "1\n")],
            &[],
            "curator: this repo is at version 1",
            &alex(1_700_000_000),
        ),
    );
    at.insert(
        "lantern made",
        save(
            &mut repo,
            &[("decks/lantern.deck.toml", &deck("Lantern", &["Sol Ring"]))],
            &[],
            "lantern: +1 Sol Ring",
            &alex(1_700_100_000),
        ),
    );
    save(
        &mut repo,
        &[("decks/old.deck.toml", &deck("Old", &["Island"]))],
        &[],
        "old: +1 Island",
        &alex(1_700_150_000),
    );
    at.insert(
        "loam made",
        save(
            &mut repo,
            &[
                (
                    "decks/loam.deck.toml",
                    &deck("Loam", &["Life from the Loam"]),
                ),
                ("collection.toml", COLLECTION),
            ],
            &[],
            "loam: +1 Life from the Loam",
            &sam(1_700_200_000),
        ),
    );
    at.insert(
        "lantern 2",
        save(
            &mut repo,
            &[(
                "decks/lantern.deck.toml",
                &deck("Lantern", &["Sol Ring", "Mind Stone"]),
            )],
            &[],
            "lantern: +1 Mind Stone",
            &alex(1_700_300_000),
        ),
    );
    save(
        &mut repo,
        &[],
        &["decks/old.deck.toml"],
        "old: delete",
        &alex(1_700_350_000),
    );
    at.insert(
        "budget made",
        save(
            &mut repo,
            &[(
                "decks/lantern-budget.deck.toml",
                &variant("lantern", "Lantern (budget)", &["Sol Ring", "Mind Stone"]),
            )],
            &[],
            "lantern-budget: +1 Sol Ring, +1 Mind Stone",
            &alex(1_700_400_000),
        ),
    );
    at.insert(
        "both",
        save(
            &mut repo,
            &[
                (
                    "decks/lantern.deck.toml",
                    &deck("Lantern", &["Sol Ring", "Mind Stone", "Arcane Signet"]),
                ),
                (
                    "decks/loam.deck.toml",
                    &deck("Loam", &["Life from the Loam", "Sol Ring"]),
                ),
            ],
            &[],
            "sync: merge origin/main",
            &sam(1_700_500_000),
        ),
    );
    at.insert(
        "settings",
        save(
            &mut repo,
            &[("meldweb.toml", SETTINGS)],
            &[],
            "meldweb.toml: move loam to 1 of 3",
            &alex(1_700_600_000),
        ),
    );
    at.insert(
        "budget 2",
        save(
            &mut repo,
            &[(
                "decks/lantern-budget.deck.toml",
                &variant(
                    "lantern",
                    "Lantern (budget)",
                    &["Sol Ring", "Mind Stone", "Wayfarer's Bauble"],
                ),
            )],
            &[],
            "lantern-budget: +1 Wayfarer's Bauble",
            &alex(1_700_700_000),
        ),
    );
    at.insert(
        "criteria",
        save(
            &mut repo,
            &[(
                "decks/loam.criteria.toml",
                "[[question]]\nname = \"t3 loam\"\n",
            )],
            &[],
            "loam: criteria",
            &sam(1_700_800_000),
        ),
    );
    let tag = |repo: &mut Repo<FsStore>, name: &str, at: CommitId, label: &str, t: i64| {
        repo.create_tag(
            &TagName::new(name).unwrap(),
            &at,
            &sig("Alex", t, 120),
            &format!("{label}\n"),
        )
        .unwrap();
    };
    tag(
        &mut repo,
        "decks/lantern/fnm",
        at["lantern 2"],
        "FNM 14 Sep",
        1_700_310_000,
    );
    // On a commit that changed another deck: it stands for loam as it was.
    tag(
        &mut repo,
        "decks/loam/gp",
        at["settings"],
        "GP Lille",
        1_700_610_000,
    );
    tag(
        &mut repo,
        "decks/lantern-budget/first",
        at["budget made"],
        "First",
        1_700_410_000,
    );
    (repo, at)
}

fn messages(dir: &Path, rev: &str, path: &str) -> Vec<String> {
    git(dir, &["log", "--format=%s", rev, "--", path])
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_v1_repo_migrates_into_a_line_per_deck_the_same_way_every_time() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let a_dir = dir.path().join("a.git");
    let (mut a, at) = v1_repo(&a_dir);
    let v1_tip = a.branch_tip(&main_branch()).unwrap().unwrap();
    let before_refs = git(
        &a_dir,
        &["for-each-ref", "--format=%(objectname) %(refname)"],
    );

    // Rewriting moves no ref, and rewriting again makes the same objects.
    let once = layout::rewrite_v1(&mut a, &v1_tip).unwrap();
    let twice = layout::rewrite_v1(&mut a, &v1_tip).unwrap();
    assert_eq!(once, twice);
    assert_eq!(
        git(
            &a_dir,
            &["for-each-ref", "--format=%(objectname) %(refname)"]
        ),
        before_refs,
        "rewriting is not migrating"
    );

    let migrated = layout::migrate(&mut a).unwrap();
    assert_eq!((migrated.decks, migrated.snapshots), (3, 3));
    let refs = git(
        &a_dir,
        &["for-each-ref", "--format=%(refname) %(objecttype)"],
    );
    eprintln!("refs after migrating:\n{refs}");
    assert_eq!(
        refs,
        "refs/heads/decks/lantern-budget/main commit\nrefs/heads/decks/lantern/main commit\n\
         refs/heads/decks/loam/main commit\nrefs/heads/main commit\n\
         refs/tags/decks/lantern-budget/first tag\nrefs/tags/decks/lantern/fnm tag\n\
         refs/tags/decks/loam/gp tag"
    );
    fsck(&a_dir);

    // main: the old history, then one commit by Progress Engine at the old
    // tip's committer time, that takes decks/ away.
    assert_eq!(git(&a_dir, &["rev-parse", "main^"]), v1_tip.to_hex());
    assert_eq!(
        git(&a_dir, &["log", "-1", "--format=%an <%ae> %at %ai|%cn %ct|%P", "main"]),
        format!("Progress Engine <curator@localhost> 1700800000 2023-11-23 23:26:40 -0500|Progress Engine 1700800000|{}", v1_tip.to_hex())
    );
    assert_eq!(
        git(&a_dir, &["ls-tree", "--name-only", "main"]),
        "VERSION\ncollection.toml\nmeldweb.toml"
    );
    assert_eq!(git(&a_dir, &["show", "main:VERSION"]), "2");
    assert_eq!(
        git(&a_dir, &["show", "main:collection.toml"]),
        COLLECTION
            .replace("deck = \"decks/lantern.deck.toml\"", "deck = \"lantern\"")
            .trim_end()
    );
    // Edited in place: every byte but the paths as it was.
    assert_eq!(
        git(&a_dir, &["show", "main:meldweb.toml"]),
        SETTINGS
            .replace("decks/loam.deck.toml", "loam")
            .replace("decks/lantern.deck.toml", "lantern")
            .replace("decks/lantern-budget.deck.toml", "lantern-budget")
            .trim_end()
    );
    assert_eq!(
        git(&a_dir, &["rev-list", "--count", "main"]),
        "12",
        "eleven v1 commits and the migration: nothing on main was rewritten"
    );

    // Each deck's own line: its commits, messages, authors and dates as
    // they were, its file at its root.
    assert_eq!(
        messages(&a_dir, "decks/lantern/main", "lantern.deck.toml"),
        [
            "sync: merge origin/main",
            "lantern: +1 Mind Stone",
            "lantern: +1 Sol Ring"
        ]
    );
    assert_eq!(
        git(&a_dir, &["log", "--format=%an %at %ai", "decks/lantern/main"]),
        "Sam 1700500000 2023-11-20 12:06:40 -0500\nAlex 1700300000 2023-11-18 11:33:20 +0200\nAlex 1700100000 2023-11-16 04:00:00 +0200"
    );
    assert_eq!(
        git(&a_dir, &["ls-tree", "--name-only", "decks/lantern/main"]),
        "lantern.deck.toml"
    );
    assert_eq!(
        git(&a_dir, &["show", "decks/lantern/main:lantern.deck.toml"]),
        deck("Lantern", &["Sol Ring", "Mind Stone", "Arcane Signet"]).trim_end()
    );
    assert_eq!(
        git(&a_dir, &["ls-tree", "--name-only", "decks/loam/main"]),
        "loam.criteria.toml\nloam.deck.toml"
    );
    assert_eq!(
        messages(&a_dir, "decks/loam/main", "."),
        [
            "loam: criteria",
            "sync: merge origin/main",
            "loam: +1 Life from the Loam"
        ]
    );

    // The variant is a fork: its first commit's parent is lantern's commit
    // as it stood when the variant was made, and variant_of is gone.
    let fork_root = git(
        &a_dir,
        &[
            "rev-list",
            "--max-parents=1",
            "--min-parents=1",
            "decks/lantern-budget/main",
            "--not",
            "decks/lantern/main",
        ],
    );
    eprintln!("lantern-budget's own commits:\n{fork_root}");
    assert_eq!(
        git(&a_dir, &["log", "--format=%s", "decks/lantern-budget/main"]),
        "lantern-budget: +1 Wayfarer's Bauble\nlantern-budget: +1 Sol Ring, +1 Mind Stone\nlantern: +1 Mind Stone\nlantern: +1 Sol Ring"
    );
    assert_eq!(
        git(
            &a_dir,
            &[
                "merge-base",
                "decks/lantern-budget/main",
                "decks/lantern/main"
            ]
        ),
        git(&a_dir, &["rev-parse", "decks/lantern/main~1"]),
        "forked from lantern after +1 Mind Stone"
    );
    let budget = git(
        &a_dir,
        &["show", "decks/lantern-budget/main:lantern-budget.deck.toml"],
    );
    assert!(!budget.contains("variant_of"), "{budget}");
    assert_eq!(
        git(
            &a_dir,
            &["ls-tree", "--name-only", "decks/lantern-budget/main"]
        ),
        "lantern-budget.deck.toml"
    );
    let lines = DeckLines::read(&a).unwrap();
    let parents = lines.parents(&a).unwrap();
    assert_eq!(
        parents
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect::<Vec<_>>(),
        [("lantern-budget", "lantern")]
    );

    // Each snapshot is on its deck's line, at the commit standing for the
    // one it named; its label and date are as they were.
    let target = |tag: &str| git(&a_dir, &["rev-parse", &format!("{tag}^{{commit}}")]);
    assert_eq!(
        target("decks/lantern/fnm"),
        git(&a_dir, &["rev-parse", "decks/lantern/main~1"])
    );
    assert_eq!(
        target("decks/loam/gp"),
        git(&a_dir, &["rev-parse", "decks/loam/main~1"])
    );
    assert_eq!(
        target("decks/lantern-budget/first"),
        git(&a_dir, &["rev-parse", "decks/lantern-budget/main~1"])
    );
    assert_eq!(
        git(
            &a_dir,
            &[
                "for-each-ref",
                "--format=%(contents:subject) %(taggerdate:unix)",
                "refs/tags/decks/lantern/fnm"
            ]
        ),
        "FNM 14 Sep 1700310000"
    );
    // The old history stays reachable from main, so pushing main is a
    // fast-forward and the old ids still resolve.
    assert_eq!(
        git(
            &a_dir,
            &[
                "merge-base",
                "--is-ancestor",
                &at["budget made"].to_hex(),
                "main"
            ]
        ),
        ""
    );

    // Another browser, with the same v1 repo built apart, migrates it to
    // the very same ids.
    let b_dir = dir.path().join("b.git");
    let (mut b, _) = v1_repo(&b_dir);
    layout::migrate(&mut b).unwrap();
    assert_eq!(
        git(
            &b_dir,
            &["for-each-ref", "--format=%(objectname) %(refname)"]
        ),
        git(
            &a_dir,
            &["for-each-ref", "--format=%(objectname) %(refname)"]
        ),
        "two browsers migrating the same repo make byte-identical commits and tags"
    );
    // Migrating again is refused: it is at version 2.
    assert!(layout::migrate(&mut b).unwrap_err().contains("version 1"));
    eprintln!(
        "{}",
        git(&a_dir, &["log", "--all", "--graph", "--format=%h %d %s"])
    );
}

#[test]
fn a_v1_repo_with_what_v2_has_no_place_for_is_refused_and_left_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
    let who = sig("Alex", 1_700_000_000, 0);
    save(
        &mut repo,
        &[
            ("VERSION", "1\n"),
            ("decks/a.deck.toml", "name = \"A\"\n"),
            ("decks/notes.md", "hi"),
        ],
        &[],
        "x",
        &who,
    );
    let tip = repo.branch_tip(&main_branch()).unwrap();
    let err = layout::migrate(&mut repo).unwrap_err();
    assert!(err.contains("decks/notes.md"), "{err}");
    assert_eq!(repo.branch_tip(&main_branch()).unwrap(), tip);
    assert!(repo.branch_tip(&branch("decks/a/main")).unwrap().is_none());
}

/// A deck's named criteria sets, `<stem>.<name>.criteria.toml`, as the
/// cid-goldfish work wrote beside a deck, go onto its line with their
/// history; a stem with a dot in it keeps its own criteria.
#[test]
fn a_v1_decks_named_criteria_sets_migrate_onto_its_line() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
    save(
        &mut repo,
        &[
            ("VERSION", "1\n"),
            ("decks/cids.deck.toml", &deck("Cids", &["Mill"])),
            ("decks/cids.criteria.toml", "# main\n"),
            ("decks/cids.v2.deck.toml", &deck("Cids 2", &["Mill"])),
            ("decks/cids.v2.criteria.toml", "# v2's own\n"),
        ],
        &[],
        "cids: made",
        &sig("Alex", 1_700_000_000, 0),
    );
    save(
        &mut repo,
        &[("decks/cids.mill.criteria.toml", "# mill 1\n")],
        &[],
        "cids: mill",
        &sig("Alex", 1_700_000_100, 0),
    );
    save(
        &mut repo,
        &[
            ("decks/cids.mill.criteria.toml", "# mill 2\n"),
            ("decks/cids.pips.criteria.toml", "# pips\n"),
        ],
        &[],
        "cids: pips",
        &sig("Alex", 1_700_000_200, 0),
    );
    layout::migrate(&mut repo).unwrap();

    let line = repo
        .branch_tip(&branch("decks/cids/main"))
        .unwrap()
        .unwrap();
    let file = |repo: &Repo<FsStore>, at: &CommitId, path: &str| {
        repo.read_file(at, &p(path))
            .unwrap()
            .map(|b| String::from_utf8(b).unwrap())
    };
    assert_eq!(
        file(&repo, &line, "cids.criteria.toml").unwrap(),
        "# main\n"
    );
    assert_eq!(
        file(&repo, &line, "cids.mill.criteria.toml").unwrap(),
        "# mill 2\n"
    );
    assert_eq!(
        file(&repo, &line, "cids.pips.criteria.toml").unwrap(),
        "# pips\n"
    );
    assert_eq!(
        messages(dir.path(), "decks/cids/main", "cids.mill.criteria.toml"),
        ["cids: pips", "cids: mill"]
    );
    let v2 = repo
        .branch_tip(&branch("decks/cids.v2/main"))
        .unwrap()
        .unwrap();
    assert_eq!(
        file(&repo, &v2, "cids.v2.criteria.toml").unwrap(),
        "# v2's own\n"
    );
    assert_eq!(
        repo.list_dir(&v2, None).unwrap().unwrap().len(),
        2,
        "only the deck and its own criteria"
    );
    let main = repo.branch_tip(&main_branch()).unwrap().unwrap();
    assert!(repo.list_dir(&main, Some(&p("decks"))).unwrap().is_none());
    fsck(dir.path());
}

/// Criteria naming no deck, even as a named set, are still refused.
#[test]
fn a_named_criteria_set_without_its_deck_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
    save(
        &mut repo,
        &[
            ("VERSION", "1\n"),
            ("decks/a.deck.toml", "name = \"A\"\n"),
            ("decks/b.mill.criteria.toml", "# lone\n"),
        ],
        &[],
        "x",
        &sig("Alex", 1_700_000_000, 0),
    );
    let err = layout::migrate(&mut repo).unwrap_err();
    assert!(err.contains("decks/b.mill.criteria.toml"), "{err}");
}

// Syncing across versions.

/// What the browser's sync merges a file both changed with: a deck by card,
/// anything else by lines.
fn as_curator_merges(path: &str, base: &[u8], ours: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    merging::merge_bytes(
        path,
        base,
        ours,
        theirs,
        &Settling::default(),
        &mut Stops::default(),
    )
}

fn fetch(repo: &mut Repo<FsStore>, server: &Server) {
    let ad = server.advertise("upload-pack");
    match repo.fetch_request(&ad).unwrap() {
        FetchRequest::HaveAll => repo.fetch_finish(&ad, None).unwrap(),
        FetchRequest::Want(body) => {
            let response = server.post("upload-pack", &body);
            repo.fetch_finish(&ad, Some(&response)).unwrap();
        }
    }
}

/// What the browser's integrate does: a remote on version 1 is migrated
/// here first.
fn integrate(repo: &mut Repo<FsStore>, at: i64) -> Vec<(BranchName, Integrated)> {
    layout::integrate(repo, &sig("Alex", at, 0), None, &mut |_, p, b, o, t| {
        as_curator_merges(p, b, o, t)
    })
    .unwrap()
}

fn push(repo: &mut Repo<FsStore>, server: &Server) -> PushRequest {
    let ad = server.advertise("receive-pack");
    let plan = repo.push_request(&ad).unwrap();
    if let PushRequest::Send(body) = &plan {
        let response = server.post("receive-pack", body);
        let pushed = repo.push_finish(body, &response).unwrap();
        eprintln!(
            "pushed {:?}, rejected {:?}",
            pushed.updated, pushed.rejected
        );
        assert!(pushed.rejected.is_empty(), "{:?}", pushed.rejected);
    }
    plan
}

/// The v1 repo pushed to a fresh server, as a v1 browser had it, and the
/// browser that pushed it, with its remote set.
fn v1_on_server(dir: &Path) -> (Server, Repo<FsStore>, PathBuf) {
    let server = Server::new(dir);
    let a_dir = dir.join("a.git");
    let (mut a, _) = v1_repo(&a_dir);
    a.set_remote(&RemoteUrl::new("https://example.com/alex/mtg.git").unwrap())
        .unwrap();
    fetch(&mut a, &server);
    assert!(matches!(push(&mut a, &server), PushRequest::Send(_)));
    (server, a, a_dir)
}

fn refs_of(dir: &Path) -> String {
    git(
        dir,
        &[
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            "refs/heads",
            "refs/tags",
        ],
    )
}

#[test]
fn a_browser_that_migrates_pushes_deck_lines_main_as_a_fast_forward_and_moves_snapshots() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (server, mut a, a_dir) = v1_on_server(dir.path());
    let v1_main = git(&server.0, &["rev-parse", "main"]);
    let v1_fnm = git(&server.0, &["rev-parse", "decks/lantern/fnm"]);

    layout::migrate(&mut a).unwrap();
    fetch(&mut a, &server);
    let outcomes = integrate(&mut a, 1_800_000_000);
    assert!(
        outcomes
            .iter()
            .all(|(_, o)| *o == Integrated::Ahead || *o == Integrated::UpToDate),
        "{outcomes:?}"
    );
    let status = a.remote_status().unwrap();
    assert_eq!(
        (status.branches, status.tags),
        (4, 3),
        "main moved, three decks and three snapshots"
    );
    assert!(matches!(push(&mut a, &server), PushRequest::Send(_)));
    assert_eq!(refs_of(&server.0), refs_of(&a_dir));
    assert_eq!(
        git(&server.0, &["rev-parse", "main^"]),
        v1_main,
        "main went forward"
    );
    assert_ne!(
        git(&server.0, &["rev-parse", "decks/lantern/fnm"]),
        v1_fnm,
        "the snapshot moved"
    );
    assert_eq!(
        git(&server.0, &["rev-parse", "decks/lantern/fnm^{commit}"]),
        git(&server.0, &["rev-parse", "decks/lantern/main~1"])
    );
    fsck(&server.0);
    assert!(matches!(push(&mut a, &server), PushRequest::Nothing));
    eprintln!("server after the migration was pushed:\n{}", server.refs());
}

#[test]
fn a_v1_browser_migrating_after_the_remote_did_lands_on_the_same_commits() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (server, mut a, _) = v1_on_server(dir.path());
    // A second browser had synced the same v1 repo, and is still on v1.
    let b_dir = dir.path().join("b.git");
    let mut b = Repo::init(FsStore::new(&b_dir)).unwrap();
    b.set_remote(&RemoteUrl::new("https://example.com/alex/mtg.git").unwrap())
        .unwrap();
    fetch(&mut b, &server);
    integrate(&mut b, 1);
    assert_eq!(
        layout::version_at(&b, &b.branch_tip(&main_branch()).unwrap().unwrap()).unwrap(),
        Some(1)
    );

    layout::migrate(&mut a).unwrap();
    fetch(&mut a, &server);
    integrate(&mut a, 2);
    push(&mut a, &server);

    // B opens with the new code: it migrates its own v1 first, which makes
    // exactly what A pushed, so the sync finds nothing to do either way.
    layout::migrate(&mut b).unwrap();
    fetch(&mut b, &server);
    let outcomes = integrate(&mut b, 3);
    assert!(
        outcomes.iter().all(|(_, o)| *o == Integrated::UpToDate),
        "{outcomes:?}"
    );
    assert!(matches!(push(&mut b, &server), PushRequest::Nothing));
    assert_eq!(refs_of(&b_dir), refs_of(&server.0));
    fsck(&b_dir);
}

#[test]
fn a_v1_browser_with_unpushed_saves_migrates_and_merges_into_the_remotes_v2() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (server, mut a, _) = v1_on_server(dir.path());
    let c_dir = dir.path().join("c.git");
    let mut c = Repo::init(FsStore::new(&c_dir)).unwrap();
    c.set_remote(&RemoteUrl::new("https://example.com/alex/mtg.git").unwrap())
        .unwrap();
    fetch(&mut c, &server);
    integrate(&mut c, 1);
    // C saves, offline, on v1: lantern and the collection.
    save(
        &mut c,
        &[
            (
                "decks/lantern.deck.toml",
                &deck(
                    "Lantern",
                    &["Sol Ring", "Mind Stone", "Arcane Signet", "Lotus Petal"],
                ),
            ),
            (
                "collection.toml",
                &COLLECTION.replace(
                    "{ name = \"Mind Stone\", at = \"Bulk\" },",
                    "{ name = \"Mind Stone\", at = \"Bulk\" },\n  { name = \"Lotus Petal\" },",
                ),
            ),
        ],
        &[],
        "lantern: +1 Lotus Petal",
        &sig("Cleo", 1_701_000_000, 60),
    );
    // Meanwhile A migrated and pushed.
    layout::migrate(&mut a).unwrap();
    fetch(&mut a, &server);
    integrate(&mut a, 2);
    push(&mut a, &server);

    layout::migrate(&mut c).unwrap();
    fetch(&mut c, &server);
    let outcomes = integrate(&mut c, 1_801_000_000);
    eprintln!("C after migrating: {outcomes:?}");
    let of = |name: &str| {
        outcomes
            .iter()
            .find(|(b, _)| b.as_str() == name)
            .unwrap()
            .1
            .clone()
    };
    assert!(
        matches!(of("main"), Integrated::Merged { .. }),
        "{outcomes:?}"
    );
    assert_eq!(of("decks/lantern/main"), Integrated::Ahead);
    assert_eq!(of("decks/loam/main"), Integrated::UpToDate);
    assert!(matches!(push(&mut c, &server), PushRequest::Send(_)));
    assert_eq!(
        git(&server.0, &["show", "decks/lantern/main:lantern.deck.toml"]),
        deck(
            "Lantern",
            &["Sol Ring", "Mind Stone", "Arcane Signet", "Lotus Petal"]
        )
        .trim_end()
    );
    let collection = git(&server.0, &["show", "main:collection.toml"]);
    assert!(
        collection.contains("Lotus Petal") && collection.contains("deck = \"lantern\""),
        "{collection}"
    );
    assert_eq!(git(&server.0, &["show", "main:VERSION"]), "2");
    assert_eq!(
        git(&server.0, &["ls-tree", "--name-only", "main"]),
        "VERSION\ncollection.toml\nmeldweb.toml"
    );
    fsck(&server.0);
    fsck(&c_dir);
    eprintln!(
        "{}",
        git(&server.0, &["log", "--all", "--graph", "--format=%h %d %s"])
    );
}

/// One commit on `branch` putting `text` at `path`.
fn put_on(repo: &mut Repo<FsStore>, on: &str, path: &str, text: &str, who: &Signature) {
    let b = branch(on);
    let tip = repo.branch_tip(&b).unwrap().unwrap();
    repo.commit(
        &b,
        Tip::At(tip),
        &WriteCommit {
            changes: vec![Change::Put {
                path: p(path),
                bytes: text.as_bytes().to_vec(),
            }],
            message: "lantern: edit\n".into(),
            author: who.clone(),
            committer: who.clone(),
        },
    )
    .unwrap();
}

#[test]
fn a_sync_merges_a_deck_both_browsers_changed_by_card() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (server, mut a, _) = v1_on_server(dir.path());
    layout::migrate(&mut a).unwrap();
    fetch(&mut a, &server);
    integrate(&mut a, 1);
    push(&mut a, &server);
    let b_dir = dir.path().join("b.git");
    let mut b = Repo::init(FsStore::new(&b_dir)).unwrap();
    b.set_remote(&RemoteUrl::new("https://example.com/alex/mtg.git").unwrap())
        .unwrap();
    fetch(&mut b, &server);
    integrate(&mut b, 2);

    let line = "decks/lantern/main";
    let lantern = |repo: &Repo<FsStore>| {
        let tip = repo.branch_tip(&branch(line)).unwrap().unwrap();
        String::from_utf8(
            repo.read_file(&tip, &p("lantern.deck.toml"))
                .unwrap()
                .unwrap(),
        )
        .unwrap()
    };
    let was = lantern(&a);
    println!("lantern on both:\n{was}");
    // Neighbouring lines: A runs two Sol Rings, B three Mind Stones.
    let mine = was.replace(
        "{ name = \"Sol Ring\" }",
        "{ name = \"Sol Ring\", qty = 2 }",
    );
    let theirs = was.replace(
        "{ name = \"Mind Stone\" }",
        "{ name = \"Mind Stone\", qty = 3 }",
    );
    assert!(
        meldweb_git::merge_lines(&was, &mine, &theirs).is_none(),
        "by lines these clash"
    );
    put_on(
        &mut a,
        line,
        "lantern.deck.toml",
        &mine,
        &sig("Alex", 1_802_000_000, 0),
    );
    push(&mut a, &server);
    put_on(
        &mut b,
        line,
        "lantern.deck.toml",
        &theirs,
        &sig("Alex", 1_802_000_100, 0),
    );
    fetch(&mut b, &server);
    let outcomes = integrate(&mut b, 1_802_000_200);
    let lantern_outcome = &outcomes.iter().find(|(n, _)| n.as_str() == line).unwrap().1;
    assert!(
        matches!(lantern_outcome, Integrated::Merged { .. }),
        "{outcomes:?}"
    );
    let merged = lantern(&b);
    println!("merged by card:\n{merged}");
    assert!(merged.contains("{ name = \"Sol Ring\", qty = 2 }"));
    assert!(merged.contains("{ name = \"Mind Stone\", qty = 3 }"));
    assert!(matches!(push(&mut b, &server), PushRequest::Send(_)));
    fsck(&b_dir);
    fsck(&server.0);

    // Both change one card's quantity: the sync's merge stops, and says
    // the card.
    let base_text = lantern(&b);
    let a_text = base_text.replace("qty = 3", "qty = 1");
    let b_text = base_text.replace("qty = 3", "qty = 5");
    let merged = merging::merge_file(
        "lantern.deck.toml",
        &base_text,
        &a_text,
        &b_text,
        FileSettling::default(),
    );
    let Err(FileConflict::EachClash { clashes: said, .. }) = merged else {
        panic!("{merged:?}")
    };
    println!("{said:#?}");
    assert_eq!(
        (
            said[0].subject.as_str(),
            said[0].attribute.as_str(),
            said[0].ours.as_str(),
            said[0].theirs.as_str()
        ),
        ("Mind Stone", "quantity", "1", "5")
    );
}

#[test]
fn a_v2_browser_meeting_a_remote_still_on_v1_migrates_it_here_and_pushes_it_there() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (server, mut a, _) = v1_on_server(dir.path());

    // D, migrated, has not pushed; meanwhile an old v1 browser (A) saved loam.
    let d_dir = dir.path().join("d.git");
    let mut d = Repo::init(FsStore::new(&d_dir)).unwrap();
    d.set_remote(&RemoteUrl::new("https://example.com/alex/mtg.git").unwrap())
        .unwrap();
    fetch(&mut d, &server);
    integrate(&mut d, 1);
    layout::migrate(&mut d).unwrap();
    save(
        &mut a,
        &[(
            "decks/loam.deck.toml",
            &deck("Loam", &["Life from the Loam", "Sol Ring", "Crop Rotation"]),
        )],
        &[],
        "loam: +1 Crop Rotation",
        &sig("Alex", 1_701_100_000, 120),
    );
    fetch(&mut a, &server);
    push(&mut a, &server);

    fetch(&mut d, &server);
    let outcomes = integrate(&mut d, 1_802_000_000);
    eprintln!("D meeting a v1 remote that moved: {outcomes:?}");
    let of = |name: &str| {
        outcomes
            .iter()
            .find(|(b, _)| b.as_str() == name)
            .unwrap()
            .1
            .clone()
    };
    assert!(matches!(of("main"), Integrated::Merged { .. }));
    assert!(matches!(
        of("decks/loam/main"),
        Integrated::FastForwarded { .. }
    ));
    assert!(matches!(push(&mut d, &server), PushRequest::Send(_)));
    assert_eq!(
        git(&server.0, &["log", "-1", "--format=%s", "decks/loam/main"]),
        "loam: +1 Crop Rotation"
    );
    assert_eq!(git(&server.0, &["show", "main:VERSION"]), "2");
    assert_eq!(refs_of(&server.0), refs_of(&d_dir));
    fsck(&server.0);

    // A fresh v2 browser meeting the same kind of remote takes the
    // migration of it whole.
    std::fs::create_dir_all(dir.path().join("two")).unwrap();
    let server2 = Server::new(&dir.path().join("two"));
    a.set_remote(&RemoteUrl::new("https://example.com/alex/mtg2.git").unwrap())
        .unwrap();
    fetch(&mut a, &server2);
    push(&mut a, &server2);
    let e_dir = dir.path().join("e.git");
    let mut e = Repo::init(FsStore::new(&e_dir)).unwrap();
    e.set_remote(&RemoteUrl::new("https://example.com/alex/mtg2.git").unwrap())
        .unwrap();
    save(
        &mut e,
        &[("VERSION", "2\n")],
        &[],
        "curator: this repo is at version 2",
        &sig("Eve", 1_800_000_000, 0),
    );
    fetch(&mut e, &server2);
    let outcomes = integrate(&mut e, 1_803_000_000);
    eprintln!("a fresh browser meeting a v1 remote: {outcomes:?}");
    assert!(
        outcomes
            .iter()
            .all(|(_, o)| matches!(o, Integrated::Adopted { .. })),
        "{outcomes:?}"
    );
    let migrated_here = refs_of(&e_dir);
    push(&mut e, &server2);
    assert_eq!(refs_of(&server2.0), migrated_here);
    // ... and it is what migrating that repo in place makes.
    layout::migrate(&mut a).unwrap();
    assert_eq!(
        refs_of(&a_dir_of(&dir)),
        migrated_here,
        "migrating the remote's v1 here made the same commits as migrating it where it was"
    );
    fsck(&e_dir);
    fsck(&server2.0);
    let _ = DeckStem::new("x");
}

fn a_dir_of(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("a.git")
}

/// One browser's repo as the page holds it ([`Local`]), syncing with
/// `server` the way `web/src/remote/sync.ts` drives it.
mod browser {
    use super::*;
    use meldweb_wasm::repo::{FileRead, Local, Who, Wrote};

    pub fn who(at: i64) -> Who {
        Who {
            name: "Alex".into(),
            email: "alex@example.com".into(),
            seconds: 1_802_000_000 + at,
            offset_minutes: 0,
        }
    }

    pub fn new(url: &str) -> Local {
        let mut l = Local::default();
        l.init().unwrap();
        l.set_remote(url).unwrap();
        l
    }

    pub fn read(l: &Local, branch: &str, path: &str) -> (String, String) {
        match l.get_file(path, Some(branch)).unwrap() {
            FileRead::File { text, sha } => (text, sha),
            FileRead::NoFile => panic!("{branch} has no {path}"),
        }
    }

    pub fn put(l: &mut Local, branch: &str, path: &str, text: &str, at: i64) -> String {
        let sha = match l.get_file(path, Some(branch)).unwrap() {
            FileRead::File { sha, .. } => Some(sha),
            FileRead::NoFile => None,
        };
        let wrote = l
            .put_file(
                path,
                text,
                sha.as_deref(),
                "lantern: edit",
                &who(at),
                Some(branch),
                None,
            )
            .unwrap();
        let Wrote::Committed { commit, .. } = wrote else {
            panic!("{wrote:?}")
        };
        commit
    }

    pub fn fetch(l: &mut Local, server: &Server) {
        let ad = server.advertise("upload-pack");
        let want = l.fetch_request(&ad).unwrap();
        let response = want.map(|body| server.post("upload-pack", &body));
        l.fetch_finish(&ad, response.as_deref()).unwrap();
    }

    pub fn push(l: &mut Local, server: &Server) {
        let ad = server.advertise("receive-pack");
        let plan = l.push_request(&ad).unwrap();
        assert_eq!(plan.kind(), "send");
        let response = server.post("receive-pack", &plan.body());
        let pushed = l.push_finish(&plan.body(), &response).unwrap();
        assert!(pushed.rejected.is_empty(), "{:?}", pushed.rejected);
    }

    pub fn integrate(l: &mut Local, settle: &SyncSettle, at: i64) -> Synced {
        l.integrate(&who(at), settle).unwrap()
    }
}

#[test]
fn a_syncs_clashes_on_two_branches_are_settled_each_by_its_own_picks() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let server = Server::new(dir.path());
    let url = "https://example.com/alex/mtg.git";
    let (main, budget, path) = (
        "decks/lantern/main",
        "decks/lantern/budget",
        "lantern.deck.toml",
    );

    // A makes the deck and its budget branch, and pushes both.
    let mut a = browser::new(url);
    browser::put(&mut a, "main", "VERSION", "2\n", 0);
    let was = deck("Lantern", &["Sol Ring", "Mind Stone", "Island"]);
    let first = browser::put(&mut a, main, path, &was, 1);
    a.create_branch("lantern", "budget", &first).unwrap();
    browser::fetch(&mut a, &server);
    browser::integrate(&mut a, &SyncSettle::default(), 2);
    browser::push(&mut a, &server);

    // B clones it.
    let mut b = browser::new(url);
    browser::fetch(&mut b, &server);
    browser::integrate(&mut b, &SyncSettle::default(), 3);
    assert_eq!(browser::read(&b, budget, path).0, was);

    // Both change Sol Ring's quantity on both branches, each differently.
    let qty = |n: u32| {
        was.replace(
            "{ name = \"Sol Ring\" }",
            &format!("{{ name = \"Sol Ring\", qty = {n} }}"),
        )
    };
    browser::put(&mut a, main, path, &qty(2), 4);
    browser::put(&mut a, budget, path, &qty(4), 5);
    browser::push(&mut a, &server);
    browser::put(&mut b, main, path, &qty(3), 6);
    browser::put(&mut b, budget, path, &qty(5), 7);
    browser::fetch(&mut b, &server);

    let Synced::Conflict { conflicts } = browser::integrate(&mut b, &SyncSettle::default(), 8)
    else {
        panic!("both branches clash");
    };
    println!("{conflicts:#?}");
    let keys: Vec<(&str, Vec<&str>)> = conflicts
        .iter()
        .map(|c| {
            let paths = c
                .conflict
                .files
                .iter()
                .map(|f| match f {
                    FileConflict::EachClash { path, .. } => path.as_str(),
                    other => panic!("{other:?}"),
                })
                .collect();
            (c.branch.as_str(), paths)
        })
        .collect();
    assert_eq!(keys, vec![(budget, vec![path]), (main, vec![path])]);
    for (c, (ours, theirs)) in conflicts.iter().zip([("5", "4"), ("3", "2")]) {
        let [FileConflict::EachClash { clashes, .. }] = &c.conflict.files[..] else {
            panic!("{c:?}")
        };
        assert_eq!(clashes.len(), 1);
        let c = &clashes[0];
        assert_eq!(
            (
                c.subject.as_str(),
                c.attribute.as_str(),
                c.ours.as_str(),
                c.theirs.as_str()
            ),
            ("Sol Ring", "quantity", ours, theirs)
        );
    }
    let settle_of = |conflicts: &[SyncConflict], branch: &str, pick: Side| {
        let c = conflicts.iter().find(|c| c.branch == branch).unwrap();
        Settle {
            guard: c.conflict.guard.clone(),
            prefer: None,
            files: vec![FileSettle::EachClash {
                path: path.to_owned(),
                picks: vec![pick],
            }],
        }
    };

    // A pick made against other tips settles nothing: B saved again since.
    let stale = settle_of(&conflicts, main, Side::Theirs);
    browser::put(&mut b, main, path, &qty(3).replace("Island", "Plains"), 9);
    let Synced::Conflict { conflicts: again } = browser::integrate(
        &mut b,
        &SyncSettle {
            prefer: None,
            settles: vec![stale.clone(), settle_of(&conflicts, budget, Side::Ours)],
        },
        10,
    ) else {
        panic!("main's pick was made against a tip B has moved from");
    };
    assert_eq!(again.len(), 1, "budget's pick settled budget: {again:#?}");
    assert_eq!(again[0].branch.as_str(), main);
    assert_ne!(again[0].conflict.guard, stale.guard, "main's tip moved");
    let budget_merged = browser::read(&b, budget, path).0;
    assert!(budget_merged.contains("qty = 5"), "{budget_merged}");

    // Picked against the tips it has now, main takes A's quantity.
    let fresh = settle_of(&again, main, Side::Theirs);
    let synced = browser::integrate(
        &mut b,
        &SyncSettle {
            prefer: None,
            settles: vec![fresh],
        },
        11,
    );
    assert!(matches!(synced, Synced::Merged { .. }), "{synced:?}");
    let main_merged = browser::read(&b, main, path).0;
    println!("main merged:\n{main_merged}\nbudget merged:\n{budget_merged}");
    assert!(main_merged.contains("{ name = \"Sol Ring\", qty = 2 }"));
    assert!(main_merged.contains("Plains"), "B's other change stays");
    assert!(budget_merged.contains("{ name = \"Sol Ring\", qty = 5 }"));
    browser::push(&mut b, &server);

    for branch in [main, budget] {
        println!(
            "{}",
            git(
                &server.0,
                &["log", "--graph", "--format=%h %s (%an)", branch]
            )
        );
        let parents = git(&server.0, &["rev-list", "--parents", "-n1", branch]);
        assert_eq!(
            parents.split(' ').count(),
            3,
            "{branch} ends in a merge: {parents}"
        );
    }
    assert!(git(&server.0, &["show", &format!("{main}:{path}")]).contains("qty = 2"));
    assert!(git(&server.0, &["show", &format!("{budget}:{path}")]).contains("qty = 5"));
    fsck(&server.0);

    // B's repo, as its zip unpacks, is as sound as the server's.
    let b_dir = dir.path().join("b.git");
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(b.export_zip().unwrap())).unwrap();
    zip.extract(&b_dir).unwrap();
    fsck(&b_dir);
}
