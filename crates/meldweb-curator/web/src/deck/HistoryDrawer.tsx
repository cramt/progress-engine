import { useCallback, useEffect, useState } from "react";
import type { GitHubApi, RepoRef } from "../github/api";
import { type DeckEntry, listDecks } from "../github/decks";
import { deckText } from "../github/deckText";
import {
  deckSnapshots,
  PAGE,
  type Revision,
  revisionAt,
  revisions,
  type Snapshot,
} from "../github/history";
import type { SaveStatus } from "../github/save";
import { BranchIcon, CloseIcon, TagIcon } from "../ui/icons";
import { when } from "./VersionView";
import { body, byDay, JUMPS, subject, type Viewing } from "./versions";

type Loaded<T> =
  | { kind: "loading" }
  | { kind: "failed"; message: string }
  | { kind: "ready"; value: T };

/** `load()` run again whenever `load` changes, its latest answer kept. */
function useLoaded<T>(load: () => Promise<T>): Loaded<T> {
  const [state, setState] = useState<Loaded<T>>({ kind: "loading" });
  useEffect(() => {
    let live = true;
    load().then(
      (value) => live && setState({ kind: "ready", value }),
      (e) =>
        live &&
        setState({
          kind: "failed",
          message: e instanceof Error ? e.message : String(e),
        }),
    );
    return () => {
      live = false;
    };
  }, [load]);
  return state;
}

/**
 * The deck's past down the right of the page: how it looked on a day, its
 * snapshots, its variants, and every save, newest first and grouped by day.
 * Choosing any of them shows it in place of the deck (`VersionView`); the
 * deck as it is stays in the editor underneath, saving as it does.
 */
export function HistoryDrawer({
  api,
  repo,
  path,
  stem,
  variantOf,
  viewing,
  saveStatus,
  refresh,
  onView,
  onClose,
  onSnapshot,
  onVariant,
}: {
  api: GitHubApi;
  repo: RepoRef;
  path: string;
  stem: string;
  variantOf: string | undefined;
  viewing: Viewing;
  saveStatus: SaveStatus;
  /** Bumped when a snapshot is taken or dropped, to read them again. */
  refresh: number;
  onView: (viewing: Viewing) => void;
  onClose: () => void;
  /** `null` is the deck as it is now, once it is saved. */
  onSnapshot: (commit: string | null) => void;
  onVariant: () => void;
}) {
  const snaps = useLoaded(
    // biome-ignore lint/correctness/useExhaustiveDependencies: `refresh` reads them again
    useCallback(
      () => deckSnapshots(api, repo, path),
      [api, repo, path, refresh],
    ),
  );
  return (
    <aside className="history-drawer" aria-label="History">
      <header className="history-header">
        <h2>History</h2>
        <button
          type="button"
          className="icon ghost"
          aria-label="Close history"
          onClick={onClose}
        >
          <CloseIcon />
        </button>
      </header>
      <JumpBack api={api} repo={repo} path={path} onView={onView} />
      <Variants
        api={api}
        repo={repo}
        path={path}
        variantOf={variantOf}
        viewing={viewing}
        onView={onView}
        onVariant={onVariant}
      />
      <Snapshots
        snaps={snaps}
        viewing={viewing}
        onView={onView}
        onSnapshot={onSnapshot}
      />
      <Timeline
        api={api}
        repo={repo}
        path={path}
        stem={stem}
        viewing={viewing}
        saveStatus={saveStatus}
        snaps={snaps}
        onView={onView}
      />
    </aside>
  );
}

