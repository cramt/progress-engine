//! Forking another player's deck and taking their changes (ADR-0041),
//! against the real `git`: alice's repo and yours are bare repos on disk,
//! alice's served through `git upload-pack --stateless-rpc` as a smart HTTP
//! host answers, and yours pushed to an origin of its own through `git
//! receive-pack`. Every repo passes `git fsck --strict` at the end.
//!
//! Without `git` on PATH these print `SKIPPED` and pass, unless
//! `MELDWEB_GIT_REQUIRE` is set.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chip_decklist::stem::DeckStem;
use meldweb_git::{
    BranchName, Change, CommitId, FsStore, Identity, PushRequest, RemoteUrl, Repo, RepoPath, Side,
    Signature, Timestamp, Tip, WriteCommit,
};
use meldweb_wasm::blame::{self, Blame, DeckBlame};
use meldweb_wasm::branches::{MergeDone, MergePlan};
use meldweb_wasm::layout;
use meldweb_wasm::merging::Settle;
use meldweb_wasm::preference::{self, Upstream};
use meldweb_wasm::upstream::{self, Forked};

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

/// A bare repo on disk, answering as a smart HTTP host would.
struct Host(PathBuf);

impl Host {
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
}

fn sig(name: &str, seconds: i64) -> Signature {
    Signature {
        who: Identity::new(name, format!("{}@example.com", name.to_lowercase())).unwrap(),
        when: Timestamp::new(seconds, 60).unwrap(),
    }
}

fn p(path: &str) -> RepoPath {
    RepoPath::new(path).unwrap()
}

fn branch(name: &str) -> BranchName {
    BranchName::new(name).unwrap()
}

fn stem(s: &str) -> DeckStem {
    DeckStem::new(s).unwrap()
}

