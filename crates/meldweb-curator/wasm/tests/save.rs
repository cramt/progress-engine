//! A save (ADR-0021) against the real `git`: the editor's text, made from
//! the file as it read it, committed in one call that merges by card when
//! the deck's line moved meanwhile, such as by a sync's merge. Two browsers
//! sync through a bare repo served by `git upload-pack` and `git
//! receive-pack --stateless-rpc`, as a smart HTTP host answers, and every
//! repo passes `git fsck --strict` at the end.
//!
//! Without `git` on PATH these print `SKIPPED` and pass, unless
//! `MELDWEB_GIT_REQUIRE` is set.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use meldweb_git::{
    BranchName, Change, CommitId, FetchRequest, FsStore, Identity, Integrated, PushRequest,
    RemoteUrl, Repo, RepoPath, Signature, Timestamp, Tip, WriteCommit,
};
use meldweb_wasm::layout;
use meldweb_wasm::merging::{
    Conflict, FileConflict, FileSettle, Settle, Side, SyncMerging, SyncSettle,
};
use meldweb_wasm::saving::{self, Made, Moved, SaveAsk, Saved};

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
}

fn sig(name: &str, seconds: i64) -> Signature {
    Signature {
        who: Identity::new(name, format!("{}@example.com", name.to_lowercase())).unwrap(),
        when: Timestamp::new(seconds, 0).unwrap(),
    }
}

fn p(path: &str) -> RepoPath {
    RepoPath::new(path).unwrap()
}

const LINE: &str = "decks/lantern/main";
const DECK: &str = "lantern.deck.toml";
const CRITERIA: &str = "lantern.criteria.toml";

fn line() -> BranchName {
    BranchName::new(LINE).unwrap()
}

fn commit(repo: &mut Repo<FsStore>, on: &str, puts: &[(&str, &str)], who: &Signature) {
    let b = BranchName::new(on).unwrap();
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
            message: "edit\n".into(),
            author: who.clone(),
            committer: who.clone(),
        },
    )
    .unwrap();
}

fn deck(cards: &[(&str, u32)]) -> String {
    let mut s = String::from("name = \"Lantern\"\nformat = \"commander\"\ncards = [\n");
    for (c, qty) in cards {
        match qty {
            1 => s.push_str(&format!("  {{ name = \"{c}\" }},\n")),
            n => s.push_str(&format!("  {{ name = \"{c}\", qty = {n} }},\n")),
        }
    }
    s.push_str("]\n");
    s
}

