//! The real `git` against the crate, both ways: git checks and reads a repo
//! the crate wrote, and the crate reads a repo git wrote, with the same ids.

use std::path::Path;
use std::process::Command;

use meldweb_git::{
    BranchName, Change, CommitId, EntryKind, FsStore, Identity, PathChange, Repo, RepoPath,
    Signature, TagKind, TagName, Timestamp, Tip, WriteCommit,
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
        eprintln!("SKIPPED: no `git` on PATH, so nothing checks the crate's objects against git's");
    }
    found
}

/// `git -C dir args`, isolated from the user's config, which may sign
/// commits or run hooks.
fn git_raw(dir: &Path, args: &[&str]) -> std::process::Output {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "gc.auto=0",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Friend")
        .env("GIT_AUTHOR_EMAIL", "friend@example.com")
        .env("GIT_COMMITTER_NAME", "Friend")
        .env("GIT_COMMITTER_EMAIL", "friend@example.com")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn git(dir: &Path, args: &[&str]) -> String {
    String::from_utf8(git_raw(dir, args).stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}

fn lines(s: String) -> Vec<String> {
    s.lines().map(str::to_owned).collect()
}

fn sig(name: &str, seconds: i64, offset_minutes: i32) -> Signature {
    Signature {
        who: Identity::new(name, format!("{}@example.com", name.to_lowercase())).unwrap(),
        when: Timestamp::new(seconds, offset_minutes).unwrap(),
    }
}

fn path(p: &str) -> RepoPath {
    RepoPath::new(p).unwrap()
}

fn branch(b: &str) -> BranchName {
    BranchName::new(b).unwrap()
}

fn put(p: &str, bytes: &str) -> Change {
    Change::Put {
        path: path(p),
        bytes: bytes.as_bytes().to_vec(),
    }
}

fn edit(changes: Vec<Change>, message: &str, at: i64) -> WriteCommit {
    WriteCommit {
        changes,
        message: format!("{message}\n"),
        author: sig("Alex", at, 120),
        committer: sig("Alex", at + 1, -300),
    }
}

const DECK: &str = "decks/lantern.deck.toml";

#[test]
fn git_accepts_and_reads_what_the_crate_writes() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let gitdir = dir.path().join("repo.git");
    let mut repo = Repo::init(FsStore::new(&gitdir)).unwrap();
    let (main, fork) = (branch("main"), branch("friend/fork"));
    let t = 1_760_000_000;

    let c1 = repo
        .commit(
            &main,
            Tip::Unborn,
            &edit(
                vec![
                    put(DECK, "[deck]\nname = \"Lantern\"\n"),
                    put("collection.toml", ""),
                ],
                "lantern: start",
                t,
            ),
        )
        .unwrap();
    let c2 = repo
        .commit(
            &main,
            Tip::At(c1),
            &edit(vec![put(DECK, "v2\n")], "lantern: v2", t + 10),
        )
        .unwrap();
    repo.create_branch(&fork, &c1).unwrap();
    let f1 = repo
        .commit(
            &fork,
            Tip::At(c1),
            &edit(
                vec![put("decks/loam.deck.toml", "loam\n")],
                "loam: start",
                t + 20,
            ),
        )
        .unwrap();
    let m = repo
        .commit_merge(
            &main,
            &c2,
            &f1,
            &edit(
                vec![put("decks/loam.deck.toml", "loam\n")],
                "merge fork",
                t + 30,
            ),
        )
        .unwrap();
    let c4 = repo
        .commit(
            &main,
            Tip::At(m),
            &edit(
                vec![
                    Change::Delete { path: path(DECK) },
                    put("collection.toml", "moved\n"),
                ],
                "lantern: retire",
                t + 40,
            ),
        )
        .unwrap();
    let tag = repo
        .create_tag(
            &TagName::new("snapshot/v2").unwrap(),
            &c2,
            &sig("Alex", t + 50, 60),
            "v2 as built\n",
        )
        .unwrap();

    // The crate's objects pass git's strictest check, and nothing dangles.
    let fsck = git_raw(&gitdir, &["fsck", "--strict", "--full", "--no-dangling"]);
    eprintln!(
        "git fsck --strict: status {}, stdout {:?}, stderr {:?}",
        fsck.status,
        String::from_utf8_lossy(&fsck.stdout),
        String::from_utf8_lossy(&fsck.stderr)
    );
    assert!(fsck.stdout.is_empty() && fsck.stderr.is_empty());

    let refs = git(
        &gitdir,
        &["for-each-ref", "--format=%(refname) %(objectname)"],
    );
    eprintln!("git for-each-ref:\n{refs}");
    assert_eq!(
        lines(refs),
        [
            format!("refs/heads/friend/fork {f1}"),
            format!("refs/heads/main {c4}"),
            format!("refs/tags/snapshot/v2 {tag}"),
        ]
    );

    let log = git(
        &gitdir,
        &["log", "--first-parent", "--format=%H", "main", "--", DECK],
    );
    eprintln!("git log --first-parent main -- {DECK}:\n{log}");
    let history = repo.history(&main, &path(DECK)).unwrap();
    assert_eq!(
        lines(log),
        history
            .iter()
            .map(|h| h.commit.to_hex())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        history.iter().map(|h| h.change).collect::<Vec<_>>(),
        [PathChange::Deleted, PathChange::Modified, PathChange::Added]
    );
    // The merge brought loam in from the fork; first-parent history sees it there.
    let loam = git(
        &gitdir,
        &[
            "log",
            "--first-parent",
            "--format=%H",
            "main",
            "--",
            "decks/loam.deck.toml",
        ],
    );
    assert_eq!(lines(loam), [m.to_hex()]);
    assert_eq!(
        repo.history(&main, &path("decks/loam.deck.toml")).unwrap()[0].commit,
        m
    );

    // Every commit's header fields, as git reads them.
    let all = git(
        &gitdir,
        &[
            "log",
            "--format=%H|%T|%P|%an <%ae> %at %ad|%cn <%ce> %ct %cd|%B",
            "--date=format:%z",
            "--all",
        ],
    );
    for record in all.split('\n').filter(|l| l.contains('|')) {
        let f = record.split('|').collect::<Vec<_>>();
        let info = repo
            .read_commit(&CommitId::from_hex(f[0]).unwrap())
            .unwrap();
        assert_eq!(f[1], info.tree.to_hex());
        assert_eq!(
            f[2],
            info.parents
                .iter()
                .map(CommitId::to_hex)
                .collect::<Vec<_>>()
                .join(" ")
        );
        let fmt = |s: &Signature| {
            let o = s.when.offset_minutes();
            format!(
                "{} <{}> {} {}{:02}{:02}",
                s.who.name(),
                s.who.email(),
                s.when.seconds(),
                if o < 0 { '-' } else { '+' },
                o.abs() / 60,
                o.abs() % 60
            )
        };
        assert_eq!(f[3], fmt(&info.author));
        assert_eq!(f[4], fmt(&info.committer));
        assert_eq!(format!("{}\n", f[5]), info.message);
    }

    assert_eq!(git(&gitdir, &["rev-parse", &format!("{m}^2")]), f1.to_hex());
    let shown = git_raw(&gitdir, &["cat-file", "blob", &format!("{c2}:{DECK}")]).stdout;
    assert_eq!(shown, repo.read_file(&c2, &path(DECK)).unwrap().unwrap());
    assert_eq!(
        git(&gitdir, &["ls-tree", "--name-only", &c4.to_hex(), "decks/"]),
        "decks/loam.deck.toml"
    );

    let base = git(
        &gitdir,
        &["merge-base", "--all", &c4.to_hex(), &f1.to_hex()],
    );
    assert_eq!(lines(base), [f1.to_hex()]);
    assert_eq!(repo.merge_bases(&c4, &f1).unwrap(), [f1]);
    let base = git(
        &gitdir,
        &["merge-base", "--all", &c2.to_hex(), &f1.to_hex()],
    );
    assert_eq!(lines(base), [c1.to_hex()]);
    assert_eq!(repo.merge_bases(&c2, &f1).unwrap(), [c1]);

    let show = git(&gitdir, &["cat-file", "-p", "snapshot/v2"]);
    eprintln!("git cat-file -p snapshot/v2:\n{show}");
    assert_eq!(
        show,
        format!("object {c2}\ntype commit\ntag snapshot/v2\ntagger Alex <alex@example.com> {} +0100\n\nv2 as built", t + 50)
    );
    assert_eq!(
        git(&gitdir, &["rev-parse", "snapshot/v2^{commit}"]),
        c2.to_hex()
    );
}

#[test]
fn criss_cross_merge_bases_match_git_merge_base_all() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut repo = Repo::init(FsStore::new(dir.path())).unwrap();
    let (x, y) = (branch("x"), branch("y"));
    let base = repo
        .commit(&x, Tip::Unborn, &edit(vec![put("f", "0")], "base", 0))
        .unwrap();
    repo.create_branch(&y, &base).unwrap();
    let x1 = repo
        .commit(&x, Tip::At(base), &edit(vec![put("x", "1")], "x1", 1))
        .unwrap();
    let y1 = repo
        .commit(&y, Tip::At(base), &edit(vec![put("y", "1")], "y1", 2))
        .unwrap();
    let x2 = repo
        .commit_merge(&x, &x1, &y1, &edit(vec![put("y", "1")], "x2", 3))
        .unwrap();
    let y2 = repo
        .commit_merge(&y, &y1, &x1, &edit(vec![put("x", "1")], "y2", 4))
        .unwrap();
    git_raw(dir.path(), &["fsck", "--strict"]);
    let mut from_git = lines(git(
        dir.path(),
        &["merge-base", "--all", &x2.to_hex(), &y2.to_hex()],
    ));
    from_git.sort();
    eprintln!("git merge-base --all x2 y2: {from_git:?}");
    assert_eq!(from_git.len(), 2);
    assert_eq!(
        repo.merge_bases(&x2, &y2)
            .unwrap()
            .iter()
            .map(CommitId::to_hex)
            .collect::<Vec<_>>(),
        from_git
    );
}

