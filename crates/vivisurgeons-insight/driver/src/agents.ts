import { execFileSync } from "node:child_process";
import { appendFileSync, mkdirSync } from "node:fs";
import { basename, dirname, resolve } from "node:path";
import {
  type HookCallback,
  type HookJSONOutput,
  type Options,
  query,
  type SDKResultMessage,
} from "@anthropic-ai/claude-agent-sdk";
import type { Parsed } from "./schemas.ts";
import { log, secs } from "./sh.ts";
import type {
  AgentRecord,
  AgentRole,
  AgentRun,
  ResultSummary,
} from "./types.ts";

/** Settings every agent shares, from the command line. */
export type AgentEnv = {
  model: string | null;
  reviewModel: string | null;
  claudePath: string | null;
  /** What is left of --max-cost-usd; each call is capped at it. */
  budgetUsd: () => number;
};

// Sub-agents are banned: a nested batch measured ~3% of trees wrong in
// game-changing ways and can't be stopped through its parent (supervisor.md).
const SPAWNERS = ["Agent", "Task", "Workflow"];

const allow: HookJSONOutput = {};
const deny = (reason: string): HookJSONOutput => ({
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "deny",
    permissionDecisionReason: reason,
  },
});

function input(i: unknown): Record<string, unknown> {
  return typeof i === "object" && i !== null
    ? (i as Record<string, unknown>)
    : {};
}

// The subcommand position only, so a commit message that says "push" passes.
export const PUSH = /\bgit(\s+(-C|-c)\s+\S+|\s+--\S+)*\s+push\b/;
export const PIN = /\binsight\s+pin\b|\bcargo\s+run\b[^\n;&|]*--\s+pin\b/;

/**
 * Whether a tool call would change `path`. Read may; any edit or write to it
 * may not, nor any shell command that names it, since a shell can change a
 * file in more ways than a pattern can list.
 */
export function touches(
  tool: string,
  toolInput: unknown,
  path: string,
): boolean {
  const ti = input(toolInput);
  if (tool === "Bash") {
    const c = typeof ti.command === "string" ? ti.command : "";
    return c.includes(path) || c.includes(basename(path));
  }
  if (tool === "Read") return false;
  const p = ti.file_path ?? ti.notebook_path;
  return typeof p === "string" && resolve(p) === resolve(path);
}

/**
 * The writer's rules that a prompt can only ask for, enforced: never push,
 * never pin, never touch the supervisor's decisions, and once the clock or
 * the commit cap runs out, nothing but reading and updating the handoff.
 */
export function writerGuard(g: {
  repo: string;
  baseline: string;
  commitLimit: number;
  softDeadline: number;
  handoffPath: string;
  decisionsPath: string;
}): HookCallback {
  const commits = (): number =>
    Number(
      execFileSync("git", ["rev-list", "--count", `${g.baseline}..HEAD`], {
        cwd: g.repo,
        encoding: "utf8",
      }).trim(),
    );
  return async (hook) => {
    if (hook.hook_event_name !== "PreToolUse") return allow;
    const ti = input(hook.tool_input);
    if (touches(hook.tool_name, hook.tool_input, g.decisionsPath)) {
      return deny(
        `${g.decisionsPath} is the supervisor's; read it, never change it. Report a decision you carried out in your final summary, and a question about one in escalations.`,
      );
    }
    if (hook.tool_name === "Bash" && typeof ti.command === "string") {
      if (PUSH.test(ti.command)) {
        return deny(
          "The driver pushes after its gates pass. Commit locally only.",
        );
      }
      if (PIN.test(ti.command)) {
        return deny(
          "Pinning is the human's step. Report the milestone instead.",
        );
      }
    }
    const late = Date.now() > g.softDeadline;
    const n = commits();
    if (!late && n < g.commitLimit) return allow;
    const file = typeof ti.file_path === "string" ? resolve(ti.file_path) : "";
    if (hook.tool_name === "Read") return allow;
    if (
      (hook.tool_name === "Edit" || hook.tool_name === "Write") &&
      file === g.handoffPath
    ) {
      return allow;
    }
    const why = late ? "the batch's time is up" : `the batch has ${n} commits`;
    return deny(
      `Stop: ${why}. Update ${g.handoffPath}, then end with your final structured report.`,
    );
  };
}

/** The blind rule: a reviewer reads ast.rs, SEMANTICS.md and its input, nothing else. */
export function reviewerGuard(readable: string[]): HookCallback {
  const ok = new Set(readable.map((p) => resolve(p)));
  return async (hook) => {
    if (hook.hook_event_name !== "PreToolUse") return allow;
    const ti = input(hook.tool_input);
    const path =
      hook.tool_name === "Read"
        ? ti.file_path
        : hook.tool_name === "Grep"
          ? ti.path
          : undefined;
    if (hook.tool_name !== "Read" && hook.tool_name !== "Grep") return allow;
    if (typeof path === "string" && ok.has(resolve(path))) return allow;
    return deny(`A blind reviewer may read only: ${[...ok].join(", ")}`);
  };
}

