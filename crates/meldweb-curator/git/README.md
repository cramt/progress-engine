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
- merge two texts three-way by lines (`merge_lines`, Curator's one line
  merge), and two commits' trees path by path from a merge base, a file both
  sides changed going to a caller's merge. `merge_lines_with` also names
  each stretch both sides changed differently, or settles every such stretch
  with one side and keeps the rest of both.
- plan merging one commit into a branch with no ref moved (`plan_merge`):
  nothing when the branch already holds it, a fast-forward when the branch
  is its ancestor, otherwise the merged tree; and count the commits one tip
  has that another lacks (`count_only_in`, as `git rev-list --count b..a`).
  Curator's deck branches (`meldweb-wasm`'s `branches`) are built on these.
- keep one remote, `origin`, in `config` as `git remote add` would, and speak
  git's smart HTTP protocol v0 to it as pure functions over bytes
  (`src/remote.rs`): the caller does the HTTP. A fetch turns the
  `info/refs` answer into an upload-pack request, writes the pack it gets
  back as loose objects (ofs-deltas and ref-deltas resolved), and tracks the
  remote's branches under `refs/remotes/origin/` and its tags under
  `refs/remotes/origin/tags/`. Every branch and tag syncs (a deck is a
  branch, a snapshot a tag, ADR-0041), each settled three ways against what
  was tracked before the fetch: the side that made, moved or deleted a ref
  wins, so the fetch itself takes a branch or tag only the remote changed,
  and a ref deleted here and untouched there stays deleted for the push.
  `integrate` then brings each tracked branch into the branch of its name (or
  `integrate_onto` brings a map the caller made, as Curator does for a
  remote still at repo version 1): a fast-forward, the remote's history over
  a `main` that is only its first `VERSION` commit, a branch only the remote
  has, or a merge commit with two parents; it refuses unrelated histories and
  stops at a conflicting file. A push sends every branch the remote's is
  behind, every tag it lacks or has elsewhere, and the deletion of each ref
  deleted here since the last sync, with an undeltified pack, naming the
  advertised old id so that receive-pack refuses it if the remote moved.
  Setting a new URL forgets what was tracked of the old one.
- write a blob, a tree of given files, and a commit of a given tree with
  given parents and signatures (`write_commit_of`), and retarget an
  annotated tag keeping its tagger and label, so history can be replayed
  into another line with the same ids anywhere.

Protocol v0, not v2: receive-pack has no v2, so its advertisement parser is
needed anyway, and v2's ref-prefix filter gains nothing on a repo of a few
dozen refs. The pack reader is here rather than gix-pack's: gix-pack 0.76
builds for wasm32 with default features off, but its delta application is
crate-private and its only public way to resolve deltas writes a pack and an
index to a directory. pkt-line framing is forty lines here rather than
gix-packetline's IO-feature-gated encoder.

The core never reads a clock: the caller passes every author, committer and
tagger time. Ids are branded by kind (`CommitId`, `TreeId`, `BlobId`, `TagId`),
and a read refuses an object of the wrong kind. Branch and tag names pass
git's ref-name rules. Paths pass git's checkout rules for Windows, macOS and
Linux. Every read checks the object's hash.

## What it refuses

- **Packfiles on disk.** A fetched pack is unpacked into loose objects. If a
  missing object might be in `objects/pack/`, the read
  fails with `Unsupported::Packfile`, not `MissingObject`. A repo that `git gc`
  or `git clone` made needs `git unpack-objects` before this crate can read it.
- **Packed refs.** If `packed-refs` exists, every ref operation fails with
  `Unsupported::PackedRefs`.
- **Symbolic branch or tag refs, and tags of non-commits.**
- **The index, a working tree, and HTTP itself.**

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

`tests/remote.rs` makes a bare repo on disk the server, answering through
`git upload-pack --stateless-rpc` and `git receive-pack --stateless-rpc`
as `git http-backend` does: a clone of a repo `git gc --aggressive`
deltified (delta chains up to 5), an incremental fetch (a pack of exactly
the 4 new objects), ref-deltas from `pack-objects` without side-band, a
fast-forward push with a tag, a push refused by the compare-and-swap and
then merged and pushed, a conflict settled by choosing a side, an empty
remote, and unrelated history; and every branch and tag syncing both ways:
decks made here pushed, a deck the remote moved fast-forwarded on fetch, one
it made taken, one both moved merged (`sync: merge origin/decks/a/main`), a
branch and a snapshot deleted or retargeted here pushed as deletions and
updates, one deleted there deleted here. `git fsck --strict` passes on both
sides.

If `git` is not on PATH, those tests print `SKIPPED` and pass.

`examples/remote_e2e.rs` runs the same steps against a real host:
`cargo run -p meldweb-git --example remote_e2e -- https://github.com/<o>/<r>.git "$(gh auth token)" <empty dir> [<clone>]`.
On 2026-10-22 against a private github.com repo with a `gho_` token it
adopted the remote, pushed a commit and a tag, had a raced push refused
(`cannot lock ref 'refs/heads/main': is at … but expected …`), merged and
pushed again, and a fresh `git clone` showed the merge and the tag.

On 2026-10-22, 30 tests pass: 10 unit, 11 in `memory.rs`, 3 in
`git_cli.rs` and 6 in `remote.rs`. Repo version 2's migration and the
mixed-version syncs are `meldweb-wasm`'s (`tests/layout.rs`,
`examples/migrate_e2e.rs`), since what a deck is is not this crate's, and so
are a deck's branches merged against `git` (`tests/branches.rs`: `fsck
--strict`, the two-parent merge in `log --graph`, `rev-list --count`). Measured on 2026-10-21: a throwaway cdylib built for wasm32 at
opt-level `z` with LTO calls init, commit, tag, history, merge_base and
read_file. It is 325 KB, or 124 KB gzipped. `gix-date` pulls in `jiff` and
`jiff-tzdb`, and `gix-utils` pulls in `prodash`. LTO drops the time zone
database: the probe holds no `TZif` data and no zone names.