fn commit(
    repo: &mut Repo<FsStore>,
    on: &str,
    puts: &[(&str, &str)],
    message: &str,
    who: &Signature,
) -> CommitId {
    let b = branch(on);
    let tip = repo.branch_tip(&b).unwrap();
    repo.commit(
        &b,
        tip.map_or(Tip::Unborn, Tip::At),
        &WriteCommit {
            changes: puts
                .iter()
                .map(|(path, text)| Change::Put {
                    path: p(path),
                    bytes: text.as_bytes().to_vec(),
                })
                .collect(),
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

const ALICE_URL: &str = "https://alice.test/alice/mtg.git";
const BASE: [&str; 4] = [
    "Lantern of Insight",
    "Codex Shredder",
    "Ensnaring Bridge",
    "Island",
];

/// A repo at version 2 with nothing but its `main`.
fn v2_repo(dir: &Path, who: &Signature) -> Repo<FsStore> {
    let mut repo = Repo::init(FsStore::new(dir)).unwrap();
    commit(&mut repo, "main", &[("VERSION", "2\n")], "init", who);
    repo
}

fn alice_up() -> Upstream {
    Upstream::new(ALICE_URL, "lantern", None).unwrap()
}

/// Fetches alice's lantern into `me`, as the page does: one advertisement,
/// one upload-pack request for the deck's tip alone.
fn fetch(me: &mut Repo<FsStore>, alice: &Host, up: &Upstream) -> upstream::Fetched {
    let ad = alice.advertise("upload-pack");
    let want = upstream::fetch_request(me, up, &ad).unwrap();
    let response = want.map(|body| alice.post("upload-pack", &body));
    upstream::fetch_finish(me, up, &ad, response.as_deref()).unwrap()
}

fn mine(me: &Repo<FsStore>, s: &str) -> CommitId {
    me.branch_tip(&layout::main_line(&stem(s)))
        .unwrap()
        .unwrap()
}

fn text_at(repo: &Repo<FsStore>, at: &CommitId, path: &str) -> String {
    String::from_utf8(repo.read_file(at, &p(path)).unwrap().unwrap()).unwrap()
}

#[test]
fn a_fork_of_alices_deck_keeps_her_commits_and_takes_her_changes() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let a = |t: i64| sig("Alice", 1_760_000_000 + t);
    let me_at = |t: i64| sig("Me", 1_760_100_000 + t);

    // Alice's repo: her lantern over two saves, and a loam that is none of
    // this fork's business.
    let alice_dir = dir.path().join("alice.git");
    let mut alice = v2_repo(&alice_dir, &a(0));
    commit(
        &mut alice,
        "decks/lantern/main",
        &[
            ("lantern.deck.toml", &deck("Lantern", &BASE[..3])),
            ("lantern.criteria.toml", "# her questions\n"),
        ],
        "lantern: new deck",
        &a(10),
    );
    let her_second = commit(
        &mut alice,
        "decks/lantern/main",
        &[("lantern.deck.toml", &deck("Lantern", &BASE))],
        "lantern: +1 Island",
        &a(20),
    );
    let loam = commit(
        &mut alice,
        "decks/loam/main",
        &[("loam.deck.toml", &deck("Loam", &["Life from the Loam"]))],
        "loam: new deck",
        &a(30),
    );
    let host = Host(alice_dir.clone());

    // Fork it: only her lantern's line comes down.
    let me_dir = dir.path().join("me.git");
    let mut me = v2_repo(&me_dir, &me_at(0));
    let up = alice_up();
    let fetched = fetch(&mut me, &host, &up);
    assert_eq!(fetched.commit, her_second.to_hex());
    assert_eq!(fetched.name.as_deref(), Some("Lantern"));
    assert!(!fetched.migrated);
    assert!(
        me.read_commit(&loam).is_err(),
        "her loam was fetched too, but only lantern's line was asked for"
    );
    let forked = upstream::fork(&mut me, &up, &stem("lantern"), &me_at(1)).unwrap();
    let Forked::Forked {
        commit: forked_at, ..
    } = forked
    else {
        panic!("{forked:?}")
    };
    assert_eq!(forked_at, her_second.to_hex(), "the fork is her tip itself");
    let info = me.read_commit(&her_second).unwrap();
    assert_eq!(info.author.who.name(), "Alice");
    // The upstream is in meldweb.toml on main, and reads back as given.
    let main_tip = me.branch_tip(&branch("main")).unwrap().unwrap();
    let settings = text_at(&me, &main_tip, "meldweb.toml");
    println!("meldweb.toml after the fork:\n{settings}");
    assert_eq!(
        preference::read(Some(&settings))
            .unwrap()
            .upstreams
            .get(&stem("lantern")),
        Some(&up)
    );
    // The file is new here, so it says the default rules it writes too.
    assert_eq!(
        me.read_commit(&main_tip).unwrap().message.lines().next(),
        Some("meldweb.toml: rank printings by 10 rules, lantern follows alice's lantern")
    );
    // A stem taken is refused, and nothing moves.
    let again = upstream::fork(&mut me, &up, &stem("lantern"), &me_at(2)).unwrap();
    assert!(matches!(again, Forked::Taken { .. }), "{again:?}");
    // Blame says Alice for every card she brought.
    let mut memo = blame::Memo::default();
    let DeckBlame::Blamed { cards, .. } =
        blame::blame(&me, &mut memo, &stem("lantern"), mine(&me, "lantern"), None).unwrap()
    else {
        panic!("no blame")
    };
    for c in &cards {
        let Blame::Added { added } = &c.blame else {
            panic!("{c:?}")
        };
        assert_eq!(added.author, "Alice");
    }

    // She adds a card: one new, a fast-forward, no commit of mine.
    let her_third = commit(
        &mut alice,
        "decks/lantern/main",
        &[(
            "lantern.deck.toml",
            &deck("Lantern", &[&BASE[..], &["Sol Ring"]].concat()),
        )],
        "lantern: +1 Sol Ring",
        &a(40),
    );
    fetch(&mut me, &host, &up);
    let f = upstream::following(&me, &stem("lantern")).unwrap().unwrap();
    let theirs = f.theirs.unwrap();
    assert_eq!((theirs.new, theirs.ahead), (1, 0));
    assert_eq!(theirs.base.as_deref(), Some(her_second.to_hex().as_str()));
    let plan = upstream::plan(&mut me, &stem("lantern"), None).unwrap();
    let MergePlan::FastForward { into, from, .. } = &plan else {
        panic!("{plan:?}")
    };
    let done = upstream::take(&mut me, &stem("lantern"), into, from, None, &me_at(3)).unwrap();
    assert!(
        matches!(&done, MergeDone::Merged { commit, message: None } if *commit == her_third.to_hex()),
        "{done:?}"
    );

    // I add a card at the top and she one at the bottom: a real merge.
    let mut my_cards = vec!["Mind Stone"];
    my_cards.extend(BASE);
    my_cards.push("Sol Ring");
    commit(
        &mut me,
        "decks/lantern/main",
        &[("lantern.deck.toml", &deck("Lantern", &my_cards))],
        "lantern: +1 Mind Stone",
        &me_at(4),
    );
    let her_fourth = commit(
        &mut alice,
        "decks/lantern/main",
        &[(
            "lantern.deck.toml",
            &deck(
                "Lantern",
                &[&BASE[..], &["Sol Ring", "Arcane Signet"]].concat(),
            ),
        )],
        "lantern: +1 Arcane Signet",
        &a(50),
    );
    fetch(&mut me, &host, &up);
    let f = upstream::following(&me, &stem("lantern")).unwrap().unwrap();
    let theirs = f.theirs.unwrap();
    assert_eq!((theirs.new, theirs.ahead), (1, 1));
    let plan = upstream::plan(&mut me, &stem("lantern"), None).unwrap();
    let MergePlan::Merge {
        into,
        from,
        message,
        after,
        ..
    } = &plan
    else {
        panic!("{plan:?}")
    };
    assert_eq!(message, "lantern: merge alice's lantern: +1 Arcane Signet");
    let after = after.clone().unwrap();
    assert!(after.contains("Mind Stone") && after.contains("Arcane Signet"));
    let done = upstream::take(&mut me, &stem("lantern"), into, from, None, &me_at(5)).unwrap();
    let MergeDone::Merged { commit: merged, .. } = done else {
        panic!("{done:?}")
    };
    let merged = CommitId::from_hex(&merged).unwrap();
    assert_eq!(
        me.read_commit(&merged).unwrap().parents[1],
        her_fourth,
        "her tip is the merge's second parent"
    );
    let graph = git(
        &me_dir,
        &["log", "--graph", "--format=%s (%an)", "decks/lantern/main"],
    );
    println!("{graph}");
    assert!(graph.starts_with("*   lantern: merge alice's lantern: +1 Arcane Signet (Me)"));

    // We both change the same card's quantity: a clash on that card alone,
    // until a side is picked for it.
    let signets = |cards: &[&str], n: u32| {
        deck("Lantern", cards).replace(
            "{ name = \"Arcane Signet\" }",
            &format!("{{ name = \"Arcane Signet\", qty = {n} }}"),
        )
    };
    let mut both_mine = my_cards.clone();
    both_mine.push("Arcane Signet");
    commit(
        &mut me,
        "decks/lantern/main",
        &[("lantern.deck.toml", &signets(&both_mine, 2))],
        "lantern: Arcane Signet: 1 → 2",
        &me_at(6),
    );
    let hers = [&BASE[..], &["Sol Ring", "Arcane Signet"]].concat();
    commit(
        &mut alice,
        "decks/lantern/main",
        &[("lantern.deck.toml", &signets(&hers, 3))],
        "lantern: Arcane Signet: 1 → 3",
        &a(60),
    );
    fetch(&mut me, &host, &up);
    let plan = upstream::plan(&mut me, &stem("lantern"), None).unwrap();
    let MergePlan::Conflict { clashes, hunks, .. } = &plan else {
        panic!("{plan:?}")
    };
    println!("{clashes:#?}");
    assert!(hunks.is_empty(), "{hunks:?}");
    assert_eq!(clashes.len(), 1, "{clashes:?}");
    let c = &clashes[0];
    assert_eq!(
        (
            c.subject.as_str(),
            c.attribute.as_str(),
            c.ours.as_str(),
            c.theirs.as_str()
        ),
        ("Arcane Signet", "quantity", "2", "3")
    );
    let pick_theirs = Settle {
        prefer: None,
        picks: vec![Side::Theirs],
    };
    let plan = upstream::plan(&mut me, &stem("lantern"), pick_theirs.clone()).unwrap();
    let MergePlan::Merge {
        into, from, after, ..
    } = &plan
    else {
        panic!("{plan:?}")
    };
    let after = after.clone().unwrap();
    assert!(
        after.contains("{ name = \"Arcane Signet\", qty = 3 }") && after.contains("Mind Stone")
    );
    let done = upstream::take(
        &mut me,
        &stem("lantern"),
        into,
        from,
        pick_theirs,
        &me_at(7),
    )
    .unwrap();
    assert!(matches!(done, MergeDone::Merged { .. }), "{done:?}");
    // What I changed since the last merge base: Mind Stone, as mine.
    let f = upstream::following(&me, &stem("lantern")).unwrap().unwrap();
    let theirs = f.theirs.unwrap();
    assert_eq!(theirs.new, 0);
    assert!(theirs.mine_text.unwrap().contains("Mind Stone"));

    // A fork under another stem is one commit on her tip that names the
    // files its own, and her next change merges into them.
    let renamed = upstream::fork(&mut me, &up, &stem("my-lantern"), &me_at(8)).unwrap();
    let Forked::Forked { commit: rename, .. } = renamed else {
        panic!("{renamed:?}")
    };
    let rename = CommitId::from_hex(&rename).unwrap();
    let her_tip = me
        .read_commit(&rename)
        .unwrap()
        .parents
        .first()
        .copied()
        .unwrap();
    let names: Vec<String> = me
        .list_dir(&rename, None)
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["my-lantern.criteria.toml", "my-lantern.deck.toml"]);
    let her_fifth = commit(
        &mut alice,
        "decks/lantern/main",
        &[(
            "lantern.deck.toml",
            &signets(
                &[&BASE[..], &["Sol Ring", "Arcane Signet", "Mox Opal"]].concat(),
                3,
            ),
        )],
        "lantern: +1 Mox Opal",
        &a(70),
    );
    assert_ne!(her_tip, her_fifth);
    fetch(&mut me, &host, &up);
    let plan = upstream::plan(&mut me, &stem("my-lantern"), None).unwrap();
    let MergePlan::Merge {
        into,
        from,
        message,
        ..
    } = &plan
    else {
        panic!("{plan:?}")
    };
    assert_eq!(message, "my-lantern: merge alice's lantern: +1 Mox Opal");
    let done = upstream::take(&mut me, &stem("my-lantern"), into, from, None, &me_at(9)).unwrap();
    let MergeDone::Merged { commit: m, .. } = done else {
        panic!("{done:?}")
    };
    let m = CommitId::from_hex(&m).unwrap();
    assert!(text_at(&me, &m, "my-lantern.deck.toml").contains("Mox Opal"));
    assert!(me.read_file(&m, &p("lantern.deck.toml")).unwrap().is_none());
    // Blame through the rename and the merge: Mox Opal is hers.
    let DeckBlame::Blamed { cards, .. } =
        blame::blame(&me, &mut memo, &stem("my-lantern"), m, None).unwrap()
    else {
        panic!("no blame")
    };
    let opal = cards.last().unwrap();
    let Blame::Added { added } = &opal.blame else {
        panic!("{opal:?}")
    };
    assert_eq!(
        (added.author.as_str(), added.commit.as_str()),
        ("Alice", her_fifth.to_hex().as_str())
    );

    // Unfollowing keeps the history.
    upstream::unfollow(&mut me, &stem("my-lantern"), &me_at(10)).unwrap();
    assert!(upstream::following(&me, &stem("my-lantern"))
        .unwrap()
        .is_none());
    assert_eq!(mine(&me, "my-lantern"), m);

    // Pushed to my own origin: my branches, her commits in them, and none
    // of what is tracked of her repo.
    let origin_dir = dir.path().join("origin.git");
    git(
        dir.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin_dir.to_str().unwrap(),
        ],
    );
    let origin = Host(origin_dir.clone());
    me.set_remote(&RemoteUrl::new("https://me.test/me/mtg.git").unwrap())
        .unwrap();
    let PushRequest::Send(body) = me.push_request(&origin.advertise("receive-pack")).unwrap()
    else {
        panic!("nothing to push")
    };
    let response = origin.post("receive-pack", &body);
    let pushed = me.push_finish(&body, &response).unwrap();
    assert!(pushed.rejected.is_empty(), "{pushed:?}");
    let refs = git(&origin_dir, &["for-each-ref", "--format=%(refname)"]);
    println!("origin's refs:\n{refs}");
    assert_eq!(
        refs.lines().collect::<Vec<_>>(),
        [
            "refs/heads/decks/lantern/main",
            "refs/heads/decks/my-lantern/main",
            "refs/heads/main"
        ]
    );
    let mine_refs = git(&me_dir, &["for-each-ref", "--format=%(refname)"]);
    println!("my refs:\n{mine_refs}");
    assert!(mine_refs.contains("refs/remotes/alice.test/alice/mtg/decks/lantern/main"));
    assert_eq!(
        git(
            &origin_dir,
            &["log", "--format=%an", "-1", &her_second.to_hex()]
        ),
        "Alice"
    );

    fsck(&alice_dir);
    fsck(&me_dir);
    fsck(&origin_dir);
}

