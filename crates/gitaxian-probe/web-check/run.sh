#!/usr/bin/env bash
# The web check, as the flake runs it: build the check page, serve it
# cross-origin isolated, and run it in headless Chromium, once on the page and
# once inside a module worker, for each of the three model tiers. Fails unless
# all six match the native accuracy numbers (card name 6/6, exact printing 4/6
# for every tier), and prints the report.
#
# A check whose inputs have not changed is not run again; pass --rebuild to
# run it anyway.
set -euo pipefail
root=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
system=$(nix eval --impure --raw --expr builtins.currentSystem)
cat "$(nix build -L --no-link --print-out-paths "$@" "$root#checks.$system.gitaxian-probe-web-check")"
