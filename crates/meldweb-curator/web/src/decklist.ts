import type { Parsed } from "./decklist.gen";
import init, { initSync, parse } from "./wasm/pkg/meldweb_wasm.js";

export type { Category, Entry, Parsed } from "./decklist.gen";

/** Loads the parser. Everything else in this module needs it done first. */
export async function loadDecklist(): Promise<void> {
  await init();
}

/** For Node, which has no `fetch` for a file URL: hand over the bytes. */
export function loadDecklistSync(module: BufferSource): void {
  initSync({ module });
}

/**
 * The shape is generated from the Rust side, and meldweb-wasm's tests pin the
 * JSON to it, so this cast is the one place the two are trusted to agree.
 */
export function parseDecklist(text: string): Parsed {
  return JSON.parse(parse(text)) as Parsed;
}
