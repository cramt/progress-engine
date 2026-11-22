// One insight batch, as subcommands a Claude Code thread calls in order:
// start, gate, finish, triage (resume clears a stop). Everything a script can
// decide (measuring, gating, pushing or parking, triage, metrics, stopping)
// is decided here. README.md beside this directory says how it runs.

import {
  appendFileSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import {
  describe,
  type Guarded,
  gateReport,
  runGates,
  sha256,
} from "./gates.ts";
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
  triage as triageQueue,
  writeJsonl,
} from "./queue.ts";
import { buildInputs, findingsMarkdown } from "./review.ts";
import { parseReport, parseReview } from "./schemas.ts";
import { log, must, openLog, run, setBatch } from "./sh.ts";
import { otherLiveThreads } from "./t3.ts";
import type {
  BatchOutcome,
  Escalation,
  GateFailure,
  GateRun,
  InFlight,
  Measurement,
  Milestone,
  MilestoneStatus,
  NonEmpty,
  Queued,
  Report,
  State,
} from "./types.ts";

const DRIVER = dirname(dirname(fileURLToPath(import.meta.url)));
const REPO = resolve(DRIVER, "../../..");

/** Exit statuses. Anything thrown exits 70 (EX_SOFTWARE). */
const OK = 0;
const GATES_FAILED = 1;
/** The call was wrong (no batch at this step, a malformed file): fix, rerun. */
const USAGE = 2;
/** A stop: nothing more to do in this run. */
const STOP = 3;

const { values: args, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    "state-dir": { type: "string" },
    branch: { type: "string", default: "main" },
    "dry-run": { type: "boolean", default: false },
    "writer-hours": { type: "string", default: "3" },
    "commit-cap": { type: "string", default: "15" },
    "method-every": { type: "string", default: "3" },
    "review-chunk": { type: "string", default: "90" },
    "review-changed-cap": { type: "string", default: "300" },
    "failing-shown": { type: "string", default: "120" },
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
  writerHours: num("writer-hours"),
  commitCap: num("commit-cap"),
  methodEvery: num("method-every"),
  reviewChunk: num("review-chunk"),
  reviewChangedCap: num("review-changed-cap"),
  failingShown: num("failing-shown"),
};

setBranch(args.branch);

const stateDir = args["state-dir"] ? resolve(args["state-dir"]) : "";
const paths: Paths = pathsFor(REPO);
const file = (name: string) => join(stateDir, name);
const batchDir = (b: number) => file(`batches/${b}`);
const HANDOFF = () => file("handoff.md");
const METHOD_NOTES = () => file("method-notes.md");
const DECISIONS = () => file("decisions.md");
const AST = join(paths.crate, "src/ast.rs");
const SEMANTICS = join(paths.crate, "SEMANTICS.md");

const milestones = JSON.parse(
  readFileSync(join(DRIVER, "milestones.json"), "utf8"),
) as Milestone[];

const now = () => new Date().toISOString();

/** The result an agent reads; log lines go to stderr and driver.log. */
const out = (text: string) => console.log(text);

class Exit extends Error {
  readonly code: number;
  constructor(code: number, message: string) {
    super(message);
    this.code = code;
  }
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
      stopped: null,
    };
  }
  const s = JSON.parse(readFileSync(p, "utf8")) as State;
  if (s.inFlight && !("startedAt" in s.inFlight)) {
    throw new Exit(
      STOP,
      `${p} holds batch ${(s.inFlight as { batch: number }).batch} in flight from the loop driver, which this one can't resume. Finish it with that driver, or set inFlight to null.`,
    );
  }
  return { ...s, stopped: s.stopped ?? null };
}

function saveState(s: State): void {
  const p = file("state.json");
  writeFileSync(`${p}.tmp`, `${JSON.stringify(s, null, 2)}\n`);
  renameSync(`${p}.tmp`, p);
}

function initStateDir(): void {
  mkdirSync(stateDir, { recursive: true });
  const seed = (p: string, text: string) => {
    if (!existsSync(p)) writeFileSync(p, text);
  };
  seed(HANDOFF(), "No handoff yet.\n");
  seed(METHOD_NOTES(), "No method notes yet.\n");
  seed(DECISIONS(), "## Decisions from the supervisor\n\nNone yet.\n");
}

