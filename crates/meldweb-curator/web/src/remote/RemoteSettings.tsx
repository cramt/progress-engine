import { useEffect, useMemo, useState } from "react";
import type { RemoteFacts } from "../deck.gen";
import { newRepoUrl } from "../github/onboarding";
import { MAGIC_REPO } from "../github/repo";
import { githubUrl } from "./github";
import { hostName, type Remote, useSyncState } from "./index";
import type { SyncState } from "./sync";
import "./remote.css";

const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * Set while adding a GitHub remote, which leaves the page to log in and to
 * install the app; whatever page it comes back to, settings picks it up.
 */
export const ADDING = "meldweb.addingRemote";

/** `login` once the page has left to log in, so coming back logged out stops. */
type Step = "github" | "login";
const adding = (): Step | null => {
  const v = sessionStorage.getItem(ADDING);
  return v === "github" || v === "login" ? v : null;
};
const setAdding = (step: Step | null) =>
  step
    ? sessionStorage.setItem(ADDING, step)
    : sessionStorage.removeItem(ADDING);

const plural = (n: number, one: string, many: string) =>
  `${n} ${n === 1 ? one : many}`;

/** What is waiting to go to the remote, in words. */
export function waiting(facts: RemoteFacts, host: string): string {
  const parts = [
    facts.ahead > 0 ? plural(facts.ahead, "commit", "commits") : null,
    facts.tags > 0 ? plural(facts.tags, "snapshot", "snapshots") : null,
  ].filter((p): p is string => p !== null);
  if (parts.length === 0) return `Nothing is waiting to go to ${host}.`;
  const what = parts.join(" and ");
  return `${what} ${facts.ahead + facts.tags === 1 ? "is" : "are"} waiting to go to ${host}.`;
}

function since(at: number | null): string {
  if (at === null) return "Not synced yet.";
  const minutes = Math.round((Date.now() - at) / 60_000);
  if (minutes < 1) return "Last synced just now.";
  if (minutes < 60)
    return `Last synced ${plural(minutes, "minute", "minutes")} ago.`;
  return `Last synced ${new Date(at).toLocaleString()}.`;
}

/** The repo's page on its host, without `.git`. */
const pageOf = (url: string) => url.replace(/\.git$/, "");

type Adding =
  | { kind: "no" }
  | { kind: "looking" }
  | { kind: "onboarding"; login: string; repoExists: boolean };

/**
 * The repo's remote, beside "This browser" (ADR-0041): adding one on GitHub
 * (its login, and making `mtg` and installing the app when they are not
 * done), what the sync is doing, and the choices it stops for.
 */
