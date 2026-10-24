import { Link, useRouter } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { connect } from "../github/connect";
import { Sheet } from "../ui/Sheet";
import { hostName, type Remote, useSyncState } from "./index";
import { conflictNames, SyncConflicts } from "./SyncConflicts";
import type { SyncState } from "./sync";
import "./remote.css";

const remoteOf = (): Promise<Remote | null> =>
  connect().then((c) => (c.kind === "local" ? c.remote : null));

/** What the corner says about syncing, or null while it goes by itself. */
export function syncLine(state: SyncState): string | null {
  if (state.kind === "off") return null;
  if (state.kind === "unreadable")
    return `This browser's repo could not be read to sync it, and will be tried again: ${state.message}`;
  const host = hostName(state.url);
  switch (state.phase.kind) {
    case "conflict":
      return `Syncing with ${host} stopped: ${conflictNames(state.phase.conflicts).join(", ")} changed here and on ${host} in ways that clash.`;
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
  const [picking, setPicking] = useState(false);
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const text = syncLine(state);
  if (!text) return null;
  const conflict =
    state.kind === "on" && state.phase.kind === "conflict" ? state : null;
  const act = async (work: () => Promise<void>) => {
    setBusy(true);
    setRefusal(null);
    try {
      await work();
      setPicking(false);
    } catch (e) {
      setRefusal(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="sync-status" role="alert">
      <span>{text}</span>
      <span className="sync-status-actions">
        {conflict && held && (
          <button
            type="button"
            className="small primary"
            onClick={() => setPicking(true)}
          >
            Pick sides…
          </button>
        )}
        <Link to="/settings" hash="remote">
          Settings
        </Link>
      </span>
      {picking && conflict?.phase.kind === "conflict" && held && (
        <Sheet
          label="Pick a side for each clash"
          onClose={() => setPicking(false)}
          wide
        >
          <div className="merge-dialog">
            <h2>Syncing with {hostName(conflict.url)} stopped</h2>
            <p className="hint">
              These changed here and on {hostName(conflict.url)} in ways that
              clash. Pick a side for each, and the sync makes a merge commit
              with your picks.
            </p>
            <SyncConflicts
              conflicts={conflict.phase.conflicts}
              host={hostName(conflict.url)}
              disabled={busy}
              onSettle={(settles) => void act(() => held.sync.settle(settles))}
              onResolve={(side) => void act(() => held.sync.resolve(side))}
            />
            {refusal && (
              <p className="refusal" role="alert">
                {refusal}
              </p>
            )}
            <div className="dialog-buttons">
              <button type="button" onClick={() => setPicking(false)}>
                Close
              </button>
            </div>
          </div>
        </Sheet>
      )}
    </div>
  );
}
