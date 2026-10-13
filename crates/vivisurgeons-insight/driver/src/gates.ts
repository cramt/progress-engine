import { readFileSync } from "node:fs";
import {
  batchCommitOnOrigin,
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
} from "./types.ts";

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
): Promise<GateFailure[]> {
  const failures: GateFailure[] = [];
  const b = await gitBranch(paths.repo);
  if (b !== "main") failures.push({ gate: "git", reason: `on ${b}, not main` });
  const d = await dirty(paths.repo);
  if (d) failures.push({ gate: "git", reason: `uncommitted changes:\n${d}` });
  const pushed = await batchCommitOnOrigin(paths.repo, baseline.head);
  if (pushed) {
    failures.push({
      gate: "git",
      reason: `origin/main holds the batch's commit ${pushed.slice(0, 7)}; the writer must not push`,
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
 * Every gate, in order: git hygiene, fmt/clippy/test, then the ratchets
 * against the baseline. `strictGit` is off only for a dry run, whose tree
 * holds whatever is being worked on.
 */
export async function runGates(
  paths: Paths,
  baseline: Measurement,
  snapshotPath: string,
  strictGit: boolean,
): Promise<GateReport> {
  const failures: GateFailure[] = [];
  const g = await gitFailures(paths, baseline);
  if (strictGit) failures.push(...g);
  else for (const f of g) log("gate", `ignored in a dry run: ${describe(f)}`);
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

/** The gate output a fix-up writer is given. */
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
