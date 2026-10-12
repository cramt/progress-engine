// Readers for the insight CLI's plain-text output. Each refuses output it
// does not recognise rather than guessing a number.

import type { CoverageRow, LintCounts, Roundtrip, SnapDiff } from "./types.ts";

const COVERAGE_ROW =
  /^(.+?)\s+cards\s+(\d+)\/(\d+)\s+\S+\s+abilities\s+(\d+)\/(\d+)\s+\S+$/;

export function parseCoverage(stdout: string): CoverageRow[] {
  const rows: CoverageRow[] = [];
  for (const line of stdout.split("\n")) {
    const m = COVERAGE_ROW.exec(line);
    if (!m) continue;
    const [, label, co, ct, ao, at] = m;
    rows.push({
      label: (label ?? "").trim(),
      cards: { ok: Number(co), total: Number(ct) },
      abilities: { ok: Number(ao), total: Number(at) },
    });
  }
  if (!rows.some((r) => r.label === "all cards")) {
    throw new Error("insight coverage printed no 'all cards' row");
  }
  return rows;
}

const ROUNDTRIP = /^(\d+)\/(\d+) parsed abilities round-trip/m;

export function parseRoundtrip(stdout: string): Roundtrip {
  const m = ROUNDTRIP.exec(stdout);
  if (!m) throw new Error("insight roundtrip printed no summary line");
  const ok = Number(m[1]);
  const parsed = Number(m[2]);
  return { ok, parsed, failures: parsed - ok };
}

export function parseLint(stdout: string): LintCounts {
  const counts: LintCounts = {};
  for (const line of stdout.split("\n")) {
    const m = /^([a-z][a-z-]*) (\d+)$/.exec(line.trim());
    if (m?.[1] && m[2]) counts[m[1]] = Number(m[2]);
  }
  if (Object.keys(counts).length === 0) {
    throw new Error("insight lint --counts printed no counts");
  }
  return counts;
}

/**
 * `insight failing` prints three lines per failing ability: the rank padded to
 * five columns, then two lines indented six.
 */
export function failingCount(stdout: string): number {
  return stdout.split("\n").filter((l) => /^ {0,4}\d+ \S/.test(l)).length;
}

type Row = { key: string; card: string; text: string; tree: string };

/** A snapshot row is "card | text | tree"; the key is the first two fields. */
export function splitRow(line: string): Row | null {
  const a = line.indexOf(" | ");
  if (a < 0) return null;
  const b = line.indexOf(" | ", a + 3);
  if (b < 0) return null;
  const card = line.slice(0, a);
  const text = line.slice(a + 3, b);
  return { key: `${card} | ${text}`, card, text, tree: line.slice(b + 3) };
}

function byKey(snapshot: string): Map<string, Row[]> {
  const map = new Map<string, Row[]>();
  for (const line of snapshot.split("\n")) {
    const row = splitRow(line);
    if (!row) continue;
    const rows = map.get(row.key);
    if (rows) rows.push(row);
    else map.set(row.key, [row]);
  }
  return map;
}

/** /tmp/insight/sd.sh, ported: lost keys, new keys, and changed trees. */
export function snapDiff(oldText: string, newText: string): SnapDiff {
  const before = byKey(oldText);
  const after = byKey(newText);
  const diff: SnapDiff = { lost: [], added: [], changed: [] };
  for (const key of before.keys()) if (!after.has(key)) diff.lost.push(key);
  for (const [key, rows] of after) {
    const old = before.get(key);
    if (!old) {
      diff.added.push(...rows);
      continue;
    }
    const o = old.map((r) => r.tree).sort();
    const n = rows.map((r) => r.tree).sort();
    if (o.length === n.length && o.every((t, i) => t === n[i])) continue;
    const first = rows[0];
    if (!first) continue;
    diff.changed.push({
      key,
      card: first.card,
      text: first.text,
      old: o,
      new: n,
    });
  }
  diff.lost.sort();
  diff.added.sort((x, y) => x.key.localeCompare(y.key));
  diff.changed.sort((x, y) => x.key.localeCompare(y.key));
  return diff;
}
