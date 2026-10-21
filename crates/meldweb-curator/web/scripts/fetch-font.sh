#!/usr/bin/env bash
# EB Garamond, the brand's serif, subset to Latin from google/fonts at a pinned
# commit. Subset here rather than taken from @fontsource/eb-garamond, whose
# files drop `smcp` and `c2sc`: the wordmark and headings are real small caps,
# and a browser left without them shrinks capitals instead.
# Run to re-pin: `nix shell --impure --expr 'with import <nixpkgs> {}; [curl (python3.withPackages (p: [p.fonttools p.brotli]))]' -c scripts/fetch-font.sh`
set -euo pipefail
cd "$(dirname "$0")/../public/fonts"
rev=f8c1d3d6cc75e30d77130bdcbfbff27e3b6233fe
base="https://raw.githubusercontent.com/google/fonts/$rev/ofl/ebgaramond"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -sfL "$base/OFL.txt" -o OFL.txt
for face in EBGaramond EBGaramond-Italic; do
  curl -sfL "$base/$face%5Bwght%5D.ttf" -o "$tmp/$face.ttf"
  python3 -I -m fontTools.subset "$tmp/$face.ttf" \
    --unicodes='U+0000-00FF,U+0131,U+0152-0153,U+02BB-02BC,U+02C6,U+02DA,U+02DC,U+2000-206F,U+20AC,U+2122,U+2190-2193,U+2212,U+2215' \
    --layout-features='kern,liga,smcp,c2sc,onum,lnum,pnum,tnum,case' \
    --flavor=woff2 --output-file="$face.woff2"
done
