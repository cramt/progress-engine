#!/usr/bin/env bash
# Fetch the auto-captions of the rules channels the scenarios are distilled
# from, as plain text, into ~/.cache/chancellor/yt/<channel>/<id>.txt.
# The transcripts stay in the cache, never in the repo: the scenarios keep
# only what a video claims, checked against the CR, with a link back.
#
#   fetch.sh [max-per-channel]
#
# Skips what is cached, so it resumes.
set -euo pipefail

max=${1:-100000}
cache=${XDG_CACHE_HOME:-$HOME/.cache}/chancellor/yt
# Resolved once: `nix run` per call spent most of a minute per video.
ytdlp=$(nix build 'nixpkgs#yt-dlp^out' --no-link --print-out-paths)/bin/yt-dlp
[ -x "$ytdlp" ] || { echo "yt-dlp not built: $ytdlp" >&2; exit 1; }
yt() { "$ytdlp" "$@"; }

channels=(
  "keepingitcasualmtg https://www.youtube.com/@keepingitcasualmtg/shorts"
  "thisisacommanderchannel https://www.youtube.com/@thisisacommanderchannel/videos"
)

for entry in "${channels[@]}"; do
  read -r name url <<<"$entry"
  dir=$cache/$name
  mkdir -p "$dir/vtt"
  yt --flat-playlist --print "%(id)s	%(title)s" --playlist-end "$max" "$url" 2>/dev/null >"$dir/index.tsv"
  while IFS=$'\t' read -r id title; do
    [ -s "$dir/$id.txt" ] && continue
    vtt=$dir/vtt/$id.en.vtt
    # YouTube answers caption downloads in a burst with 429, so pace them
    # and back off when it does.
    for wait in 30 120 600; do
      log=$(yt --skip-download --write-auto-subs --sub-langs en --sub-format vtt \
        --sleep-subtitles 5 -o "$dir/vtt/%(id)s" \
        "https://www.youtube.com/watch?v=$id" 2>&1) || true
      [ -s "$vtt" ] && break
      grep -q '429' <<<"$log" || break
      echo "429 on $id, waiting ${wait}s" >&2
      sleep "$wait"
    done
    [ -s "$vtt" ] || { echo "no captions: $id $title" >&2; continue; }
    # Auto-captions repeat each line as the next cue scrolls in, and carry
    # word timings in <c> tags; keep each line once, untimed.
    {
      printf '# %s\n# https://www.youtube.com/watch?v=%s\n\n' "$title" "$id"
      grep -v -e '^WEBVTT' -e '^Kind:' -e '^Language:' -e '-->' -e '^\s*$' "$vtt" |
        sed -e 's/<[^>]*>//g' | awk '!seen[$0]++'
    } >"$dir/$id.txt"
  done <"$dir/index.tsv"
  echo "$name: $(ls "$dir"/*.txt 2>/dev/null | wc -l) of $(wc -l <"$dir/index.tsv") cached"
done
