# The Delver X archive

Every Delver X engine build that [Gitaxian Probe](../) might be pinned to, kept
after upstream has stopped serving it, as one tag per model tier in the public
OCI artifact `ghcr.io/cramt/delver-x`. The artifact is only a vehicle for the
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

A build is three tags, one per model tier: alpha, lambda and gamma. Each holds
the engine exactly as upstream served it (`version.txt`, `core.js`,
`core.wasm`, `data.7z`, `data.md5`, `data.size`), then that tier's
`model-<tier>.7z` and `model-<tier>.size`, and a `SHA256SUMS`. A tag is
self-contained, so pulling alpha's takes about 42 MB and never lambda's or
gamma's weights, and the registry keeps the engine's blobs once however many
tags hold them. Each file is its own blob, pushed as it is rather than tarred,
so **a blob's digest is the file's sha256**: the pin in `assets/pin.json` is a
list of blob addresses, and a file is at
`ghcr.io/v2/cramt/delver-x/blobs/sha256:<its sha256>`.

Lambda and gamma download from Delver without a token, which gates booting them
and not fetching them. The probe refuses to boot them so far (the engine
README, *Scope*); they are kept so a host that holds a token and can boot them
finds every build's weights here.

A tier's tag is:

```
delver-<version.txt>-<tier>-<first 12 hex of sha256(SHA256SUMS)>
e.g. delver-1.89.beta-alpha-eeb9c6a9c3ec
```

`SHA256SUMS` lists the engine and then the model in the order above, in
`sha256sum` format. The hash is in the tag because upstream rebuilds its data
and model often (the probe's FINDINGS §5 has them rebuilt daily), and nothing
guarantees each rebuild changes `version.txt`. Two different builds must never
share a tag. `latest-<tier>` is the newest one pushed for that tier, and the
manifest's `org.opencontainers.image.version` annotation carries
`version.txt`.

Tags from before 2026-10-02 are alpha only and carry no tier,
`delver-<version>-<12 hex>`, and `latest` is the last of them. They stay, since
a published tag is an address someone may hold, but nothing pushes or reads
them any more.

**The file lists and the tag rule are a contract with the probe.**
`assets/pin.json` pins the same files in the same order (the engine once, and
each tier's model with its tag), `assets/build.rs` recomputes each tag from
them, and `assets/repin.py` writes the pin from the three tiers' manifests.
Change one side and you must change the other.

## How builds get here

`.github/workflows/probe-archive.yml` runs daily, and on demand from the
Actions tab. It `fetch`es what Delver serves and `publish`es each tier's tag
with [oras](https://oras.land), unless that tag already exists. It needs nothing but
the workflow's own `GITHUB_TOKEN` with `packages: write`.

By hand, after `oras login ghcr.io` with a token that can write packages:

```sh
python3 archive.py fetch /tmp/delver      # prints the version and each tier's tag
python3 archive.py publish /tmp/delver
```

## Reading it

Nothing needs a login:

```sh
oras manifest fetch ghcr.io/cramt/delver-x:latest-alpha
oras pull ghcr.io/cramt/delver-x:delver-1.89.beta-alpha-eeb9c6a9c3ec -o /tmp/delver
```

The engine README's *The archive* says how the probe's build, the flake and
the pin workflow use it.
