# The Delver X archive

Every Delver X engine build that [Gitaxian Probe](../) might be pinned to, kept
after upstream has stopped serving it, as one version each of the npm package
`@cramt/delver-x` on GitHub Packages. The package is only a vehicle for the
files: it has no code, and nothing installs it.

**The package must stay private.** It is a build cache for progress-engine, not
a place anyone downloads Delver's engine from. The files are Delver Lab's, and
serving them to anyone else is redistribution;
`docs/research/probe-in-curator.md` explains the distinction. A package on
GitHub Packages starts private, and linking it to this public repo gives it the
repo's access, not its visibility. Making it public cannot be undone.

## Why

`https://mtg.delver.app` serves only its current build, and it ships often:
1.83.beta to 1.89.beta took about a week. A pin stops building the day upstream
moves, and a build nobody saved the day it shipped is gone.

## What is in a version

The files exactly as upstream served them (`version.txt`, `core.js`,
`core.wasm`, `data.7z`, `data.md5`, `data.size`, `model-alpha.7z`,
`model-alpha.size`), a `SHA256SUMS`, and the `package.json`: about 36 MB as a
tarball. Only the alpha tier is kept: lambda and gamma are gated behind a token,
and the probe refuses to boot them anyway.

A build's version is:

```
0.0.0-delver-<version.txt>-<first 12 hex of sha256(SHA256SUMS)>
with every character but [0-9A-Za-z-] turned into -
e.g. 0.0.0-delver-1-89-beta-eeb9c6a9c3ec
```

npm wants semver, and `1.89.beta` is not, so the whole identity is one
prerelease identifier. `SHA256SUMS` lists the files in the order above, in
`sha256sum` format, so `sha256sum -c SHA256SUMS` checks an unpacked version. The
hash is in the version because upstream rebuilds its data and model often (the
probe's FINDINGS §5 has them rebuilt daily), and nothing guarantees each rebuild
changes `version.txt`. Two different builds must never share a version. The
`latest` dist-tag is the newest one published.

**The file list and the version rule are a contract with the probe.**
`assets/src/pin.rs` pins the same files in the same order, `assets/build.rs`
recomputes the version from them, and `assets/repin.py` checks it. Change one
side and you must change the other.

## How builds get here

`.github/workflows/probe-archive.yml` runs daily, and on demand from the
Actions tab. It `fetch`es what Delver serves and `publish`es it, unless that
version already exists. It needs nothing but the workflow's own `GITHUB_TOKEN`
with `packages: write`.

By hand, with an `.npmrc` that authenticates to `npm.pkg.github.com` (a classic
token with `write:packages`):

```sh
python3 archive.py fetch /tmp/delver      # prints the version and the npm version
python3 archive.py publish /tmp/delver
```

## Reading it

The engine README's *The archive* says how the probe's build, pin and web check
use it. By hand:

```sh
npm view @cramt/delver-x versions --registry https://npm.pkg.github.com
npm pack @cramt/delver-x@latest --registry https://npm.pkg.github.com
```

Both need a classic token with `read:packages`; GitHub Packages takes no
fine-grained token.