function escalate(
  batch: number | null,
  source: string,
  items: Escalation[] | string,
): void {
  const p = file("escalations.md");
  const head = `\n## ${batch === null ? "-" : `b${batch}`} ${now()} (${source})\n\n`;
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
    `${source}: ${typeof items === "string" ? items.split("\n")[0] : `${items.length} item(s)`} -> ${p}`,
  );
}

const RESUME = () =>
  `nix develop --command node crates/vivisurgeons-insight/driver/src/main.ts resume --state-dir ${stateDir}`;

/**
 * A stop only the human can clear: escalated, persisted as `stopped` so
 * triggers skip, and the batch (if any) no longer in flight.
 */
function stopForHuman(
  state: State,
  batch: number | null,
  source: string,
  reason: string,
): never {
  escalate(
    batch,
    source,
    `${reason}\n\nThe driver is stopped until a human runs \`${RESUME()}\` in the worktree.`,
  );
  saveState({ ...state, inFlight: null, stopped: { reason, at: now() } });
  throw new Exit(STOP, `stopped: ${reason}`);
}

// ---------- measuring ----------

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

function milestoneStatus(m: Measurement): MilestoneStatus {
  for (const ms of milestones) {
    // A milestone list added to main mid-batch has no row in that batch's
    // baseline; the batch keeps the milestone it started on.
    if (!m.coverage.some((c) => c.label === ms.coverageLabel)) continue;
    const r = row(m.coverage, ms.coverageLabel);
    const done =
      r.cards.ok === r.cards.total && r.abilities.ok === r.abilities.total;
    // A pinned list can grow (new decks in my-decks), which reopens it: the
    // new cards need parsing, then a re-pin so golden holds them too. `pin`
    // writes exactly the cards coverage counts, so a count that differs is
    // a golden file from an older list.
    const golden = join(paths.crate, "golden", `${ms.name}.txt`);
    const pinnedCards = existsSync(golden)
      ? new Set(
          readFileSync(golden, "utf8")
            .split("\n")
            .filter((l) => l.startsWith("## ")),
        ).size
      : null;
    if (done && pinnedCards === r.cards.total) continue;
    return { kind: done ? "ready-to-pin" : "working", milestone: ms, row: r };
  }
  return { kind: "all-pinned" };
}

function rowLine(s: MilestoneStatus): string {
  if (s.kind === "all-pinned") return "";
  const r = s.row;
  return `Coverage: cards ${r.cards.ok}/${r.cards.total}, abilities ${r.abilities.ok}/${r.abilities.total}.`;
}