function JumpBack({
  api,
  repo,
  path,
  onView,
}: {
  api: GitHubApi;
  repo: RepoRef;
  path: string;
  onView: (viewing: Viewing) => void;
}) {
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const jump = async (to: Date) => {
    setBusy(true);
    setNotice(null);
    try {
      const at = await revisionAt(api, repo, path, to);
      if (at) onView({ kind: "revision", commit: at.commit });
      else
        setNotice(
          `The deck did not exist yet on ${to.toLocaleDateString(undefined, { day: "numeric", month: "long", year: "numeric" })}.`,
        );
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const daysAgo = (days: number) => {
    const d = new Date();
    d.setDate(d.getDate() - days);
    return d;
  };
  return (
    <section className="history-section" aria-busy={busy}>
      <h3>How did it look…</h3>
      <div className="history-jumps">
        {JUMPS.map((j) => (
          <button
            key={j.days}
            type="button"
            className="small"
            disabled={busy}
            onClick={() => void jump(daysAgo(j.days))}
          >
            {j.label}
          </button>
        ))}
        <label className="history-date">
          <span className="visually-hidden">On a day</span>
          <input
            type="date"
            max={new Date().toISOString().slice(0, 10)}
            disabled={busy}
            onChange={(e) => {
              if (!e.target.value) return;
              // The end of the day picked, in the reader's own time zone.
              const [y, m, d] = e.target.value.split("-").map(Number);
              void jump(new Date(y ?? 0, (m ?? 1) - 1, d ?? 1, 23, 59, 59));
            }}
          />
        </label>
      </div>
      {notice && <p className="hint">{notice}</p>}
    </section>
  );
}

function Variants({
  api,
  repo,
  path,
  variantOf,
  viewing,
  onView,
  onVariant,
}: {
  api: GitHubApi;
  repo: RepoRef;
  path: string;
  variantOf: string | undefined;
  viewing: Viewing;
  onView: (viewing: Viewing) => void;
  onVariant: () => void;
}) {
  const decks = useLoaded(
    useCallback(() => listDecks(api, repo, deckText), [api, repo]),
  );
  const all = decks.kind === "ready" ? decks.value : [];
  const parent = variantOf && all.find((d) => d.path === variantOf);
  const children = all.filter((d) => d.variantOf === path);
  const row = (d: DeckEntry, label: string) => (
    <li key={d.path}>
      <button
        type="button"
        className={
          viewing.kind === "deck" && viewing.path === d.path
            ? "history-row selected"
            : "history-row"
        }
        onClick={() => onView({ kind: "deck", path: d.path })}
        title={`Compare with ${d.name}`}
      >
        <BranchIcon />
        <span className="history-row-text">
          <span className="history-row-title">{d.name}</span>
          <span className="history-row-sub">{label}</span>
        </span>
      </button>
    </li>
  );
  return (
    <section className="history-section">
      <h3>
        Variants
        <button type="button" className="small ghost" onClick={onVariant}>
          + New variant…
        </button>
      </h3>
      {decks.kind === "failed" && <p className="hint">{decks.message}</p>}
      {variantOf && !parent && decks.kind === "ready" && (
        <p className="hint">
          A variant of <code>{variantOf}</code>, which is gone.
        </p>
      )}
      <ul className="history-list">
        {parent && row(parent, "The deck this is a variant of")}
        {children.map((d) => row(d, "A variant of this deck"))}
      </ul>
      {decks.kind === "ready" && !parent && children.length === 0 && (
        <p className="hint">
          A variant is a copy that remembers this deck: a budget build, a
          version for another pod. Either can take the other's changes.
        </p>
      )}
    </section>
  );
}

function Snapshots({
  snaps,
  viewing,
  onView,
  onSnapshot,
}: {
  snaps: Loaded<Snapshot[]>;
  viewing: Viewing;
  onView: (viewing: Viewing) => void;
  onSnapshot: (commit: string | null) => void;
}) {
  return (
    <section className="history-section">
      <h3>
        Snapshots
        <button
          type="button"
          className="small ghost"
          onClick={() => onSnapshot(null)}
        >
          + Snapshot now…
        </button>
      </h3>
      {snaps.kind === "failed" && <p className="hint">{snaps.message}</p>}
      {snaps.kind === "ready" && snaps.value.length === 0 && (
        <p className="hint">
          Name a version to find it again: the list you took to FNM, the build
          before a big swap.
        </p>
      )}
      {snaps.kind === "ready" && snaps.value.length > 0 && (
        <ul className="history-list">
          {snaps.value.map((s) => (
            <li key={s.tag}>
              <button
                type="button"
                className={
                  viewing.kind === "revision" && viewing.commit === s.commit
                    ? "history-row selected"
                    : "history-row"
                }
                onClick={() => onView({ kind: "revision", commit: s.commit })}
              >
                <TagIcon />
                <span className="history-row-text">
                  <span className="history-row-title">{s.label}</span>
                  <span className="history-row-sub">{when(s.date)}</span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function Timeline({
  api,
  repo,
  path,
  stem,
  viewing,
  saveStatus,
  snaps,
  onView,
}: {
  api: GitHubApi;
  repo: RepoRef;
  path: string;
  stem: string;
  viewing: Viewing;
  saveStatus: SaveStatus;
  snaps: Loaded<Snapshot[]>;
  onView: (viewing: Viewing) => void;
}) {
  const [pages, setPages] = useState(1);
  // A save adds a commit at the top; the first page is read again after one.
  const saved = saveStatus === "saved";
  const log = useLoaded(
    // biome-ignore lint/correctness/useExhaustiveDependencies: a save reads the log again
    useCallback(async () => {
      const read = await Promise.all(
        Array.from({ length: pages }, (_, i) =>
          revisions(api, repo, path, i + 1),
        ),
      );
      return { revisions: read.flat(), more: read.at(-1)?.length === PAGE };
    }, [api, repo, path, pages, saved]),
  );
  const tagsOf = (r: Revision): Snapshot[] =>
    snaps.kind === "ready"
      ? snaps.value.filter((s) => s.commit === r.commit)
      : [];

  return (
    <section className="history-section history-timeline">
      <h3>Every save</h3>
      <ul className="history-list">
        <li>
          <button
            type="button"
            className={
              viewing.kind === "now" ? "history-row selected" : "history-row"
            }
            onClick={() => onView({ kind: "now" })}
          >
            <span className="history-dot now" />
            <span className="history-row-text">
              <span className="history-row-title">Now</span>
              <span className="history-row-sub">
                {saveStatus === "saved"
                  ? "Everything is saved"
                  : "With edits not saved yet"}
              </span>
            </span>
          </button>
        </li>
      </ul>
      {log.kind === "failed" && <p className="hint">{log.message}</p>}
      {log.kind === "loading" && <p className="hint">Reading the log…</p>}
      {log.kind === "ready" &&
        byDay(log.value.revisions, new Date()).map(({ day, revisions: rs }) => (
          <div key={day} className="history-day">
            <h4>{day}</h4>
            <ul className="history-list">
              {rs.map((r) => {
                const selected =
                  viewing.kind === "revision" && viewing.commit === r.commit;
                const more = body(r.message);
                return (
                  <li key={r.commit}>
                    <button
                      type="button"
                      className={
                        selected ? "history-row selected" : "history-row"
                      }
                      onClick={() =>
                        onView({ kind: "revision", commit: r.commit })
                      }
                      title={r.message}
                    >
                      <span className="history-dot" />
                      <span className="history-row-text">
                        <span className="history-row-title">
                          {subject(r.message, stem)}
                        </span>
                        <span className="history-row-sub">
                          {new Date(r.date).toLocaleTimeString(undefined, {
                            hour: "2-digit",
                            minute: "2-digit",
                          })}
                          {r.author && ` · ${r.author}`}
                          {tagsOf(r).map((s) => (
                            <span key={s.tag} className="history-tag">
                              <TagIcon />
                              {s.label}
                            </span>
                          ))}
                        </span>
                        {selected && more.length > 0 && (
                          <ul className="history-body">
                            {more.map((line) => (
                              <li key={line}>{line}</li>
                            ))}
                          </ul>
                        )}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      {log.kind === "ready" && log.value.more && (
        <button
          type="button"
          className="small history-older"
          onClick={() => setPages((p) => p + 1)}
        >
          Older saves
        </button>
      )}
    </section>
  );
}
