#!/usr/bin/env python3
"""Keep every Delver X build the probe might be pinned to.

Upstream serves only its current build (engine README, "On the web"), so a pin
stops building the day Delver ships. The archive is a private GitHub repo that
holds nothing but releases: one per build, the files exactly as upstream served
them, plus a SHA256SUMS. `gitaxian-probe-assets` fetches a pinned build from it
once upstream has moved on (archive/README.md).

    archive.py fetch <dir>     download upstream's current build into <dir>
    archive.py publish <dir>   make <dir>'s release in the archive, unless it has one (gh, GH_TOKEN)
    archive.py pin <dir>       point assets/src/pin.rs at <dir>'s build

A build's tag is `delver-<version>-<first 12 hex of sha256(SHA256SUMS)>`, so a
rebuild upstream ships under an unchanged version string still gets its own.
SHA256SUMS lists the files in pin.rs's order, in `sha256sum` format, and
assets/build.rs recomputes the tag from pin.rs the same way.
"""

import hashlib
import os
import re
import subprocess
import sys
import urllib.request
from pathlib import Path

PIN = Path(__file__).resolve().parent.parent / "assets" / "src" / "pin.rs"
USER_AGENT = "gitaxian-probe-archive/0.1"


def pin_text() -> str:
    return PIN.read_text()


def pin_const(name: str) -> str:
    m = re.search(rf'pub const {name}: &str = "([^"]*)";', pin_text())
    if not m:
        sys.exit(f"{PIN} has no {name}")
    return m.group(1)


def pinned_names() -> list[str]:
    """The files a build is made of: pin.rs's table, in its order."""
    return re.findall(r'name: "([^"]+)",\n\s*sha256:', pin_text())


def sums_text(hashes: list[tuple[str, str]]) -> str:
    return "".join(f"{sha}  {name}\n" for name, sha in hashes)


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


def read_build(d: Path) -> tuple[str, str, str]:
    """(version, SHA256SUMS, tag) of a directory `fetch` filled."""
    sums = (d / "SHA256SUMS").read_text()
    version = (d / "version.txt").read_text().strip()
    return version, sums, tag_of(version, sums)


def fetch(d: Path) -> None:
    origin = pin_const("ORIGIN")
    d.mkdir(parents=True, exist_ok=True)
    before = get(f"{origin}/version.txt")
    hashes = []
    for name in pinned_names():
        data = before if name == "version.txt" else get(f"{origin}/{name}")
        (d / name).write_bytes(data)
        hashes.append((name, hashlib.sha256(data).hexdigest()))
    # Upstream deploys the bundle atomically, but not atomically with us: a
    # deploy between the first request and the last would mix two builds.
    after = get(f"{origin}/version.txt")
    if after != before:
        sys.exit(f"upstream moved from {before!r} to {after!r} mid-fetch; run again")
    sums = sums_text(hashes)
    (d / "SHA256SUMS").write_text(sums)
    version = before.decode().strip()
    tag = tag_of(version, sums)
    output(version=version, tag=tag, pinned=str(tag == pin_const("ARCHIVE_TAG")).lower())


def gh(*args: str, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(["gh", *args], check=check, capture_output=True, text=True)


def publish(d: Path) -> None:
    version, sums, tag = read_build(d)
    repo = os.environ.get("ARCHIVE_REPO") or pin_const("ARCHIVE")
    if gh("release", "view", tag, "--repo", repo, check=False).returncode == 0:
        output(tag=tag, archived="already")
        return
    files = [str(d / n) for n in pinned_names()] + [str(d / "SHA256SUMS")]
    notes = (
        f"Delver X {version}, as {pin_const('ORIGIN')} served it.\n\n"
        f"```\n{sums}```\n"
    )
    gh("release", "create", tag, "--repo", repo, "--title", f"Delver X {version}",
       "--notes", notes, *files)
    output(tag=tag, archived="new")


def pin(d: Path) -> None:
    version, sums, tag = read_build(d)
    got = {name: sha for sha, name in (l.split("  ", 1) for l in sums.splitlines())}
    if set(got) != set(pinned_names()):
        sys.exit(f"{d} holds {sorted(got)}, pin.rs pins {sorted(pinned_names())}")
    text = pin_text()
    for name, sha in got.items():
        text = re.sub(
            rf'(name: "{re.escape(name)}",\n\s*sha256: ")[0-9a-f]+',
            rf"\g<1>{sha}",
            text,
        )
    text = re.sub(r'(pub const VERSION: &str = ")[^"]*', rf"\g<1>{version}", text)
    text = re.sub(r'(pub const ARCHIVE_TAG: &str = ")[^"]*', rf"\g<1>{tag}", text)
    PIN.write_text(text)
    output(version=version, tag=tag)


def main() -> None:
    commands = {"fetch": fetch, "publish": publish, "pin": pin}
    if len(sys.argv) != 3 or sys.argv[1] not in commands:
        sys.exit(__doc__)
    commands[sys.argv[1]](Path(sys.argv[2]))


if __name__ == "__main__":
    main()
