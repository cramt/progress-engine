// The batch's domain. Each union is closed: a state a batch cannot be in has
// no spelling here.

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

/** What the writer leaves in batches/<n>/report.json before `finish`. */
export type Report = {
  summary: string;
  resolved: Resolution[];
  escalations: Escalation[];
};

export type ReviewOutput = { findings: Finding[] };

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

/**
 * A card whose unparsed lines read differently after the batch: what the
 * lexer made of them before (`was`) and now, beside the card's Oracle text.
 */
export type Relexed = { card: string; was: string[]; now: string[] };

export type GateReport =
  | { kind: "pass"; after: Measurement; diff: SnapDiff }
  | { kind: "fail"; failures: NonEmpty<GateFailure> };

export type ReviewInput = {
  label: string;
  kind: "new" | "changed" | "relexed";
  path: string;
  trees: number;
};

export type BatchOutcome =
  | {
      kind: "pushed";
      head: string;
      commits: number;
      fixedUp: boolean;
      after: Measurement;
      review: ReviewInput[];
    }
  | {
      kind: "failed";
      branch: string;
      commits: number;
      failures: NonEmpty<GateFailure>;
    }
  /**
   * Passed against its own baseline, but the branch moved under it and the
   * rebase conflicted or failed the gates again; parked, reset to origin.
   */
  | {
      kind: "unpushed";
      branch: string;
      commits: number;
      cause: Unpushed<NonEmpty<GateFailure>>;
    }
  /** The writer finished without a commit; nothing to gate or review. */
  | { kind: "empty" }
  /** Its thread died: found in flight with no live thread in T3, commits parked. */
  | { kind: "abandoned"; branch: string | null; commits: number }
  /** Git refused to park or reset it; the batch stopped the driver. */
  | { kind: "refused"; reason: string };

/** The gates' last run, so `finish` needn't run them again at the same HEAD. */
export type GateRun =
  | { head: string; kind: "pass"; baseline: Measurement; after: Measurement }
  | { head: string; kind: "fail"; failures: NonEmpty<GateFailure> };

/**
 * The batch between `start` and `triage`. A trigger skips while it is set; a
 * start that finds one with no other live thread in the project (T3 Code's
 * `thread list`) takes its thread for dead, whatever its age.
 */
export type InFlight =
  /** Preflight and baseline still running. */
  | { step: "start"; batch: number; startedAt: string }
  | {
      step: "write";
      batch: number;
      startedAt: string;
      baseline: Measurement;
      /** decisions.md as the batch started; the gates hold it to that. */
      decisionsSha: string;
      failedGates: number;
      lastGate: GateRun | null;
    }
  /** Pushed, parked or empty: what is left is review, handoff and triage. */
  | {
      step: "finished";
      batch: number;
      startedAt: string;
      baseline: Measurement;
      outcome: BatchOutcome;
      report: Report | null;
    };

export type State = {
  version: 1;
  nextBatch: number;
  consecutiveFailures: number;
  /** No longer counted (agents run in T3 Code); kept for the metrics. */
  lifetimeCostUsd: number;
  inFlight: InFlight | null;
  /**
   * A stop only a human can clear. Triggers skip while it is set, `start`
   * exits 3, and `resume` clears it.
   */
  stopped: { reason: string; at: string } | null;
};
