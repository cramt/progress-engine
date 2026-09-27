import { useEffect, useSyncExternalStore } from "react";
import { flushOnLeave, type SaveState, type SaveStore } from "./save";

const idle: SaveState = { status: "saved" };
const none = () => () => {};

/**
 * The save status of `store`, with the page's hide, close and unload listeners
 * attached while the component is mounted. Unmounting (leaving the deck for
 * another route) saves what is pending at once.
 */
export function useSave(store: SaveStore | null): SaveState {
  useEffect(() => {
    if (!store) return;
    const detach = store.attach();
    return () => {
      detach();
      // Not `dispose`: under StrictMode the store is attached again before
      // this save lands, and disposing would detach that too.
      void flushOnLeave(store);
    };
  }, [store]);
  return useSyncExternalStore(
    store ? store.subscribe : none,
    store ? store.getState : () => idle,
  );
}
