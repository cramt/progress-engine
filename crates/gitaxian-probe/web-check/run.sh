#!/usr/bin/env bash
# Build the web check, serve it cross-origin isolated, and run it in headless
# Chromium - once on the page and once inside a module worker. Exits non-zero
# unless both match the native accuracy numbers (card name 6/6, exact printing
# 4/6).
#
# Needs: the wasm32-unknown-unknown target, a wasm-bindgen CLI matching the
# wasm-bindgen crate in Cargo.lock, python3, node with playwright, and the
# fixture frames (engine/.fixtures/fetch-cards.sh). CHROMIUM overrides the
# browser playwright launches.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
probe="$(dirname "$here")"
out="$probe/target/web-check"

for slug in lotus counterspell llanowar shock swords thoughtseize; do
  [ -f "$probe/engine/.fixtures/$slug-frame.jpg" ] || {
    echo "no $slug frame; run engine/.fixtures/fetch-cards.sh first" >&2
    exit 1
  }
done

want=$(grep -A1 '^name = "wasm-bindgen"$' "$probe/Cargo.lock" | sed -n 's/^version = "\(.*\)"/\1/p')
have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}') || true
[ "$want" = "$have" ] || {
  echo "wasm-bindgen CLI is ${have:-missing}, Cargo.lock has $want:" >&2
  echo "  cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
}

cargo build --manifest-path "$probe/Cargo.toml" --release \
  -p gitaxian-probe-web-check --target wasm32-unknown-unknown
rm -rf "$out" && mkdir -p "$out/fixtures"
wasm-bindgen --target web --out-dir "$out/pkg" \
  "$probe/target/wasm32-unknown-unknown/release/gitaxian_probe_web_check.wasm"
cargo run --manifest-path "$probe/Cargo.toml" --quiet \
  -p gitaxian-probe-assets --example copy -- "$out/gitaxian-probe"
cp "$probe"/engine/.fixtures/*-frame.jpg "$out/fixtures/"
cp "$here/index.html" "$here/frames.js" "$here/worker.js" "$out/"

port=${PORT:-8791}
python3 "$here/serve.py" "$out" "$port" &
server=$!
trap 'kill $server 2>/dev/null' EXIT
node "$here/drive.mjs" "http://127.0.0.1:$port/"
node "$here/drive.mjs" "http://127.0.0.1:$port/?worker"
