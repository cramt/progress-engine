# meldweb-git

The core of Meldweb Curator's offline repo: a bare git repository of loose
objects over a `Store` trait. In the browser, the store will be OPFS and the
repo will travel between PCs as a zip of the bare dir. Every object is a real
git object, zlib-compressed and named by its SHA-1, so `git` reads an exported
repo, and a fork's commits keep the same ids in every repo that holds them.

It can:

- init a bare repo (HEAD is the unborn `main`), and open one.
- read a file, list a directory, or give the entry (and its blob id) at a
  path, at a commit.
- commit puts and deletes to several paths at once, rebuilding the nested
  trees. It keeps an executable bit and drops directories that end up empty.
- move a branch by compare-and-swap. `Tip::Unborn` means the branch must not
  exist. A mismatch is `Error::Conflict`, and nothing moves.
- write merge commits (two parents). The caller resolves the content.
- create, list and delete branches, and create, list by prefix and delete
  annotated tags. It also reads lightweight tags.
- give a path's history along the first-parent chain, newest first, with each
  commit marked as added, modified or deleted.
- give `merge_base` over all parents, as `git merge-base --all` does: none for
  unrelated histories, two after a criss-cross merge.

The core never reads a clock: the caller passes every author, committer and
tagger time. Ids are branded by kind (`CommitId`, `TreeId`, `BlobId`, `TagId`),
and a read refuses an object of the wrong kind. Branch and tag names pass
git's ref-name rules. Paths pass git's checkout rules for Windows, macOS and
Linux. Every read checks the object's hash.

## What it refuses

- **Packfiles.** If a missing object might be in `objects/pack/`, the read
  fails with `Unsupported::Packfile`, not `MissingObject`. A repo that `git gc`
  or `git clone` made needs `git unpack-objects` before this crate can read it.
- **Packed refs.** If `packed-refs` exists, every ref operation fails with
  `Unsupported::PackedRefs`.
- **Symbolic branch or tag refs, and tags of non-commits.**
- **The index, a working tree, network transport, and 3-way content merge.**

There is one writer at a time. A ref update checks the expected value and then
writes, and those are two store calls, so two tabs need a lock above the
crate.

## Verification

```
cargo test -p meldweb-git
nix develop --command cargo build -p meldweb-git --target wasm32-unknown-unknown
```

`tests/memory.rs` checks the API against the in-memory store.
`tests/git_cli.rs` runs the real `git` both ways:

- The crate writes a repo with a fork, a merge, a deletion and an annotated
  tag. `git fsck --strict --full --no-dangling` prints nothing.
  `for-each-ref`, `log --first-parent -- <path>`, every commit's
  tree/parents/author/committer/message, `cat-file`, `merge-base --all`
  (including a criss-cross) and `cat-file -p <tag>` all match the crate.
- `git` writes a repo with a branch, a `--no-ff` merge, an executable file, a
  deletion, an annotated tag and a lightweight tag. The crate reads the same
  files, history, merge base, branches and tags. It then commits on top, and
  `git fsck --strict` passes again.

If `git` is not on PATH, those tests print `SKIPPED` and pass.

Measured on 2026-10-21: 14 tests pass. A throwaway cdylib built for wasm32 at
opt-level `z` with LTO calls init, commit, tag, history, merge_base and
read_file. It is 325 KB, or 124 KB gzipped. `gix-date` pulls in `jiff` and
`jiff-tzdb`, and `gix-utils` pulls in `prodash`. LTO drops the time zone
database: the probe holds no `TZif` data and no zone names.
