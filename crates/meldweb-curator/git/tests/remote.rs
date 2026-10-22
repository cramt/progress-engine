//! The remote protocol against the real `git`: a bare repo on disk is the
//! server, `git upload-pack --stateless-rpc` and `git receive-pack
//! --stateless-rpc` answer exactly as `git http-backend` does behind a smart
//! HTTP host, and every byte between them and the crate is what the browser
//! would send through the worker. `git fsck --strict` checks both sides.
//!
//! Without `git` on PATH these print `SKIPPED` and pass, unless
//! `MELDWEB_GIT_REQUIRE` is set.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use meldweb_git::{
    merge_lines, BranchName, Change, FetchRequest, FsStore, Identity, Integrated, PushRequest,
    RemoteUrl, Repo, RepoPath, Side, Signature, TagName, Timestamp, Tip, WriteCommit,
};

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
        .args([
            "-c",
            "gc.auto=0",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
        ])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Friend")
        .env("GIT_AUTHOR_EMAIL", "friend@example.com")
        .env("GIT_COMMITTER_NAME", "Friend")
        .env("GIT_COMMITTER_EMAIL", "friend@example.com")
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
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

/// What `git http-backend` puts before an advertisement, so the parser
/// sees exactly what a host sends.
fn with_service_header(service: &str, ad: Vec<u8>) -> Vec<u8> {
    let line = format!("# service={service}\n");
    let mut out = format!("{:04x}{line}0000", line.len() + 4).into_bytes();
    out.extend(ad);
    out
}

/// A bare repo answering as a smart HTTP host would.
struct Server(PathBuf);

impl Server {
    fn advertise(&self, service: &str) -> Vec<u8> {
        let out = git_cmd(&self.0)
            .args([service, "--stateless-rpc", "--advertise-refs", "."])
            .output()
            .unwrap();
        assert!(out.status.success());
        with_service_header(&format!("git-{service}"), out.stdout)
    }

    fn post(&self, service: &str, body: &[u8]) -> Vec<u8> {
        git_stdin(&self.0, &[service, "--stateless-rpc", "."], body)
    }

    fn fsck(&self) {
        git(&self.0, &["fsck", "--strict", "--full", "--no-dangling"]);
    }

    fn rev(&self, r: &str) -> String {
        git(&self.0, &["rev-parse", r])
    }
}

fn sig(seconds: i64) -> Signature {
    Signature {
        who: Identity::new("Alex", "alex@example.com").unwrap(),
        when: Timestamp::new(seconds, 60).unwrap(),
    }
}

fn path(p: &str) -> RepoPath {
    RepoPath::new(p).unwrap()
}

fn main_branch() -> BranchName {
    BranchName::new("main").unwrap()
}

fn commit(repo: &mut Repo<FsStore>, files: &[(&str, &str)], message: &str, at: i64) -> String {
    let tip = repo.branch_tip(&main_branch()).unwrap();
    let id = repo
        .commit(
            &main_branch(),
            tip.map_or(Tip::Unborn, Tip::At),
            &WriteCommit {
                changes: files
                    .iter()
                    .map(|(p, t)| Change::Put {
                        path: path(p),
                        bytes: t.as_bytes().to_vec(),
                    })
                    .collect(),
                message: format!("{message}\n"),
                author: sig(at),
                committer: sig(at),
            },
        )
        .unwrap();
    id.to_hex()
}

fn by_lines(_: &str, base: &[u8], ours: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    let s = |b: &[u8]| String::from_utf8(b.to_vec()).ok();
    merge_lines(&s(base)?, &s(ours)?, &s(theirs)?).map(String::into_bytes)
}

fn fetch(repo: &mut Repo<FsStore>, server: &Server) -> Option<Vec<u8>> {
    let ad = server.advertise("upload-pack");
    match repo.fetch_request(&ad).unwrap() {
        FetchRequest::HaveAll => {
            repo.fetch_finish(&ad, None).unwrap();
            None
        }
        FetchRequest::Want(body) => {
            let response = server.post("upload-pack", &body);
            repo.fetch_finish(&ad, Some(&response)).unwrap();
            Some(response)
        }
    }
}

