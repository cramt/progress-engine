import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { type Branch, MAIN } from "../github/branches";
import { deckSnapshots, type Snapshot } from "../github/history";
import type { Repo } from "../repo/api";
import { BranchIcon, TagIcon } from "../ui/icons";
import { messageOf } from "../ui/Sheet";
import { when } from "./VersionView";
import type { Viewing } from "./versions";

type Tab = "branches" | "snapshots";

type Refs =
  | { kind: "loading" }
  | { kind: "failed"; message: string }
  | { kind: "ready"; branches: Branch[]; snapshots: Snapshot[] };

/**
 * One picker for the deck's branches and snapshots, as a repo page on GitHub
 * has one for its branches and tags (ADR-0041). Choosing a branch opens it
 * for editing; choosing a snapshot shows it read-only, as a revision is. From
 * here a branch starts where the page is: the branch open, or the snapshot
 * or revision shown.
 */
export function RefPicker({
  repo,
  stem,
  branch,
  viewing,
  refresh,
  from,
  onBranch,
  onView,
  onNewBranch,
  onSnapshot,
}: {
  repo: Repo;
  stem: string;
  /** The branch open for editing. */
  branch: string;
  viewing: Viewing;
  /** Bumped when a branch or snapshot is made, merged or deleted. */
  refresh: number;
  /** Where a new branch or snapshot would start, for the reader. */
  from: string;
  onBranch: (name: string) => void;
  onView: (viewing: Viewing) => void;
  /** Opens the new-branch dialog with `name` filled in. */
  onNewBranch: (name: string) => void;
  onSnapshot: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<Tab>("branches");
  const [filter, setFilter] = useState("");
  const [refs, setRefs] = useState<Refs>({ kind: "loading" });
  const box = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  // Where the menu opens: under the button, drawn over the page from the
  // body, since the toolbar's title clips what overflows it.
  const [at, setAt] = useState({ top: 0, left: 0 });

  // biome-ignore lint/correctness/useExhaustiveDependencies: `refresh` and opening read them again
  useEffect(() => {
    let live = true;
    Promise.all([repo.branches(stem), deckSnapshots(repo, stem)]).then(
      ([branches, snapshots]) =>
        live && setRefs({ kind: "ready", branches, snapshots }),
      (e) => live && setRefs({ kind: "failed", message: messageOf(e) }),
    );
    return () => {
      live = false;
    };
  }, [repo, stem, refresh, open]);

  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!box.current?.contains(target) && !menu.current?.contains(target))
        setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const snapshots = refs.kind === "ready" ? refs.snapshots : [];
  const shown =
    viewing.kind === "revision"
      ? snapshots.find((s) => s.commit === viewing.commit)
      : undefined;
  const close = () => {
    setOpen(false);
    setFilter("");
  };
  const needle = filter.trim().toLowerCase();
  const branches = (refs.kind === "ready" ? refs.branches : []).filter((b) =>
    b.name.toLowerCase().includes(needle),
  );
  const tags = snapshots.filter((s) => s.label.toLowerCase().includes(needle));
  const exact =
    refs.kind === "ready" && refs.branches.some((b) => b.name === needle);

  return (
    <div className="ref-picker" ref={box}>
      <button
        type="button"
        className="ref-picker-button"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => {
          if (open) return close();
          const r = box.current?.getBoundingClientRect();
          if (r) setAt({ top: r.bottom + 6, left: r.left });
          setOpen(true);
        }}
        title="Switch branches or snapshots"
      >
        {shown ? <TagIcon /> : <BranchIcon />}
        <span className="ref-picker-name">{shown ? shown.label : branch}</span>
        <span aria-hidden="true">▾</span>
      </button>
      {open &&
        createPortal(
          <div
            ref={menu}
            className="ref-picker-menu"
            role="dialog"
            aria-label="Branches and snapshots"
            style={at}
          >
            <header>Switch branches or snapshots</header>
            <input
              className="ref-picker-filter"
              value={filter}
              placeholder={
                tab === "branches"
                  ? "Find or create a branch…"
                  : "Find a snapshot…"
              }
              aria-label="Filter"
              // biome-ignore lint/a11y/noAutofocus: the picker opens to type into
              autoFocus
              onChange={(e) => setFilter(e.target.value)}
            />
            <div className="ref-picker-tabs" role="tablist">
              {(["branches", "snapshots"] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  role="tab"
                  aria-selected={tab === t}
                  onClick={() => setTab(t)}
                >
                  {t === "branches" ? "Branches" : "Snapshots"}
                </button>
              ))}
            </div>
            {refs.kind === "loading" && (
              <p className="hint">Reading the refs…</p>
            )}
            {refs.kind === "failed" && <p className="hint">{refs.message}</p>}
            {tab === "branches" && (
              <ul className="ref-picker-list">
                {branches.map((b) => (
                  <li key={b.name}>
                    <button
                      type="button"
                      className={
                        b.name === branch && viewing.kind === "now"
                          ? "ref-row selected"
                          : "ref-row"
                      }
                      onClick={() => {
                        close();
                        onBranch(b.name);
                      }}
                    >
                      <BranchIcon />
                      <span className="ref-row-text">
                        <span className="ref-row-title">
                          {b.name}
                          {b.name === MAIN && (
                            <span
                              className="badge"
                              title="The collection, Build, Sourcing and the wanted list read main"
                            >
                              built deck
                            </span>
                          )}
                        </span>
                        <span className="ref-row-sub">
                          {when(b.date)}
                          {b.unmerged > 0 && ` · ${b.unmerged} not in main`}
                          {b.behind > 0 && ` · ${b.behind} behind main`}
                        </span>
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
            {tab === "branches" && (
              <button
                type="button"
                className="ref-picker-action"
                onClick={() => {
                  const name = exact ? "" : filter.trim();
                  close();
                  onNewBranch(name);
                }}
              >
                <BranchIcon />
                {needle && !exact ? (
                  <>
                    Create branch <strong>{filter.trim()}</strong> from {from}
                  </>
                ) : (
                  <>New branch from {from}…</>
                )}
              </button>
            )}
            {tab === "snapshots" && (
              <ul className="ref-picker-list">
                {tags.map((s) => (
                  <li key={s.tag}>
                    <button
                      type="button"
                      className={
                        shown?.tag === s.tag ? "ref-row selected" : "ref-row"
                      }
                      onClick={() => {
                        close();
                        onView({ kind: "revision", commit: s.commit });
                      }}
                    >
                      <TagIcon />
                      <span className="ref-row-text">
                        <span className="ref-row-title">{s.label}</span>
                        <span className="ref-row-sub">{when(s.date)}</span>
                      </span>
                    </button>
                  </li>
                ))}
                {refs.kind === "ready" && tags.length === 0 && (
                  <li className="hint">
                    {snapshots.length === 0
                      ? "No snapshots yet."
                      : "None match."}
                  </li>
                )}
              </ul>
            )}
            {tab === "snapshots" && (
              <button
                type="button"
                className="ref-picker-action"
                onClick={() => {
                  close();
                  onSnapshot();
                }}
              >
                <TagIcon />
                Snapshot {from}…
              </button>
            )}
          </div>,
          document.body,
        )}
    </div>
  );
}
