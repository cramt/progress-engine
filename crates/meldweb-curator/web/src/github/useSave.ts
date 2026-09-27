import { useEffect, useSyncExternalStore } from "react";
import type { SaveState, SaveStore } from "./save";

const idle: SaveState = { status: "saved" };
const none = () => () => {};

/**
 * The save status of `store`, with the page's hide, close and unload listeners
 * attached while the component is mounted. Unmounting (leaving the deck for
 * another route) saves what is pending at once and lets the store go.
 */
export function useSave(store: SaveStore | null): SaveState {
  useEffect(() => {
    if (!store) return;
    const detach = store.attach();
    return () => {
      detach();
      void store.flush().finally(() => store.dispose());
    };
  }, [store]);
  return useSyncExternalStore(
    store ? store.subscribe : none,
    store ? store.getState : () => idle,
  );
}
