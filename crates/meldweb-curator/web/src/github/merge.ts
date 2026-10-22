/**
 * A three-way merge of one file, for a save refused because another client
 * moved the file on (ADR-0021). Which merge is Rust's to say by the path
 * (`meldweb_wasm::merging`): a deck merges card by card, each card's
 * quantity, categories, printing and finish on its own, so two edits to
 * neighbouring cards, or a printing pinned on one side and a quantity
 * changed on the other, both land; any other file merges by lines. Where
 * both changed the same thing differently it gives up rather than guess,
 * and says what, for a deck card by card.
 *
 * A sync and every branch merge use the same dispatch, so a save and a sync
 * merge alike.
 */
import type { TextMerge } from "../deck.gen";
import type { MergeSide } from "../repo/api";
import { merge_file } from "../wasm/pkg/meldweb_wasm.js";

export type { TextMerge };

/**
 * `ours` and `theirs`, both made from `base`, merged as `path`'s kind of
 * file merges, a deck's clashes settled by `picks` (a side for each, by
 * its index) where the user picked them.
 */
export function mergeFile(
  path: string,
  base: string,
  ours: string,
  theirs: string,
  picks: readonly MergeSide[] = [],
): TextMerge {
  return JSON.parse(
    merge_file(path, base, ours, theirs, JSON.stringify(picks)),
  ) as TextMerge;
}