/// Integrates every branch, and gives `main`'s outcome; each other
/// branch's is in [`integrate_all`].
fn integrate(repo: &mut Repo<FsStore>, prefer: Option<Side>, at: i64) -> Integrated {
    integrate_all(repo, prefer, at)
        .into_iter()
        .find(|(b, _)| *b == main_branch())
        .expect("main always has an outcome")
        .1
}

fn integrate_all(
    repo: &mut Repo<FsStore>,
    prefer: Option<Side>,
    at: i64,
) -> Vec<(BranchName, Integrated)> {
    repo.integrate(&sig(at), prefer, &mut by_lines).unwrap()
}

fn branch(name: &str) -> BranchName {
    BranchName::new(name).unwrap()
}

/// One commit on `name`, a branch of its own.
fn commit_on(
    repo: &mut Repo<FsStore>,
    name: &str,
    files: &[(&str, &str)],
    message: &str,
    at: i64,
) -> String {
    let b = branch(name);
    let tip = repo.branch_tip(&b).unwrap();
    repo.commit(
        &b,
        tip.map_or(Tip::Unborn, Tip::At),
        &WriteCommit {
            changes: files
                .iter()
                .map(|(p, t)| Change::Put {
                    path: path(p),
                    bytes: t.as_bytes().to_vec(),
                })
                .collect(),
            message: format!("{message}\n"),
            author: sig(at),
            committer: sig(at),
        },
    )
    .unwrap()
    .to_hex()
}

/// Pushes; the outcome's rejected refs, or `None` when there was nothing
/// to push or the remote is ahead.
fn push(repo: &mut Repo<FsStore>, server: &Server) -> Result<Vec<String>, PushRequest> {
    let ad = server.advertise("receive-pack");
    match repo.push_request(&ad).unwrap() {
        PushRequest::Send(body) => {
            let response = server.post("receive-pack", &body);
            let pushed = repo.push_finish(&body, &response).unwrap();
            eprintln!(
                "pushed {:?}, rejected {:?}",
                pushed.updated, pushed.rejected
            );
            Ok(pushed.rejected.into_iter().map(|(r, _)| r).collect())
        }
        other => Err(other),
    }
}

fn fsck(dir: &Path) {
    let out = git(dir, &["fsck", "--strict", "--full", "--no-dangling"]);
    assert!(out.is_empty(), "fsck: {out}");
}

/// A deck large enough, and similar enough between versions, that git
/// stores its later versions as deltas.
fn big_deck(version: usize) -> String {
    let mut s = String::from("[deck]\nname = \"Lantern\"\n\n");
    for i in 0..600 {
        let qty = if i % 97 == version % 97 {
            version + 2
        } else {
            1
        };
        s.push_str(&format!(
            "[[cards]]\nname = \"Card number {i}\"\nqty = {qty}\n\n"
        ));
    }
    s
}

