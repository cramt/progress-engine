//! The repo over the in-memory store: the API's own promises, without git.

use meldweb_git::{
    BranchName, Change, CommitId, EntryKind, Error, HistoryEntry, Identity, Invalid, MemStore,
    PathChange, Repo, RepoPath, Signature, Store, TagKind, TagName, Timestamp, Tip, WriteCommit,
};

fn sig(seconds: i64) -> Signature {
    Signature {
        who: Identity::new("Alex", "alex@example.com").unwrap(),
        when: Timestamp::new(seconds, 120).unwrap(),
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

fn delete(p: &str) -> Change {
    Change::Delete { path: path(p) }
}

fn edit(changes: Vec<Change>, message: &str, at: i64) -> WriteCommit {
    WriteCommit {
        changes,
        message: message.to_owned(),
        author: sig(at),
        committer: sig(at),
    }
}

fn repo() -> Repo<MemStore> {
    Repo::init(MemStore::new()).unwrap()
}

#[test]
fn init_makes_an_unborn_main_and_refuses_twice() {
    let repo = repo();
    assert_eq!(repo.head_branch().unwrap(), branch("main"));
    assert_eq!(repo.branch_tip(&branch("main")).unwrap(), None);
    assert!(repo.branches().unwrap().is_empty());
    let store = repo.into_store();
    assert!(matches!(
        Repo::init(store.clone()),
        Err(Error::AlreadyARepo)
    ));
    assert!(Repo::open(store).is_ok());
    assert!(matches!(Repo::open(MemStore::new()), Err(Error::NotARepo)));
}

#[test]
fn ids_are_gits_own() {
    let mut repo = repo();
    let main = branch("main");
    let c = repo
        .commit(
            &main,
            Tip::Unborn,
            &edit(vec![put("hello", "hello\n")], "hi\n", 0),
        )
        .unwrap();
    // `printf 'hello\n' | git hash-object --stdin`
    let Some(entries) = repo.list_dir(&c, None).unwrap() else {
        panic!("root exists")
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].kind,
        EntryKind::File(
            meldweb_git::BlobId::from_hex("ce013625030ba8dba906f756967f9e9ca394464a").unwrap()
        )
    );
    assert_eq!(
        repo.entry(&c, &path("hello")).unwrap(),
        Some(entries[0].kind)
    );
    assert_eq!(repo.entry(&c, &path("nope")).unwrap(), None);
    assert_eq!(repo.entry(&c, &path("hello/deeper")).unwrap(), None);
    // Deleting the only file leaves git's well-known empty tree.
    let d = repo
        .commit(&main, Tip::At(c), &edit(vec![delete("hello")], "bye\n", 1))
        .unwrap();
    assert_eq!(
        repo.read_commit(&d).unwrap().tree.to_hex(),
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
    );
}

#[test]
fn a_commit_changes_several_paths_and_rebuilds_nested_trees() {
    let mut repo = repo();
    let main = branch("main");
    let c1 = repo
        .commit(
            &main,
            Tip::Unborn,
            &edit(
                vec![
                    put("decks/lantern.deck.toml", "a"),
                    put("decks/old/loam.deck.toml", "b"),
                    put("collection.toml", "c"),
                ],
                "one",
                10,
            ),
        )
        .unwrap();
    let c2 = repo
        .commit(
            &main,
            Tip::At(c1),
            &edit(
                vec![
                    put("collection.toml", "c2"),
                    delete("decks/old/loam.deck.toml"),
                    put("decks/new/x.deck.toml", "x"),
                ],
                "two",
                20,
            ),
        )
        .unwrap();
    assert_eq!(repo.branch_tip(&main).unwrap(), Some(c2));
    let info = repo.read_commit(&c2).unwrap();
    assert_eq!(info.parents, vec![c1]);
    assert_eq!(info.message, "two");
    assert_eq!(info.author, sig(20));

    assert_eq!(
        repo.read_file(&c2, &path("collection.toml"))
            .unwrap()
            .unwrap(),
        b"c2"
    );
    assert_eq!(
        repo.read_file(&c1, &path("collection.toml"))
            .unwrap()
            .unwrap(),
        b"c"
    );
    assert_eq!(
        repo.read_file(&c2, &path("decks/lantern.deck.toml"))
            .unwrap()
            .unwrap(),
        b"a"
    );
    assert_eq!(
        repo.read_file(&c2, &path("decks/old/loam.deck.toml"))
            .unwrap(),
        None
    );
    // The emptied directory is gone, as git cannot hold an empty tree.
    assert_eq!(repo.list_dir(&c2, Some(&path("decks/old"))).unwrap(), None);
    let names = |c: &CommitId, d: &str| {
        repo.list_dir(c, Some(&path(d)))
            .unwrap()
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&c2, "decks"), ["lantern.deck.toml", "new"]);
    assert_eq!(names(&c1, "decks"), ["lantern.deck.toml", "old"]);
    assert!(matches!(
        repo.read_file(&c2, &path("decks")),
        Err(Error::NotAFile(_))
    ));
    assert!(matches!(
        repo.list_dir(&c2, Some(&path("collection.toml"))),
        Err(Error::NotADirectory(_))
    ));
}

