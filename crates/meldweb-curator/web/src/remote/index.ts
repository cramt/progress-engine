/**
 * The browser repo's remote, wired to the page: the sync engine over the
 * repo's worker, a transport per host, and what starts a sync.
 */
import { useEffect, useState, useSyncExternalStore } from "react";
import type { LocalRepo } from "../repo/local";
import { CHANNEL } from "../repo/protocol";
import { type GitHubHost, githubHost, isGitHub } from "./github";
import { createSyncEngine, type SyncEngine, type SyncState } from "./sync";
import { ANONYMOUS, relayTransport } from "./transport";

export interface Remote {
  sync: SyncEngine;
  github: GitHubHost;
}

/**
 * The engine for `repo`. A remote on github.com logs in with GitHub; one on
 * another host has no adapter yet, so it fetches anonymously, which a public
 * repo answers.
 */
export function createRemote(repo: LocalRepo): Remote {
  const github = githubHost();
  const sync = createSyncEngine({
    steps: repo.remote,
    transport: (url) =>
      relayTransport({
        credentials: isGitHub(url) ? github.credentials : ANONYMOUS,
      }),
  });
  return { sync, github };
}

let started = false;

/**
 * Syncs now, when the page comes back into view, and once a commit or
 * snapshot has been idle a moment. Every tab's repo worker says on
 * `CHANNEL` when it wrote, this tab's included.
 */
export function startSync(sync: SyncEngine): void {
  if (started || typeof window === "undefined") return;
  started = true;
  void sync.refresh().then(() => {
    if (sync.getState().kind === "on") void sync.sync();
  });
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") sync.focused();
  });
  window.addEventListener("focus", () => sync.focused());
  const channel = new BroadcastChannel(CHANNEL);
  channel.onmessage = () => sync.committed();
}

const off: SyncState = { kind: "off" };

/** The sync's state, once the remote is known; `off` until then. */
export function useSyncState(remote: Promise<Remote | null>): {
  state: SyncState;
  remote: Remote | null;
} {
  const [held, setHeld] = useState<Remote | null>(null);
  useEffect(() => {
    let live = true;
    void remote.then((r) => {
      if (live) setHeld(r);
    });
    return () => {
      live = false;
    };
  }, [remote]);
  const state = useSyncExternalStore(
    held ? held.sync.subscribe : noop,
    held ? held.sync.getState : () => off,
  );
  return { state, remote: held };
}

const noop = () => () => {};

/** How a remote's host is named to the user: GitHub, or its host name. */
export function hostName(url: string): string {
  if (isGitHub(url)) return "GitHub";
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
