#!/usr/bin/env python3
"""Point src/pin.rs at a build the archive holds.

    repin.py <tag> <SHA256SUMS>

`tag` and `SHA256SUMS` are one release of the private archive
(cramt/gitaxian-probe-archive; its README says how releases are made). The tag
has to be the one the archive names that SHA256SUMS by, and the files have to
be the ones pin.rs pins; otherwise nothing is written. Prints `changed=true`
or `changed=false`, and hands it to a later step under GitHub Actions.
"""

import hashlib
import os
import re
import sys
from pathlib import Path

PIN = Path(__file__).resolve().parent / "src" / "pin.rs"


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    tag, sums = sys.argv[1], Path(sys.argv[2]).read_text()
    m = re.fullmatch(r"delver-(.+)-([0-9a-f]{12})", tag)
    if not m:
        sys.exit(f"{tag} is not an archive tag")
    version, digest = m.groups()
    if hashlib.sha256(sums.encode()).hexdigest()[:12] != digest:
        sys.exit(f"{tag} does not name that SHA256SUMS")

    pinned = [(sha, name) for sha, name in (l.split("  ", 1) for l in sums.splitlines())]
    text = PIN.read_text()
    names = re.findall(r'name: "([^"]+)",\n\s*sha256:', text)
    if [n for _, n in pinned] != names:
        sys.exit(f"{tag} holds {[n for _, n in pinned]}, pin.rs pins {names}, in that order")

    new = text
    for sha, name in pinned:
        new = re.sub(rf'(name: "{re.escape(name)}",\n\s*sha256: ")[0-9a-f]+', rf"\g<1>{sha}", new)
    new = re.sub(r'(pub const VERSION: &str = ")[^"]*', rf"\g<1>{version}", new)
    new = re.sub(r'(pub const ARCHIVE_TAG: &str = ")[^"]*', rf"\g<1>{tag}", new)
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