/// The working clone a friend pushes from, and the bare server.
fn setup() -> (tempfile::TempDir, Server, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("server.git");
    let work = dir.path().join("work");
    git(
        dir.path(),
        &["init", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(dir.path(), &["init", "-b", "main", work.to_str().unwrap()]);
    git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
    (dir, Server(bare), work)
}

fn friend_commits(work: &Path, files: &[(&str, &str)], message: &str) {
    for (p, t) in files {
        let at = work.join(p);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, t).unwrap();
        git(work, &["add", p]);
    }
    git(work, &["commit", "-q", "-m", message]);
    git(work, &["push", "-q", "origin", "main", "--tags"]);
}

fn browser(dir: &Path, name: &str) -> (Repo<FsStore>, PathBuf) {
    browser_at(dir, name, 1_760_000_000)
}

/// Two browsers' first commits differ by when they were made.
fn browser_at(dir: &Path, name: &str, at: i64) -> (Repo<FsStore>, PathBuf) {
    let gitdir = dir.join(name);
    let mut repo = Repo::init(FsStore::new(&gitdir)).unwrap();
    repo.set_remote(&RemoteUrl::new("https://example.com/friend/mtg.git").unwrap())
        .unwrap();
    // A fresh browser repo is one VERSION commit (openMagicRepo writes it).
    commit(
        &mut repo,
        &[("VERSION", "1\n")],
        "curator: this repo is at version 1",
        at,
    );
    (repo, gitdir)
}

/// The pack inside upload-pack's side-band answer, checked by git itself.
fn delta_count(dir: &Path, response: &[u8]) -> usize {
    let mut pack = Vec::new();
    let mut at = 0;
    let mut in_pack = false;
    while at + 4 <= response.len() {
        let len =
            usize::from_str_radix(std::str::from_utf8(&response[at..at + 4]).unwrap(), 16).unwrap();
        if len == 0 {
            if in_pack {
                break;
            }
            at += 4;
            continue;
        }
        let payload = &response[at + 4..at + len];
        if payload.first() == Some(&1) {
            in_pack = true;
            pack.extend_from_slice(&payload[1..]);
        }
        at += len;
    }
    let file = dir.join("fetched.pack");
    std::fs::write(&file, &pack).unwrap();
    git(dir, &["index-pack", file.to_str().unwrap()]);
    let verify = git(
        dir,
        &[
            "verify-pack",
            "-v",
            dir.join("fetched.idx").to_str().unwrap(),
        ],
    );
    eprintln!(
        "{}",
        verify
            .lines()
            .filter(|l| l.contains("chain length") || l.starts_with("non delta"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    verify
        .lines()
        .filter(|l| l.split_whitespace().count() == 7)
        .count()
}

#[test]
fn a_clone_resolves_deltas_and_fetches_only_what_is_new() {
    if !have_git() {
        return;
    }
    let (dir, server, work) = setup();
    for v in 0..6 {
        friend_commits(
            &work,
            &[
                ("VERSION", "1\n"),
                ("decks/lantern.deck.toml", &big_deck(v)),
            ],
            &format!("lantern: v{v}"),
        );
    }
    git(
        &work,
        &["tag", "-a", "decks/lantern/fnm", "-m", "Friday night"],
    );
    git(&work, &["push", "-q", "origin", "--tags"]);
    git(&server.0, &["gc", "-q", "--aggressive"]);

    let (mut repo, gitdir) = browser(dir.path(), "browser.git");
    let response = fetch(&mut repo, &server).expect("a fresh repo wants everything");
    let deltas = delta_count(dir.path(), &response);
    eprintln!("the pack upload-pack sent holds {deltas} deltified objects");
    assert!(deltas > 0, "git should have deltified the deck's versions");

    let outcome = integrate(&mut repo, None, 1_760_000_100);
    eprintln!("integrate: {outcome:?}");
    assert!(matches!(outcome, Integrated::Adopted { .. }));
    let tip = repo.branch_tip(&main_branch()).unwrap().unwrap();
    assert_eq!(tip.to_hex(), server.rev("main"));
    assert_eq!(
        repo.read_file(&tip, &path("decks/lantern.deck.toml"))
            .unwrap()
            .unwrap(),
        big_deck(5).into_bytes()
    );
    let tags = repo.tags("decks/").unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].commit.to_hex(), server.rev("main"));
    fsck(&gitdir);
    eprintln!("refs here:\n{}", git(&gitdir, &["for-each-ref"]));

    // Nothing new: no POST at all.
    assert!(fetch(&mut repo, &server).is_none());
    assert_eq!(integrate(&mut repo, None, 1), Integrated::UpToDate);

    // One new commit: the pack holds that commit's objects and nothing older.
    friend_commits(
        &work,
        &[("decks/lantern.deck.toml", &big_deck(6))],
        "lantern: v6",
    );
    let before = std::fs::read_dir(gitdir.join("objects")).unwrap().count();
    let response = fetch(&mut repo, &server).unwrap();
    let count = u32::from_be_bytes(
        response[response.windows(4).position(|w| w == b"PACK").unwrap() + 8..][..4]
            .try_into()
            .unwrap(),
    );
    eprintln!("incremental fetch: pack of {count} objects; object dirs before {before}");
    assert_eq!(
        count, 4,
        "a commit, its tree, its decks tree, and the one changed blob"
    );
    let outcome = integrate(&mut repo, None, 2);
    assert!(
        matches!(outcome, Integrated::FastForwarded { .. }),
        "{outcome:?}"
    );
    assert_eq!(
        repo.branch_tip(&main_branch()).unwrap().unwrap().to_hex(),
        server.rev("main")
    );
    fsck(&gitdir);
    server.fsck();
}

#[test]
fn a_push_fast_forwards_and_a_moved_remote_refuses_it_until_merged() {
    if !have_git() {
        return;
    }
    let (dir, server, work) = setup();
    friend_commits(
        &work,
        &[
            ("VERSION", "1\n"),
            ("decks/lantern.deck.toml", "a\nb\nc\nd\n"),
            ("collection.toml", "x\n"),
        ],
        "start",
    );
    let (mut repo, gitdir) = browser(dir.path(), "browser.git");
    fetch(&mut repo, &server);
    assert!(matches!(
        integrate(&mut repo, None, 10),
        Integrated::Adopted { .. }
    ));

    // Fast-forward, with a snapshot tag going along.
    let mine = commit(
        &mut repo,
        &[("decks/lantern.deck.toml", "A\nb\nc\nd\n")],
        "lantern: A",
        20,
    );
    let tip = repo.branch_tip(&main_branch()).unwrap().unwrap();
    repo.create_tag(
        &TagName::new("decks/lantern/fnm").unwrap(),
        &tip,
        &sig(21),
        "fnm\n",
    )
    .unwrap();
    assert_eq!(repo.remote_status().unwrap().ahead, 1);
    assert_eq!(repo.remote_status().unwrap().tags, 1);
    assert_eq!(repo.remote_status().unwrap().branches, 1);
    assert_eq!(push(&mut repo, &server), Ok(vec![]));
    assert_eq!(server.rev("main"), mine);
    assert_eq!(server.rev("decks/lantern/fnm^{commit}"), mine);
    let status = repo.remote_status().unwrap();
    assert_eq!((status.ahead, status.tags), (0, 0));
    server.fsck();
    assert_eq!(push(&mut repo, &server), Err(PushRequest::Nothing));

    // The browser commits; a friend pushes before it does. The browser's
    // push names the main it saw advertised as the old id, so receive-pack
    // refuses it: the compare-and-swap.
    commit(
        &mut repo,
        &[("decks/lantern.deck.toml", "A\nb\nc\nD\n")],
        "lantern: D",
        30,
    );
    let stale_ad = server.advertise("receive-pack");
    let PushRequest::Send(body) = repo.push_request(&stale_ad).unwrap() else {
        panic!("there is a commit to push");
    };
    git(&work, &["pull", "-q", "--ff-only", "origin", "main"]);
    friend_commits(&work, &[("collection.toml", "x\ny\n")], "collection: y");
    let response = server.post("receive-pack", &body);
    let pushed = repo.push_finish(&body, &response).unwrap();
    eprintln!("stale push: {pushed:?}");
    assert_eq!(pushed.rejected.len(), 1);
    assert_eq!(pushed.rejected[0].0, "refs/heads/main");
    assert_ne!(
        server.rev("main"),
        repo.branch_tip(&main_branch()).unwrap().unwrap().to_hex()
    );

    // Asked again, the push says to fetch first; fetch, merge, push.
    assert_eq!(push(&mut repo, &server), Err(PushRequest::Behind));
    fetch(&mut repo, &server);
    let outcome = integrate(&mut repo, None, 40);
    eprintln!("integrate after a rejected push: {outcome:?}");
    let Integrated::Merged { commit, taken } = outcome else {
        panic!("both moved, on different files");
    };
    assert_eq!(taken, vec!["collection.toml".to_owned()]);
    assert_eq!(push(&mut repo, &server), Ok(vec![]));
    assert_eq!(server.rev("main"), commit.to_hex());
    assert_eq!(
        git(&server.0, &["rev-parse", "main^1", "main^2"])
            .lines()
            .count(),
        2
    );
    assert_eq!(git(&server.0, &["show", "main:collection.toml"]), "x\ny");
    assert_eq!(
        git(&server.0, &["show", "main:decks/lantern.deck.toml"]),
        "A\nb\nc\nD"
    );
    eprintln!(
        "server log:\n{}",
        git(
            &server.0,
            &["log", "--graph", "--oneline", "--decorate", "main"]
        )
    );
    server.fsck();
    fsck(&gitdir);
}

#[test]
fn the_same_lines_conflict_until_a_side_is_chosen() {
    if !have_git() {
        return;
    }
    let (dir, server, work) = setup();
    friend_commits(
        &work,
        &[("VERSION", "1\n"), ("decks/a.deck.toml", "a\nb\nc\n")],
        "start",
    );
    let (mut repo, gitdir) = browser(dir.path(), "browser.git");
    fetch(&mut repo, &server);
    integrate(&mut repo, None, 1);
    commit(
        &mut repo,
        &[("decks/a.deck.toml", "a\nMINE\nc\n")],
        "mine",
        2,
    );
    friend_commits(&work, &[("decks/a.deck.toml", "a\nTHEIRS\nc\n")], "theirs");
    fetch(&mut repo, &server);
    let before = repo.branch_tip(&main_branch()).unwrap();
    assert_eq!(
        integrate(&mut repo, None, 3),
        Integrated::Conflict {
            paths: vec!["decks/a.deck.toml".to_owned()]
        }
    );
    assert_eq!(
        repo.branch_tip(&main_branch()).unwrap(),
        before,
        "nothing moved"
    );
    let Integrated::Merged { commit, .. } = integrate(&mut repo, Some(Side::Theirs), 4) else {
        panic!("taking theirs merges");
    };
    assert_eq!(
        repo.read_file(&commit, &path("decks/a.deck.toml"))
            .unwrap()
            .unwrap(),
        b"a\nTHEIRS\nc\n"
    );
    push(&mut repo, &server).unwrap();
    server.fsck();
    fsck(&gitdir);
}

#[test]
fn an_empty_remote_takes_a_push_and_an_unrelated_one_is_refused() {
    if !have_git() {
        return;
    }
    let (dir, server, work) = setup();
    let (mut repo, gitdir) = browser(dir.path(), "browser.git");
    commit(&mut repo, &[("decks/a.deck.toml", "mine\n")], "a deck", 2);
    assert!(fetch(&mut repo, &server).is_none());
    assert_eq!(integrate(&mut repo, None, 3), Integrated::RemoteEmpty);
    assert_eq!(push(&mut repo, &server), Ok(vec![]));
    assert_eq!(
        server.rev("main"),
        repo.branch_tip(&main_branch()).unwrap().unwrap().to_hex()
    );
    server.fsck();

    // Another browser with its own history meets that remote.
    let (mut other, other_dir) = browser_at(dir.path(), "other.git", 1_760_000_500);
    commit(
        &mut other,
        &[("decks/b.deck.toml", "theirs\n")],
        "b deck",
        5,
    );
    fetch(&mut other, &server);
    let before = other.branch_tip(&main_branch()).unwrap();
    assert_eq!(integrate(&mut other, None, 6), Integrated::Unrelated);
    assert_eq!(other.branch_tip(&main_branch()).unwrap(), before);
    assert_eq!(
        other
            .push_request(&server.advertise("receive-pack"))
            .unwrap(),
        PushRequest::Behind
    );
    other.adopt_remote().unwrap();
    assert_eq!(
        other.branch_tip(&main_branch()).unwrap().unwrap().to_hex(),
        server.rev("main")
    );
    fsck(&other_dir);
    fsck(&gitdir);

    // Removing the remote forgets what was tracked, and nothing else.
    other.remove_remote().unwrap();
    assert!(other.remote().unwrap().is_none());
    assert_eq!(
        git(&other_dir, &["for-each-ref", "--format=%(refname)"]),
        "refs/heads/main"
    );
    let _ = work;
}

#[test]
fn ref_deltas_resolve_and_a_pack_without_side_band_reads() {
    if !have_git() {
        return;
    }
    let (dir, server, work) = setup();
    for v in 0..4 {
        friend_commits(
            &work,
            &[
                ("VERSION", "1\n"),
                ("decks/lantern.deck.toml", &big_deck(v)),
            ],
            &format!("lantern: v{v}"),
        );
    }
    // Without --delta-base-offset, pack-objects names each delta's base by
    // id (REF_DELTA), as an old server or a dumb relay might send.
    let pack = git_stdin(
        &server.0,
        &["pack-objects", "--stdout", "--revs", "-q"],
        b"main\n",
    );
    let file = dir.path().join("ref.pack");
    std::fs::write(&file, &pack).unwrap();
    git(dir.path(), &["index-pack", file.to_str().unwrap()]);
    let verify = git(
        dir.path(),
        &[
            "verify-pack",
            "-v",
            dir.path().join("ref.idx").to_str().unwrap(),
        ],
    );
    let deltas = verify
        .lines()
        .filter(|l| l.split_whitespace().count() == 7)
        .count();
    eprintln!("pack-objects made {deltas} ref-deltas");
    assert!(deltas > 0);

    let mut response = b"0008NAK\n".to_vec();
    response.extend(pack);
    let (mut repo, gitdir) = browser(dir.path(), "browser.git");
    let ad = server.advertise("upload-pack");
    assert!(matches!(
        repo.fetch_request(&ad).unwrap(),
        FetchRequest::Want(_)
    ));
    repo.fetch_finish(&ad, Some(&response)).unwrap();
    assert!(matches!(
        integrate(&mut repo, None, 9),
        Integrated::Adopted { .. }
    ));
    let tip = repo.branch_tip(&main_branch()).unwrap().unwrap();
    assert_eq!(
        repo.read_file(&tip, &path("decks/lantern.deck.toml"))
            .unwrap()
            .unwrap(),
        big_deck(3).into_bytes()
    );
    fsck(&gitdir);
}

/// Every branch and tag syncs (ADR-0041: a deck is a branch, a snapshot a
/// tag): each side's made, moved and deleted refs reach the other, a branch
/// both moved merges, and receive-pack's compare-and-swap guards each one.
#[test]
fn every_branch_and_tag_syncs_both_ways() {
    if !have_git() {
        return;
    }
    let (dir, server, work) = setup();
    friend_commits(&work, &[("VERSION", "2\n")], "start");
    let (mut repo, gitdir) = browser(dir.path(), "browser.git");
    fetch(&mut repo, &server);
    integrate(&mut repo, None, 10);

    // Two decks and a snapshot, made here, go out as new refs.
    let a1 = commit_on(
        &mut repo,
        "decks/a/main",
        &[("a.deck.toml", "1\n2\n3\n")],
        "a: made",
        20,
    );
    let b1 = commit_on(
        &mut repo,
        "decks/b/main",
        &[("b.deck.toml", "b\n")],
        "b: made",
        21,
    );
    let a1_id = repo.branch_tip(&branch("decks/a/main")).unwrap().unwrap();
    repo.create_tag(
        &TagName::new("decks/a/fnm").unwrap(),
        &a1_id,
        &sig(22),
        "FNM\n",
    )
    .unwrap();
    let status = repo.remote_status().unwrap();
    assert_eq!((status.ahead, status.branches, status.tags), (2, 2, 1));
    assert_eq!(push(&mut repo, &server), Ok(vec![]));
    assert_eq!(server.rev("decks/a/main"), a1);
    assert_eq!(server.rev("decks/b/main"), b1);
    assert_eq!(server.rev("decks/a/fnm^{commit}"), a1);
    let status = repo.remote_status().unwrap();
    assert_eq!((status.ahead, status.branches, status.tags), (0, 0, 0));
    server.fsck();

    // A friend moves a, makes c and a snapshot of c; this repo takes all
    // three on fetch, before integrating anything.
    git(&work, &["fetch", "-q", "origin"]);
    git(
        &work,
        &[
            "checkout",
            "-q",
            "-b",
            "decks/a/main",
            "origin/decks/a/main",
        ],
    );
    friend_commits_on(
        &work,
        "decks/a/main",
        &[("a.deck.toml", "1\n2\n3\n4\n")],
        "a: +4",
    );
    git(&work, &["checkout", "-q", "--orphan", "decks/c/main"]);
    git(&work, &["rm", "-rfq", "."]);
    friend_commits_on(&work, "decks/c/main", &[("c.deck.toml", "c\n")], "c: made");
    git(&work, &["tag", "-a", "decks/c/gp", "-m", "GP"]);
    git(&work, &["push", "-q", "origin", "decks/c/gp"]);
    fetch(&mut repo, &server);
    assert_eq!(
        repo.branch_tip(&branch("decks/a/main"))
            .unwrap()
            .unwrap()
            .to_hex(),
        server.rev("decks/a/main"),
        "moved only there: a fast-forward"
    );
    assert_eq!(
        repo.branch_tip(&branch("decks/c/main"))
            .unwrap()
            .unwrap()
            .to_hex(),
        server.rev("decks/c/main"),
        "made only there: made here"
    );
    assert_eq!(repo.tags("decks/c/").unwrap().len(), 1);
    let outcomes = integrate_all(&mut repo, None, 30);
    assert!(
        outcomes.iter().all(|(_, o)| *o == Integrated::UpToDate),
        "{outcomes:?}"
    );

    // Both move a, on different lines: a merge commit on a, and only a.
    commit_on(
        &mut repo,
        "decks/a/main",
        &[("a.deck.toml", "ONE\n2\n3\n4\n")],
        "a: ONE",
        40,
    );
    friend_commits_on(
        &work,
        "decks/a/main",
        &[("a.deck.toml", "1\n2\n3\n4\nFIVE\n")],
        "a: +FIVE",
    );
    assert_eq!(push(&mut repo, &server), Err(PushRequest::Behind));
    fetch(&mut repo, &server);
    let outcomes = integrate_all(&mut repo, None, 41);
    eprintln!("both moved a: {outcomes:?}");
    let a_merged = outcomes
        .iter()
        .find(|(b, _)| b.as_str() == "decks/a/main")
        .unwrap();
    let Integrated::Merged { commit, taken } = &a_merged.1 else {
        panic!("a merges: {outcomes:?}");
    };
    assert_eq!(taken, &["a.deck.toml".to_owned()]);
    assert_eq!(
        repo.read_commit(commit).unwrap().message,
        "sync: merge origin/decks/a/main\n\na.deck.toml\n"
    );
    assert_eq!(
        repo.read_file(commit, &path("a.deck.toml"))
            .unwrap()
            .unwrap(),
        b"ONE\n2\n3\n4\nFIVE\n"
    );

    // This repo deletes b and moves a's snapshot; the friend deletes c.
    let b = repo.branch_tip(&branch("decks/b/main")).unwrap().unwrap();
    repo.delete_branch(&branch("decks/b/main"), &b).unwrap();
    repo.retarget_tag(&TagName::new("decks/a/fnm").unwrap(), commit)
        .unwrap();
    assert_eq!(push(&mut repo, &server), Ok(vec![]));
    assert!(git(&server.0, &["branch", "--list", "decks/b/main"]).is_empty());
    assert_eq!(server.rev("decks/a/fnm^{commit}"), commit.to_hex());
    assert_eq!(server.rev("decks/a/main"), commit.to_hex());
    git(&work, &["push", "-q", "origin", "--delete", "decks/c/main"]);
    fetch(&mut repo, &server);
    assert_eq!(
        repo.branch_tip(&branch("decks/c/main")).unwrap(),
        None,
        "deleted there and untouched here: deleted here"
    );
    assert!(repo.branch_tip(&branch("decks/b/main")).unwrap().is_none());
    assert_eq!(push(&mut repo, &server), Err(PushRequest::Nothing));
    eprintln!(
        "server refs:\n{}\nlog:\n{}",
        git(&server.0, &["for-each-ref"]),
        git(
            &server.0,
            &["log", "--all", "--graph", "--oneline", "--decorate"]
        )
    );
    server.fsck();
    fsck(&gitdir);
}

fn friend_commits_on(work: &Path, branch: &str, files: &[(&str, &str)], message: &str) {
    if git(work, &["branch", "--show-current"]) != branch {
        git(work, &["checkout", "-q", branch]);
    }
    for (p, t) in files {
        std::fs::write(work.join(p), t).unwrap();
        git(work, &["add", p]);
    }
    git(work, &["commit", "-q", "-m", message]);
    git(work, &["push", "-q", "origin", branch]);
}

/// A store that takes only `left` more object writes and then fails, as
/// the browser's storage keeps only the first of a fetch's writes when its
/// tab closes mid-flush.
struct CutShort {
    inner: FsStore,
    left: usize,
}

impl meldweb_git::Store for CutShort {
    type Error = std::io::Error;

    fn read(&self, path: &str) -> std::io::Result<Option<Vec<u8>>> {
        self.inner.read(path)
    }

    fn write(&mut self, path: &str, bytes: &[u8]) -> std::io::Result<()> {
        if path.starts_with("objects/") {
            if self.left == 0 {
                return Err(std::io::Error::other("the tab closed"));
            }
            self.left -= 1;
        }
        self.inner.write(path, bytes)
    }

    fn remove(&mut self, path: &str) -> std::io::Result<bool> {
        self.inner.remove(path)
    }

    fn list(&self, dir: &str) -> std::io::Result<Vec<String>> {
        self.inner.list(dir)
    }

    fn create_dir(&mut self, dir: &str) -> std::io::Result<()> {
        self.inner.create_dir(dir)
    }
}

/// The friend's history the next two tests fetch: six commits of a deck,
/// deltified as a host packs it.
fn six_versions() -> (tempfile::TempDir, Server) {
    let (dir, server, work) = setup();
    for v in 0..6 {
        friend_commits(
            &work,
            &[
                ("VERSION", "1\n"),
                ("decks/lantern.deck.toml", &big_deck(v)),
            ],
            &format!("lantern: v{v}"),
        );
    }
    git(&server.0, &["gc", "-q", "--aggressive"]);
    (dir, server)
}

/// The fetch after one cut short wants the remote's history again and
/// ends in it whole, however many of the first fetch's objects were kept.
fn refetch_is_whole(repo: &mut Repo<FsStore>, gitdir: &Path, server: &Server) {
    repo.remote_status().unwrap();
    assert!(
        fetch(repo, server).is_some(),
        "a tip without its history is not had"
    );
    repo.remote_status().unwrap();
    assert!(matches!(
        integrate(repo, None, 9),
        Integrated::Adopted { .. }
    ));
    assert_eq!(
        repo.branch_tip(&main_branch()).unwrap().unwrap().to_hex(),
        server.rev("main")
    );
    fsck(gitdir);
}

#[test]
fn a_fetch_cut_short_never_keeps_a_commit_without_its_history() {
    if !have_git() {
        return;
    }
    let (dir, server) = six_versions();
    let ad = server.advertise("upload-pack");
    for kept in 0.. {
        let name = format!("cut-{kept}.git");
        let (_, gitdir) = browser(dir.path(), &name);
        let mut cut = Repo::open(CutShort {
            inner: FsStore::new(&gitdir),
            left: kept,
        })
        .unwrap();
        let FetchRequest::Want(body) = cut.fetch_request(&ad).unwrap() else {
            panic!("a fresh repo wants everything");
        };
        let response = server.post("upload-pack", &body);
        if cut.fetch_finish(&ad, Some(&response)).is_ok() {
            eprintln!("every fetch cut short after 0..{kept} objects synced whole");
            break;
        }
        let mut repo = Repo::open(FsStore::new(&gitdir)).unwrap();
        refetch_is_whole(&mut repo, &gitdir, &server);
    }
}

/// A browser whose storage already lost a commit under the tracked tip, as
/// one did before fetches wrote history parents first, syncs whole again.
#[test]
fn a_tracked_tip_missing_history_is_fetched_again() {
    if !have_git() {
        return;
    }
    let (dir, server) = six_versions();
    let (mut repo, gitdir) = browser(dir.path(), "browser.git");
    fetch(&mut repo, &server).unwrap();
    let lost = server.rev("main~2");
    std::fs::remove_file(gitdir.join("objects").join(&lost[..2]).join(&lost[2..])).unwrap();
    refetch_is_whole(&mut repo, &gitdir, &server);
}
