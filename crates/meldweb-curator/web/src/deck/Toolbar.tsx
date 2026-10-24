import { Link } from "@tanstack/react-router";
import { type ReactNode, useState } from "react";
import type { Conflict, FileConflict, Settle, Side } from "../deck.gen";
import type { SaveState } from "../github/save";
import { BrandMark, CloseIcon, RedoIcon, UndoIcon } from "../ui/icons";
import { Waiting } from "../ui/Spinner";
import { ClashPicks, picked, settleOf } from "./ClashPicks";

/**
 * The bar along the top of every page, sticky on scroll: Archidekt's toolbar,
 * what stays in reach while reading a long deck. Every slot but the title is
 * optional, and each holds one thing, so a feature adds itself by filling its
 * slot rather than by rearranging the bar. Left to right:
 *
 *   mark · Decks / title · count | search  quickAdd | … | actions  status  history
 */
export function Toolbar({
  name,
  up = true,
  count,
  search,
  quickAdd,
  actions,
  status,
  history,
}: {
  /** Where this page is: the deck's name, or its path until it has one. */
  name: ReactNode;
  /** Whether the title sits under Decks; the deck list is the top. */
  up?: boolean;
  /** Cards on the page, each counted once whatever categories it is in (ADR-0020). */
  count?: number;
  /** The card search button, which opens the search overlay. */
  search?: ReactNode;
  /** The quick add box. */
  quickAdd?: ReactNode;
  /** Page-wide actions, such as copying the deck as Archidekt text. */
  actions?: ReactNode;
  /** Whether the page's file is saved: unsaved changes, saving, committed. */
  status?: ReactNode;
  /** Undo and redo; see `UndoRedo`. */
  history?: ReactNode;
}) {
  return (
    <div className="toolbar" role="toolbar" aria-label="Page">
      <div className="toolbar-deck">
        <Link to="/" className="brand" title="Progress Engine: all decks">
          <BrandMark />
          <span className="wordmark brand-name">Progress Engine</span>
        </Link>
        <h1 className="crumbs">
          {up && (
            <>
              <Link to="/" className="toolbar-back" title="All decks">
                Decks
              </Link>
              <span className="crumb-sep" aria-hidden="true">
                /
              </span>
            </>
          )}
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
      {status && <div className="toolbar-status">{status}</div>}
      {history && <div className="toolbar-history">{history}</div>}
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
