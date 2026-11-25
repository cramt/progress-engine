#!/usr/bin/env bash
# usage: lock.sh take | release
# One overseer tick at a time. The scheduled task fires every 30 minutes, and a
# tick that reviews a batch runs longer; on 2026-11-25 two ticks shared the
# thread's worktree and one checked out origin/main under the other's build.
# The lock holds the pid of this session's `claude` process, so a tick that
# died leaves a lock nobody honours.
set -euo pipefail
LOCK=/pool/dev/progress-engine-insight-state/overseer.lock

session_pid() {
  local p=$$
  while [ "$p" -gt 1 ]; do
    # nix wraps the binary, so its comm is .claude-wrapped
    case "$(ps -o comm= -p "$p")" in *claude*) echo "$p"; return ;; esac
    p=$(ps -o ppid= -p "$p" | tr -d ' ')
  done
  echo "lock.sh: no claude ancestor" >&2
  exit 2
}

me=$(session_pid)
case "${1:-}" in
take)
  if [ -f "$LOCK" ]; then
    held=$(cat "$LOCK")
    if [ "$held" != "$me" ] && kill -0 "$held" 2>/dev/null; then
      echo "busy: overseer session pid $held holds the tick"
      exit 1
    fi
  fi
  echo "$me" >"$LOCK"
  echo "taken by $me"
  ;;
release)
  [ -f "$LOCK" ] && [ "$(cat "$LOCK")" = "$me" ] && rm -f "$LOCK"
  echo released
  ;;
*)
  echo "usage: lock.sh take | release" >&2
  exit 2
  ;;
esac