#[test]
fn bad_changes_are_refused_and_move_nothing() {
    let mut repo = repo();
    let main = branch("main");
    let c1 = repo
        .commit(&main, Tip::Unborn, &edit(vec![put("a/b", "1")], "one", 0))
        .unwrap();
    let tip = Tip::At(c1);
    let refused = |repo: &mut Repo<MemStore>, changes| {
        repo.commit(&main, tip, &edit(changes, "x", 1)).unwrap_err()
    };
    assert!(matches!(
        refused(&mut repo, vec![delete("nope")]),
        Error::DeleteMissing(_)
    ));
    assert!(matches!(
        refused(&mut repo, vec![delete("a/b/c")]),
        Error::DeleteMissing(_)
    ));
    assert!(matches!(
        refused(&mut repo, vec![delete("a")]),
        Error::NotAFile(_)
    ));
    assert!(matches!(
        refused(&mut repo, vec![put("a", "dir")]),
        Error::NotAFile(_)
    ));
    assert!(
        matches!(refused(&mut repo, vec![put("a/b/c", "under a file")]), Error::NotADirectory(p) if p == "a/b")
    );
    assert!(matches!(
        refused(&mut repo, vec![put("x", "1"), put("x", "2")]),
        Error::DuplicatePath(_)
    ));
    assert_eq!(repo.branch_tip(&main).unwrap(), Some(c1));
}

#[test]
fn a_ref_update_is_a_compare_and_swap() {
    let mut repo = repo();
    let main = branch("main");
    let c1 = repo
        .commit(&main, Tip::Unborn, &edit(vec![put("f", "1")], "one", 0))
        .unwrap();
    assert!(matches!(
        repo.commit(&main, Tip::Unborn, &edit(vec![put("f", "2")], "again", 1)),
        Err(Error::Conflict {
            expected: None,
            found: Some(_),
            ..
        })
    ));
    let c2 = repo
        .commit(&main, Tip::At(c1), &edit(vec![put("f", "2")], "two", 1))
        .unwrap();
    // Another tab committed c2; this one still thinks the tip is c1.
    let stale = repo.commit(&main, Tip::At(c1), &edit(vec![put("f", "3")], "lost", 2));
    assert!(matches!(stale, Err(Error::Conflict { .. })));
    assert_eq!(repo.branch_tip(&main).unwrap(), Some(c2));
    assert!(matches!(
        repo.delete_branch(&main, &c1),
        Err(Error::Conflict { .. })
    ));
    repo.delete_branch(&main, &c2).unwrap();
    assert_eq!(repo.branch_tip(&main).unwrap(), None);
}

