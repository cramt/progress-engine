// The insight batch loop: baseline, one writer, deterministic gates, blind
// review, triage, metrics. README.md beside this directory says how to run it.

import {
  appendFileSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import {
  type AgentEnv,
  type AgentSpec,
  reviewerGuard,
  runAgent,
  writerGuard,
} from "./agents.ts";
import { describe, gateReport, runGates } from "./gates.ts";
import {
  branchName,
  commitsSince,
  dirty,
  branch as gitBranch,
  parkAndReset,
  preflight,
  pushRebasing,
  setBranch,
  type Unpushed,
} from "./git.ts";
import {
  buildRelease,
  bulkFingerprint,
  failing,
  measure,
  type Paths,
  pathsFor,
  row,
} from "./insight.ts";
import { failingCount, snapDiff } from "./parse.ts";
import {
  readJsonl,
  resolve as resolveQueue,
  triage,
  writeJsonl,
} from "./queue.ts";
import {
  buildInputs,
  findingsMarkdown,
  pool,
  type ReviewInput,
} from "./review.ts";
import {
  methodSchema,
  parseMethod,
  parseReview,
  parseWriter,
  reviewSchema,
  writerSchema,
} from "./schemas.ts";
import { log, must, openLog, run, setBatch } from "./sh.ts";
import type {
  AgentRecord,
  AgentRun,
  BatchOutcome,
  Escalation,
  GateFailure,
  InFlight,
  Measurement,
  Milestone,
  MilestoneStatus,
  NonEmpty,
  Queued,
  State,
  StopReason,
  WriterOutput,
} from "./types.ts";

const DRIVER = dirname(dirname(fileURLToPath(import.meta.url)));
const REPO = resolve(DRIVER, "../../..");

const { values: args } = parseArgs({
  options: {
    "state-dir": { type: "string", default: "/tmp/insight/driver" },
    "handoff-source": { type: "string", default: "/tmp/insight/handoff.md" },
    "max-batches": { type: "string", default: "3" },
    "max-cost-usd": { type: "string", default: "50" },
    "writer-hours": { type: "string", default: "3" },
    "commit-cap": { type: "string", default: "15" },
    "fixup-minutes": { type: "string", default: "45" },
    "method-every": { type: "string", default: "3" },
    "review-chunk": { type: "string", default: "90" },
    "review-changed-cap": { type: "string", default: "300" },
    "review-parallel": { type: "string", default: "3" },
    "failing-shown": { type: "string", default: "120" },
    model: { type: "string" },
    "review-model": { type: "string" },
    "claude-path": { type: "string" },
    "dry-run": { type: "boolean", default: false },
    "compare-snapshot": { type: "string" },
    "reset-failures": { type: "boolean", default: false },
    branch: { type: "string", default: "main" },
  },
  strict: true,
});

function num(name: keyof typeof args): number {
  const n = Number(args[name]);
  if (!Number.isFinite(n) || n <= 0)
    throw new Error(`--${name} must be a positive number`);
  return n;
}

const cfg = {
  stateDir: resolve(args["state-dir"]),
  maxBatches: num("max-batches"),
  maxCostUsd: num("max-cost-usd"),
  writerMs: num("writer-hours") * 3_600_000,
  commitCap: num("commit-cap"),
  fixupMs: num("fixup-minutes") * 60_000,
  methodEvery: num("method-every"),
  reviewChunk: num("review-chunk"),
  reviewChangedCap: num("review-changed-cap"),
  reviewParallel: num("review-parallel"),
  failingShown: num("failing-shown"),
};

setBranch(args.branch);

const paths: Paths = pathsFor(REPO);
const file = (name: string) => join(cfg.stateDir, name);
const batchDir = (b: number) => file(`batches/${b}`);
const HANDOFF = file("handoff.md");
const METHOD_NOTES = file("method-notes.md");
const DECISIONS = file("decisions.md");
const AST = join(paths.crate, "src/ast.rs");
const SEMANTICS = join(paths.crate, "SEMANTICS.md");

const milestones = JSON.parse(
  readFileSync(join(DRIVER, "milestones.json"), "utf8"),
) as Milestone[];

let spentUsd = 0;
const env: AgentEnv = {
  model: args.model ?? null,
  reviewModel: args["review-model"] ?? null,
  claudePath: args["claude-path"] ?? null,
  budgetUsd: () => cfg.maxCostUsd - spentUsd,
};

async function agent<T>(
  spec: AgentSpec<T>,
): Promise<{ run: AgentRun<T>; record: AgentRecord }> {
  const r = await runAgent(env, spec);
  spentUsd += r.record.result?.costUsd ?? 0;
  return r;
}

// ---------- state ----------

function loadState(): State {
  const p = file("state.json");
  if (!existsSync(p)) {
    return {
      version: 1,
      nextBatch: 1,
      consecutiveFailures: 0,
      lifetimeCostUsd: 0,
      inFlight: null,
    };
  }
  return JSON.parse(readFileSync(p, "utf8")) as State;
}

function saveState(s: State): void {
  const p = file("state.json");
  writeFileSync(`${p}.tmp`, `${JSON.stringify(s, null, 2)}\n`);
  renameSync(`${p}.tmp`, p);
}

function initStateDir(): void {
  mkdirSync(cfg.stateDir, { recursive: true });
  if (!existsSync(HANDOFF)) {
    copyFileSync(args["handoff-source"], HANDOFF);
    log("init", `handoff copied from ${args["handoff-source"]} to ${HANDOFF}`);
  }
  if (!existsSync(METHOD_NOTES))
    writeFileSync(METHOD_NOTES, "No method notes yet.\n");
  if (!existsSync(DECISIONS))
    writeFileSync(DECISIONS, "## Decisions from the supervisor\n\nNone yet.\n");
}

const decisionsText = (): string => readFileSync(DECISIONS, "utf8").trim();

// ---------- prompts ----------

function render(
  template: string,
  vars: Record<string, string | number>,
): string {
  const text = readFileSync(join(DRIVER, "prompts", template), "utf8").replace(
    /\{\{(\w+)\}\}/g,
    (m, k: string) => (k in vars ? String(vars[k]) : m),
  );
  const left = text.match(/\{\{\w+\}\}/g);
  if (left)
    throw new Error(`${template}: unfilled ${[...new Set(left)].join(", ")}`);
  return text;
}

function lintLine(m: Measurement): string {
  const hits = Object.entries(m.lint).filter(([, n]) => n > 0);
  return hits.length
    ? hits.map(([k, n]) => `${k} ${n}`).join(", ")
    : "all zero";
}

function baselineLine(m: Measurement): string {
  const all = row(m.coverage, "all cards");
  return `abilities ${all.abilities.ok}/${all.abilities.total}, cards ${all.cards.ok}/${all.cards.total}, round-trip failures ${m.roundtrip.failures}, lint ${lintLine(m)}`;
}

function queueText(queue: Queued[]): string {
  if (queue.length === 0) return "None queued.";
  const shown = queue.slice(0, 60).map((f, i) => {
    const tag = f.regression ? "REGRESSION" : f.severity;
    const sem = f.semantics_row ? ` (SEMANTICS: ${f.semantics_row})` : "";
    return `${i + 1}. [${tag}] **${f.card}**: \`${f.node}\` should be \`${f.should_be}\`${sem} (b${f.batch} ${f.source})`;
  });
  const more =
    queue.length > 60
      ? `\n\n...and ${queue.length - 60} more in ${file("queue.jsonl")}.`
      : "";
  return shown.join("\n") + more;
}

function milestoneStatus(m: Measurement): MilestoneStatus {
  for (const ms of milestones) {
    if (existsSync(join(paths.crate, "golden", `${ms.name}.txt`))) continue;
    const r = row(m.coverage, ms.coverageLabel);
    const done =
      r.cards.ok === r.cards.total && r.abilities.ok === r.abilities.total;
    return { kind: done ? "ready-to-pin" : "working", milestone: ms, row: r };
  }
  return { kind: "all-pinned" };
}

function rowLine(s: MilestoneStatus): string {
  if (s.kind === "all-pinned") return "";
  const r = s.row;
  return `Coverage: cards ${r.cards.ok}/${r.cards.total}, abilities ${r.abilities.ok}/${r.abilities.total}.`;
}

async function writerPrompt(
  batch: number,
  baseline: Measurement,
  s: Extract<MilestoneStatus, { kind: "working" }>,
  dir: string,
): Promise<string> {
  const list = await failing(paths, s.milestone);
  const failingPath = join(dir, "failing.txt");
  writeFileSync(failingPath, `${list}\n`);
  const count = failingCount(list);
  const shown = Math.min(count, cfg.failingShown);
  const queue = readJsonl<Queued>(file("queue.jsonl"));
  log(
    "prompt",
    `${s.milestone.name}: ${count} failing abilities (${shown} in the prompt), ${queue.length} queued findings`,
  );
  return render("writer.md", {
    batch,
    repo: REPO,
    crate: paths.crate,
    handoffPath: HANDOFF,
    handoff: readFileSync(HANDOFF, "utf8"),
    decisionsPath: DECISIONS,
    decisions: decisionsText(),
    commitCap: cfg.commitCap,
    hours: cfg.writerMs / 3_600_000,
    milestone: s.milestone.name,
    milestoneRow: rowLine(s),
    baselineHead: baseline.head.slice(0, 7),
    baseline: baselineLine(baseline),
    roundtripFailures: baseline.roundtrip.failures,
    abilitiesOk: row(baseline.coverage, "all cards").abilities.ok,
    methodNotes: readFileSync(METHOD_NOTES, "utf8").trim(),
    queueCount: queue.length,
    queue: queueText(queue),
    failingCount: count,
    failingShown: shown,
    failingPath,
    failing:
      list
        .split("\n")
        .slice(0, shown * 3)
        .join("\n") || "None.",
  });
}

function writerSpec(
  label: string,
  prompt: string,
  transcript: string,
  baseline: Measurement,
  softMs: number,
  commitLimit: number,
  graceMs: number,
): AgentSpec<WriterOutput> {
  return {
    role: label === "writer" ? "writer" : "fixup",
    label,
    prompt,
    cwd: REPO,
    tools: [
      "Bash",
      "Read",
      "Edit",
      "Write",
      "Grep",
      "Glob",
      "WebFetch",
      "WebSearch",
      "TodoWrite",
    ],
    loadProject: true,
    writes: true,
    guard: writerGuard({
      repo: REPO,
      baseline: baseline.head,
      commitLimit,
      softDeadline: Date.now() + softMs,
      handoffPath: HANDOFF,
      decisionsPath: DECISIONS,
    }),
    schema: writerSchema,
    parse: parseWriter,
    extraDirs: [cfg.stateDir],
    hardMs: softMs + graceMs,
    transcript,
  };
}

function escalate(
  batch: number | null,
  source: string,
  items: Escalation[] | string,
): void {
  const p = file("escalations.md");
  const head = `\n## ${batch === null ? "-" : `b${batch}`} ${new Date().toISOString()} (${source})\n\n`;
  const lines =
    typeof items === "string"
      ? [items]
      : items.map(
          (e) =>
            `- [${e.kind}] ${e.cards.length ? `${e.cards.join(", ")}: ` : ""}${e.question}`,
        );
  appendFileSync(p, `${head}${lines.join("\n")}\n`);
  log(
    "escalate",
    `${source}: ${typeof items === "string" ? items : `${items.length} item(s)`} -> ${p}`,
  );
}

// ---------- the batch ----------

/** A baseline measured at another commit, in a worktree, leaving main alone. */
async function measureAt(sha: string, snapshot: string): Promise<Measurement> {
  const wt = file("rebaseline-wt");
  const target = file("rebaseline-target");
  await run("git", ["worktree", "remove", "--force", wt], { cwd: REPO });
  await must("git", ["worktree", "add", "--detach", wt, sha], {
    cwd: REPO,
  });
  try {
    await buildRelease(wt, { ...process.env, CARGO_TARGET_DIR: target });
    return await measure(
      {
        repo: wt,
        crate: join(wt, "crates/vivisurgeons-insight"),
        insight: join(target, "release/insight"),
      },
      snapshot,
    );
  } finally {
    await run("git", ["worktree", "remove", "--force", wt], { cwd: REPO });
  }
}

type Started =
  | { kind: "started"; inFlight: InFlight }
  | { kind: "stop"; reason: StopReason };

async function startBatch(batch: number): Promise<Started> {
  const pf = await preflight(REPO);
  if (pf.kind === "refused")
    return { kind: "stop", reason: { kind: "unsafe-git", reason: pf.reason } };
  log(
    "baseline",
    `HEAD ${pf.head.slice(0, 7)}, in sync with origin/${branchName()}`,
  );
  await buildRelease(REPO);
  const dir = batchDir(batch);
  mkdirSync(dir, { recursive: true });
  const baseline = await measure(paths, join(dir, "baseline.snap"));
  writeFileSync(join(dir, "baseline.json"), JSON.stringify(baseline, null, 2));
  const s = milestoneStatus(baseline);
  switch (s.kind) {
    case "all-pinned":
      return { kind: "stop", reason: { kind: "all-pinned" } };
    case "ready-to-pin":
      escalate(
        batch,
        "milestone",
        `${s.milestone.name} is at 100% (${rowLine(s)}). Review it, then pin with \`insight pin ${s.milestone.target}\`; the driver moves on once golden/${s.milestone.name}.txt exists.`,
      );
      return {
        kind: "stop",
        reason: { kind: "milestone-ready", milestone: s.milestone.name },
      };
    case "working":
      log("milestone", `${s.milestone.name}: ${rowLine(s)}`);
      return {
        kind: "started",
        inFlight: { step: "write", batch, baseline, agents: [] },
      };
  }
}

async function stepWrite(
  f: Extract<InFlight, { step: "write" }>,
): Promise<InFlight> {
  const dir = batchDir(f.batch);
  const already = await commitsSince(REPO, f.baseline.head);
  if (already.length > 0) {
    log(
      "writer",
      `resumed with ${already.length} commits past the baseline; gating them`,
    );
    return { ...f, step: "gate", writer: null, fixupTried: false };
  }
  const s = milestoneStatus(f.baseline);
  if (s.kind !== "working")
    throw new Error(
      `batch ${f.batch} started on a milestone that is ${s.kind}`,
    );
  const prompt = await writerPrompt(f.batch, f.baseline, s, dir);
  writeFileSync(join(dir, "writer-prompt.md"), prompt);
  const r = await agent(
    writerSpec(
      "writer",
      prompt,
      join(dir, "writer.jsonl"),
      f.baseline,
      cfg.writerMs,
      cfg.commitCap,
      20 * 60_000,
    ),
  );
  const writer = r.run.kind === "completed" ? r.run.output : null;
  if (writer?.escalations.length)
    escalate(f.batch, "writer", writer.escalations);
  if (writer) log("writer", `summary: ${writer.summary.slice(0, 300)}`);
  return {
    ...f,
    step: "gate",
    agents: [...f.agents, r.record],
    writer,
    fixupTried: false,
  };
}

const PUSH_REBASES = 3;

/**
 * The gates again, for commits rebased onto a main someone else moved: the
 * ratchets compare against a baseline measured at origin/main, so a batch may
 * not regress what is there now.
 */
async function regate(
  dir: string,
  onto: string,
  attempt: number,
): Promise<
  | { ok: true; value: { baseline: Measurement; after: Measurement } }
  | { ok: false; failure: NonEmpty<GateFailure> }
> {
  const baseline = await measureAt(
    onto,
    join(dir, `baseline-origin-${attempt}.snap`),
  );
  writeFileSync(
    join(dir, `baseline-origin-${attempt}.json`),
    JSON.stringify(baseline, null, 2),
  );
  const report = await runGates(
    paths,
    baseline,
    join(dir, `after-rebase-${attempt}.snap`),
    true,
  );
  if (report.kind === "pass")
    return { ok: true, value: { baseline, after: report.after } };
  writeFileSync(
    join(dir, `gates-rebase-${attempt}.md`),
    gateReport(report.failures),
  );
  return { ok: false, failure: report.failures };
}

function unpushedLine(c: Unpushed<NonEmpty<GateFailure>>): string {
  switch (c.kind) {
    case "conflict":
      return `rebasing onto origin/main ${c.onto.slice(0, 7)} conflicted: ${c.detail}`;
    case "gates":
      return `rebased onto origin/main ${c.onto.slice(0, 7)}, it failed the gates: ${c.failure.map(describe).join("; ")}`;
    case "rejected":
      return `the push was rejected ${c.attempts} times: ${c.detail}`;
    case "error":
      return `git failed: ${c.detail}`;
  }
}

async function stepGate(
  f: Extract<InFlight, { step: "gate" }>,
): Promise<InFlight | StopReason> {
  const dir = batchDir(f.batch);
  const commits = await commitsSince(REPO, f.baseline.head);
  if (commits.length === 0 && !(await dirty(REPO))) {
    log("gate", "the writer made no commits");
    return { ...f, step: "record", outcome: { kind: "empty" }, findings: [] };
  }
  let baseline = f.baseline;
  if (JSON.stringify(bulkFingerprint()) !== JSON.stringify(baseline.bulk)) {
    log(
      "gate",
      "the Scryfall bulk changed during the batch; measuring the baseline again in a worktree",
    );
    baseline = await measureAt(
      baseline.head,
      join(dir, "baseline-rebased.snap"),
    );
  }
  const report = await runGates(
    paths,
    baseline,
    join(dir, f.fixupTried ? "after-fixup.snap" : "after.snap"),
    true,
  );
  if (report.kind === "pass") {
    const p = await pushRebasing(REPO, {
      rebases: PUSH_REBASES,
      parkAs: `insight/unpushed-${f.batch}`,
      regate: (onto, attempt) => regate(dir, onto, attempt),
    });
    switch (p.kind) {
      case "pushed": {
        // After a rebase the review and the metrics compare against what
        // origin held, so the human's commits aren't counted as the batch's.
        const now = p.regated ?? { baseline, after: report.after };
        log(
          "push",
          `${commits.length} commits pushed${p.rebases ? ` after ${p.rebases} rebase(s)` : ""}, main at ${p.head.slice(0, 7)}`,
        );
        return {
          step: "review",
          batch: f.batch,
          baseline: now.baseline,
          agents: f.agents,
          writer: f.writer,
          outcome: {
            kind: "pushed",
            head: p.head,
            commits: commits.length,
            fixedUp: f.fixupTried,
            after: now.after,
          },
        };
      }
      case "unpushed":
        escalate(
          f.batch,
          "push",
          `The batch passed its gates but could not be pushed: ${unpushedLine(p.cause)}. Its ${commits.length} commits are on ${p.branch}; main was reset to origin/main ${p.resetTo.slice(0, 7)}. Rebase and push them by hand, or drop the branch.`,
        );
        return {
          ...f,
          baseline,
          step: "record",
          outcome: {
            kind: "unpushed",
            branch: p.branch,
            commits: commits.length,
            cause: p.cause,
          },
          findings: [],
        };
      case "refused":
        escalate(
          f.batch,
          "git",
          `The batch passed its gates but could not be pushed (${unpushedLine(p.cause)}), and the driver refused to park it: ${p.reason}. Nothing was reset.`,
        );
        return { kind: "unsafe-git", reason: p.reason };
    }
  }
  const text = gateReport(report.failures);
  writeFileSync(join(dir, f.fixupTried ? "gates-fixup.md" : "gates.md"), text);
  log(
    "gate",
    `failed: ${report.failures.map(describe).join("; ").slice(0, 400)}`,
  );
  if (!f.fixupTried) {
    // Saved before the call, so a crash during the fix-up can't earn a second one.
    saveState({
      ...loadState(),
      inFlight: { ...f, baseline, fixupTried: true },
    });
    const prompt = render("fixup.md", {
      batch: f.batch,
      crate: paths.crate,
      repo: REPO,
      commits: commits.length,
      baselineHead: baseline.head.slice(0, 7),
      gateOutput: text,
      roundtripFailures: baseline.roundtrip.failures,
      abilitiesOk: row(baseline.coverage, "all cards").abilities.ok,
      lintBaseline: lintLine(baseline),
      baselineSnapshot: baseline.snapshotPath,
      handoffPath: HANDOFF,
      decisionsPath: DECISIONS,
      decisions: decisionsText(),
    });
    writeFileSync(join(dir, "fixup-prompt.md"), prompt);
    const r = await agent(
      writerSpec(
        "fixup",
        prompt,
        join(dir, "fixup.jsonl"),
        baseline,
        cfg.fixupMs,
        commits.length + 5,
        15 * 60_000,
      ),
    );
    const out = r.run.kind === "completed" ? r.run.output : null;
    if (out?.escalations.length) escalate(f.batch, "fixup", out.escalations);
    return {
      ...f,
      baseline,
      agents: [...f.agents, r.record],
      writer: out ?? f.writer,
      fixupTried: true,
    };
  }
  const parked = await parkAndReset(
    REPO,
    baseline.head,
    `insight/failed-${f.batch}`,
  );
  if (parked.kind === "refused") {
    escalate(
      f.batch,
      "git",
      `Gates failed after the fix-up, and the driver refused to reset main: ${parked.reason}. Nothing was reset.`,
    );
    return { kind: "unsafe-git", reason: parked.reason };
  }
  return {
    ...f,
    baseline,
    step: "record",
    outcome: {
      kind: "failed",
      branch: parked.branch,
      commits: commits.length,
      failures: report.failures,
    },
    findings: [],
  };
}

async function stepReview(
  f: Extract<InFlight, { step: "review" }>,
): Promise<InFlight> {
  const dir = join(batchDir(f.batch), "review");
  const diff = snapDiff(
    readFileSync(f.baseline.snapshotPath, "utf8"),
    readFileSync(f.outcome.after.snapshotPath, "utf8"),
  );
  const inputs = buildInputs(diff, dir, cfg.reviewChunk, cfg.reviewChangedCap);
  log(
    "review",
    `${inputs.length} inputs from ${diff.added.length} new and ${diff.changed.length} changed trees`,
  );
  const results = await pool(inputs, cfg.reviewParallel, (input) =>
    review(f.batch, input),
  );
  const findings = results.flatMap((r) => r.findings);
  writeJsonl(join(dir, "findings.jsonl"), findings);
  return {
    step: "record",
    batch: f.batch,
    baseline: f.baseline,
    agents: [...f.agents, ...results.map((r) => r.record)],
    writer: f.writer,
    outcome: f.outcome,
    findings,
  };
}

async function review(
  batch: number,
  input: ReviewInput,
): Promise<{ findings: Queued[]; record: AgentRecord }> {
  const prompt = render("reviewer.md", {
    astPath: AST,
    semanticsPath: SEMANTICS,
    inputPath: input.path,
    kind: input.kind,
    trees: input.trees,
    kindNote:
      input.kind === "new"
        ? "These abilities did not parse before this batch, so `regression` is always false."
        : "Each tree shows its old form (`-`) and its new one (`+`). Judge the new one, and say whether the old one was right.",
  });
  const r = await agent({
    role: "reviewer",
    label: `reviewer ${input.label}`,
    prompt,
    cwd: paths.crate,
    tools: ["Read", "Grep", "Glob", "WebFetch"],
    loadProject: false,
    writes: false,
    guard: reviewerGuard([AST, SEMANTICS, input.path]),
    schema: reviewSchema,
    parse: parseReview,
    extraDirs: [dirname(input.path)],
    hardMs: 45 * 60_000,
    transcript: join(dirname(input.path), `${input.label}.jsonl`),
  });
  const found = r.run.kind === "completed" ? r.run.output.findings : [];
  writeFileSync(
    join(dirname(input.path), `findings-${input.label}.md`),
    findingsMarkdown(batch, input, found),
  );
  return {
    findings: found.map((x) => ({ ...x, batch, source: input.label })),
    record: r.record,
  };
}

function batchCost(agents: AgentRecord[]): number {
  return agents.reduce((s, a) => s + (a.result?.costUsd ?? 0), 0);
}

function gained(baseline: Measurement, outcome: BatchOutcome, label: string) {
  const before = row(baseline.coverage, label);
  if (outcome.kind !== "pushed") {
    return {
      label,
      abilities: {
        before: before.abilities.ok,
        after: before.abilities.ok,
        gained: 0,
      },
      cards: { before: before.cards.ok, after: before.cards.ok, gained: 0 },
    };
  }
  const after = row(outcome.after.coverage, label);
  return {
    label,
    abilities: {
      before: before.abilities.ok,
      after: after.abilities.ok,
      gained: after.abilities.ok - before.abilities.ok,
    },
    cards: {
      before: before.cards.ok,
      after: after.cards.ok,
      gained: after.cards.ok - before.cards.ok,
    },
  };
}

function stepRecord(
  f: Extract<InFlight, { step: "record" }>,
  state: State,
): State {
  // Resolutions apply to the queue the writer was shown, before this batch's
  // findings join it: a new regression on the same node must survive.
  let held = readJsonl<Queued>(file("queue.jsonl"));
  let closed = 0;
  if (f.outcome.kind === "pushed" && f.writer) {
    const r = resolveQueue(held, f.writer.resolved);
    held = r.queue;
    closed = r.closed;
  }
  const q = triage(held, readJsonl<Queued>(file("backlog.jsonl")), f.findings);
  const queue = q.queue;
  writeJsonl(file("queue.jsonl"), queue);
  writeJsonl(file("backlog.jsonl"), q.backlog);
  log(
    "triage",
    `${q.added} new findings held; ${closed} closed by the writer; queue ${queue.length}, backlog ${q.backlog.length}`,
  );
  const s = milestoneStatus(f.baseline);
  const after = f.outcome.kind === "pushed" ? f.outcome.after : f.baseline;
  const cost = batchCost(f.agents);
  const metrics = {
    batch: f.batch,
    startedAt: f.baseline.takenAt,
    endedAt: new Date().toISOString(),
    outcome: f.outcome.kind,
    ...(f.outcome.kind === "failed"
      ? {
          failedBranch: f.outcome.branch,
          failures: f.outcome.failures.map(describe),
        }
      : {}),
    ...(f.outcome.kind === "unpushed"
      ? {
          unpushedBranch: f.outcome.branch,
          unpushed: unpushedLine(f.outcome.cause),
        }
      : {}),
    commits: f.outcome.kind === "empty" ? 0 : f.outcome.commits,
    fixedUp: f.outcome.kind === "pushed" ? f.outcome.fixedUp : null,
    head: { before: f.baseline.head, after: after.head },
    overall: gained(f.baseline, f.outcome, "all cards"),
    milestone:
      s.kind === "all-pinned"
        ? null
        : {
            name: s.milestone.name,
            ...gained(f.baseline, f.outcome, s.milestone.coverageLabel),
          },
    roundtripFailures: {
      before: f.baseline.roundtrip.failures,
      after: after.roundtrip.failures,
    },
    lint: { before: f.baseline.lint, after: after.lint },
    findings: {
      total: f.findings.length,
      game: f.findings.filter((x) => x.severity === "game").length,
      minor: f.findings.filter((x) => x.severity === "minor").length,
      regressions: f.findings.filter((x) => x.regression).length,
    },
    queue: { length: queue.length, closed },
    costUsd: cost,
    agents: f.agents,
  };
  appendFileSync(file("metrics.jsonl"), `${JSON.stringify(metrics)}\n`);
  log(
    "metrics",
    `${f.outcome.kind}: abilities +${metrics.overall.abilities.gained}, ${metrics.milestone?.name ?? "-"} +${metrics.milestone?.abilities.gained ?? 0}, findings ${metrics.findings.total} (${metrics.findings.game} game, ${metrics.findings.regressions} regressions), $${cost.toFixed(2)}`,
  );
  const failed = f.outcome.kind !== "pushed";
  return {
    ...state,
    nextBatch: f.batch + 1,
    consecutiveFailures: failed ? state.consecutiveFailures + 1 : 0,
    lifetimeCostUsd: state.lifetimeCostUsd + cost,
    inFlight: null,
  };
}

async function methodReview(batch: number): Promise<number> {
  const dir = batchDir(batch);
  const findings = [...Array(Math.min(batch, cfg.methodEvery)).keys()]
    .map((i) => join(batchDir(batch - i), "review"))
    .filter((d) => existsSync(d))
    .map((d) => `\`${d}/findings-*.md\``);
  const prompt = render("method.md", {
    batch,
    commitCap: cfg.commitCap,
    hours: cfg.writerMs / 3_600_000,
    metricsPath: file("metrics.jsonl"),
    findingsPaths: findings.length ? findings.join(", ") : "none yet",
    methodNotes: readFileSync(METHOD_NOTES, "utf8").trim(),
  });
  const r = await agent({
    role: "method",
    label: "method review",
    prompt,
    cwd: cfg.stateDir,
    tools: ["Read", "Grep", "Glob"],
    loadProject: false,
    writes: false,
    guard: async () => ({}),
    schema: methodSchema,
    parse: parseMethod,
    extraDirs: [cfg.stateDir],
    hardMs: 30 * 60_000,
    transcript: join(dir, "method.jsonl"),
  });
  if (r.run.kind === "completed") {
    copyFileSync(METHOD_NOTES, join(dir, "method-notes.before.md"));
    writeFileSync(METHOD_NOTES, `${r.run.output.method_notes.trim()}\n`);
    writeFileSync(
      join(dir, "method-rationale.md"),
      `${r.run.output.rationale.trim()}\n`,
    );
    log(
      "method",
      `method-notes.md rewritten (${r.run.output.method_notes.split("\n").length} lines)`,
    );
  }
  appendFileSync(
    file("metrics.jsonl"),
    `${JSON.stringify({ methodReview: batch, at: new Date().toISOString(), agent: r.record })}\n`,
  );
  return r.record.result?.costUsd ?? 0;
}

function stopFor(state: State, ran: number): StopReason | null {
  if (state.consecutiveFailures >= 2)
    return { kind: "consecutive-failures", count: state.consecutiveFailures };
  if (ran >= cfg.maxBatches) return { kind: "max-batches", batches: ran };
  if (spentUsd >= cfg.maxCostUsd) return { kind: "max-cost", spentUsd };
  return null;
}

/**
 * The exit status of a run that stopped on something only a human can clear,
 * so whatever schedules the driver knows not to just start it again.
 */
const NEEDS_HUMAN = 3;

function needsHuman(r: StopReason): boolean {
  switch (r.kind) {
    case "max-batches":
    case "max-cost":
      return false;
    case "consecutive-failures":
    case "milestone-ready":
    case "all-pinned":
    case "unsafe-git":
      return true;
  }
}

function stopLine(r: StopReason): string {
  switch (r.kind) {
    case "max-batches":
      return `ran ${r.batches} batches (--max-batches)`;
    case "max-cost":
      return `spent $${r.spentUsd.toFixed(2)} (--max-cost-usd)`;
    case "consecutive-failures":
      return `${r.count} consecutive batches failed; look, then rerun with --reset-failures`;
    case "milestone-ready":
      return `${r.milestone} is at 100% and waits for a human to pin it`;
    case "all-pinned":
      return "every milestone is pinned";
    case "unsafe-git":
      return `git: ${r.reason}`;
  }
}

async function loop(): Promise<void> {
  let state = loadState();
  if (args["reset-failures"] && state.consecutiveFailures > 0) {
    log("init", `consecutive failures reset from ${state.consecutiveFailures}`);
    state = { ...state, consecutiveFailures: 0 };
    saveState(state);
  }
  log(
    "init",
    `state ${file("state.json")}: next batch ${state.nextBatch}, ${state.inFlight ? `resuming b${state.inFlight.batch} at ${state.inFlight.step}` : "nothing in flight"}, lifetime $${state.lifetimeCostUsd.toFixed(2)}`,
  );
  let ran = 0;
  let stop: StopReason | null = null;
  while (!stop) {
    let inFlight = state.inFlight;
    if (!inFlight) {
      stop = stopFor(state, ran);
      if (stop) break;
      setBatch(state.nextBatch);
      const s = await startBatch(state.nextBatch);
      if (s.kind === "stop") {
        stop = s.reason;
        break;
      }
      inFlight = s.inFlight;
      state = { ...state, inFlight };
      saveState(state);
    }
    let f: InFlight | StopReason = inFlight;
    setBatch(f.batch);
    while ("step" in f && f.step !== "record") {
      f =
        f.step === "write"
          ? await stepWrite(f)
          : f.step === "gate"
            ? await stepGate(f)
            : await stepReview(f);
      if ("step" in f) {
        state = { ...loadState(), inFlight: f };
        saveState(state);
      }
    }
    if (!("step" in f)) {
      stop = f;
      break;
    }
    const batch = f.batch;
    state = stepRecord(f, state);
    saveState(state);
    ran++;
    if (batch % cfg.methodEvery === 0) {
      const cost = await methodReview(batch);
      state = { ...state, lifetimeCostUsd: state.lifetimeCostUsd + cost };
      saveState(state);
    }
  }
  setBatch(null);
  if (stop.kind === "consecutive-failures" || stop.kind === "unsafe-git")
    escalate(null, "stop", stopLine(stop));
  if (needsHuman(stop)) process.exitCode = NEEDS_HUMAN;
  log(
    "stop",
    `${stopLine(stop)}; this run spent $${spentUsd.toFixed(2)}, lifetime $${state.lifetimeCostUsd.toFixed(2)}`,
  );
}

// ---------- dry run ----------

async function dryRun(): Promise<void> {
  const dir = file("dry-run");
  mkdirSync(dir, { recursive: true });
  const state = loadState();
  log(
    "dry-run",
    `no agent is called, nothing is pushed or reset, ${file("state.json")} is not written (next batch would be ${state.nextBatch})`,
  );
  const b = await gitBranch(REPO);
  const d = await dirty(REPO);
  log(
    "preflight",
    `on ${b}; tree ${d ? `dirty (${d.split("\n").length} paths; a real run refuses)` : "clean"}`,
  );
  const pf = await preflight(REPO);
  log(
    "preflight",
    pf.kind === "ready"
      ? `ready at ${pf.head.slice(0, 7)}`
      : `a real run would refuse: ${pf.reason.split("\n")[0]}`,
  );
  await buildRelease(REPO);
  const baseline = await measure(paths, join(dir, "baseline.snap"));
  writeFileSync(join(dir, "baseline.json"), JSON.stringify(baseline, null, 2));
  for (const ms of milestones) {
    const r = row(baseline.coverage, ms.coverageLabel);
    const pinned = existsSync(join(paths.crate, "golden", `${ms.name}.txt`));
    log(
      "milestone",
      `${ms.name}: abilities ${r.abilities.ok}/${r.abilities.total}, cards ${r.cards.ok}/${r.cards.total}${pinned ? " (pinned)" : ""}`,
    );
  }
  const s = milestoneStatus(baseline);
  log(
    "milestone",
    s.kind === "all-pinned"
      ? "all pinned"
      : `current: ${s.milestone.name} (${s.kind})`,
  );
  if (s.kind === "working") {
    const prompt = await writerPrompt(state.nextBatch, baseline, s, dir);
    writeFileSync(join(dir, "writer-prompt.md"), prompt);
    log(
      "prompt",
      `writer prompt rendered: ${prompt.length} chars -> ${join(dir, "writer-prompt.md")}`,
    );
  }
  const report = await runGates(
    paths,
    baseline,
    join(dir, "after.snap"),
    false,
  );
  if (report.kind === "fail") {
    writeFileSync(join(dir, "gates.md"), gateReport(report.failures));
    log("gate", `FAIL: ${report.failures.map(describe).join("; ")}`);
  } else {
    log("gate", "pass");
  }
  const old = args["compare-snapshot"];
  const diff =
    old !== undefined
      ? snapDiff(
          readFileSync(old, "utf8"),
          readFileSync(baseline.snapshotPath, "utf8"),
        )
      : report.kind === "pass"
        ? report.diff
        : null;
  if (!diff) {
    log("review", "no diff to build review inputs from");
    return;
  }
  log(
    "review",
    `diff ${old ?? "baseline"} -> HEAD: lost ${diff.lost.length}, new ${diff.added.length}, changed ${diff.changed.length}`,
  );
  const inputs = buildInputs(
    diff,
    join(dir, "review"),
    cfg.reviewChunk,
    cfg.reviewChangedCap,
  );
  for (const i of inputs)
    log("review", `${i.label}: ${i.trees} trees -> ${i.path}`);
  const first = inputs[0];
  if (first) {
    render("reviewer.md", {
      astPath: AST,
      semanticsPath: SEMANTICS,
      inputPath: first.path,
      kind: first.kind,
      trees: first.trees,
      kindNote: "",
    });
    log(
      "prompt",
      `reviewer prompt renders for ${first.label}; ${inputs.length} reviewers would run, ${cfg.reviewParallel} at a time`,
    );
  }
  log("dry-run", "done");
}

async function main(): Promise<void> {
  openLog(file("driver.log"));
  initStateDir();
  if (args["dry-run"]) await dryRun();
  else await loop();
}

main().catch((e: unknown) => {
  log("crash", e instanceof Error ? (e.stack ?? e.message) : String(e));
  process.exitCode = 1;
});
