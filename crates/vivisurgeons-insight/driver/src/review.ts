import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Finding, SnapDiff } from "./types.ts";

export type ReviewInput = {
  label: string;
  kind: "new" | "changed";
  path: string;
  trees: number;
};

function chunks<T>(xs: T[], size: number): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < xs.length; i += size) out.push(xs.slice(i, i + size));
  return out;
}

/** Evenly spaced, so the same diff always yields the same sample. */
export function sample<T>(xs: T[], cap: number): T[] {
  if (xs.length <= cap) return xs;
  const step = xs.length / cap;
  return Array.from({ length: cap }, (_, i) => xs[Math.floor(i * step)] as T);
}

/**
 * The reviewers' inputs: every new tree, and an even sample of the changed
 * ones, each with its Oracle text, in files of about `size` trees.
 */
export function buildInputs(
  diff: SnapDiff,
  dir: string,
  size: number,
  changedCap: number,
): ReviewInput[] {
  mkdirSync(dir, { recursive: true });
  const inputs: ReviewInput[] = [];
  const write = (kind: "new" | "changed", i: number, body: string[]) => {
    const label = `${kind}-${i + 1}`;
    const path = join(dir, `${label}.txt`);
    const head =
      kind === "new"
        ? "# New trees: abilities that did not parse before this batch.\n"
        : "# Changed trees: abilities whose tree this batch changed. `-` is before, `+` after.\n";
    writeFileSync(path, `${head}\n${body.join("\n\n")}\n`);
    inputs.push({ label, kind, path, trees: body.length });
  };
  chunks(diff.added, size).forEach((c, i) => {
    write(
      "new",
      i,
      c.map((r) => `== ${r.card}\n  text: ${r.text}\n  tree: ${r.tree}`),
    );
  });
  const picked = sample(diff.changed, changedCap);
  chunks(picked, size).forEach((c, i) => {
    write(
      "changed",
      i,
      c.map(
        (r) =>
          `== ${r.card}\n  text: ${r.text}\n${r.old.map((t) => `  - ${t}`).join("\n")}\n${r.new
            .map((t) => `  + ${t}`)
            .join("\n")}`,
      ),
    );
  });
  return inputs;
}

/** The findings file, in the shape the manual reviews used. */
export function findingsMarkdown(
  batch: number,
  input: ReviewInput,
  findings: Finding[],
): string {
  const game = findings.filter((f) => f.severity === "game");
  const minor = findings.filter((f) => f.severity === "minor");
  const regressions = findings.filter((f) => f.regression).length;
  const line = (f: Finding) =>
    `**${f.card}**: ${f.regression ? "REGRESSION. " : ""}${f.node} should be ${f.should_be}.${
      f.semantics_row ? ` (SEMANTICS: ${f.semantics_row})` : ""
    }`;
  return [
    `# Blind review: b${batch} ${input.label} (${input.trees} trees, ${game.length} game-changing, ${minor.length} minor, ${regressions} regression)`,
    "",
    "## Changes the game",
    ...(game.length ? game.map((f, i) => `${i + 1}. ${line(f)}`) : ["None."]),
    "",
    "## Minor",
    ...(minor.length ? minor.map((f) => `- ${line(f)}`) : ["None."]),
    "",
  ].join("\n");
}

/** Runs `work` over `items` with at most `limit` in flight, keeping order. */
export async function pool<T, R>(
  items: T[],
  limit: number,
  work: (item: T) => Promise<R>,
): Promise<R[]> {
  const out: R[] = new Array(items.length);
  let next = 0;
  const lanes = Array.from(
    { length: Math.min(limit, items.length) },
    async () => {
      while (next < items.length) {
        const i = next++;
        out[i] = await work(items[i] as T);
      }
    },
  );
  await Promise.all(lanes);
  return out;
}
