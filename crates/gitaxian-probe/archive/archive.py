#!/usr/bin/env python3
"""Archive the Delver X build https://mtg.delver.app is serving right now.

Delver serves only its current build, so one not saved the day it ships is
gone. Each is kept as one version of a private npm package on GitHub Packages:
the files exactly as served, plus a SHA256SUMS and a package.json, and no code.
See README.md.

    archive.py fetch <dir>     download upstream's current build into <dir>
    archive.py publish <dir>   publish <dir> as its version, unless it exists (npm)

A build's npm version is `0.0.0-delver-<version>-<first 12 hex of
sha256(SHA256SUMS)>`, with every character semver refuses in a prerelease
turned into `-`, so a rebuild upstream ships under an unchanged version string
still gets its own. SHA256SUMS lists FILES in order, in `sha256sum` format.
Gitaxian Probe's pin recomputes the version the same way (assets/build.rs), so
FILES and that rule are a contract with it: change either there too.
"""

import hashlib
import json
import os
import re
import subprocess
import sys
import urllib.request
from pathlib import Path

ORIGIN = "https://mtg.delver.app"
USER_AGENT = "gitaxian-probe-archive/0.1"
PACKAGE = "@cramt/delver-x"
REGISTRY = "https://npm.pkg.github.com"
REPOSITORY = "https://github.com/cramt/progress-engine"

# The alpha tier only: lambda and gamma are gated behind a token, and the probe
# refuses to boot them anyway. The same list, in the same order, as the PINNED
# table in crates/gitaxian-probe/assets/src/pin.rs.
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


def npm_version(version: str, sums: str) -> str:
    digest = hashlib.sha256(sums.encode()).hexdigest()[:12]
    return "0.0.0-" + re.sub(r"[^0-9A-Za-z-]", "-", f"delver-{version}-{digest}")


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
    (d / "package.json").write_text(json.dumps(package_json(version, sums), indent=2) + "\n")
    output(version=version, npm_version=npm_version(version, sums))


def package_json(version: str, sums: str) -> dict:
    # `repository` links the package to progress-engine, whose workflows can
    # then read it with their own GITHUB_TOKEN. The link carries access, not
    # visibility: the package stays private, and must, because making a package
    # public cannot be undone and these files are Delver Lab's.
    return {
        "name": PACKAGE,
        "version": npm_version(version, sums),
        "description": f"Delver X {version}, as {ORIGIN} served it. Not for redistribution.",
        "repository": {"type": "git", "url": f"git+{REPOSITORY}.git"},
        "license": "UNLICENSED",
        "files": [*FILES, "SHA256SUMS"],
        "publishConfig": {"registry": REGISTRY},
    }


def npm(*args: str, cwd: Path, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(["npm", *args], cwd=cwd, check=check, capture_output=True, text=True)


def publish(d: Path) -> None:
    version = json.loads((d / "package.json").read_text())["version"]
    seen = npm("view", f"{PACKAGE}@{version}", "version", "--registry", REGISTRY, cwd=d, check=False)
    if seen.returncode == 0 and seen.stdout.strip() == version:
        output(npm_version=version, archived="already")
        return
    # A prerelease version needs an explicit dist-tag; `latest` is what the
    # pin workflow reads as the newest build.
    npm("publish", "--tag", "latest", cwd=d)
    output(npm_version=version, archived="new")


def main() -> None:
    commands = {"fetch": fetch, "publish": publish}
    if len(sys.argv) != 3 or sys.argv[1] not in commands:
        sys.exit(__doc__)
    commands[sys.argv[1]](Path(sys.argv[2]))


if __name__ == "__main__":
    main()
