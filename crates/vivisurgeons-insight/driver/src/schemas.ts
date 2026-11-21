// Readers for the JSON the batch's agents leave in files: the writer's report
// and each reviewer's findings. Each refuses a shape it does not recognise.

import type {
  Escalation,
  Finding,
  Report,
  Resolution,
  ReviewOutput,
} from "./types.ts";

export type Parsed<T> = { ok: true; value: T } | { ok: false; error: string };

const isObj = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);
const isStr = (v: unknown): v is string => typeof v === "string";

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

/** `{"findings": [{card, severity, regression, node, should_be, semantics_row?}]}` */
export function parseReview(v: unknown): Parsed<ReviewOutput> {
  if (!isObj(v) || !Array.isArray(v.findings)) {
    return { ok: false, error: "no findings array" };
  }
  const findings: Finding[] = [];
  for (const [i, raw] of v.findings.entries()) {
    const f = parseFinding(raw);
    if (!f)
      return {
        ok: false,
        error: `finding ${i} is malformed: needs card, node, should_be strings, severity "game" or "minor", regression boolean`,
      };
    findings.push(f);
  }
  return { ok: true, value: { findings } };
}

/** `{"summary", "resolved": [{card, node, verdict}], "escalations": [{kind, cards, question}]}` */
export function parseReport(v: unknown): Parsed<Report> {
  if (!isObj(v)) return { ok: false, error: "not an object" };
  const { summary, resolved, escalations } = v;
  if (!isStr(summary)) return { ok: false, error: "no summary string" };
  if (!Array.isArray(resolved) || !Array.isArray(escalations)) {
    return { ok: false, error: "resolved and escalations must be arrays" };
  }
  const res: Resolution[] = [];
  for (const r of resolved) {
    if (!isObj(r) || !isStr(r.card) || !isStr(r.node)) {
      return { ok: false, error: "a resolution needs card and node strings" };
    }
    if (r.verdict !== "fixed" && r.verdict !== "not-a-defect") {
      return {
        ok: false,
        error: `verdict must be "fixed" or "not-a-defect" (${r.card})`,
      };
    }
    res.push({ card: r.card, node: r.node, verdict: r.verdict });
  }
  const esc: Escalation[] = [];
  for (const e of escalations) {
    if (!isObj(e) || !isStr(e.question) || !Array.isArray(e.cards)) {
      return {
        ok: false,
        error: "an escalation needs a question string and a cards array",
      };
    }
    if (e.kind !== "exclusion" && e.kind !== "decision") {
      return {
        ok: false,
        error: 'escalation kind must be "exclusion" or "decision"',
      };
    }
    esc.push({
      kind: e.kind,
      cards: e.cards.filter(isStr),
      question: e.question,
    });
  }
  return { ok: true, value: { summary, resolved: res, escalations: esc } };
}