/// The fork's own editor pinned a printing on every card named by name
/// when it opened the deck (ADR-0027), so the fork differs from hers on
/// every card line. Her next change to a card still merges into it: a line
/// merge clashed on it, a merge by card does not.
#[test]
fn her_change_merges_into_a_fork_whose_editor_pinned_every_printing() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let a = |t: i64| sig("Alice", 1_760_000_000 + t);
    let me_at = |t: i64| sig("Me", 1_760_100_000 + t);
    let alice_dir = dir.path().join("alice.git");
    let mut alice = v2_repo(&alice_dir, &a(0));
    let hers = deck("Lantern", &BASE);
    commit(
        &mut alice,
        "decks/lantern/main",
        &[("lantern.deck.toml", &hers)],
        "lantern: new deck",
        &a(10),
    );
    let host = Host(alice_dir.clone());
    let me_dir = dir.path().join("me.git");
    let mut me = v2_repo(&me_dir, &me_at(0));
    let up = alice_up();
    fetch(&mut me, &host, &up);
    upstream::fork(&mut me, &up, &stem("lantern"), &me_at(1)).unwrap();

    // Opening it pins each card, as the editor does: every line changes.
    let printings = [
        ("Lantern of Insight", "5dn/135"),
        ("Codex Shredder", "rav/254"),
        ("Ensnaring Bridge", "sth/129"),
        ("Island", "unf/236"),
    ];
    let mut pinned = hers.clone();
    for (name, printing) in printings {
        pinned = pinned.replace(
            &format!("{{ name = \"{name}\" }},"),
            &format!("{{ printing = \"{printing}\" }},  # {name}"),
        );
    }
    let pin = commit(
        &mut me,
        "decks/lantern/main",
        &[("lantern.deck.toml", &pinned)],
        "lantern: Lantern of Insight: any printing → 5dn/135, and 3 more",
        &me_at(2),
    );
    // She runs a second Codex Shredder and adds Mox Opal.
    let changed = hers
        .replace(
            "{ name = \"Codex Shredder\" }",
            "{ name = \"Codex Shredder\", qty = 2 }",
        )
        .replace("]\n", "  { name = \"Mox Opal\" },\n]\n");
    let her_change = commit(
        &mut alice,
        "decks/lantern/main",
        &[("lantern.deck.toml", &changed)],
        "lantern: +1 Mox Opal, Codex Shredder: 1 → 2",
        &a(20),
    );
    fetch(&mut me, &host, &up);

    // Before: the line merge every deck merge used clashed on it.
    let by_lines = meldweb_git::merge_lines_with(&hers, &pinned, &changed, None);
    println!("by lines: {by_lines:#?}");
    assert!(by_lines.is_err());

    let plan = upstream::plan(&mut me, &stem("lantern"), None).unwrap();
    let MergePlan::Merge {
        into,
        from,
        after,
        message,
        ..
    } = &plan
    else {
        panic!("{plan:?}")
    };
    let after = after.clone().unwrap();
    println!("{message}\n{after}");
    assert_eq!(
        message,
        "lantern: merge alice's lantern: +1 Mox Opal, Codex Shredder: 1 → 2"
    );
    assert!(after.contains("{ printing = \"rav/254\", qty = 2 },  # Codex Shredder"));
    assert!(after.contains("{ printing = \"5dn/135\" },  # Lantern of Insight"));
    assert!(after.contains("{ name = \"Mox Opal\" }"));
    let done = upstream::take(&mut me, &stem("lantern"), into, from, None, &me_at(3)).unwrap();
    let MergeDone::Merged { commit: merged, .. } = done else {
        panic!("{done:?}")
    };
    let merged = CommitId::from_hex(&merged).unwrap();
    assert_eq!(me.read_commit(&merged).unwrap().parents, [pin, her_change]);

    // Blame: Mox Opal is hers, an untouched pin is mine, and the card both
    // changed, each its own way, was last changed where the two met.
    let mut memo = blame::Memo::default();
    let DeckBlame::Blamed { cards, .. } =
        blame::blame(&me, &mut memo, &stem("lantern"), merged, None).unwrap()
    else {
        panic!("no blame")
    };
    let last = |i: usize| match &cards[i].blame {
        Blame::Changed { changed, .. } => changed.commit.clone(),
        Blame::Added { added } => added.commit.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(last(0), pin.to_hex(), "Lantern of Insight");
    assert_eq!(last(1), merged.to_hex(), "Codex Shredder");
    assert_eq!(last(4), her_change.to_hex(), "Mox Opal");

    fsck(&alice_dir);
    fsck(&me_dir);
}

