# The Delver X archive

Delver serves only its current build, and it ships often: 1.83.beta to
1.89.beta took about a week. So a pin in `../assets/src/pin.rs` stops building
the day upstream moves, and a build that is not saved the day it ships is gone.

The archive keeps every build. It is a **private** GitHub repo,
`cramt/gitaxian-probe-archive`, that holds nothing but releases: one per
build, carrying the files exactly as `https://mtg.delver.app` served them, plus
a `SHA256SUMS`. It is a cache for building and testing this repo. It is not a
place anyone else downloads Delver's engine from, and it has to stay private
for that reason ([probe-in-curator.md](../../../docs/research/probe-in-curator.md)).

## How a build is named

```
delver-<version.txt>-<first 12 hex of sha256(SHA256SUMS)>
e.g. delver-1.89.beta-eeb9c6a9c3ec
```

`SHA256SUMS` lists the files in `pin.rs`'s order, in `sha256sum` format, so
`sha256sum -c SHA256SUMS` checks a downloaded release. The hash is in the tag
because upstream rebuilds its data and model often (FINDINGS §5 has them
rebuilt daily), and nothing guarantees each rebuild changes `version.txt`.
Two different builds must never share a release.

`pin.rs` carries the tag as `ARCHIVE_TAG`, and `../assets/build.rs`
recomputes it from the table on every build. A table edited by hand without
its tag fails the build with the tag it should be.

## What uses it

- **`gitaxian-probe-assets`** takes the pinned build from the archive when
  `GITAXIAN_PROBE_ARCHIVE_TOKEN` holds a token that can read it. The order is
  its build cache, `GITAXIAN_PROBE_ASSETS_FROM`, the archive, then Delver. So
  with the token set, any pin builds whatever Delver is serving that day, and
  every file is still checked against the pin. The Scan button's
  `MELDWEB_PROBE=1 pnpm dev` runs that same build and inherits the variable.
- **`.github/workflows/delver-archive.yml`**, daily and on demand:
  `archive.py fetch` what Delver serves, `publish` it to the archive if it is
  new, and when it is not the pinned build, `pin` it on a branch and open a PR.
  It opens one PR per build, ever: a PR closed without merging stays closed.
- **`.github/workflows/probe-web-check.yml`** runs `web-check/run.sh` on every
  PR that touches the probe, the pin PRs included, with ImageMagick-made
  fixture frames. That check says whether the new build still fingerprints as
  `KNOWN_FINGERPRINT` and still scores 6/6 on card name and 4/6 on exact
  printing. The workflow does not merge anything. A moved fingerprint or a
  moved number needs someone to read FINDINGS.md first.

## By hand

```sh
cd crates/gitaxian-probe
python3 archive/archive.py fetch /tmp/delver    # prints version, tag, and whether it is the pinned build
GH_TOKEN=… python3 archive/archive.py publish /tmp/delver
python3 archive/archive.py pin /tmp/delver      # rewrites assets/src/pin.rs
```

`publish` needs the `gh` CLI. `ARCHIVE_REPO=owner/name` points it somewhere
other than `pin.rs`'s `ARCHIVE`.

## Setting it up

1. Create the repo, private and empty:
   `gh repo create cramt/gitaxian-probe-archive --private`.
2. Create a fine-grained personal access token, owned by `cramt`, for the two
   repos:
   - `cramt/gitaxian-probe-archive`: Contents read and write, which is what
     creating releases needs.
   - `cramt/progress-engine`: Contents and Pull requests read and write, to
     push the pin branch and open its PR.
3. Save it as the `GITAXIAN_PROBE_ARCHIVE_TOKEN` Actions secret in
   `cramt/progress-engine`. The pin PR is pushed with this token, not
   `GITHUB_TOKEN`, because a PR opened with `GITHUB_TOKEN` runs no workflows,
   and the web check is what reviews the new build.
4. Run **Delver archive** once from the Actions tab (`workflow_dispatch`). If
   Delver still serves the pinned build, that archives it and opens nothing.
5. For local builds, a second token with only Contents read on the archive is
   enough: `export GITAXIAN_PROBE_ARCHIVE_TOKEN=…`.

Until step 4 has run, the pinned build exists only on Delver's origin and in
whatever build caches have it.
