#!/usr/bin/env bash
# The reference frames the engine's accuracy tests read: the scans cards.nix
# pins, each framed the way a camera would see it. The flake builds them, the
# same frames its checks test against, and this copies them here for a plain
# `cargo test`.
set -euo pipefail
cd "$(dirname "$0")"
frames=$(nix build --no-link --print-out-paths "$(git rev-parse --show-toplevel)#gitaxian-probe-frames")
cp --no-preserve=mode "$frames"/*-frame.jpg .
ls -1 *-frame.jpg
