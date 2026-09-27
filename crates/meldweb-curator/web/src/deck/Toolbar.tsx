import type { ReactNode } from "react";

/**
 * Archidekt's toolbar, sticky on scroll: what stays in reach while reading a
 * long deck. Every slot but the deck's name and count is optional, and each
 * holds one thing, so a feature adds itself by filling its slot rather than by
 * rearranging the bar. Left to right:
 *
 *   name · count | search  quickAdd | … | actions  status  history
 */
export function Toolbar({
  name,
  count,
  search,
  quickAdd,
  actions,
  status,
  history,
}: {
  /** The deck's name, or its path until it has one. */
  name: ReactNode;
  /** Cards in the deck, each counted once whatever categories it is in (ADR-0020). */
  count: number;
  /** The card search button, which opens the search overlay. */
  search?: ReactNode;
  /** The quick add box. */
  quickAdd?: ReactNode;
  /** Deck-wide actions, such as copying the deck as Archidekt text. */
  actions?: ReactNode;
  /** Whether the deck is saved: unsaved changes, saving, committed. */
  status?: ReactNode;
  /** Undo and redo; see `UndoRedo`. */
  history?: ReactNode;
}) {
  return (
    <div className="toolbar" role="toolbar" aria-label="Deck">
      <div className="toolbar-deck">
        <h1 className="toolbar-name">{name}</h1>
        <span className="toolbar-count">
          {count} card{count === 1 ? "" : "s"}
        </span>
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
        onClick={onUndo}
        disabled={!canUndo}
        title="Undo (Ctrl+Z)"
      >
        Undo
      </button>
      <button
        type="button"
        onClick={onRedo}
        disabled={!canRedo}
        title="Redo (Ctrl+Shift+Z)"
      >
        Redo
      </button>
    </>
  );
}
