import { Link, useRouterState } from "@tanstack/react-router";
import { type ReactNode, useEffect, useRef, useState } from "react";
import type { Conflict, FileConflict, Settle, Side } from "../deck.gen";
import type { SaveState } from "../github/save";
import {
  BrandMark,
  ChevronIcon,
  CloseIcon,
  RedoIcon,
  UndoIcon,
} from "../ui/icons";
import { Waiting } from "../ui/Spinner";
import { ClashPicks, picked, settleOf } from "./ClashPicks";

/**
 * The site's places, in every page's bar so any page is one click from any
 * other. A deck is one of the decks, and a trade is worked out from Wanted.
 */
const PLACES: readonly {
  to: "/" | "/collection" | "/wanted" | "/cards" | "/settings";
  label: string;
  title: string;
  here: (path: string) => boolean;
}[] = [
  {
    to: "/",
    label: "Decks",
    title: "Every deck in this repo",
    here: (p) => p === "/" || p.startsWith("/deck/"),
  },
  {
    to: "/collection",
    label: "Collection",
    title: "The cards you own, each copy in its place",
    here: (p) => p.startsWith("/collection"),
  },
  {
    to: "/wanted",
    label: "Wanted",
    title: "Cards you want, and the ones your decks hold that you don't own",
    here: (p) => p.startsWith("/wanted") || p.startsWith("/trade"),
  },
  {
    to: "/cards",
    label: "Custom cards",
    title: "Cards of your own making, drawn and printed by Mirage Mockery",
    here: (p) => p.startsWith("/cards"),
  },
  {
    to: "/settings",
    label: "Settings",
    title: "Which printing of a card is offered first",
    here: (p) => p.startsWith("/settings"),
  },
];

function SiteNav() {
  const path = useRouterState({ select: (s) => s.location.pathname });
  return (
    <nav className="site-nav" aria-label="Site">
      {PLACES.map((place) => (
        <Link
          key={place.to}
          to={place.to}
          title={place.title}
          aria-current={place.here(path) ? "page" : undefined}
        >
          {place.label}
        </Link>
      ))}
    </nav>
  );
}

/**
 * The bar along the top of every page, sticky on scroll: the site's places
 * above, and under them this page's name and tools, Archidekt's toolbar, what
 * stays in reach while reading a long deck. Every slot but the title is
 * optional, and each holds one thing, so a feature adds itself by filling its
 * slot rather than by rearranging the bar. Left to right:
 *
 *   mark · Decks  Collection  Wanted  Custom cards  Settings
 *   title · count | search  quickAdd | … | actions  more  status  history
 */
export function Toolbar({
  name,
  count,
  search,
  quickAdd,
  actions,
  more,
  status,
  history,
}: {
  /** Where this page is: the deck's name, or its path until it has one. */
  name: ReactNode;
  /** Cards on the page, each counted once whatever categories it is in (ADR-0020). */
  count?: number;
  /** The card search button, which opens the search overlay. */
  search?: ReactNode;
  /** The quick add box. */
  quickAdd?: ReactNode;
  /** The page's actions used while working on it, such as Scan. */
  actions?: ReactNode;
  /**
   * Actions that take the page elsewhere, such as Copy for…, under one More
   * button, and what they are, said on hover; see `MoreMenu`.
   */
  more?: { title: string; actions: ReactNode };
  /** Whether the page's file is saved: unsaved changes, saving, committed. */
  status?: ReactNode;
  /** Undo and redo; see `UndoRedo`. */
  history?: ReactNode;
}) {
  return (
    <header className="toolbar">
      <div className="toolbar-site">
        <Link to="/" className="brand" title="Progress Engine: all decks">
          <BrandMark />
          <span className="wordmark brand-name">Progress Engine</span>
        </Link>
        <SiteNav />
      </div>
      <div className="toolbar-page" role="toolbar" aria-label="Page">
        <div className="toolbar-deck">
          <h1 className="crumbs">
            <span className="crumb-here">{name}</span>
          </h1>
          {count !== undefined && (
            <span className="badge toolbar-count" title="Cards">
              {count} card{count === 1 ? "" : "s"}
            </span>
          )}
        </div>
        {search && <div className="toolbar-search">{search}</div>}
        {quickAdd && <div className="toolbar-quick-add">{quickAdd}</div>}
        <div className="toolbar-spacer" />
        {actions && <div className="toolbar-actions">{actions}</div>}
        {more && <MoreMenu title={more.title}>{more.actions}</MoreMenu>}
        {status && <div className="toolbar-status">{status}</div>}
        {history && <div className="toolbar-history">{history}</div>}
      </div>
    </header>
  );
}

/**
 * More: the page's less used actions, one button opening a list of them.
 * The list stays mounted while closed, only out of sight, because an action
 * in it may own a dialog that has to outlive the list (Proxies…); its
 * stylesheet makes a dialog visible inside a hidden list. Picking an entry,
 * a click elsewhere and Escape close it.
 */
function MoreMenu({ title, children }: { title: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: Event) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("keydown", key);
    };
  }, [open]);
  return (
    <div className="more" ref={ref}>
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        title={title}
        onClick={() => setOpen((was) => !was)}
      >
        More
        <ChevronIcon />
      </button>
      {/* Entries are buttons and links already; closing follows their own click. */}
      {/* biome-ignore lint/a11y/useKeyWithClickEvents: Escape and the entries' own keys cover the keyboard */}
      <div
        className={open ? "more-list open" : "more-list"}
        role="menu"
        onClick={(e) => {
          if ((e.target as Element).closest("button, a")) setOpen(false);
        }}
      >
        {children}
      </div>
    </div>
  );
}