/// One commit on a version-1 `main`.
fn save_v1(repo: &mut Repo<FsStore>, puts: &[(&str, &str)], message: &str, who: &Signature) {
    commit(repo, "main", puts, message, who);
}

#[test]
fn a_fork_from_a_repo_still_at_version_1_holds_the_ids_its_own_migration_makes() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let a = |t: i64| sig("Alice", 1_750_000_000 + t);
    let alice_dir = dir.path().join("alice.git");
    let mut alice = Repo::init(FsStore::new(&alice_dir)).unwrap();
    save_v1(&mut alice, &[("VERSION", "1\n")], "init", &a(0));
    save_v1(
        &mut alice,
        &[("decks/lantern.deck.toml", &deck("Lantern", &BASE[..2]))],
        "decks/lantern.deck.toml: new deck",
        &a(10),
    );
    save_v1(
        &mut alice,
        &[(
            "decks/loam.deck.toml",
            &deck("Loam", &["Life from the Loam"]),
        )],
        "decks/loam.deck.toml: new deck",
        &a(20),
    );
    save_v1(
        &mut alice,
        &[
            ("decks/lantern.deck.toml", &deck("Lantern", &BASE)),
            ("decks/lantern.criteria.toml", "# hers\n"),
        ],
        "decks/lantern.deck.toml: +2",
        &a(30),
    );
    let host = Host(alice_dir.clone());

    let me_dir = dir.path().join("me.git");
    let mut me = v2_repo(&me_dir, &sig("Me", 1_760_000_000));
    let up = alice_up();
    let fetched = fetch(&mut me, &host, &up);
    assert!(fetched.migrated);
    let Forked::Forked { commit, .. } =
        upstream::fork(&mut me, &up, &stem("lantern"), &sig("Me", 1_760_000_100)).unwrap()
    else {
        panic!()
    };

    // Alice migrates her own repo, in a copy, as her browser would.
    let copy = dir.path().join("alice-migrated.git");
    git(
        dir.path(),
        &[
            "clone",
            "-q",
            "--bare",
            alice_dir.to_str().unwrap(),
            copy.to_str().unwrap(),
        ],
    );
    // A clone packs its objects; this crate reads loose ones only.
    for pack in std::fs::read_dir(copy.join("objects/pack")).unwrap() {
        let pack = pack.unwrap().path();
        if pack.extension().is_some_and(|e| e == "pack") {
            let bytes = std::fs::read(&pack).unwrap();
            std::fs::rename(&pack, dir.path().join("moved.pack")).unwrap();
            git_stdin(&copy, &["unpack-objects", "-q"], &bytes);
        }
    }
    let _ = std::fs::remove_file(copy.join("packed-refs"));
    std::fs::create_dir_all(copy.join("refs/heads")).unwrap();
    std::fs::write(
        copy.join("refs/heads/main"),
        format!("{}\n", alice.branch_tip(&branch("main")).unwrap().unwrap()),
    )
    .unwrap();
    let mut hers = Repo::open(FsStore::new(&copy)).unwrap();
    layout::migrate(&mut hers).unwrap();
    let her_line = hers
        .branch_tip(&layout::main_line(&stem("lantern")))
        .unwrap()
        .unwrap();
    println!("the fork: {commit}\nher own migration: {her_line}");
    assert_eq!(commit, her_line.to_hex());
    assert_eq!(fetched.commit, her_line.to_hex());
    let log = git(
        &me_dir,
        &["log", "--format=%H %an %s", "decks/lantern/main"],
    );
    println!("{log}");
    assert_eq!(
        log,
        git(&copy, &["log", "--format=%H %an %s", "decks/lantern/main"])
    );
    fsck(&me_dir);
    fsck(&copy);
}