/// Lantern with `sol` Sol Rings and `stone` Mind Stones, side by side.
fn lantern(sol: u32, stone: u32) -> String {
    deck(&[
        ("Lantern of Insight", 1),
        ("Sol Ring", sol),
        ("Mind Stone", stone),
    ])
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

fn push(repo: &mut Repo<FsStore>, server: &Server) {
    let ad = server.advertise("receive-pack");
    if let PushRequest::Send(body) = repo.push_request(&ad).unwrap() {
        let response = server.post("receive-pack", &body);
        let pushed = repo.push_finish(&body, &response).unwrap();
        assert!(pushed.rejected.is_empty(), "{:?}", pushed.rejected);
    }
}

/// A sync as the browser's makes one: every branch both moved merged, a
/// deck by card.
fn sync(repo: &mut Repo<FsStore>, at: i64) -> Vec<(BranchName, Integrated)> {
    let settle = SyncSettle::default();
    let mut merging = SyncMerging {
        settle: &settle,
        stops: BTreeMap::new(),
    };
    layout::integrate(repo, &sig("Bo", at), None, &mut merging).unwrap()
}

fn tip(repo: &Repo<FsStore>) -> CommitId {
    repo.branch_tip(&line()).unwrap().unwrap()
}

fn text_at(repo: &Repo<FsStore>, at: &CommitId, path: &str) -> String {
    String::from_utf8(repo.read_file(at, &p(path)).unwrap().unwrap()).unwrap()
}

/// Alex's browser, which made the deck and pushed it, and Bo's, which
/// fetched it, both with the server as their remote.
fn two_browsers(dir: &Path) -> (Server, Repo<FsStore>, Repo<FsStore>) {
    let server = Server::new(dir);
    let url = RemoteUrl::new("https://example.com/alex/mtg.git").unwrap();
    let mut alex = Repo::init(FsStore::new(dir.join("alex.git"))).unwrap();
    commit(&mut alex, "main", &[("VERSION", "2\n")], &sig("Alex", 1));
    commit(
        &mut alex,
        LINE,
        &[(DECK, &lantern(1, 1)), (CRITERIA, "# none yet\n")],
        &sig("Alex", 2),
    );
    alex.set_remote(&url).unwrap();
    fetch(&mut alex, &server);
    push(&mut alex, &server);
    let mut bo = Repo::init(FsStore::new(dir.join("bo.git"))).unwrap();
    bo.set_remote(&url).unwrap();
    fetch(&mut bo, &server);
    sync(&mut bo, 3);
    assert_eq!(text_at(&bo, &tip(&bo), DECK), lantern(1, 1));
    (server, alex, bo)
}

fn ask(text: &str, base: &str, moved: Moved) -> SaveAsk {
    SaveAsk {
        path: DECK.into(),
        branch: Some(LINE.into()),
        text: text.into(),
        made: Made::From {
            base: Some(base.into()),
            moved,
        },
    }
}

fn merging(settle: Option<Settle>) -> Moved {
    Moved::Merge { settle }
}

#[test]
fn a_save_whose_deck_line_a_sync_merged_merges_by_card_and_commits_in_one_call() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (server, mut alex, mut bo) = two_browsers(dir.path());

    // Alex runs two Sol Rings; Bo's own line moves too, in its criteria, so
    // the sync that brings Alex's change in is a merge, not a fast-forward.
    commit(&mut alex, LINE, &[(DECK, &lantern(2, 1))], &sig("Alex", 10));
    push(&mut alex, &server);
    commit(
        &mut bo,
        LINE,
        &[(CRITERIA, "# by turn 3\n")],
        &sig("Bo", 11),
    );
    fetch(&mut bo, &server);
    let synced = sync(&mut bo, 12);
    let outcome = &synced.iter().find(|(b, _)| b.as_str() == LINE).unwrap().1;
    assert!(matches!(outcome, Integrated::Merged { .. }), "{synced:?}");
    let sync_merge = tip(&bo);
    assert_eq!(bo.read_commit(&sync_merge).unwrap().parents.len(), 2);

    // Bo's editor read the deck before the sync, and now runs three Mind
    // Stones, on the line beside Sol Ring: by lines that clashes.
    let saved = saving::save(
        &mut bo,
        &ask(&lantern(1, 3), &lantern(1, 1), merging(None)),
        &sig("Bo", 13),
    )
    .unwrap();
    println!("{saved:?}");
    let Saved::Committed {
        merged: Some(merged),
        sha,
        commit,
    } = saved
    else {
        panic!("not committed as a merge: {saved:?}")
    };
    assert_eq!(merged, lantern(2, 3));
    let made = CommitId::from_hex(&commit).unwrap();
    assert_eq!(tip(&bo), made);
    assert_eq!(text_at(&bo, &made, DECK), lantern(2, 3));
    assert_eq!(text_at(&bo, &made, CRITERIA), "# by turn 3\n");
    let info = bo.read_commit(&made).unwrap();
    assert_eq!(info.parents, vec![sync_merge], "one commit on the sync");
    // What Bo changed, over the file as it is now.
    assert_eq!(info.message, "lantern: Mind Stone: 1 → 3\n");
    assert_eq!(
        git(
            &dir.path().join("bo.git"),
            &["rev-parse", &format!("{commit}:{DECK}")]
        ),
        sha
    );

    // Saving goes on from the merge: unmoved, it is one plain commit.
    let next = saving::save(
        &mut bo,
        &ask(&lantern(2, 4), &merged, merging(None)),
        &sig("Bo", 14),
    )
    .unwrap();
    assert!(
        matches!(next, Saved::Committed { merged: None, .. }),
        "{next:?}"
    );
    push(&mut bo, &server);
    fsck(&dir.path().join("bo.git"));
    fsck(&server.0);
}

fn clash_of(saved: &Saved) -> &Conflict {
    match saved {
        Saved::Clashed { conflict } => conflict,
        other => panic!("not a clash: {other:?}"),
    }
}

fn theirs_of(conflict: &Conflict) -> Vec<String> {
    match &conflict.files[..] {
        [FileConflict::EachClash { clashes, .. }] => {
            clashes.iter().map(|c| c.theirs.clone()).collect()
        }
        other => panic!("{other:?}"),
    }
}

fn picking(conflict: &Conflict, side: Side) -> Settle {
    Settle {
        guard: conflict.guard.clone(),
        prefer: None,
        files: vec![FileSettle::EachClash {
            path: DECK.into(),
            picks: vec![side],
        }],
    }
}

