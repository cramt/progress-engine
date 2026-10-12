import { existsSync, readFileSync, writeFileSync } from "node:fs";
import type { Finding, Queued, Resolution } from "./types.ts";

export function readJsonl<T>(path: string): T[] {
  if (!existsSync(path)) return [];
  return readFileSync(path, "utf8")
    .split("\n")
    .filter((l) => l.trim())
    .map((l) => JSON.parse(l) as T);
}

export function writeJsonl<T>(path: string, rows: T[]): void {
  writeFileSync(path, rows.map((r) => `${JSON.stringify(r)}\n`).join(""));
}

const norm = (s: string) => s.trim().toLowerCase().replace(/\s+/g, " ");
export const keyOf = (f: { card: string; node: string }) =>
  `${norm(f.card)}\u0000${norm(f.node)}`;

const urgent = (f: Finding) => f.regression || f.severity === "game";
const rank = (f: Finding) => (f.regression ? 0 : f.severity === "game" ? 1 : 2);

/**
 * Regressions and game-changing findings go to the head of the queue, minor
 * ones to the backlog. A finding already held anywhere is not added twice; a
 * backlog finding that comes back as urgent moves up to the queue.
 */
export function triage(
  queue: Queued[],
  backlog: Queued[],
  found: Queued[],
): { queue: Queued[]; backlog: Queued[]; added: number } {
  const held = new Set([...queue, ...backlog].map(keyOf));
  const fresh: Queued[] = [];
  const seen = new Set<string>();
  for (const f of found) {
    const k = keyOf(f);
    if (seen.has(k)) continue;
    seen.add(k);
    if (!held.has(k)) fresh.push(f);
    else if (urgent(f) && backlog.some((b) => keyOf(b) === k)) fresh.push(f);
  }
  const promoted = new Set(fresh.filter(urgent).map(keyOf));
  const nextBacklog = [
    ...backlog.filter((b) => !promoted.has(keyOf(b))),
    ...fresh.filter((f) => !urgent(f)),
  ];
  // Stable: new before old within a rank.
  const nextQueue = [...fresh.filter(urgent), ...queue].sort(
    (a, b) => rank(a) - rank(b),
  );
  return { queue: nextQueue, backlog: nextBacklog, added: fresh.length };
}

/** Drops what a pushed batch's writer says it closed. */
export function resolve(
  queue: Queued[],
  resolved: Resolution[],
): { queue: Queued[]; closed: number } {
  const done = new Set(resolved.map(keyOf));
  const next = queue.filter((q) => !done.has(keyOf(q)));
  return { queue: next, closed: queue.length - next.length };
}
