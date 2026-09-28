#!/usr/bin/env python3
"""Point src/pin.rs at a build the archive holds.

    repin.py <package dir>

`package dir` is one version of the archive's npm package, unpacked
(@cramt/delver-x on GitHub Packages; archive/README.md says how versions are
made): its package.json, version.txt and SHA256SUMS are read. The npm version
has to be the one the archive names that build by, and the files have to be
the ones pin.rs pins; otherwise nothing is written. Prints `changed=true` or
`changed=false`, and hands it to a later step under GitHub Actions.
"""

import hashlib
import json
import os
import re
import sys
from pathlib import Path

PIN = Path(__file__).resolve().parent / "src" / "pin.rs"


def npm_version(version: str, sums: str) -> str:
    # The rule archive/archive.py publishes by and build.rs checks.
    digest = hashlib.sha256(sums.encode()).hexdigest()[:12]
    return "0.0.0-" + re.sub(r"[^0-9A-Za-z-]", "-", f"delver-{version}-{digest}")


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    d = Path(sys.argv[1])
    published = json.loads((d / "package.json").read_text())["version"]
    version = (d / "version.txt").read_text().strip()
    sums = (d / "SHA256SUMS").read_text()
    if npm_version(version, sums) != published:
        sys.exit(f"{published} does not name Delver X {version} with that SHA256SUMS")

    pinned = [(sha, name) for sha, name in (l.split("  ", 1) for l in sums.splitlines())]
    text = PIN.read_text()
    names = re.findall(r'name: "([^"]+)",\n\s*sha256:', text)
    if [n for _, n in pinned] != names:
        sys.exit(f"{published} holds {[n for _, n in pinned]}, pin.rs pins {names}, in that order")

    new = text
    for sha, name in pinned:
        new = re.sub(rf'(name: "{re.escape(name)}",\n\s*sha256: ")[0-9a-f]+', rf"\g<1>{sha}", new)
    new = re.sub(r'(pub const VERSION: &str = ")[^"]*', rf"\g<1>{version}", new)
    new = re.sub(r'(pub const ARCHIVE_VERSION: &str = ")[^"]*', rf"\g<1>{published}", new)
    changed = new != text
    if changed:
        PIN.write_text(new)
    line = f"changed={str(changed).lower()}"
    print(line)
    if path := os.environ.get("GITHUB_OUTPUT"):
        with open(path, "a") as f:
            f.write(line + "\n")


if __name__ == "__main__":
    main()
