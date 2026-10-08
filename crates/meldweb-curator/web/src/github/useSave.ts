import { useBlocker } from "@tanstack/react-router";
import { useEffect, useSyncExternalStore } from "react";
import { leave, type SaveState, type SaveStore } from "./save";

const idle: SaveState = { status: "saved" };
const none = () => () => {};

const unsaved: Record<"conflict" | "error", string> = {
  conflict: "This file changed on GitHub, so your edits were not saved.",
  error: "Saving failed, so your latest edits are not on GitHub.",
};

/**
 * The save status of `store`, with the page's hide, close and unload listeners
 * attached while the component is mounted. Leaving the deck for another route
 * saves what is pending first, and asks before throwing away edits that
 * cannot be saved. A deck and the collection save alike.
 */
export function useSave(store: SaveStore | null): SaveState {
  useEffect(() => {
    if (!store) return;
    const detach = store.attach();
    return () => {
      detach();
      // Not `dispose`: under StrictMode the store is attached again before
      // this save lands, and disposing would detach that too.
      void leave(store);
    };
  }, [store]);
  useBlocker({
    disabled: !store,
    shouldBlockFn: async ({ current, next }) => {
      // Opening the history or a past version stays on the page, and the
      // editor with it; only leaving the page is leaving.
      if (current.pathname === next.pathname) return false;
      if (!store || (await store.settle())) return false;
      const { status } = store.getState();
      const why = status === "conflict" ? unsaved.conflict : unsaved.error;
      return !window.confirm(`${why}\n\nLeave and lose them?`);
    },
    // `store.attach` already warns on unload while anything is unsaved.
    enableBeforeUnload: false,
  });
  return useSyncExternalStore(
    store ? store.subscribe : none,
    store ? store.getState : () => idle,
  );
}
