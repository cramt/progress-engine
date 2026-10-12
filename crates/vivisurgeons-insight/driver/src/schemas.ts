// JSON schemas for the agents' structured output, each beside the guard that
// turns the SDK's `structured_output: unknown` into a typed value.

import type {
  Escalation,
  Finding,
  MethodOutput,
  Resolution,
  ReviewOutput,
  WriterOutput,
} from "./types.ts";

export type Parsed<T> = { ok: true; value: T } | { ok: false; error: string };

const isObj = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);
const isStr = (v: unknown): v is string => typeof v === "string";

export const findingSchema = {
  type: "object",
  additionalProperties: false,
  required: ["card", "severity", "regression", "node", "should_be"],
  properties: {
    card: { type: "string", description: "The card's exact name" },
    severity: {
      type: "string",
      enum: ["game", "minor"],
      description: "game: the tree would change how the game plays out",
    },
    regression: {
      type: "boolean",
      description: "true when the old tree was right and the new one is wrong",
    },
    node: { type: "string", description: "The node that is wrong, as printed" },
    should_be: { type: "string", description: "The node it should be" },
    semantics_row: {
      type: "string",
      description: "The SEMANTICS.md row it breaks or that is missing, if any",
    },
  },
} as const;

export const reviewSchema = {
  type: "object",
  additionalProperties: false,
  required: ["findings"],
  properties: { findings: { type: "array", items: findingSchema } },
} as const;

function parseFinding(v: unknown): Finding | null {
  if (!isObj(v)) return null;
  const { card, severity, regression, node, should_be, semantics_row } = v;
  if (!isStr(card) || !isStr(node) || !isStr(should_be)) return null;
  if (severity !== "game" && severity !== "minor") return null;
  if (typeof regression !== "boolean") return null;
  const f: Finding = { card, severity, regression, node, should_be };
  if (isStr(semantics_row) && semantics_row.trim())
    f.semantics_row = semantics_row;
  return f;
}

export function parseReview(v: unknown): Parsed<ReviewOutput> {
  if (!isObj(v) || !Array.isArray(v.findings)) {
    return { ok: false, error: "no findings array" };
  }
  const findings: Finding[] = [];
  for (const [i, raw] of v.findings.entries()) {
    const f = parseFinding(raw);
    if (!f) return { ok: false, error: `finding ${i} is malformed` };
    findings.push(f);
  }
  return { ok: true, value: { findings } };
}

export const writerSchema = {
  type: "object",
  additionalProperties: false,
  required: ["summary", "resolved", "escalations", "handoff_updated"],
  properties: {
    summary: {
      type: "string",
      description: "What this batch changed, with the measured counts",
    },
    resolved: {
      type: "array",
      description:
        "Queued findings this batch closed, copied card and node exactly",
      items: {
        type: "object",
        additionalProperties: false,
        required: ["card", "node", "verdict"],
        properties: {
          card: { type: "string" },
          node: { type: "string" },
          verdict: { type: "string", enum: ["fixed", "not-a-defect"] },
        },
      },
    },
    escalations: {
      type: "array",
      description: "Decisions only the human can make, such as card exclusions",
      items: {
        type: "object",
        additionalProperties: false,
        required: ["kind", "cards", "question"],
        properties: {
          kind: { type: "string", enum: ["exclusion", "decision"] },
          cards: { type: "array", items: { type: "string" } },
          question: { type: "string" },
        },
      },
    },
    handoff_updated: { type: "boolean" },
  },
} as const;

export function parseWriter(v: unknown): Parsed<WriterOutput> {
  if (!isObj(v)) return { ok: false, error: "not an object" };
  const { summary, resolved, escalations, handoff_updated } = v;
  if (!isStr(summary)) return { ok: false, error: "no summary" };
  if (!Array.isArray(resolved) || !Array.isArray(escalations)) {
    return { ok: false, error: "resolved and escalations must be arrays" };
  }
  if (typeof handoff_updated !== "boolean") {
    return { ok: false, error: "no handoff_updated" };
  }
  const res: Resolution[] = [];
  for (const r of resolved) {
    if (!isObj(r) || !isStr(r.card) || !isStr(r.node)) {
      return { ok: false, error: "malformed resolution" };
    }
    if (r.verdict !== "fixed" && r.verdict !== "not-a-defect") {
      return { ok: false, error: "malformed verdict" };
    }
    res.push({ card: r.card, node: r.node, verdict: r.verdict });
  }
  const esc: Escalation[] = [];
  for (const e of escalations) {
    if (!isObj(e) || !isStr(e.question) || !Array.isArray(e.cards)) {
      return { ok: false, error: "malformed escalation" };
    }
    if (e.kind !== "exclusion" && e.kind !== "decision") {
      return { ok: false, error: "malformed escalation kind" };
    }
    esc.push({
      kind: e.kind,
      cards: e.cards.filter(isStr),
      question: e.question,
    });
  }
  return {
    ok: true,
    value: { summary, resolved: res, escalations: esc, handoff_updated },
  };
}

export const methodSchema = {
  type: "object",
  additionalProperties: false,
  required: ["method_notes", "rationale"],
  properties: {
    method_notes: {
      type: "string",
      description: "The whole new method-notes.md, in markdown",
    },
    rationale: {
      type: "string",
      description: "Which numbers in metrics.jsonl led to each change",
    },
  },
} as const;

export function parseMethod(v: unknown): Parsed<MethodOutput> {
  if (!isObj(v) || !isStr(v.method_notes) || !isStr(v.rationale)) {
    return { ok: false, error: "needs method_notes and rationale strings" };
  }
  return {
    ok: true,
    value: { method_notes: v.method_notes, rationale: v.rationale },
  };
}
