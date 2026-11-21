import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  batchCommitOnOrigin,
  branchName,
  commitsSince,
  dirty,
  branch as gitBranch,
} from "./git.ts";
import { buildRelease, measure, type Paths, row } from "./insight.ts";
import { snapDiff } from "./parse.ts";
import { log, run, secs, tail } from "./sh.ts";
import type {
  CommandFailure,
  GateFailure,
  GateReport,
  Measurement,
  Milestone,
} from "./types.ts";

/** What the gates hold the writer to beyond the code: the human's files. */
export type Guarded = {
  decisionsPath: string;
  /** decisions.md's sha256 when the batch started. */
  decisionsSha: string;
  milestones: Milestone[];
};

export const sha256 = (path: string): string =>
  existsSync(path)
    ? createHash("sha256").update(readFileSync(path)).digest("hex")
    : "missing";

/**
 * The cards a golden file pins: `insight pin` writes one `## ` line per card.
 * A count that moves means someone pinned, which is the human's step;
 * blessing trees (UPDATE_GOLDEN=1) leaves it alone.
 */
export function pinnedCards(text: string | null): number | null {
  if (text === null) return null;
  return new Set(text.split("\n").filter((l) => l.startsWith("## "))).size;
}

async function goldenAt(
  repo: string,
  rev: string,
  name: string,
): Promise<string | null> {
  const r = await run(
    "git",
    ["show", `${rev}:crates/vivisurgeons-insight/golden/${name}.txt`],
    { cwd: repo },
  );
  return r.code === 0 ? r.stdout : null;
}

const COMMANDS: { name: string; args: string[] }[] = [
  { name: "cargo fmt --all --check", args: ["fmt", "--all", "--check"] },
  {
    name: "cargo clippy --all-targets -- -D warnings",
    args: ["clippy", "--all-targets", "--", "-D", "warnings"],
  },
  {
    name: "cargo test -p vivisurgeons-insight",
    args: ["test", "-p", "vivisurgeons-insight"],
  },
];

// The user's rule: no AI attribution in any commit.
const ATTRIBUTION =
  /co-authored-by:|generated with|claude code|anthropic\.com/i;

async function gitFailures(
  paths: Paths,
  baseline: Measurement,
  guarded: Guarded,
): Promise<GateFailure[]> {
  const failures: GateFailure[] = [];
  const b = await gitBranch(paths.repo);
  if (b !== branchName())
    failures.push({ gate: "git", reason: `on ${b}, not ${branchName()}` });
  const d = await dirty(paths.repo);
  if (d) failures.push({ gate: "git", reason: `uncommitted changes:\n${d}` });
  const pushed = await batchCommitOnOrigin(paths.repo, baseline.head);
  if (pushed) {
    failures.push({
      gate: "git",
      reason: `origin/${branchName()} holds the batch's commit ${pushed.slice(0, 7)}; the writer must not push`,
    });
  }
  for (const c of await commitsSince(paths.repo, baseline.head)) {
    if (!c.subject.startsWith("insight: ")) {
      failures.push({
        gate: "commits",
        reason: `${c.sha.slice(0, 7)} "${c.subject}" does not start with "insight: "`,
      });
    }
    if (ATTRIBUTION.test(`${c.subject}\n${c.body}`)) {
      failures.push({
        gate: "commits",
        reason: `${c.sha.slice(0, 7)} carries AI attribution`,
      });
    }
  }
  if (sha256(guarded.decisionsPath) !== guarded.decisionsSha) {
    failures.push({
      gate: "git",
      reason: `${guarded.decisionsPath} changed during the batch; it is the human's. Put it back as it was and report the decision in report.json instead`,
    });
  }
  for (const m of guarded.milestones) {
    const before = pinnedCards(
      await goldenAt(paths.repo, baseline.head, m.name),
    );
    const file = join(paths.crate, "golden", `${m.name}.txt`);
    const now = pinnedCards(
      existsSync(file) ? readFileSync(file, "utf8") : null,
    );
    if (before !== now) {
      failures.push({
        gate: "git",
        reason: `golden/${m.name}.txt went from ${before ?? "absent"} to ${now ?? "absent"} pinned cards; pinning is the human's step`,
      });
    }
  }
  return failures;
}

