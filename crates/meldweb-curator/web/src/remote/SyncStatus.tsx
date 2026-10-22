import { Link, useRouter } from "@tanstack/react-router";
import { useEffect, useMemo } from "react";
import { connect } from "../github/connect";
import { hostName, type Remote, useSyncState } from "./index";
import type { SyncState } from "./sync";
import "./remote.css";

const remoteOf = (): Promise<Remote | null> =>
  connect().then((c) => (c.kind === "local" ? c.remote : null));

/** What the corner says about syncing, or null while it goes by itself. */
export function syncLine(state: SyncState): string | null {
  if (state.kind === "off") return null;
  const host = hostName(state.url);
  switch (state.phase.kind) {
    case "conflict":
      return `Syncing with ${host} stopped: ${state.phase.paths.join(", ")} changed here and on ${host} in the same lines.`;
    case "unrelated":
      return `Syncing with ${host} stopped: its repo has a different history from this browser's.`;
    case "logged-out":
      return `Log in to ${host} to sync this browser's repo with it.`;
    case "failed":
      return `Syncing with ${host} failed, and will be tried again: ${state.phase.message}`;
    case "idle":
    case "syncing":
      return null;
  }
}

/**
 * The remote's sync where it needs the user, in the corner of every page,
 * and the pages' data reloaded when a sync moved the repo under them. An
 * open editor keeps its text; its next save merges what came in, as a
 * save after another tab's does.
 */
export function SyncStatus() {
  const router = useRouter();
  const remote = useMemo(remoteOf, []);
  const { state, remote: held } = useSyncState(remote);
  useEffect(() => {
    if (!held) return;
    const stop = held.sync.onMoved(() => void router.invalidate());
    return () => {
      stop();
    };
  }, [held, router]);
  const text = syncLine(state);
  if (!text) return null;
  return (
    <div className="sync-status" role="alert">
      <span>{text}</span>
      <Link to="/settings" hash="remote">
        Settings
      </Link>
    </div>
  );
}