#[test]
fn a_pick_against_a_saves_clash_is_refused_once_the_file_moved_again() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (server, mut alex, mut bo) = two_browsers(dir.path());
    let before = lantern(1, 1);
    let mine = lantern(1, 3);

    // Both change Mind Stone: Alex to 5, synced in; Bo's editor to 3.
    commit(&mut alex, LINE, &[(DECK, &lantern(1, 5))], &sig("Alex", 10));
    push(&mut alex, &server);
    fetch(&mut bo, &server);
    sync(&mut bo, 11);
    let at = tip(&bo);
    let first = saving::save(&mut bo, &ask(&mine, &before, merging(None)), &sig("Bo", 12)).unwrap();
    let shown = clash_of(&first).clone();
    assert_eq!(theirs_of(&shown), ["5"]);
    assert_eq!(tip(&bo), at, "a clash commits nothing");

    // Alex moves it again, to 7, before Bo picks: Bo's pick of their side
    // was made against 5 and settles nothing.
    commit(&mut alex, LINE, &[(DECK, &lantern(1, 7))], &sig("Alex", 13));
    push(&mut alex, &server);
    fetch(&mut bo, &server);
    sync(&mut bo, 14);
    let moved = tip(&bo);
    let stale = saving::save(
        &mut bo,
        &ask(&mine, &before, merging(Some(picking(&shown, Side::Theirs)))),
        &sig("Bo", 15),
    )
    .unwrap();
    let again = clash_of(&stale).clone();
    assert_eq!(theirs_of(&again), ["7"], "shown anew, as it is now");
    assert_ne!(again.guard, shown.guard);
    assert_eq!(tip(&bo), moved);

    // So is a pick once the editor's own text moved on.
    let edited = lantern(1, 2);
    let mine_moved = saving::save(
        &mut bo,
        &ask(&edited, &before, merging(Some(picking(&again, Side::Ours)))),
        &sig("Bo", 16),
    )
    .unwrap();
    assert_eq!(
        theirs_of(clash_of(&mine_moved)),
        ["7"],
        "a pick for 3 is not one for 2"
    );

    // Picked against what is there now, it settles it.
    let settled = saving::save(
        &mut bo,
        &ask(&mine, &before, merging(Some(picking(&again, Side::Ours)))),
        &sig("Bo", 17),
    )
    .unwrap();
    let Saved::Committed {
        merged: Some(merged),
        ..
    } = &settled
    else {
        panic!("{settled:?}")
    };
    assert_eq!(merged, &lantern(1, 3));
    assert_eq!(text_at(&bo, &tip(&bo), DECK), lantern(1, 3));
    fsck(&dir.path().join("bo.git"));
}

#[test]
fn a_save_that_does_not_merge_stops_when_the_file_moved_and_overwrite_wins() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (_server, _alex, mut bo) = two_browsers(dir.path());
    commit(&mut bo, LINE, &[(DECK, &lantern(2, 1))], &sig("Tab", 10));
    let at = tip(&bo);
    let refused = saving::save(
        &mut bo,
        &ask(&lantern(1, 3), &lantern(1, 1), Moved::Refuse),
        &sig("Bo", 11),
    )
    .unwrap();
    assert!(matches!(refused, Saved::Moved { .. }), "{refused:?}");
    assert_eq!(tip(&bo), at);

    // A text that is no deck is never committed, moved or not.
    let broken = saving::save(
        &mut bo,
        &ask("cards = [\n", &lantern(2, 1), Moved::Refuse),
        &sig("Bo", 11),
    )
    .unwrap();
    assert!(matches!(broken, Saved::Refused { .. }), "{broken:?}");
    assert_eq!(tip(&bo), at);

    let over = saving::save(
        &mut bo,
        &SaveAsk {
            made: Made::Over,
            ..ask(&lantern(1, 3), "", Moved::Refuse)
        },
        &sig("Bo", 12),
    )
    .unwrap();
    assert!(
        matches!(over, Saved::Committed { merged: None, .. }),
        "{over:?}"
    );
    let info = bo.read_commit(&tip(&bo)).unwrap();
    // Said against the file it replaced.
    assert_eq!(
        info.message,
        "lantern: Mind Stone: 1 → 3, Sol Ring: 2 → 1\n"
    );

    // A new file that is already there is a move too.
    let made = saving::save(
        &mut bo,
        &SaveAsk {
            path: "collection.toml".into(),
            branch: None,
            text: "cards = []\n\n[places]\nBulk = {}\n".into(),
            made: Made::From {
                base: None,
                moved: Moved::Refuse,
            },
        },
        &sig("Bo", 13),
    )
    .unwrap();
    assert!(matches!(made, Saved::Committed { .. }), "{made:?}");
    let again = saving::save(
        &mut bo,
        &SaveAsk {
            path: "collection.toml".into(),
            branch: None,
            text: "cards = []\n".into(),
            made: Made::From {
                base: None,
                moved: Moved::Refuse,
            },
        },
        &sig("Bo", 14),
    )
    .unwrap();
    assert!(matches!(again, Saved::Moved { .. }), "{again:?}");
    fsck(&dir.path().join("bo.git"));
}