#[test]
fn the_crate_reads_and_extends_what_git_writes() {
    if !have_git() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path();
    let commit = |msg: &str, when: &str| {
        let out = Command::new("git")
            .arg("-C")
            .arg(work)
            .args([
                "-c",
                "gc.auto=0",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "-m",
                msg,
            ])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Friend")
            .env("GIT_AUTHOR_EMAIL", "friend@example.com")
            .env("GIT_COMMITTER_NAME", "Friend")
            .env("GIT_COMMITTER_EMAIL", "friend@example.com")
            .env("GIT_AUTHOR_DATE", when)
            .env("GIT_COMMITTER_DATE", when)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let write = |p: &str, s: &str| {
        let full = work.join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, s).unwrap();
    };

    git(work, &["init", "-q", "-b", "main"]);
    write(DECK, "one\n");
    write("collection.toml", "c\n");
    write("tools/run.sh", "#!/bin/sh\n");
    git(work, &["add", "-A"]);
    // On disk too, or a later `commit -a` stages it back to 644.
    std::fs::set_permissions(
        work.join("tools/run.sh"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    git(work, &["add", "-A"]);
    commit("start", "1700000000 +0200");
    write(DECK, "two\n");
    git(work, &["commit", "-qam", "two", "--date=1700000100 -0130"]);
    git(work, &["checkout", "-qb", "side"]);
    write("decks/side.deck.toml", "side\n");
    git(work, &["add", "-A"]);
    commit("side", "1700000200 +0000");
    git(work, &["checkout", "-q", "main"]);
    write(DECK, "three\n");
    git(work, &["commit", "-qam", "three"]);
    git(
        work,
        &["merge", "-q", "--no-ff", "-m", "merge side", "side"],
    );
    git(work, &["rm", "-q", DECK]);
    commit("drop lantern", "1700000400 +0900");
    git(
        work,
        &["tag", "-a", "-m", "the start", "snap/start", "main~4"],
    );
    git(work, &["tag", "light", "side"]);
    assert!(
        !work.join(".git/packed-refs").exists()
            && !work
                .join(".git/objects/pack")
                .read_dir()
                .unwrap()
                .any(|_| true)
    );

    let gitdir = work.join(".git");
    let mut repo = Repo::open(FsStore::new(&gitdir)).unwrap();
    let main = branch("main");
    let tip = repo.branch_tip(&main).unwrap().unwrap();
    assert_eq!(tip.to_hex(), git(work, &["rev-parse", "main"]));
    assert_eq!(repo.head_branch().unwrap(), main);

    let refs = lines(git(
        work,
        &[
            "for-each-ref",
            "--format=%(refname:strip=2) %(objectname)",
            "refs/heads",
        ],
    ));
    let ours = repo
        .branches()
        .unwrap()
        .into_iter()
        .map(|(b, c)| format!("{b} {c}"))
        .collect::<Vec<_>>();
    assert_eq!(ours, refs);

    // Files at each commit, byte for byte.
    for rev in ["main~1", "main~2", "main~3", "side"] {
        let c = CommitId::from_hex(&git(work, &["rev-parse", rev])).unwrap();
        for file in [
            DECK,
            "collection.toml",
            "decks/side.deck.toml",
            "tools/run.sh",
        ] {
            let theirs = Command::new("git")
                .arg("-C")
                .arg(work)
                .args(["cat-file", "blob", &format!("{rev}:{file}")])
                .output()
                .unwrap();
            let ours = repo.read_file(&c, &path(file)).unwrap();
            match ours {
                Some(bytes) => assert_eq!(bytes, theirs.stdout, "{rev}:{file}"),
                None => assert!(!theirs.status.success(), "{rev}:{file} exists for git"),
            }
        }
    }

    let log = lines(git(
        work,
        &["log", "--first-parent", "--format=%H", "main", "--", DECK],
    ));
    eprintln!(
        "git-written repo, git log --first-parent main -- {DECK}:\n{}",
        log.join("\n")
    );
    let history = repo.history(&main, &path(DECK)).unwrap();
    assert_eq!(
        history
            .iter()
            .map(|h| h.commit.to_hex())
            .collect::<Vec<_>>(),
        log
    );
    assert_eq!(history.len(), 4);
    assert_eq!(history[0].change, PathChange::Deleted);

    let merge = CommitId::from_hex(&git(work, &["rev-parse", "main~1"])).unwrap();
    let side = repo.branch_tip(&branch("side")).unwrap().unwrap();
    let info = repo.read_commit(&merge).unwrap();
    assert_eq!(info.parents.len(), 2);
    assert_eq!(info.parents[1], side);
    let two = repo
        .read_commit(&CommitId::from_hex(&git(work, &["rev-parse", "main~3"])).unwrap())
        .unwrap();
    assert_eq!(
        (two.author.when.seconds(), two.author.when.offset_minutes()),
        (1_700_000_100, -90)
    );
    let base = git(work, &["merge-base", "--all", "main~2", "side"]);
    let ours = repo
        .merge_bases(&repo.read_commit(&merge).unwrap().parents[0], &side)
        .unwrap();
    assert_eq!(
        ours.iter().map(CommitId::to_hex).collect::<Vec<_>>(),
        lines(base)
    );

    let tools = repo.list_dir(&tip, Some(&path("tools"))).unwrap().unwrap();
    assert!(matches!(tools[0].kind, EntryKind::Executable(_)));

    let tags = repo.tags("").unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].name.as_str(), "light");
    assert_eq!(tags[0].kind, TagKind::Lightweight);
    assert_eq!(tags[0].commit, side);
    let TagKind::Annotated {
        message, tagger, ..
    } = &tags[1].kind
    else {
        panic!("snap/start is annotated")
    };
    assert_eq!(message, "the start\n");
    assert_eq!(tagger.as_ref().unwrap().who.name(), "Friend");
    assert_eq!(tags[1].commit.to_hex(), git(work, &["rev-parse", "main~4"]));

    // The crate commits on top of git's history; git checks it and its
    // executable bit survives an edit.
    let ours = repo
        .commit(
            &main,
            Tip::At(tip),
            &edit(
                vec![
                    put("tools/run.sh", "#!/bin/sh\necho hi\n"),
                    put(DECK, "back\n"),
                ],
                "from the browser",
                1_700_000_500,
            ),
        )
        .unwrap();
    git_raw(&gitdir, &["fsck", "--strict", "--no-dangling"]);
    assert_eq!(git(work, &["rev-parse", "main"]), ours.to_hex());
    assert_eq!(
        git(work, &["ls-tree", "main", "tools/run.sh"])
            .split(' ')
            .next(),
        Some("100755")
    );
    assert_eq!(
        git(work, &["log", "-1", "--format=%s", "main"]),
        "from the browser"
    );
}
