# The Delver X archive

Every Delver X engine build that [Gitaxian Probe](../) might be pinned to, kept
after upstream has stopped serving it, as one tag each of the public OCI
artifact `ghcr.io/cramt/delver-x`. The artifact is only a vehicle for the
files: no image, no code.

**It is public**, so that a build needs no credentials anywhere: ghcr.io hands
an anonymous pull token to anybody, where GitHub's npm registry wants a token
even for a public package. That makes the archive a place anyone can download
Delver Lab's engine from, which is redistribution; it was chosen on
2026-09-28, knowing that. A GitHub package made public cannot be made private
again. `docs/research/probe-in-curator.md` covers what shipping the engine in
the site would still need.

## Why

`https://mtg.delver.app` serves only its current build, and it ships often:
1.83.beta to 1.89.beta took about a week. A pin stops building the day upstream
moves, and a build nobody saved the day it shipped is gone.

## What is in a tag

The files exactly as upstream served them (`version.txt`, `core.js`,
`core.wasm`, `data.7z`, `data.md5`, `data.size`, `model-alpha.7z`,
`model-alpha.size`) and a `SHA256SUMS`, about 42 MB. Each is its own blob,
pushed as it is rather than tarred, so **a blob's digest is the file's
sha256**: the pin in `assets/pin.json` is a list of blob addresses, and a file
is at `ghcr.io/v2/cramt/delver-x/blobs/sha256:<its sha256>`. Only the alpha
tier is kept: lambda and gamma are gated behind a token, and the probe refuses
to boot them anyway.

A build's tag is:

```
delver-<version.txt>-<first 12 hex of sha256(SHA256SUMS)>
e.g. delver-1.89.beta-eeb9c6a9c3ec
```

`SHA256SUMS` lists the files in the order above, in `sha256sum` format. The
hash is in the tag because upstream rebuilds its data and model often (the
probe's FINDINGS §5 has them rebuilt daily), and nothing guarantees each
rebuild changes `version.txt`. Two different builds must never share a tag.
`latest` is the newest one pushed, and the manifest's
`org.opencontainers.image.version` annotation carries `version.txt`.

**The file list and the tag rule are a contract with the probe.**
`assets/pin.json` pins the same files in the same order, `assets/build.rs`
recomputes the tag from them, and `assets/repin.py` writes the pin from a
manifest. Change one side and you must change the other.

## How builds get here

`.github/workflows/probe-archive.yml` runs daily, and on demand from the
Actions tab. It `fetch`es what Delver serves and `publish`es it with
[oras](https://oras.land), unless that tag already exists. It needs nothing but
the workflow's own `GITHUB_TOKEN` with `packages: write`.

By hand, after `oras login ghcr.io` with a token that can write packages:

```sh
python3 archive.py fetch /tmp/delver      # prints the version and the tag
python3 archive.py publish /tmp/delver
```

## Reading it

Nothing needs a login:

```sh
oras manifest fetch ghcr.io/cramt/delver-x:latest
oras pull ghcr.io/cramt/delver-x:delver-1.89.beta-eeb9c6a9c3ec -o /tmp/delver
```

The engine README's *The archive* says how the probe's build, the flake and
the pin workflow use it.
