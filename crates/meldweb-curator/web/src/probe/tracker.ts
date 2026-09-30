/**
 * When a stream of frames has scanned a card, and when the same card may
 * count again. Continuous scanning reads the table a frame at a time, so a
 * card lying under the camera is in every frame until it is taken away; it is
 * one copy, not one per frame.
 *
 * A card is taken once it has been in `confirm` frames in a row, so a card
 * still sliding into place, read wrongly once, is not added. It is taken
 * once, and counts again only after it has been out of `clear` frames in a
 * row: taken away, or covered by the hand putting the next copy down. One
 * missed detection with the card still there is not enough to count it
 * twice.
 *
 * Cards are told apart by name. The engine's printing is a guess that can
 * change between two frames of the same card (docs/research/probe-in-curator.md),
 * and the name is what it gets right.
 */
export interface TrackerOptions {
  confirm: number;
  clear: number;
}

export const DEFAULT_TRACKING: TrackerOptions = { confirm: 2, clear: 2 };

interface Seen {
  /** Frames in a row it has been in, and out of. */
  streak: number;
  missed: number;
  taken: boolean;
}

export interface Tracker {
  /**
   * One frame's card names; returns the ones it scanned, each once, in the
   * frame's order.
   */
  frame(names: readonly string[]): string[];
  /** Whether a card is taken and still in view, so will not count again yet. */
  holding(name: string): boolean;
}

export function createTracker(
  options: TrackerOptions = DEFAULT_TRACKING,
): Tracker {
  const seen = new Map<string, Seen>();
  return {
    frame(names) {
      const here = new Set(names);
      const taken: string[] = [];
      for (const name of here) {
        const s = seen.get(name) ?? { streak: 0, missed: 0, taken: false };
        s.streak += 1;
        s.missed = 0;
        if (!s.taken && s.streak >= options.confirm) {
          s.taken = true;
          taken.push(name);
        }
        seen.set(name, s);
      }
      for (const [name, s] of seen) {
        if (here.has(name)) continue;
        s.streak = 0;
        s.missed += 1;
        if (!s.taken || s.missed >= options.clear) seen.delete(name);
      }
      return taken;
    },
    holding(name) {
      return seen.get(name)?.taken ?? false;
    },
  };
}
