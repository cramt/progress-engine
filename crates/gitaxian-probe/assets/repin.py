#!/usr/bin/env python3
"""Point pin.json at a build the archive holds.

    repin.py <manifest.json>

`manifest.json` is one tag's OCI manifest from the archive
(ghcr.io/cramt/delver-x, as `oras manifest fetch` prints it; archive/README.md
says how builds get there). Its layers have to be the files pin.json pins, in
the same order, plus SHA256SUMS; otherwise nothing is written. Prints
`changed=true` or `changed=false`, and hands it to a later step under GitHub
Actions.
"""

import hashlib
import json
import os
import sys
from pathlib import Path

PIN = Path(__file__).resolve().parent / "pin.json"
TITLE = "org.opencontainers.image.title"
VERSION = "org.opencontainers.image.version"


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    manifest = json.loads(Path(sys.argv[1]).read_text())
    version = manifest.get("annotations", {}).get(VERSION)
    if not version:
        sys.exit(f"the manifest has no {VERSION} annotation")
    layers = [
        (layer["annotations"][TITLE], layer["digest"].removeprefix("sha256:"))
        for layer in manifest["layers"]
        if layer["annotations"][TITLE] != "SHA256SUMS"
    ]

    pin = json.loads(PIN.read_text())
    names = [f["name"] for f in pin["files"]]
    if [n for n, _ in layers] != names:
        sys.exit(f"the manifest holds {[n for n, _ in layers]}, pin.json pins {names}, in that order")

    # The tag rule archive/archive.py publishes by and build.rs checks.
    sums = "".join(f"{sha}  {name}\n" for name, sha in layers)
    tag = f"delver-{version}-{hashlib.sha256(sums.encode()).hexdigest()[:12]}"
    new = {
        "version": version,
        "tag": tag,
        "files": [{"name": name, "sha256": sha} for name, sha in layers],
    }
    changed = new != pin
    if changed:
        PIN.write_text(json.dumps(new, indent=2) + "\n")
    line = f"changed={str(changed).lower()}"
    print(line)
    if path := os.environ.get("GITHUB_OUTPUT"):
        with open(path, "a") as f:
            f.write(line + "\n")


if __name__ == "__main__":
    main()
