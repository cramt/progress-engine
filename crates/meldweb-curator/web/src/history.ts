import { useEffect, useState } from "react";
import { isTyping } from "./card/hotkeys";

/**
 * A deck, or the collection, is its text. An edit is a new text, which makes
 * undo a stack of texts and every change a line diff against the file as
 * loaded. The settings page keeps its draft rules the same way.
 */
interface History<T> {
  past: T[];
  present: T;
  future: T[];
}

export function useHistory<T>(initial: T) {
  const [h, setH] = useState<History<T>>({
    past: [],
    present: initial,
    future: [],
  });
  /**
   * `merge` replaces the present rather than stacking on it, so a run of
   * keystrokes in one field undoes as one edit.
   */
  const edit = (next: T, merge = false) =>
    setH((h) =>
      next === h.present
        ? h
        : merge && h.past.length > 0
          ? { ...h, present: next, future: [] }
          : { past: [...h.past, h.present], present: next, future: [] },
    );
  const undo = () =>
    setH((h) => {
      const previous = h.past.at(-1);
      return previous === undefined
        ? h
        : {
            past: h.past.slice(0, -1),
            present: previous,
            future: [h.present, ...h.future],
          };
    });
  const redo = () =>
    setH((h) => {
      const [next, ...future] = h.future;
      return next === undefined
        ? h
        : { past: [...h.past, h.present], present: next, future };
    });
  /** Starts over from `text` with nothing to undo, as after a reload. */
  const reset = (text: T) => setH({ past: [], present: text, future: [] });
  return { ...h, edit, undo, redo, reset };
}

/** Ctrl+Z undoes, and Ctrl+Shift+Z or Ctrl+Y redoes, outside a text box. */
export function useUndoKeys(undo: () => void, redo: () => void) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || isTyping(e.target)) return;
      const key = e.key.toLowerCase();
      if (key === "z" && !e.shiftKey) undo();
      else if ((key === "z" && e.shiftKey) || key === "y") redo();
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, redo]);
}
