/**
 * A three-way merge by lines, for a save refused because another client
 * moved the file on. Decks and the collection hold one card per line, so two
 * clients' edits usually touch different lines; where both changed the same
 * stretch differently it gives up rather than guess. Whether the merged text
 * is still a file of its kind is Rust's to say, not this module's.
 *
 * The merge is Rust's (`meldweb_git::merge_lines`), so a save and a sync of
 * the browser's repo with its remote merge alike.
 */
import { merge_lines } from "../wasm/pkg/meldweb_wasm.js";

/**
 * `ours` and `theirs` both made from `base`, with each one's changes kept,
 * or null when they changed the same lines differently.
 */
export function mergeLines(
  base: string,
  ours: string,
  theirs: string,
): string | null {
  return merge_lines(base, ours, theirs) ?? null;
}
