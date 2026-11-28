#!/usr/bin/env bash
# Push a gated main. Rebases onto origin/main first; if what came in can change
# the build (anything but .claude/ and Markdown), refuses with exit 2 so the
# gate is rerun on the rebased tree. Batch 29 pushed over a rebase by judgement;
# this makes that judgement the script's.
set -euo pipefail
git fetch -q origin
before=$(git rev-parse HEAD)
incoming=$(git rev-list HEAD..origin/main)
if [ -n "$incoming" ]; then
  git rebase -q origin/main
  code=$(git diff --name-only "$before"...origin/main | grep -v -E '^\.claude/|\.md$' || true)
  if [ -n "$code" ]; then
    echo "rebased onto $(git rev-parse --short origin/main), which changed:" >&2
    echo "$code" | sed 's/^/  /' >&2
    echo "rerun the gate on this tree, then push.sh again" >&2
    exit 2
  fi
  echo "rebased over $(echo "$incoming" | wc -l) commit(s) touching only skills/docs; gate stands"
fi
git push -q origin HEAD:main
echo "pushed $(git rev-parse --short HEAD)"
