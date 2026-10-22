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
    merge_lines, BranchName, Change, CommitId, FetchRequest, FsStore, Identity, Integrated,
    PushRequest, RemoteUrl, Repo, RepoPath, Signature, TagName, Timestamp, Tip, WriteCommit,
};
use meldweb_wasm::layout::{self, DeckLines};

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

fn by_lines(_: &str, base: &[u8], ours: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    let s = |b: &[u8]| String::from_utf8(b.to_vec()).ok();
    merge_lines(&s(base)?, &s(ours)?, &s(theirs)?).map(String::into_bytes)
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
    layout::integrate(repo, &sig("Alex", at, 0), None, &mut by_lines).unwrap()
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
