// The driver's domain. Each union is closed: a state the loop cannot be in
// has no spelling here.

import type { Unpushed } from "./git.ts";

export type Ratio = { ok: number; total: number };

/** One row of `insight coverage`: "all cards", "EDHREC top N" or a milestone. */
export type CoverageRow = { label: string; cards: Ratio; abilities: Ratio };

export type Roundtrip = { ok: number; parsed: number; failures: number };

/** `insight lint --counts`, kind name to violation count. */
export type LintCounts = Record<string, number>;

/** What a bulk-file sync changes; a change mid-batch voids the baseline. */
export type BulkFingerprint = { cards: string; tags: string };

/** Everything the gates and the metrics compare, at one commit. */
export type Measurement = {
  head: string;
  takenAt: string;
  coverage: CoverageRow[];
  roundtrip: Roundtrip;
  lint: LintCounts;
  snapshotPath: string;
  bulk: BulkFingerprint;
};

export type Milestone = {
  name: string;
  /** The row label `insight coverage` prints for it. */
  coverageLabel: string;
  /** What `insight failing` and `insight pin` take: a milestone name or N. */
  target: string;
};

export type MilestoneStatus =
  | { kind: "working"; milestone: Milestone; row: CoverageRow }
  | { kind: "ready-to-pin"; milestone: Milestone; row: CoverageRow }
  | { kind: "all-pinned" };

export type Severity = "game" | "minor";

export type Finding = {
  card: string;
  severity: Severity;
  regression: boolean;
  node: string;
  should_be: string;
  semantics_row?: string;
};

/** A finding as the queue holds it: where it came from. */
export type Queued = Finding & { batch: number; source: string };

export type Resolution = {
  card: string;
  node: string;
  verdict: "fixed" | "not-a-defect";
};

export type Escalation = {
  kind: "exclusion" | "decision";
  cards: string[];
  question: string;
};

export type WriterOutput = {
  summary: string;
  resolved: Resolution[];
  escalations: Escalation[];
  handoff_updated: boolean;
};

export type MethodOutput = { method_notes: string; rationale: string };

export type ReviewOutput = { findings: Finding[] };

export type AgentRole = "writer" | "fixup" | "reviewer" | "method";

/** The SDK result message, reduced to what metrics keep. */
export type ResultSummary = {
  subtype: string;
  isError: boolean;
  costUsd: number;
  durationMs: number;
  numTurns: number;
  usage: Record<string, unknown>;
  modelUsage: Record<string, unknown>;
};

export type AgentRun<T> =
  | { kind: "completed"; output: T; result: ResultSummary }
  /** A result arrived but carried no valid structured output. */
  | { kind: "no-output"; reason: string; result: ResultSummary }
  /** Stopped by the hard wall clock: the CLI never sent a result, so no cost. */
  | { kind: "aborted"; reason: string }
  | { kind: "crashed"; error: string };

/** What metrics.jsonl keeps per agent call. */
export type AgentRecord = {
  role: AgentRole;
  label: string;
  run: AgentRun<unknown>["kind"];
  result: ResultSummary | null;
};

export type CommandFailure = {
  gate: "command";
  name: string;
  exitCode: number;
  output: string;
};

export type GateFailure =
  | CommandFailure
  | { gate: "git"; reason: string }
  | { gate: "commits"; reason: string }
  | { gate: "roundtrip"; before: number; after: number }
  | { gate: "coverage"; before: number; after: number }
  | { gate: "lint"; kind: string; before: number; after: number }
  | { gate: "snapshot-lost"; lost: string[] };

export type NonEmpty<T> = [T, ...T[]];

export type SnapDiff = {
  lost: string[];
  added: { key: string; card: string; text: string; tree: string }[];
  changed: {
    key: string;
    card: string;
    text: string;
    old: string[];
    new: string[];
  }[];
};

export type GateReport =
  | { kind: "pass"; after: Measurement; diff: SnapDiff }
  | { kind: "fail"; failures: NonEmpty<GateFailure> };

export type BatchOutcome =
  | {
      kind: "pushed";
      head: string;
      commits: number;
      fixedUp: boolean;
      after: Measurement;
    }
  | {
      kind: "failed";
      branch: string;
      commits: number;
      failures: NonEmpty<GateFailure>;
    }
  /**
   * Passed against its own baseline, but main moved under it and the rebase
   * conflicted or failed the gates again; parked, main reset to origin/main.
   */
  | {
      kind: "unpushed";
      branch: string;
      commits: number;
      cause: Unpushed<NonEmpty<GateFailure>>;
    }
  /** The writer finished without a commit; nothing to gate or review. */
  | { kind: "empty" };

export type StopReason =
  | { kind: "max-batches"; batches: number }
  | { kind: "max-cost"; spentUsd: number }
  | { kind: "consecutive-failures"; count: number }
  | { kind: "milestone-ready"; milestone: string }
  | { kind: "all-pinned" }
  | { kind: "unsafe-git"; reason: string }
  | { kind: "driver-updated"; from: string; to: string };

/** A batch interrupted mid-way, so a rerun picks it up where it stopped. */
export type InFlight =
  | {
      step: "write";
      batch: number;
      baseline: Measurement;
      agents: AgentRecord[];
    }
  | {
      step: "gate";
      batch: number;
      baseline: Measurement;
      agents: AgentRecord[];
      writer: WriterOutput | null;
      fixupTried: boolean;
    }
  | {
      step: "review";
      batch: number;
      baseline: Measurement;
      agents: AgentRecord[];
      writer: WriterOutput | null;
      outcome: Extract<BatchOutcome, { kind: "pushed" }>;
    }
  | {
      step: "record";
      batch: number;
      baseline: Measurement;
      agents: AgentRecord[];
      writer: WriterOutput | null;
      outcome: BatchOutcome;
      findings: Queued[];
    };

export type State = {
  version: 1;
  nextBatch: number;
  consecutiveFailures: number;
  lifetimeCostUsd: number;
  inFlight: InFlight | null;
};
