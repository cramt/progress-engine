import { useEffect, useState } from "react";
import { isTyping } from "./card/hotkeys";

/**
 * A deck, or the collection, is its text. An edit is a new text, which makes
 * undo a stack of texts and every change a line diff against the file as
 * loaded.
 */
interface History {
  past: string[];
  present: string;
  future: string[];
}

export function useHistory(initial: string) {
  const [h, setH] = useState<History>({
    past: [],
    present: initial,
    future: [],
  });
  const edit = (next: string) =>
    setH((h) =>
      next === h.present
        ? h
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
  const reset = (text: string) => setH({ past: [], present: text, future: [] });
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