#[test]
fn branches_are_made_listed_and_cannot_nest_in_each_other() {
    let mut repo = repo();
    let main = branch("main");
    let c1 = repo
        .commit(&main, Tip::Unborn, &edit(vec![put("f", "1")], "one", 0))
        .unwrap();
    repo.create_branch(&branch("deck/lantern"), &c1).unwrap();
    assert!(matches!(
        repo.create_branch(&branch("deck"), &c1),
        Err(Error::RefClash { .. })
    ));
    assert!(matches!(
        repo.create_branch(&branch("deck/lantern/x"), &c1),
        Err(Error::RefClash { .. })
    ));
    assert!(matches!(
        repo.create_branch(&branch("deck/lantern"), &c1),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(
        repo.branches().unwrap(),
        vec![(branch("deck/lantern"), c1), (main.clone(), c1)]
    );
    let tree = repo.read_commit(&c1).unwrap().tree;
    let not_a_commit = CommitId::from_hex(&tree.to_hex()).unwrap();
    assert!(matches!(
        repo.create_branch(&branch("t"), &not_a_commit),
        Err(Error::WrongKind { .. })
    ));
}

#[test]
fn history_is_the_first_parent_chain_where_the_path_changed() {
    let mut repo = repo();
    let main = branch("main");
    let deck = "decks/lantern.deck.toml";
    let c1 = repo
        .commit(&main, Tip::Unborn, &edit(vec![put(deck, "1")], "add", 0))
        .unwrap();
    let c2 = repo
        .commit(
            &main,
            Tip::At(c1),
            &edit(vec![put("other", "x")], "other", 1),
        )
        .unwrap();
    let c3 = repo
        .commit(&main, Tip::At(c2), &edit(vec![put(deck, "2")], "edit", 2))
        .unwrap();
    let c4 = repo
        .commit(
            &main,
            Tip::At(c3),
            &edit(vec![put(deck, "2")], "same bytes", 3),
        )
        .unwrap();
    let c5 = repo
        .commit(&main, Tip::At(c4), &edit(vec![delete(deck)], "drop", 4))
        .unwrap();
    let c6 = repo
        .commit(&main, Tip::At(c5), &edit(vec![put(deck, "1")], "back", 5))
        .unwrap();
    let h = |c, change| HistoryEntry { commit: c, change };
    assert_eq!(
        repo.history(&main, &path(deck)).unwrap(),
        vec![
            h(c6, PathChange::Added),
            h(c5, PathChange::Deleted),
            h(c3, PathChange::Modified),
            h(c1, PathChange::Added)
        ]
    );
    assert_eq!(repo.history(&main, &path("decks")).unwrap().len(), 4);
    assert!(repo
        .history(&branch("unborn"), &path(deck))
        .unwrap()
        .is_empty());
}

#[test]
fn merge_base_walks_every_parent() {
    let mut repo = repo();
    let (main, fork) = (branch("main"), branch("fork"));
    let base = repo
        .commit(&main, Tip::Unborn, &edit(vec![put("f", "0")], "base", 0))
        .unwrap();
    repo.create_branch(&fork, &base).unwrap();
    let m1 = repo
        .commit(&main, Tip::At(base), &edit(vec![put("m", "1")], "m1", 1))
        .unwrap();
    let f1 = repo
        .commit(&fork, Tip::At(base), &edit(vec![put("g", "1")], "f1", 2))
        .unwrap();
    assert_eq!(repo.merge_base(&m1, &f1).unwrap(), vec![base]);
    assert_eq!(repo.merge_base(&m1, &base).unwrap(), vec![base]);

    let merge = repo
        .commit_merge(&main, &m1, &f1, &edit(vec![put("g", "1")], "merge fork", 3))
        .unwrap();
    assert_eq!(repo.read_commit(&merge).unwrap().parents, vec![m1, f1]);
    assert_eq!(repo.read_file(&merge, &path("m")).unwrap().unwrap(), b"1");
    assert_eq!(repo.merge_base(&merge, &f1).unwrap(), vec![f1]);
    let f2 = repo
        .commit(&fork, Tip::At(f1), &edit(vec![put("g", "2")], "f2", 4))
        .unwrap();
    assert_eq!(repo.merge_base(&merge, &f2).unwrap(), vec![f1]);

    // Criss-cross: each side merges the other, so both are best.
    let x = repo
        .commit_merge(&main, &merge, &f2, &edit(vec![], "x", 5))
        .unwrap();
    let y = repo
        .commit_merge(&fork, &f2, &merge, &edit(vec![], "y", 6))
        .unwrap();
    let mut both = vec![merge, f2];
    both.sort();
    assert_eq!(repo.merge_base(&x, &y).unwrap(), both);

    let mut other = Repo::init(MemStore::new()).unwrap();
    let lone = other
        .commit(&main, Tip::Unborn, &edit(vec![put("z", "z")], "lone", 0))
        .unwrap();
    let unrelated = repo
        .write_commit(&[], &edit(vec![put("z", "z")], "lone", 0))
        .unwrap();
    assert_eq!(
        unrelated, lone,
        "the same content and signatures are the same commit in any repo"
    );
    assert!(repo.merge_base(&x, &unrelated).unwrap().is_empty());
}

#[test]
fn tags_are_annotated_listed_by_prefix_and_deleted() {
    let mut repo = repo();
    let main = branch("main");
    let c1 = repo
        .commit(&main, Tip::Unborn, &edit(vec![put("f", "1")], "one", 0))
        .unwrap();
    let c2 = repo
        .commit(&main, Tip::At(c1), &edit(vec![put("f", "2")], "two", 1))
        .unwrap();
    let t1 = repo
        .create_tag(
            &TagName::new("snapshot/a").unwrap(),
            &c1,
            &sig(5),
            "first\n",
        )
        .unwrap();
    repo.create_tag(
        &TagName::new("snapshot/b").unwrap(),
        &c2,
        &sig(6),
        "second\n",
    )
    .unwrap();
    repo.create_tag(&TagName::new("release").unwrap(), &c2, &sig(7), "r\n")
        .unwrap();
    assert!(matches!(
        repo.create_tag(&TagName::new("release").unwrap(), &c1, &sig(8), "again"),
        Err(Error::Conflict { .. })
    ));
    let snapshots = repo.tags("snapshot/").unwrap();
    assert_eq!(
        snapshots
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["snapshot/a", "snapshot/b"]
    );
    assert_eq!(snapshots[0].commit, c1);
    assert_eq!(
        snapshots[0].kind,
        TagKind::Annotated {
            id: t1,
            tagger: Some(sig(5)),
            message: "first\n".to_owned()
        }
    );
    assert_eq!(repo.tags("").unwrap().len(), 3);
    repo.delete_tag(&TagName::new("snapshot/a").unwrap())
        .unwrap();
    assert!(matches!(
        repo.delete_tag(&TagName::new("snapshot/a").unwrap()),
        Err(Error::NoSuchTag(_))
    ));
    assert_eq!(repo.tags("snapshot/").unwrap().len(), 1);
}

#[test]
fn a_tampered_object_is_refused() {
    let mut repo = repo();
    let main = branch("main");
    let c1 = repo
        .commit(&main, Tip::Unborn, &edit(vec![put("f", "1")], "one", 0))
        .unwrap();
    let mut store = repo.into_store();
    let blob = store.list("objects").unwrap().into_iter().find(|p| {
        // The blob of "1": `printf 1 | git hash-object --stdin`.
        p == "objects/56/a6051ca2b02b04ef92d5150c9ef600403cb1de"
    });
    let blob = blob.expect("the blob is a loose object");
    let other = {
        let mut scratch = Repo::init(MemStore::new()).unwrap();
        scratch
            .commit(&main, Tip::Unborn, &edit(vec![put("f", "2")], "one", 0))
            .unwrap();
        let s = scratch.into_store();
        let p = s
            .list("objects")
            .unwrap()
            .into_iter()
            .find(|p| p.starts_with("objects/d8/"))
            .unwrap();
        s.read(&p).unwrap().unwrap()
    };
    store.write(&blob, &other).unwrap();
    let repo = Repo::open(store).unwrap();
    assert!(matches!(
        repo.read_file(&c1, &path("f")),
        Err(Error::Corrupt { .. })
    ));
}

#[test]
fn names_git_would_refuse_cannot_be_made() {
    for bad in [
        "", "a..b", "a/", "/a", "a.lock", "HEAD", "a b", "a~1", "x:y", "@{", "a//b",
    ] {
        assert!(
            matches!(BranchName::new(bad), Err(Invalid::Branch { .. })),
            "branch {bad:?}"
        );
    }
    for bad in [
        "",
        "/a",
        "a/",
        "a//b",
        "..",
        "../x",
        ".git/config",
        "a/.git",
        "CON",
        "a\0b",
        ".",
    ] {
        assert!(
            matches!(RepoPath::new(bad), Err(Invalid::Path { .. })),
            "path {bad:?}"
        );
    }
    assert!(matches!(
        Identity::new("A <b>", "c"),
        Err(Invalid::Identity { .. })
    ));
    assert!(matches!(
        Identity::new(" padded", "c"),
        Err(Invalid::Identity { .. })
    ));
    assert!(matches!(
        Identity::new("", "c"),
        Err(Invalid::Identity { .. })
    ));
    assert!(matches!(Timestamp::new(-1, 0), Err(Invalid::Time { .. })));
    assert!(matches!(
        Timestamp::new(0, 24 * 60),
        Err(Invalid::Time { .. })
    ));
    assert!(BranchName::new("deck/lantern-2").is_ok());
    assert!(RepoPath::new("decks/lantern.deck.toml").is_ok());
}