/** A baseline measured at another commit, in a worktree, leaving HEAD alone. */
async function measureAt(sha: string, snapshot: string): Promise<Measurement> {
  const wt = file("rebaseline-wt");
  const target = file("rebaseline-target");
  await run("git", ["worktree", "remove", "--force", wt], { cwd: REPO });
  await must("git", ["worktree", "add", "--detach", wt, sha], { cwd: REPO });
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

// ---------- the brief ----------

function render(
  template: string,
  vars: Record<string, string | number>,
): string {
  const text = readFileSync(template, "utf8").replace(
    /\{\{(\w+)\}\}/g,
    (m, k: string) => (k in vars ? String(vars[k]) : m),
  );
  const left = text.match(/\{\{\w+\}\}/g);
  if (left)
    throw new Error(`${template}: unfilled ${[...new Set(left)].join(", ")}`);
  return text;
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

async function brief(
  batch: number,
  startedAt: string,
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
    "brief",
    `${s.milestone.name}: ${count} failing abilities (${shown} shown), ${queue.length} queued findings`,
  );
  return render(join(DRIVER, "brief.md"), {
    batch,
    batchDir: dir,
    startedAt,
    repo: REPO,
    branch: branchName(),
    crate: paths.crate,
    handoffPath: HANDOFF(),
    handoff: readFileSync(HANDOFF(), "utf8"),
    decisionsPath: DECISIONS(),
    decisions: readFileSync(DECISIONS(), "utf8").trim(),
    commitCap: cfg.commitCap,
    hours: cfg.writerHours,
    milestone: s.milestone.name,
    milestoneRow: rowLine(s),
    baselineHead: baseline.head.slice(0, 7),
    baseline: baselineLine(baseline),
    roundtripFailures: baseline.roundtrip.failures,
    abilitiesOk: row(baseline.coverage, "all cards").abilities.ok,
    lintBaseline: lintLine(baseline),
    baselineSnapshot: baseline.snapshotPath,
    methodNotes: readFileSync(METHOD_NOTES(), "utf8").trim(),
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

// ---------- recording a batch ----------

type Finished = Extract<InFlight, { step: "finished" }>;
type Writing = Extract<InFlight, { step: "write" }>;

function gained(baseline: Measurement, after: Measurement, label: string) {
  const b = row(baseline.coverage, label);
  const a = row(after.coverage, label);
  return {
    label,
    abilities: {
      before: b.abilities.ok,
      after: a.abilities.ok,
      gained: a.abilities.ok - b.abilities.ok,
    },
    cards: {
      before: b.cards.ok,
      after: a.cards.ok,
      gained: a.cards.ok - b.cards.ok,
    },
  };
}

function unpushedLine(c: Unpushed<NonEmpty<GateFailure>>): string {
  switch (c.kind) {
    case "conflict":
      return `rebasing onto origin ${c.onto.slice(0, 7)} conflicted: ${c.detail}`;
    case "gates":
      return `rebased onto origin ${c.onto.slice(0, 7)}, it failed the gates: ${c.failure.map(describe).join("; ")}`;
    case "rejected":
      return `the push was rejected ${c.attempts} times: ${c.detail}`;
    case "error":
      return `git failed: ${c.detail}`;
  }
}

function outcomeFields(o: BatchOutcome): Record<string, unknown> {
  switch (o.kind) {
    case "failed":
      return { failedBranch: o.branch, failures: o.failures.map(describe) };
    case "unpushed":
      return { unpushedBranch: o.branch, unpushed: unpushedLine(o.cause) };
    case "abandoned":
      return { abandonedBranch: o.branch };
    case "refused":
      return { refused: o.reason };
    case "pushed":
    case "empty":
      return {};
  }
}

/**
 * The batch's last step: the queue takes the writer's resolutions and the
 * reviewers' findings, metrics.jsonl gets its line, and nothing is in flight.
 */
function record(
  state: State,
  f: Finished,
  findings: Queued[],
  unreviewed: string[],
): State {
  // Resolutions apply to the queue the writer was shown, before this batch's
  // findings join it: a new regression on the same node must survive.
  let held = readJsonl<Queued>(file("queue.jsonl"));
  let closed = 0;
  if (f.outcome.kind === "pushed" && f.report) {
    const r = resolveQueue(held, f.report.resolved);
    held = r.queue;
    closed = r.closed;
  }
  const q = triageQueue(
    held,
    readJsonl<Queued>(file("backlog.jsonl")),
    findings,
  );
  writeJsonl(file("queue.jsonl"), q.queue);
  writeJsonl(file("backlog.jsonl"), q.backlog);
  log(
    "triage",
    `${q.added} new findings held; ${closed} closed by the writer; queue ${q.queue.length}, backlog ${q.backlog.length}`,
  );
  const s = milestoneStatus(f.baseline);
  const after = f.outcome.kind === "pushed" ? f.outcome.after : f.baseline;
  const commits =
    "commits" in f.outcome && typeof f.outcome.commits === "number"
      ? f.outcome.commits
      : 0;
  const metrics = {
    batch: f.batch,
    startedAt: f.startedAt,
    endedAt: now(),
    outcome: f.outcome.kind,
    ...outcomeFields(f.outcome),
    commits,
    fixedUp: f.outcome.kind === "pushed" ? f.outcome.fixedUp : null,
    head: { before: f.baseline.head, after: after.head },
    overall: gained(f.baseline, after, "all cards"),
    milestone:
      s.kind === "all-pinned"
        ? null
        : {
            name: s.milestone.name,
            ...gained(f.baseline, after, s.milestone.coverageLabel),
          },
    roundtripFailures: {
      before: f.baseline.roundtrip.failures,
      after: after.roundtrip.failures,
    },
    lint: { before: f.baseline.lint, after: after.lint },
    findings: {
      total: findings.length,
      game: findings.filter((x) => x.severity === "game").length,
      minor: findings.filter((x) => x.severity === "minor").length,
      regressions: findings.filter((x) => x.regression).length,
      unreviewed,
    },
    queue: { length: q.queue.length, closed },
    // A T3 Code thread can't read its own spend; see README.md.
    costUsd: null,
  };
  appendFileSync(file("metrics.jsonl"), `${JSON.stringify(metrics)}\n`);
  log(
    "metrics",
    `${f.outcome.kind}: abilities +${metrics.overall.abilities.gained}, ${metrics.milestone?.name ?? "-"} +${metrics.milestone?.abilities.gained ?? 0}, findings ${metrics.findings.total} (${metrics.findings.game} game, ${metrics.findings.regressions} regressions)`,
  );
  return {
    ...state,
    nextBatch: f.batch + 1,
    consecutiveFailures:
      f.outcome.kind === "pushed" ? 0 : state.consecutiveFailures + 1,
    inFlight: null,
  };
}

/** A batch whose thread died: parked if it wrote anything, then recorded. */
async function recover(state: State, f: InFlight): Promise<State> {
  log("recover", `batch ${f.batch} was left at ${f.step} since ${f.startedAt}`);
  switch (f.step) {
    case "start":
      return { ...state, inFlight: null };
    case "finished":
      return record(state, f, [], []);
    case "write": {
      const commits = (await commitsSince(REPO, f.baseline.head)).length;
      let branch: string | null = null;
      if (commits > 0) {
        const parked = await parkAndReset(
          REPO,
          f.baseline.head,
          `insight/abandoned-${f.batch}`,
        );
        if (parked.kind === "refused")
          stopForHuman(
            state,
            f.batch,
            "git",
            `Batch ${f.batch}'s thread died with ${commits} commits, and parking them was refused: ${parked.reason}. Nothing was reset.`,
          );
        branch = parked.branch;
      }
      escalate(
        f.batch,
        "recover",
        `Batch ${f.batch}'s thread died at the write step${branch ? `; its ${commits} commits are on ${branch}` : ""}.`,
      );
      return record(
        state,
        {
          step: "finished",
          batch: f.batch,
          startedAt: f.startedAt,
          baseline: f.baseline,
          outcome: { kind: "abandoned", branch, commits },
          report: null,
        },
        [],
        [],
      );
    }
  }
}

// ---------- subcommands ----------

/** The driver's and the skill's trees, to tell when taking in main changed them. */
async function toolTrees(): Promise<string> {
  const r = await run(
    "git",
    [
      "rev-parse",
      "HEAD:crates/vivisurgeons-insight/driver",
      "HEAD:.claude/skills/insight-batch",
    ],
    { cwd: REPO },
  );
  return r.stdout.trim();
}

async function start(): Promise<number> {
  let state = loadState();
  if (args["dry-run"]) return dryRun(state);
  if (state.stopped) {
    throw new Exit(
      STOP,
      `stopped since ${state.stopped.at}: ${state.stopped.reason}\nA human clears it with \`${RESUME()}\`.`,
    );
  }
  if (state.inFlight) {
    const live = await otherLiveThreads(REPO);
    if (live.length > 0) {
      throw new Exit(
        STOP,
        `batch ${state.inFlight.batch} is in flight (${state.inFlight.step}, since ${state.inFlight.startedAt}); thread ${live.join(", ")} still has it`,
      );
    }
    state = await recover(state, state.inFlight);
    saveState(state);
  }
  if (state.consecutiveFailures >= 2) {
    stopForHuman(
      state,
      null,
      "stop",
      `${state.consecutiveFailures} batches in a row failed, were empty or were left unpushed; look at their gates and branches first`,
    );
  }
  const batch = state.nextBatch;
  setBatch(batch);
  const startedAt = now();
  // Before anything slow, so a trigger meanwhile skips.
  state = { ...state, inFlight: { step: "start", batch, startedAt } };
  saveState(state);

  const before = await toolTrees();
  const pf = await preflight(REPO);
  if (pf.kind === "refused")
    stopForHuman(state, batch, "git", `preflight refused: ${pf.reason}`);
  const toolsMoved = (await toolTrees()) !== before;
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
  if (s.kind === "all-pinned")
    stopForHuman(state, batch, "milestone", "every milestone is pinned");
  if (s.kind === "ready-to-pin")
    stopForHuman(
      state,
      batch,
      "milestone",
      `${s.milestone.name} is at 100% (${rowLine(s)}). Review it, then pin with \`insight pin ${s.milestone.target}\`; batches move on once golden/${s.milestone.name}.txt holds every card of it.`,
    );
  log("milestone", `${s.milestone.name}: ${rowLine(s)}`);
  const text = await brief(batch, startedAt, baseline, s, dir);
  writeFileSync(join(dir, "brief.md"), text);
  saveState({
    ...state,
    inFlight: {
      step: "write",
      batch,
      startedAt,
      baseline,
      decisionsSha: sha256(DECISIONS()),
      failedGates: 0,
      lastGate: null,
    },
  });
  if (toolsMoved)
    out(
      "NOTE: taking in main changed the driver or this skill. Read .claude/skills/insight-batch/SKILL.md again before going on.\n",
    );
  out(`Batch ${batch} started. The brief is ${join(dir, "brief.md")}:\n`);
  out(text);
  return OK;
}

function writing(state: State, cmd: string): Writing {
  const f = state.inFlight;
  if (f?.step !== "write")
    throw new Exit(
      USAGE,
      `${cmd} needs a batch at the write step; ${f ? `batch ${f.batch} is at ${f.step}` : "none is in flight (run start)"}`,
    );
  setBatch(f.batch);
  return f;
}

const guarded = (f: Writing): Guarded => ({
  decisionsPath: DECISIONS(),
  decisionsSha: f.decisionsSha,
  milestones,
});

/** The gates at HEAD, recorded in the batch as its last gate run. */
async function gateNow(f: Writing): Promise<{ f: Writing; run: GateRun }> {
  const dir = batchDir(f.batch);
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
  const head = await must("git", ["rev-parse", "HEAD"], { cwd: REPO });
  const tag = head.slice(0, 7);
  const report = await runGates(
    paths,
    baseline,
    join(dir, `after-${tag}.snap`),
    guarded(f),
  );
  const r: GateRun =
    report.kind === "pass"
      ? { head, kind: "pass", baseline, after: report.after }
      : { head, kind: "fail", failures: report.failures };
  const text =
    report.kind === "pass"
      ? `PASS at ${tag}: snapshot diff lost ${report.diff.lost.length}, new ${report.diff.added.length}, changed ${report.diff.changed.length}; ${baselineLine(report.after)}`
      : `FAIL at ${tag}:\n\n${gateReport(report.failures)}`;
  writeFileSync(join(dir, `gates-${tag}.md`), `${text}\n`);
  const next: Writing = {
    ...f,
    lastGate: r,
    failedGates: f.failedGates + (r.kind === "fail" ? 1 : 0),
  };
  saveState({ ...loadState(), inFlight: next });
  log(
    "gate",
    r.kind === "pass"
      ? `pass at ${tag}`
      : `failed: ${r.failures.map(describe).join("; ").slice(0, 400)}`,
  );
  return { f: next, run: r };
}

async function gate(): Promise<number> {
  const f = writing(loadState(), "gate");
  const commits = (await commitsSince(REPO, f.baseline.head)).length;
  const hours = (Date.now() - Date.parse(f.startedAt)) / 3_600_000;
  const g = await gateNow(f);
  out(
    `Batch ${f.batch}: ${commits}/${cfg.commitCap} commits, ${hours.toFixed(1)}/${cfg.writerHours} hours, gates failed ${g.f.failedGates} time(s) so far.\n`,
  );
  out(
    readFileSync(
      join(batchDir(f.batch), `gates-${g.run.head.slice(0, 7)}.md`),
      "utf8",
    ),
  );
  return g.run.kind === "pass" ? OK : GATES_FAILED;
}

const PUSH_REBASES = 3;

/**
 * The gates again, for commits rebased onto an origin someone else moved: the
 * ratchets compare against a baseline measured there, so a batch may not
 * regress what is there now.
 */
async function regate(
  f: Writing,
  onto: string,
  attempt: number,
): Promise<
  | { ok: true; value: { baseline: Measurement; after: Measurement } }
  | { ok: false; failure: NonEmpty<GateFailure> }
> {
  const dir = batchDir(f.batch);
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
    guarded(f),
  );
  if (report.kind === "pass")
    return { ok: true, value: { baseline, after: report.after } };
  writeFileSync(
    join(dir, `gates-rebase-${attempt}.md`),
    gateReport(report.failures),
  );
  return { ok: false, failure: report.failures };
}

function readReport(dir: string): Report {
  const p = join(dir, "report.json");
  const shape =
    '{"summary": "...", "resolved": [{"card", "node", "verdict": "fixed" | "not-a-defect"}], "escalations": [{"kind": "exclusion" | "decision", "cards": [...], "question"}]}';
  if (!existsSync(p)) throw new Exit(USAGE, `write ${p} first: ${shape}`);
  let raw: unknown;
  try {
    raw = JSON.parse(readFileSync(p, "utf8"));
  } catch (e) {
    throw new Exit(USAGE, `${p} is not JSON (${String(e)}); want ${shape}`);
  }
  const r = parseReport(raw);
  if (!r.ok) throw new Exit(USAGE, `${p}: ${r.error}; want ${shape}`);
  return r.value;
}

async function finish(): Promise<number> {
  let state = loadState();
  let f = writing(state, "finish");
  const dir = batchDir(f.batch);
  const report = readReport(dir);
  if (report.escalations.length)
    escalate(f.batch, "writer", report.escalations);
  const commits = (await commitsSince(REPO, f.baseline.head)).length;
  const head = await must("git", ["rev-parse", "HEAD"], { cwd: REPO });
  const clean = !(await dirty(REPO));
  let outcome: BatchOutcome;
  let baseline = f.baseline;
  if (commits === 0 && clean) {
    log("finish", "no commits");
    outcome = { kind: "empty" };
  } else {
    let g = f.lastGate;
    if (!g || g.head !== head || !clean) {
      log(
        "finish",
        "HEAD moved since the last gate run, or never gated; gating now",
      );
      const r = await gateNow(f);
      f = r.f;
      g = r.run;
    }
    state = loadState();
    if (g.kind === "pass") {
      baseline = g.baseline;
      const p = await pushRebasing(REPO, {
        rebases: PUSH_REBASES,
        parkAs: `insight/unpushed-${f.batch}`,
        regate: (onto, attempt) => regate(f, onto, attempt),
      });
      switch (p.kind) {
        case "pushed": {
          // After a rebase the review and the metrics compare against what
          // origin held, so the human's commits aren't counted as the batch's.
          const now = p.regated ?? { baseline, after: g.after };
          baseline = now.baseline;
          const diff = snapDiff(
            readFileSync(now.baseline.snapshotPath, "utf8"),
            readFileSync(now.after.snapshotPath, "utf8"),
          );
          const review = buildInputs(
            diff,
            join(dir, "review"),
            cfg.reviewChunk,
            cfg.reviewChangedCap,
          );
          log(
            "push",
            `${commits} commits pushed${p.rebases ? ` after ${p.rebases} rebase(s)` : ""}, at ${p.head.slice(0, 7)}; ${review.length} review inputs`,
          );
          outcome = {
            kind: "pushed",
            head: p.head,
            commits,
            fixedUp: f.failedGates > 0,
            after: now.after,
            review,
          };
          break;
        }
        case "unpushed":
          escalate(
            f.batch,
            "push",
            `The batch passed its gates but could not be pushed: ${unpushedLine(p.cause)}. Its ${commits} commits are on ${p.branch}; ${branchName()} was reset to origin ${p.resetTo.slice(0, 7)}. Rebase and push them by hand, or drop the branch.`,
          );
          outcome = {
            kind: "unpushed",
            branch: p.branch,
            commits,
            cause: p.cause,
          };
          break;
        case "refused": {
          const reason = `batch ${f.batch} passed its gates but could not be pushed (${unpushedLine(p.cause)}), and parking it was refused: ${p.reason}. Nothing was reset.`;
          state = record(
            state,
            {
              ...finished(f, baseline, report),
              outcome: { kind: "refused", reason },
            },
            [],
            [],
          );
          stopForHuman(state, f.batch, "git", reason);
        }
      }
    } else {
      const parked = await parkAndReset(
        REPO,
        f.baseline.head,
        `insight/failed-${f.batch}`,
      );
      if (parked.kind === "refused") {
        const reason = `batch ${f.batch} failed its gates, and resetting ${branchName()} was refused: ${parked.reason}. Nothing was reset.`;
        state = record(
          state,
          {
            ...finished(f, baseline, report),
            outcome: { kind: "refused", reason },
          },
          [],
          [],
        );
        stopForHuman(state, f.batch, "git", reason);
      }
      outcome = {
        kind: "failed",
        branch: parked.branch,
        commits,
        failures: g.failures,
      };
    }
  }
  saveState({
    ...state,
    inFlight: { ...finished(f, baseline, report), outcome },
  });
  out(`Batch ${f.batch}: ${outcomeLine(outcome)}.`);
  if (outcome.kind === "pushed") {
    out(
      outcome.review.length
        ? `\nReview inputs (${outcome.review.length}); each reviewer reads ${AST}, ${SEMANTICS} and its input, and its findings go to ${join(dir, "review")}/findings-<label>.json:\n${outcome.review
            .map((i) => `- ${i.label}: ${i.kind}, ${i.trees} trees, ${i.path}`)
            .join("\n")}`
        : "\nNo trees moved, so there is nothing to review.",
    );
  }
  if (f.batch % cfg.methodEvery === 0) {
    copyFileSync(METHOD_NOTES(), join(dir, "method-notes.before.md"));
    out(
      `\nMethod review is due (every ${cfg.methodEvery} batches): metrics ${file("metrics.jsonl")}, findings in ${recentReviewDirs(f.batch).join(", ") || "no batch yet"}, notes ${METHOD_NOTES()}, rationale to ${join(dir, "method-rationale.md")}.`,
    );
  }
  out(`\nThen update ${HANDOFF()} and run triage.`);
  return OK;
}

function finished(f: Writing, baseline: Measurement, report: Report): Finished {
  return {
    step: "finished",
    batch: f.batch,
    startedAt: f.startedAt,
    baseline,
    outcome: { kind: "empty" },
    report,
  };
}

function recentReviewDirs(batch: number): string[] {
  return [...Array(Math.min(batch, cfg.methodEvery)).keys()]
    .map((i) => join(batchDir(batch - i), "review"))
    .filter((d) => existsSync(d));
}

function outcomeLine(o: BatchOutcome): string {
  switch (o.kind) {
    case "pushed":
      return `pushed ${o.commits} commits, at ${o.head.slice(0, 7)}${o.fixedUp ? " (after a fix-up)" : ""}`;
    case "failed":
      return `failed the gates; ${o.commits} commits parked on ${o.branch} and ${branchName()} reset: ${o.failures.map(describe).join("; ")}`;
    case "unpushed":
      return `passed but could not be pushed; ${o.commits} commits on ${o.branch}: ${unpushedLine(o.cause)}`;
    case "empty":
      return "no commits";
    case "abandoned":
      return `abandoned${o.branch ? `, ${o.commits} commits on ${o.branch}` : ""}`;
    case "refused":
      return `refused: ${o.reason}`;
  }
}

async function triage(files: string[]): Promise<number> {
  const state = loadState();
  const f = state.inFlight;
  if (f?.step !== "finished")
    throw new Exit(
      USAGE,
      `triage needs a finished batch; ${f ? `batch ${f.batch} is at ${f.step} (run finish)` : "none is in flight"}`,
    );
  setBatch(f.batch);
  const inputs = f.outcome.kind === "pushed" ? f.outcome.review : [];
  const findings: Queued[] = [];
  const seen = new Set<string>();
  for (const p of files) {
    const label = /^findings-(.+)\.json$/.exec(basename(p))?.[1];
    const input = inputs.find((i) => i.label === label);
    if (!label || !input)
      throw new Exit(
        USAGE,
        `${p}: want findings-<label>.json for one of ${inputs.map((i) => i.label).join(", ") || "no inputs"}`,
      );
    let raw: unknown;
    try {
      raw = JSON.parse(readFileSync(p, "utf8"));
    } catch (e) {
      throw new Exit(USAGE, `${p} is not JSON: ${String(e)}`);
    }
    const r = parseReview(raw);
    if (!r.ok) throw new Exit(USAGE, `${p}: ${r.error}`);
    seen.add(label);
    writeFileSync(
      join(dirname(input.path), `findings-${label}.md`),
      findingsMarkdown(f.batch, input, r.value.findings),
    );
    findings.push(
      ...r.value.findings.map((x) => ({ ...x, batch: f.batch, source: label })),
    );
  }
  const unreviewed = inputs.map((i) => i.label).filter((l) => !seen.has(l));
  if (unreviewed.length)
    log("triage", `no findings for ${unreviewed.join(", ")}`);
  const next = record(state, f, findings, unreviewed);
  saveState(next);
  const queue = readJsonl<Queued>(file("queue.jsonl")).length;
  const backlog = readJsonl<Queued>(file("backlog.jsonl")).length;
  out(
    `Batch ${f.batch} recorded (${f.outcome.kind}): ${findings.length} findings (${findings.filter((x) => x.severity === "game").length} game, ${findings.filter((x) => x.regression).length} regressions)${unreviewed.length ? `, unreviewed: ${unreviewed.join(", ")}` : ""}; queue ${queue}, backlog ${backlog}; ${next.consecutiveFailures} failed in a row. Nothing is in flight.`,
  );
  return OK;
}

function resume(): number {
  const state = loadState();
  saveState({ ...state, stopped: null, consecutiveFailures: 0 });
  log(
    "resume",
    `cleared ${state.stopped ? `the stop "${state.stopped.reason.split("\n")[0]}"` : "no stop"}; consecutive failures ${state.consecutiveFailures} -> 0`,
  );
  out("Resumed: the next trigger starts a batch.");
  return OK;
}

// ---------- dry run ----------

/** Baseline and brief into <state>/dry-run; no git changes, state.json untouched. */
async function dryRun(state: State): Promise<number> {
  const dir = file("dry-run");
  mkdirSync(dir, { recursive: true });
  const b = await gitBranch(REPO);
  const d = await dirty(REPO);
  log(
    "dry-run",
    `on ${b} (a real start wants ${branchName()}); tree ${d ? `dirty (${d.split("\n").length} paths; a real start refuses)` : "clean"}; next batch ${state.nextBatch}, in flight ${state.inFlight ? `b${state.inFlight.batch} ${state.inFlight.step}` : "none"}, stopped ${state.stopped ? state.stopped.reason : "no"}`,
  );
  await buildRelease(REPO);
  const baseline = await measure(paths, join(dir, "baseline.snap"));
  writeFileSync(join(dir, "baseline.json"), JSON.stringify(baseline, null, 2));
  for (const ms of milestones) {
    const r = row(baseline.coverage, ms.coverageLabel);
    log(
      "milestone",
      `${ms.name}: abilities ${r.abilities.ok}/${r.abilities.total}, cards ${r.cards.ok}/${r.cards.total}`,
    );
  }
  const s = milestoneStatus(baseline);
  if (s.kind !== "working") {
    out(
      `A real start would stop: ${s.kind}${s.kind === "ready-to-pin" ? ` (${s.milestone.name})` : ""}.`,
    );
    return OK;
  }
  const text = await brief(state.nextBatch, now(), baseline, s, dir);
  writeFileSync(join(dir, "brief.md"), text);
  out(
    `Dry run: nothing pushed, state.json untouched. Brief at ${join(dir, "brief.md")}:\n`,
  );
  out(text);
  return OK;
}

// ---------- entry ----------

const USAGE_LINE =
  "usage: main.ts start [--dry-run] | gate | finish | triage <findings-*.json ...> | resume, with --state-dir <dir>";

async function main(): Promise<number> {
  const [cmd, ...rest] = positionals;
  if (!stateDir) throw new Exit(USAGE, USAGE_LINE);
  initStateDir();
  openLog(file("driver.log"));
  if (cmd !== "triage" && rest.length) throw new Exit(USAGE, USAGE_LINE);
  switch (cmd) {
    case "start":
      return start();
    case "gate":
      return gate();
    case "finish":
      return finish();
    case "triage":
      return triage(rest);
    case "resume":
      return resume();
    default:
      throw new Exit(USAGE, USAGE_LINE);
  }
}

main().then(
  (code) => {
    process.exitCode = code;
  },
  (e: unknown) => {
    if (e instanceof Exit) {
      out(e.message);
      if (stateDir) log("exit", `${e.code}: ${e.message}`);
      process.exitCode = e.code;
      return;
    }
    const msg = e instanceof Error ? (e.stack ?? e.message) : String(e);
    if (stateDir) log("crash", msg);
    else console.error(msg);
    process.exitCode = 70;
  },
);
