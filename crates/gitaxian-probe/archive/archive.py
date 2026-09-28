#!/usr/bin/env python3
"""Archive the Delver X build https://mtg.delver.app is serving right now.

Delver serves only its current build, so one not saved the day it ships is
gone. This repo keeps each as a release: the files exactly as served, plus a
SHA256SUMS. See README.md.

    archive.py fetch <dir>     download upstream's current build into <dir>
    archive.py publish <dir>   make <dir>'s release here, unless it has one (gh, GH_TOKEN)

A build's tag is `delver-<version>-<first 12 hex of sha256(SHA256SUMS)>`, so a
rebuild upstream ships under an unchanged version string still gets its own.
SHA256SUMS lists FILES in order, in `sha256sum` format. Gitaxian Probe's pin
recomputes the tag the same way (progress-engine's assets/build.rs), so FILES
and that rule are a contract with it: change either there too.
"""

import hashlib
import os
import subprocess
import sys
import urllib.request
from pathlib import Path

ORIGIN = "https://mtg.delver.app"
USER_AGENT = "gitaxian-probe-archive/0.1"

# The alpha tier only: lambda and gamma are gated behind a token, and the probe
# refuses to boot them anyway. The same list, in the same order, as the PINNED
# table in progress-engine's crates/gitaxian-probe/assets/src/pin.rs.
FILES = [
    "version.txt",
    "core.js",
    "core.wasm",
    "data.7z",
    "data.md5",
    "data.size",
    "model-alpha.7z",
    "model-alpha.size",
]


def tag_of(version: str, sums: str) -> str:
    return f"delver-{version}-{hashlib.sha256(sums.encode()).hexdigest()[:12]}"


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
    sums = ""
    for name in FILES:
        data = before if name == "version.txt" else get(f"{ORIGIN}/{name}")
        (d / name).write_bytes(data)
        sums += f"{hashlib.sha256(data).hexdigest()}  {name}\n"
    # Upstream deploys the bundle atomically, but not atomically with us: a
    # deploy between the first request and the last would mix two builds.
    after = get(f"{ORIGIN}/version.txt")
    if after != before:
        sys.exit(f"upstream moved from {before!r} to {after!r} mid-fetch; run again")
    (d / "SHA256SUMS").write_text(sums)
    version = before.decode().strip()
    output(version=version, tag=tag_of(version, sums))


def gh(*args: str, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(["gh", *args], check=check, capture_output=True, text=True)


def publish(d: Path) -> None:
    sums = (d / "SHA256SUMS").read_text()
    version = (d / "version.txt").read_text().strip()
    tag = tag_of(version, sums)
    repo = os.environ.get("ARCHIVE_REPO") or os.environ["GITHUB_REPOSITORY"]
    if gh("release", "view", tag, "--repo", repo, check=False).returncode == 0:
        output(tag=tag, archived="already")
        return
    notes = f"Delver X {version}, as {ORIGIN} served it.\n\n```\n{sums}```\n"
    files = [str(d / n) for n in FILES] + [str(d / "SHA256SUMS")]
    gh("release", "create", tag, "--repo", repo, "--title", f"Delver X {version}",
       "--notes", notes, *files)
    output(tag=tag, archived="new")


def main() -> None:
    commands = {"fetch": fetch, "publish": publish}
    if len(sys.argv) != 3 or sys.argv[1] not in commands:
        sys.exit(__doc__)
    commands[sys.argv[1]](Path(sys.argv[2]))


if __name__ == "__main__":
    main()
