/**
 * A three-way merge by lines, for a save GitHub refused because another
 * client moved the file on. Decks and the collection hold one card per line,
 * so two clients' edits usually touch different lines; where both changed the
 * same stretch differently it gives up rather than guess. Whether the merged
 * text is still a file of its kind is Rust's to say, not this module's.
 */

/**
 * `ours` and `theirs` both made from `base`, with each one's changes kept,
 * or null when they changed the same lines differently.
 */
export function mergeLines(
  base: string,
  ours: string,
  theirs: string,
): string | null {
  if (ours === theirs || theirs === base) return ours;
  if (ours === base) return theirs;
  const b = base.split("\n");
  const o = ours.split("\n");
  const t = theirs.split("\n");
  const inOurs = matching(b, o);
  const inTheirs = matching(b, t);

  // The base lines both sides kept, where each is in all three; between two
  // of them is one hunk. The ends of the texts close the last one.
  const kept: [number, number, number][] = [];
  inOurs.forEach((oi, bi) => {
    const ti = inTheirs[bi] ?? -1;
    if (oi >= 0 && ti >= 0) kept.push([bi, oi, ti]);
  });
  kept.push([b.length, o.length, t.length]);

  const out: string[] = [];
  let [i, j, k] = [0, 0, 0];
  for (const [bi, oi, ti] of kept) {
    const was = b.slice(i, bi);
    const mine = o.slice(j, oi);
    const yours = t.slice(k, ti);
    if (same(mine, was)) out.push(...yours);
    else if (same(yours, was) || same(mine, yours)) out.push(...mine);
    else return null;
    out.push(...b.slice(bi, bi + 1));
    [i, j, k] = [bi + 1, oi + 1, ti + 1];
  }
  return out.join("\n");
}

const same = (a: readonly string[], b: readonly string[]) =>
  a.length === b.length && a.every((line, n) => line === b[n]);

/**
 * For each line of `a`, the line of `b` a longest common subsequence pairs
 * it with, or -1. The shared head and tail are paired first, so the table is
 * only as big as the stretch that changed.
 */
function matching(a: readonly string[], b: readonly string[]): number[] {
  const pair = new Array<number>(a.length).fill(-1);
  let head = 0;
  while (head < a.length && head < b.length && a[head] === b[head]) {
    pair[head] = head;
    head++;
  }
  let tail = 0;
  while (
    tail < a.length - head &&
    tail < b.length - head &&
    a[a.length - 1 - tail] === b[b.length - 1 - tail]
  ) {
    pair[a.length - 1 - tail] = b.length - 1 - tail;
    tail++;
  }
  const n = a.length - head - tail;
  const m = b.length - head - tail;
  // run(x, y): the longest common subsequence of a[head+x..] and b[head+y..].
  const lcs = new Uint32Array((n + 1) * (m + 1));
  const run = (x: number, y: number) => lcs[x * (m + 1) + y] ?? 0;
  for (let x = n - 1; x >= 0; x--)
    for (let y = m - 1; y >= 0; y--)
      lcs[x * (m + 1) + y] =
        a[head + x] === b[head + y]
          ? run(x + 1, y + 1) + 1
          : Math.max(run(x + 1, y), run(x, y + 1));
  let [x, y] = [0, 0];
  while (x < n && y < m) {
    if (a[head + x] === b[head + y]) {
      pair[head + x] = head + y;
      x++;
      y++;
    } else if (run(x + 1, y) >= run(x, y + 1)) x++;
    else y++;
  }
  return pair;
}