export type AgentSpec<T> = {
  role: AgentRole;
  label: string;
  prompt: string;
  cwd: string;
  tools: string[];
  /** Project settings (CLAUDE.md) for writers; none for blind readers. */
  loadProject: boolean;
  writes: boolean;
  guard: HookCallback;
  schema: Record<string, unknown>;
  parse: (v: unknown) => Parsed<T>;
  extraDirs: string[];
  /** Hard wall clock: past it the call is aborted and reports no cost. */
  hardMs: number;
  transcript: string;
};

function summarize(r: SDKResultMessage): ResultSummary {
  return {
    subtype: r.subtype,
    isError: r.is_error,
    costUsd: r.total_cost_usd,
    durationMs: r.duration_ms,
    numTurns: r.num_turns,
    usage: { ...r.usage },
    modelUsage: { ...r.modelUsage },
  };
}

export async function runAgent<T>(
  env: AgentEnv,
  spec: AgentSpec<T>,
): Promise<{ run: AgentRun<T>; record: AgentRecord }> {
  mkdirSync(dirname(spec.transcript), { recursive: true });
  const abort = new AbortController();
  const timer = setTimeout(() => abort.abort(), spec.hardMs);
  const model =
    spec.role === "reviewer" ? (env.reviewModel ?? env.model) : env.model;
  const options: Options = {
    cwd: spec.cwd,
    abortController: abort,
    tools: spec.tools,
    allowedTools: spec.tools,
    disallowedTools: SPAWNERS,
    // Writers run unattended for hours: a permission prompt nobody answers
    // would stall the batch. The guard hook is what bounds them.
    permissionMode: spec.writes ? "bypassPermissions" : "dontAsk",
    allowDangerouslySkipPermissions: spec.writes,
    hooks: { PreToolUse: [{ hooks: [spec.guard] }] },
    settingSources: spec.loadProject ? ["project"] : [],
    settings: { attribution: false },
    additionalDirectories: spec.extraDirs,
    outputFormat: { type: "json_schema", schema: spec.schema },
    maxBudgetUsd: Math.max(env.budgetUsd(), 0.01),
    stderr: (d) =>
      appendFileSync(spec.transcript, `${JSON.stringify({ stderr: d })}\n`),
    ...(model ? { model } : {}),
    ...(env.claudePath ? { pathToClaudeCodeExecutable: env.claudePath } : {}),
  };
  log(spec.role, `${spec.label}: started (hard stop in ${secs(spec.hardMs)})`);
  let result: SDKResultMessage | null = null;
  let run: AgentRun<T>;
  try {
    for await (const m of query({ prompt: spec.prompt, options })) {
      appendFileSync(spec.transcript, `${JSON.stringify(m)}\n`);
      if (m.type === "result") result = m;
    }
  } catch (e) {
    // A single-shot query() throws after yielding an error result, so the
    // result (and its cost) may already be in hand.
    if (!result) {
      run = abort.signal.aborted
        ? { kind: "aborted", reason: `hard wall clock of ${secs(spec.hardMs)}` }
        : { kind: "crashed", error: String(e) };
      return finish(spec, run, null);
    }
  } finally {
    clearTimeout(timer);
  }
  if (!result) {
    return finish(spec, { kind: "crashed", error: "no result message" }, null);
  }
  const summary = summarize(result);
  if (result.subtype !== "success") {
    run = {
      kind: "no-output",
      reason: `${result.subtype}: ${result.errors.join("; ")}`,
      result: summary,
    };
  } else if (result.structured_output === undefined) {
    run = {
      kind: "no-output",
      reason: "no structured output",
      result: summary,
    };
  } else {
    const parsed = spec.parse(result.structured_output);
    run = parsed.ok
      ? { kind: "completed", output: parsed.value, result: summary }
      : { kind: "no-output", reason: parsed.error, result: summary };
  }
  return finish(spec, run, summary);
}

function finish<T>(
  spec: AgentSpec<T>,
  run: AgentRun<T>,
  result: ResultSummary | null,
): { run: AgentRun<T>; record: AgentRecord } {
  const cost = result
    ? `$${result.costUsd.toFixed(2)}, ${result.numTurns} turns`
    : "cost unreported";
  const detail =
    run.kind === "completed"
      ? ""
      : `: ${run.kind === "crashed" ? run.error : run.reason}`;
  log(spec.role, `${spec.label}: ${run.kind} (${cost})${detail}`);
  return {
    run,
    record: { role: spec.role, label: spec.label, run: run.kind, result },
  };
}
