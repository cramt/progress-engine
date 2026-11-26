import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import type { Finding, Relexed, ReviewInput, SnapDiff } from "./types.ts";

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

const HEADS = {
  new: "# New trees: abilities that did not parse before this batch.\n",
  changed:
    "# Changed trees: abilities whose tree this batch changed. `-` is before, `+` after.\n",
  relexed:
    "# Relexed lines: lines that still do not parse, whose text as the grammar reads it (the card's name as `~`) this batch changed. `-` is before, `+` after, `oracle` the card.\n",
} as const;

/**
 * The reviewers' inputs: every new tree, an even sample of the changed ones,
 * and every relexed card, in files of about `size` entries. `oracle` holds
 * the relexed cards' Oracle text.
 */
export function buildInputs(
  diff: SnapDiff,
  relexed: Relexed[],
  oracle: Map<string, string>,
  dir: string,
  size: number,
  changedCap: number,
): ReviewInput[] {
  mkdirSync(dir, { recursive: true });
  const inputs: ReviewInput[] = [];
  const write = (kind: ReviewInput["kind"], i: number, body: string[]) => {
    const label = `${kind}-${i + 1}`;
    const path = join(dir, `${label}.txt`);
    writeFileSync(path, `${HEADS[kind]}\n${body.join("\n\n")}\n`);
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
  chunks(relexed, size).forEach((c, i) => {
    write(
      "relexed",
      i,
      c.map(
        (r) =>
          `== ${r.card}\n  oracle: ${(oracle.get(r.card) ?? "(not in the bulk file)").replaceAll("\n", " / ")}\n${r.was
            .map((t) => `  - ${t}`)
            .join("\n")}\n${r.now.map((t) => `  + ${t}`).join("\n")}`,
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
