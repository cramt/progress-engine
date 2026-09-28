# gitaxian-probe-archive

Every Delver X engine build that [Gitaxian Probe](https://github.com/cramt/progress-engine/tree/main/crates/gitaxian-probe)
might be pinned to, kept after upstream has stopped serving it.

**This repo must stay private.** It is a build cache for progress-engine, not a
place anyone downloads Delver's engine from. The files are Delver Lab's, and
serving them to anyone else is redistribution. progress-engine's
`docs/research/probe-in-curator.md` explains the distinction.

It is written inside progress-engine at `crates/gitaxian-probe/archive/`, to be
moved into its own repo with
`git filter-repo --subdirectory-filter crates/gitaxian-probe/archive`. Until
then its workflow is inert, since GitHub runs workflows only from a repo's root
`.github/`.

## Why

`https://mtg.delver.app` serves only its current build, and it ships often:
1.83.beta to 1.89.beta took about a week. A pin stops building the day upstream
moves, and a build nobody saved the day it shipped is gone.

## What is here

Nothing but releases, one per build. Each carries the files exactly as upstream
served them (`version.txt`, `core.js`, `core.wasm`, `data.7z`, `data.md5`,
`data.size`, `model-alpha.7z`, `model-alpha.size`) plus a `SHA256SUMS`, about
42 MB in all. Only the alpha tier is kept: lambda and gamma are gated behind a
token, and the probe refuses to boot them anyway.

A build's tag is:

```
delver-<version.txt>-<first 12 hex of sha256(SHA256SUMS)>
e.g. delver-1.89.beta-eeb9c6a9c3ec
```

`SHA256SUMS` lists the files in the order above, in `sha256sum` format, so
`sha256sum -c SHA256SUMS` checks a downloaded release. The hash is in the tag
because upstream rebuilds its data and model often (the probe's FINDINGS §5
has them rebuilt daily), and nothing guarantees each rebuild changes
`version.txt`. Two different builds must never share a release.

**The file list and the tag rule are a contract with progress-engine.** Its
`assets/src/pin.rs` pins the same files in the same order, and its
`assets/build.rs` recomputes the tag from them. Change one side and you must
change the other.

## How builds get here

`.github/workflows/archive.yml` runs daily, and on demand from the Actions tab.
It `fetch`es what Delver serves and `publish`es it as a release, unless a
release with that tag already exists. It needs nothing but this repo's own
`GITHUB_TOKEN`.

By hand:

```sh
python3 archive.py fetch /tmp/delver      # prints the version and the tag
GH_TOKEN=… ARCHIVE_REPO=cramt/gitaxian-probe-archive python3 archive.py publish /tmp/delver
```

`publish` needs the `gh` CLI. Under Actions it publishes to the repo it runs in.

## Moving it out

1. `git filter-repo --subdirectory-filter crates/gitaxian-probe/archive` on a
   fresh clone of progress-engine, then push the result to a new **private**
   `cramt/gitaxian-probe-archive`.
2. Check that Actions may write releases: Settings → Actions → General →
   Workflow permissions. The workflow asks for `contents: write`, which a
   repo's default settings can still deny.
3. Run **Archive** once, so the build pinned today is saved before Delver ships
   the next one.
4. Delete `crates/gitaxian-probe/archive/` from progress-engine.