export function RemoteSettings({ remote }: { remote: Remote }) {
  const promise = useMemo(() => Promise.resolve(remote), [remote]);
  const { state } = useSyncState(promise);
  const [add, setAdd] = useState<Adding>({ kind: "no" });
  const [refusal, setRefusal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { sync, github } = remote;

  const act = async (work: () => Promise<void>) => {
    setBusy(true);
    setRefusal(null);
    try {
      await work();
    } catch (e) {
      setRefusal(messageOf(e));
    } finally {
      setBusy(false);
    }
  };

  const addGitHub = () =>
    act(async () => {
      if (!(await github.loggedIn())) {
        if (adding() === "login") {
          setAdding(null);
          throw new Error(
            "GitHub's login did not finish, so no remote was added.",
          );
        }
        setAdding("login");
        github.login("/settings#remote");
        return;
      }
      setAdding("github");
      setAdd({ kind: "looking" });
      const found = await github.find();
      if (found.kind === "onboarding") {
        setAdd(found);
        return;
      }
      setAdd({ kind: "no" });
      setAdding(null);
      await sync.connect(githubUrl(found.repo));
    });

  // Back from GitHub's login or the app's install page.
  // biome-ignore lint/correctness/useExhaustiveDependencies: once, on arrival
  useEffect(() => {
    void sync.refresh();
    if (adding()) void addGitHub();
    const again = () => {
      if (adding()) void addGitHub();
    };
    window.addEventListener("focus", again);
    return () => window.removeEventListener("focus", again);
  }, []);

  return (
    <section className="settings-rules" aria-labelledby="remote">
      <h2 id="remote">Remote</h2>
      {state.kind === "off" ? (
        <NoRemote
          add={add}
          busy={busy}
          onAdd={() => void addGitHub()}
          onCancel={() => {
            setAdding(null);
            setAdd({ kind: "no" });
          }}
          onInstall={() => void act(() => github.onboarding.install())}
          onNewRepo={() => github.onboarding.newRepo()}
        />
      ) : state.kind === "unreadable" ? (
        <p className="refusal" role="alert">
          This browser's repo could not be read to sync it: {state.message}{" "}
          <button
            type="button"
            className="small"
            disabled={busy}
            onClick={() => void act(() => sync.sync())}
          >
            Try again
          </button>
        </p>
      ) : (
        <Syncing state={state} busy={busy} act={act} remote={remote} />
      )}
      {refusal && (
        <p className="refusal" role="alert">
          {refusal}
        </p>
      )}
    </section>
  );
}

function NoRemote({
  add,
  busy,
  onAdd,
  onCancel,
  onInstall,
  onNewRepo,
}: {
  add: Adding;
  busy: boolean;
  onAdd: () => void;
  onCancel: () => void;
  onInstall: () => void;
  onNewRepo: () => void;
}) {
  if (add.kind === "onboarding") {
    return (
      <>
        <p className="settings-lede">
          Progress Engine keeps your decks in{" "}
          <code>
            {add.login}/{MAGIC_REPO}
          </code>{" "}
          on GitHub, and can reach only that one repository.
        </p>
        <ol className="settings-remote-steps">
          <li>
            <a
              href={newRepoUrl()}
              target="_blank"
              rel="noopener"
              onClick={(e) => {
                e.preventDefault();
                onNewRepo();
              }}
            >
              Create the {MAGIC_REPO} repository
            </a>
            {add.repoExists && ` (${add.login}/${MAGIC_REPO} exists)`}. Public
            or private is up to you.
          </li>
          <li>
            <button type="button" disabled={busy} onClick={onInstall}>
              Install Progress Engine on {MAGIC_REPO}
            </button>{" "}
            Choose <em>Only select repositories</em> and pick{" "}
            <code>{MAGIC_REPO}</code>.
          </li>
        </ol>
        <div className="rule-actions">
          <button type="button" disabled={busy} onClick={onAdd}>
            Check again
          </button>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
        </div>
      </>
    );
  }
  return (
    <>
      <p className="settings-lede">
        A remote keeps this repo in step with a git repository on a host, so
        your decks reach your other computers and browsers. Without one, the
        repo is only here and in the zips you export.
      </p>
      <div className="rule-actions">
        <button
          type="button"
          className="primary"
          disabled={busy || add.kind === "looking"}
          onClick={onAdd}
        >
          Add a GitHub remote
        </button>
      </div>
      {add.kind === "looking" && (
        <p className="settings-source" role="status">
          Looking for your {MAGIC_REPO} repository on GitHub…
        </p>
      )}
    </>
  );
}

function Syncing({
  state,
  busy,
  act,
  remote,
}: {
  state: Extract<SyncState, { kind: "on" }>;
  busy: boolean;
  act: (work: () => Promise<void>) => Promise<void>;
  remote: Remote;
}) {
  const { sync, github } = remote;
  const host = hostName(state.url);
  const { phase } = state;
  const stuck = busy || phase.kind === "syncing";
  return (
    <>
      <p className="settings-lede">
        This browser's repo syncs with{" "}
        <a href={pageOf(state.url)} target="_blank" rel="noopener">
          {pageOf(state.url).replace(/^https:\/\//, "")}
        </a>
        : it fetches when a page opens or comes back into view, and pushes a few
        seconds after a save.
      </p>
      <p className="settings-source" role="status">
        {phase.kind === "syncing"
          ? `Syncing with ${host}…`
          : since(state.lastSynced)}{" "}
        {waiting(state.facts, host)}
      </p>
      {phase.kind === "logged-out" && (
        <p className="refusal" role="alert">
          {host} wants you to log in before it syncs.{" "}
          {host === "GitHub" ? (
            <button
              type="button"
              className="small"
              onClick={() => github.login("/settings#remote")}
            >
              Log in with GitHub
            </button>
          ) : (
            "Progress Engine can log in only to GitHub so far."
          )}
        </p>
      )}
      {phase.kind === "failed" && (
        <p className="refusal" role="alert">
          Syncing failed: {phase.message}
        </p>
      )}
      {phase.kind === "conflict" && (
        <div className="refusal" role="alert">
          <p>
            {phase.paths.join(", ")} {phase.paths.length === 1 ? "was" : "were"}{" "}
            changed here and on {host} in the same lines, so syncing stopped.
            Both versions are in the repo's history whichever you keep.
          </p>
          <div className="rule-actions">
            <button
              type="button"
              className="small"
              disabled={stuck}
              onClick={() => void act(() => sync.resolve("theirs"))}
            >
              Reload from {host} (discard this browser's edits to{" "}
              {phase.paths.length === 1 ? "it" : "them"})
            </button>
            <button
              type="button"
              className="small"
              disabled={stuck}
              onClick={() => void act(() => sync.resolve("ours"))}
            >
              Overwrite {host}'s
            </button>
          </div>
        </div>
      )}
      {phase.kind === "unrelated" && (
        <div className="refusal" role="alert">
          <p>
            The repository on {host} and this browser's repo have different
            histories, with no commit in common, so neither can take the other's
            changes. Replace this browser's repo with the one on {host}, or
            remove the remote to keep this one.
          </p>
          <div className="rule-actions">
            <button
              type="button"
              className="small"
              disabled={stuck}
              onClick={() => {
                const sure = window.confirm(
                  `Replace this browser's repo with the one on ${host}? Every deck, the collection and their history here are replaced by ${host}'s. Export this one first to keep it.`,
                );
                if (sure) void act(() => sync.replaceWithRemote());
              }}
            >
              Replace this browser's repo with {host}'s…
            </button>
            <button
              type="button"
              className="small"
              disabled={stuck}
              onClick={() => void act(() => sync.disconnect())}
            >
              Remove the remote
            </button>
          </div>
        </div>
      )}
      <div className="rule-actions">
        <button
          type="button"
          disabled={stuck}
          onClick={() => void act(() => sync.sync())}
        >
          Sync now
        </button>
        <button
          type="button"
          disabled={stuck}
          onClick={() => {
            const sure = window.confirm(
              `Stop syncing with ${host}? The decks stay in this browser and on ${host}; nothing is deleted.`,
            );
            if (sure) void act(() => sync.disconnect());
          }}
        >
          Remove the remote
        </button>
      </div>
    </>
  );
}