async function commandFailures(paths: Paths): Promise<CommandFailure[]> {
  const failures: CommandFailure[] = [];
  for (const c of COMMANDS) {
    const r = await run("cargo", c.args, { cwd: paths.repo });
    log(
      "gate",
      `${c.name}: ${r.code === 0 ? "ok" : `exit ${r.code}`} (${secs(r.ms)})`,
    );
    if (r.code !== 0) {
      failures.push({
        gate: "command",
        name: c.name,
        exitCode: r.code,
        output: tail(`${r.stdout}\n${r.stderr}`, 150),
      });
    }
  }
  return failures;
}

function ratchets(
  baseline: Measurement,
  after: Measurement,
  lost: string[],
): GateFailure[] {
  const failures: GateFailure[] = [];
  if (after.roundtrip.failures > baseline.roundtrip.failures) {
    failures.push({
      gate: "roundtrip",
      before: baseline.roundtrip.failures,
      after: after.roundtrip.failures,
    });
  }
  const before = row(baseline.coverage, "all cards").abilities.ok;
  const now = row(after.coverage, "all cards").abilities.ok;
  if (now < before) failures.push({ gate: "coverage", before, after: now });
  for (const [kind, n] of Object.entries(after.lint)) {
    const was = baseline.lint[kind] ?? 0;
    if (n > was) failures.push({ gate: "lint", kind, before: was, after: n });
  }
  if (lost.length > 0) failures.push({ gate: "snapshot-lost", lost });
  return failures;
}

/**
 * Every gate, in order: git hygiene and the human's files, fmt/clippy/test,
 * then the ratchets against the baseline.
 */
export async function runGates(
  paths: Paths,
  baseline: Measurement,
  snapshotPath: string,
  guarded: Guarded,
): Promise<GateReport> {
  const failures: GateFailure[] = await gitFailures(paths, baseline, guarded);
  failures.push(...(await commandFailures(paths)));
  const [first, ...rest] = failures;
  if (first) return { kind: "fail", failures: [first, ...rest] };
  try {
    await buildRelease(paths.repo);
  } catch (e) {
    return {
      kind: "fail",
      failures: [
        {
          gate: "command",
          name: "cargo build --release",
          exitCode: 1,
          output: String(e),
        },
      ],
    };
  }
  const after = await measure(paths, snapshotPath);
  const diff = snapDiff(
    readFileSync(baseline.snapshotPath, "utf8"),
    readFileSync(snapshotPath, "utf8"),
  );
  log(
    "gate",
    `snapshot diff: lost ${diff.lost.length}, new ${diff.added.length}, changed ${diff.changed.length}`,
  );
  const [r0, ...rN] = ratchets(baseline, after, diff.lost);
  if (r0) return { kind: "fail", failures: [r0, ...rN] };
  return { kind: "pass", after, diff };
}

export function describe(f: GateFailure): string {
  switch (f.gate) {
    case "command":
      return `${f.name} exited ${f.exitCode}`;
    case "git":
    case "commits":
      return f.reason;
    case "roundtrip":
      return `roundtrip failures ${f.before} -> ${f.after}`;
    case "coverage":
      return `abilities parsed ${f.before} -> ${f.after}`;
    case "lint":
      return `lint ${f.kind} ${f.before} -> ${f.after}`;
    case "snapshot-lost":
      return `${f.lost.length} abilities that parsed no longer parse`;
  }
}

/** The gate output, as `gate` prints it for the fix-up. */
export function gateReport(failures: GateFailure[]): string {
  return failures
    .map((f) => {
      const head = `- ${describe(f)}`;
      switch (f.gate) {
        case "command":
          return `${head}\n\n\`\`\`\n${f.output}\n\`\`\``;
        case "snapshot-lost":
          return `${head}:\n${f.lost
            .slice(0, 200)
            .map((k) => `  - ${k}`)
            .join("\n")}`;
        default:
          return head;
      }
    })
    .join("\n\n");
}
