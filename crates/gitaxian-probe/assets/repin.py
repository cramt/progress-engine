#!/usr/bin/env python3
"""Point pin.json at a build the archive holds.

    repin.py <manifest.json>...

Each `manifest.json` is one tag's OCI manifest from the archive
(ghcr.io/cramt/delver-x, as `oras manifest fetch` prints it; archive/README.md
says how builds get there), one per tier pin.json pins, in any order. A tag's
tier is the one its model is. Each one's layers have to be the engine pin.json
pins and then that tier's model, in that order, plus SHA256SUMS, and all of
them one build: the same version and the same engine. Otherwise nothing is
written. Prints `changed=true` or `changed=false`, and hands it to a later step
under GitHub Actions.
"""

import hashlib
import json
import os
import sys
from pathlib import Path

PIN = Path(__file__).resolve().parent / "pin.json"
TITLE = "org.opencontainers.image.title"
VERSION = "org.opencontainers.image.version"


def layers(manifest: dict) -> list[tuple[str, str]]:
    return [
        (layer["annotations"][TITLE], layer["digest"].removeprefix("sha256:"))
        for layer in manifest["layers"]
        if layer["annotations"][TITLE] != "SHA256SUMS"
    ]


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    pin = json.loads(PIN.read_text())
    engine_names = [f["name"] for f in pin["engine"]]
    tier_names = list(pin["tiers"])

    version, engine, tiers = None, None, {}
    for path in sys.argv[1:]:
        manifest = json.loads(Path(path).read_text())
        v = manifest.get("annotations", {}).get(VERSION)
        if not v:
            sys.exit(f"{path} has no {VERSION} annotation")
        held = layers(manifest)
        model = held[len(engine_names):]
        tier = next((t for t in tier_names if [n for n, _ in model] == [f"model-{t}.7z", f"model-{t}.size"]), None)
        if [n for n, _ in held[:len(engine_names)]] != engine_names or tier is None:
            sys.exit(f"{path} holds {[n for n, _ in held]}, where pin.json pins {engine_names} and then one tier's model")
        if tier in tiers:
            sys.exit(f"two manifests are the {tier} tier")
        if version is not None and (v, held[:len(engine_names)]) != (version, engine):
            sys.exit(f"{path} is a different build from the other manifests: they are not one release")
        version, engine = v, held[:len(engine_names)]
        tiers[tier] = model
    if missing := [t for t in tier_names if t not in tiers]:
        sys.exit(f"no manifest for {missing}")

    new = {
        "version": version,
        "engine": [{"name": n, "sha256": s} for n, s in engine],
        "tiers": {},
    }
    for tier in tier_names:
        # The tag rule archive/archive.py publishes by and build.rs checks.
        sums = "".join(f"{s}  {n}\n" for n, s in [*engine, *tiers[tier]])
        new["tiers"][tier] = {
            "tag": f"delver-{version}-{tier}-{hashlib.sha256(sums.encode()).hexdigest()[:12]}",
            "model": [{"name": n, "sha256": s} for n, s in tiers[tier]],
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
