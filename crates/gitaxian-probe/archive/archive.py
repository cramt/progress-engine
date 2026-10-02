#!/usr/bin/env python3
"""Archive the Delver X build https://mtg.delver.app is serving right now.

Delver serves only its current build, so one not saved the day it ships is
gone. Each is kept in the public OCI artifact ghcr.io/cramt/delver-x as one
tag per model tier: the engine and that tier's model exactly as served, and a
SHA256SUMS, each its own blob, so a blob's digest is the file's sha256 and a
pin can fetch it by that alone. The engine's blobs are shared between a build's
tags, so the registry holds them once. See README.md.

    archive.py fetch <dir>     download upstream's current build into <dir>/<tier>/
    archive.py publish <dir>   push each <dir>/<tier>/ as its tag, unless it exists (oras)

A tier's tag is `delver-<version>-<tier>-<first 12 hex of sha256(SHA256SUMS)>`,
so a rebuild upstream ships under an unchanged version string still gets its
own. SHA256SUMS lists ENGINE and then the tier's model, in `sha256sum` format.
Gitaxian Probe's pin (assets/pin.json) is checked against the same rule by
assets/build.rs, so these lists and that rule are a contract with it: change
either there too.
"""

import hashlib
import os
import subprocess
import sys
import urllib.request
from pathlib import Path

ORIGIN = "https://mtg.delver.app"
USER_AGENT = "gitaxian-probe-archive/0.1"
ARTIFACT = "ghcr.io/cramt/delver-x"
ARTIFACT_TYPE = "application/vnd.progress-engine.delver-x"
REPOSITORY = "https://github.com/cramt/progress-engine"

# The same lists, in the same order, as `engine` and `tiers` in
# crates/gitaxian-probe/assets/pin.json.
ENGINE = [
    "version.txt",
    "core.js",
    "core.wasm",
    "data.7z",
    "data.md5",
    "data.size",
]
# Lambda and gamma download without a token, which gates booting them and not
# fetching them. The probe refuses to boot them so far; they are kept so that a
# host holding a token has every build's weights.
TIERS = ["alpha", "lambda", "gamma"]


def model(tier: str) -> list[str]:
    return [f"model-{tier}.7z", f"model-{tier}.size"]


def tag_of(version: str, tier: str, sums: str) -> str:
    return f"delver-{version}-{tier}-{hashlib.sha256(sums.encode()).hexdigest()[:12]}"


def get(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=300) as r:
        return r.read()


def output(**values: str) -> None:
    """Print, and hand to a later workflow step when run in GitHub Actions."""
    for k, v in values.items():
        print(f"{k}={v}")
    if path := os.environ.get("GITHUB_OUTPUT"):
        with open(path, "a") as f:
            for k, v in values.items():
                f.write(f"{k}={v}\n")


def fetch(d: Path) -> None:
    d.mkdir(parents=True, exist_ok=True)
    before = get(f"{ORIGIN}/version.txt")
    sha = {}
    for name in [*ENGINE, *(n for t in TIERS for n in model(t))]:
        data = before if name == "version.txt" else get(f"{ORIGIN}/{name}")
        (d / name).write_bytes(data)
        sha[name] = hashlib.sha256(data).hexdigest()
    # Upstream deploys the bundle atomically, but not atomically with us: a
    # deploy between the first request and the last would mix two builds.
    after = get(f"{ORIGIN}/version.txt")
    if after != before:
        sys.exit(f"upstream moved from {before!r} to {after!r} mid-fetch; run again")
    version = before.decode().strip()
    tags = {}
    # One directory per tag, because oras names each layer by its path and
    # every tag's checksum file has to be called SHA256SUMS.
    for tier in TIERS:
        t = d / tier
        t.mkdir(exist_ok=True)
        sums = ""
        for name in [*ENGINE, *model(tier)]:
            (t / name).unlink(missing_ok=True)
            os.link(d / name, t / name)
            sums += f"{sha[name]}  {name}\n"
        (t / "SHA256SUMS").write_text(sums)
        tags[tier] = tag_of(version, tier, sums)
    output(version=version, **tags)


def oras(*args: str, cwd: Path, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(["oras", *args], cwd=cwd, check=check, capture_output=True, text=True)


def publish(d: Path) -> None:
    for tier in TIERS:
        t = d / tier
        sums = (t / "SHA256SUMS").read_text()
        version = (t / "version.txt").read_text().strip()
        tag = tag_of(version, tier, sums)
        if oras("manifest", "fetch", f"{ARTIFACT}:{tag}", cwd=t, check=False).returncode == 0:
            output(**{tier: tag, f"{tier}_archived": "already"})
            continue
        # Each file is pushed as it is, not tarred, so its blob's digest is its
        # sha256. `source` links the package to progress-engine; `version` is
        # what assets/repin.py reads back.
        layers = [f"{n}:application/octet-stream" for n in [*ENGINE, *model(tier), "SHA256SUMS"]]
        oras("push", f"{ARTIFACT}:{tag}", "--artifact-type", ARTIFACT_TYPE,
             "--annotation", f"org.opencontainers.image.source={REPOSITORY}",
             "--annotation", f"org.opencontainers.image.version={version}",
             "--annotation", f"org.opencontainers.image.description=Delver X {version} with the {tier} model, as {ORIGIN} served it",
             *layers, cwd=t)
        # `latest-<tier>` is what probe-pin reads as the newest build.
        oras("tag", f"{ARTIFACT}:{tag}", f"latest-{tier}", cwd=t)
        output(**{tier: tag, f"{tier}_archived": "new"})


def main() -> None:
    commands = {"fetch": fetch, "publish": publish}
    if len(sys.argv) != 3 or sys.argv[1] not in commands:
        sys.exit(__doc__)
    commands[sys.argv[1]](Path(sys.argv[2]))


if __name__ == "__main__":
    main()