/** The toolbar's undo and redo buttons. */
export function UndoRedo({
  onUndo,
  onRedo,
  canUndo,
  canRedo,
}: {
  onUndo: () => void;
  onRedo: () => void;
  canUndo: boolean;
  canRedo: boolean;
}) {
  return (
    <>
      <button
        type="button"
        className="icon ghost"
        onClick={onUndo}
        disabled={!canUndo}
        aria-label="Undo"
        title="Undo (Ctrl+Z)"
      >
        <UndoIcon />
      </button>
      <button
        type="button"
        className="icon ghost"
        onClick={onRedo}
        disabled={!canRedo}
        aria-label="Redo"
        title="Redo (Ctrl+Shift+Z)"
      >
        <RedoIcon />
      </button>
    </>
  );
}

const statusText: Record<SaveState["status"], string> = {
  unsaved: "Unsaved",
  saving: "Saving…",
  saved: "Saved",
  conflict: "Not saved",
  error: "Not saved",
};

/** The toolbar's word on whether the file is committed; hover names it. */
export function SaveStatus({ save, path }: { save: SaveState; path: string }) {
  return (
    <span
      className={`save-status save-${save.status}`}
      title={save.status === "error" ? save.message : path}
    >
      {save.status === "saving" ? (
        <Waiting>{statusText.saving}</Waiting>
      ) : (
        statusText[save.status]
      )}
    </span>
  );
}

/**
 * What stops a save or an edit, under the toolbar: a commit made elsewhere, a
 * failed save, and the last edit refused, each with its way out. A `notice`
 * says what an action just did, such as a snapshot taken. A deck's save
 * that clashed shows each clash with its two sides, and `onPicks` merges
 * settled by the sides picked.
 */
/** A save's clashes, card by card, when its deck's merge stopped on some. */
type DeckClashes = Extract<FileConflict, { needs: "eachClash" }>;

const deckClashesOf = (conflict: Conflict | null): DeckClashes | null =>
  conflict?.files.find((f): f is DeckClashes => f.needs === "eachClash") ??
  null;

export function Banners({
  save,
  refusal,
  onReload,
  onOverwrite,
  onPicks,
  onDismiss,
  notice,
}: {
  save: SaveState;
  refusal: string | null;
  onReload: () => void;
  onOverwrite: () => void;
  onPicks?: (settle: Settle) => void;
  onDismiss: () => void;
  notice?: string | null;
}) {
  const clashed =
    save.status === "conflict" ? deckClashesOf(save.conflict) : null;
  return (
    <div className="banners">
      {save.status === "conflict" &&
        (save.conflict && clashed && onPicks ? (
          <SaveClashes
            // A new conflict is new rows, picked afresh.
            key={save.conflict.guard}
            conflict={save.conflict}
            file={clashed}
            onPicks={onPicks}
            onReload={onReload}
            onOverwrite={onOverwrite}
          />
        ) : (
          <p className="conflict" role="alert">
            {clashed
              ? `Changed elsewhere since this page loaded, and both changed ${clashed.clashes
                  .map(
                    (c) =>
                      `${c.subject}'s ${c.attribute} (here ${c.ours}, there ${c.theirs})`,
                  )
                  .join("; ")}.`
              : "Changed elsewhere since this page loaded, in a way that does not merge."}
            <button type="button" className="small" onClick={onReload}>
              Reload (discard my edits)
            </button>
            <button type="button" className="small" onClick={onOverwrite}>
              Overwrite
            </button>
          </p>
        ))}
      {save.status === "error" && (
        <p className="refusal" role="alert">
          Saving failed, and will be tried again: {save.message}
        </p>
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}
      {refusal && (
        <p className="refusal" role="alert">
          {refusal}
          <button
            type="button"
            className="icon ghost small"
            aria-label="Dismiss"
            onClick={onDismiss}
          >
            <CloseIcon />
          </button>
        </p>
      )}
    </div>
  );
}

/**
 * A refused save's clashes, a side to pick for each: what this page has
 * against what was saved elsewhere. Merging with the picks becomes the
 * editor's text as one undoable step and is saved over the other save.
 */
function SaveClashes({
  conflict,
  file,
  onPicks,
  onReload,
  onOverwrite,
}: {
  conflict: Conflict;
  file: DeckClashes;
  onPicks: (settle: Settle) => void;
  onReload: () => void;
  onOverwrite: () => void;
}) {
  const { clashes } = file;
  const [picks, setPicks] = useState<(Side | undefined)[]>(() =>
    clashes.map(() => undefined),
  );
  const settle = settleOf(conflict, () => ({ picks }));
  const n = clashes.length;
  return (
    <div className="conflict save-clashes" role="alert">
      <span>
        Changed elsewhere since this page loaded, and both changed{" "}
        {n === 1 ? "the same thing" : `${n} things`} differently. Pick a side
        for {n === 1 ? "it" : "each"}:
      </span>
      <ClashPicks
        clashes={clashes}
        picks={picks}
        onPick={(i, side) => setPicks((was) => picked(was, i, side))}
        ours="here"
        theirs="there"
        group="save-clash"
      />
      <button
        type="button"
        className="small primary"
        disabled={!settle}
        onClick={() => settle && onPicks(settle)}
      >
        Merge with these picks
      </button>
      <button type="button" className="small" onClick={onReload}>
        Reload (discard my edits)
      </button>
      <button type="button" className="small" onClick={onOverwrite}>
        Overwrite
      </button>
    </div>
  );
}
