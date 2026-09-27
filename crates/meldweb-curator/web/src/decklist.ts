import type { Category, Parsed } from "./decklist.gen";
import init, {
  initSync,
  parse,
  set_categories,
} from "./wasm/pkg/meldweb_wasm.js";

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

/** `text` with one line's categories replaced; the rest of the file is untouched. */
export function setCategories(
  text: string,
  line: number,
  categories: readonly Category[],
): string {
  return set_categories(text, line, JSON.stringify(categories));
}
