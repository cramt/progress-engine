/**
 * `meldweb.toml`'s printing rules as the settings page edits them (ADR-0026).
 * Reading, checking and writing the file are `meldweb-wasm`'s, so a rule is
 * judged by the same query reader that ranks with it; this keeps the draft.
 */
import type { Pin, RuleText, SettingsRules, Verb } from "../deck.gen";
import {
  check_rule,
  read_settings,
  settings_commit_message,
  write_settings,
} from "../wasm/pkg/meldweb_wasm.js";

export type { Pin, RuleText, Verb };

/**
 * A rule being edited. Its query may not parse yet, and its id outlives a
 * move, so a row keeps its focus and its place in a drag.
 */
export interface DraftRule extends RuleText {
  id: number;
}

let nextId = 0;

export function draftRule(rule: RuleText): DraftRule {
  return { id: nextId++, verb: rule.verb, query: rule.query };
}

/** The repo's `meldweb.toml`, or `null` for a repo without one. */
export function readSettings(text: string | null): SettingsRules {
  return JSON.parse(read_settings(text ?? undefined)) as SettingsRules;
}

/** Why `query` is not a printing rule, or null when it is one. */
export function problemOf(query: string): string | null {
  return check_rule(query) ?? null;
}

/**
 * The text of a `meldweb.toml` holding `pins`, then `rules`; throws when a
 * rule does not parse.
 */
export function writeSettings(
  pins: readonly Pin[],
  rules: readonly RuleText[],
): string {
  return write_settings(
    JSON.stringify(pins.map(({ name, set, num }) => ({ name, set, num }))),
    JSON.stringify(rules.map(({ verb, query }) => ({ verb, query }))),
  );
}

export function samePins(a: readonly Pin[], b: readonly Pin[]): boolean {
  return (
    a.length === b.length &&
    a.every(
      (p, i) =>
        p.name === b[i]?.name && p.set === b[i]?.set && p.num === b[i]?.num,
    )
  );
}

/** What the page edits: the cards pinned, and the rules for everything else. */
export interface Draft<R extends RuleText = DraftRule> {
  pins: readonly Pin[];
  rules: readonly R[];
}

export function settingsCommitMessage(before: string, after: string): string {
  return settings_commit_message(before, after);
}

export function sameRules(
  a: readonly RuleText[],
  b: readonly RuleText[],
): boolean {
  return (
    a.length === b.length &&
    a.every((r, i) => r.verb === b[i]?.verb && r.query === b[i]?.query)
  );
}

/** `rules` with the rule at `from` moved to `to`. */
export function moveRule<T>(
  rules: readonly T[],
  from: number,
  to: number,
): T[] {
  const moved = [...rules];
  const [rule] = moved.splice(from, 1);
  if (rule === undefined) return moved;
  moved.splice(Math.max(0, Math.min(to, moved.length)), 0, rule);
  return moved;
}

/** What the file was when the page loaded, to tell an edit from none. */
export interface Loaded extends Draft<RuleText> {
  /** The file's text, `""` when the repo has none. */
  text: string;
}

/**
 * The `meldweb.toml` to save for `draft`: the file as loaded when the pins
 * and rules are the loaded ones, so opening the page and undoing back commits
 * nothing and a hand-written file keeps its comments; else them written out.
 * Null while a rule does not parse, which holds the save back.
 */
export function fileFor(draft: Draft<RuleText>, loaded: Loaded): string | null {
  if (samePins(draft.pins, loaded.pins) && sameRules(draft.rules, loaded.rules))
    return loaded.text;
  if (draft.rules.some((r) => problemOf(r.query) !== null)) return null;
  return writeSettings(draft.pins, draft.rules);
}

/**
 * The pins and the rules of `draft` that parse, as a `meldweb.toml` for the
 * preview, and where each rule is in the file it ranks by, since the ranking
 * numbers rules by their place there: -1 for one left out.
 */
export function previewOf(draft: Draft<RuleText>): {
  text: string;
  at: number[];
} {
  const kept = draft.rules.filter((r) => problemOf(r.query) === null);
  let next = draft.pins.length;
  return {
    text: writeSettings(draft.pins, kept),
    at: draft.rules.map((r) => (problemOf(r.query) === null ? next++ : -1)),
  };
}

/** The default rules `draft` does not have, to offer back one at a time. */
export function missingDefaults(
  draft: readonly RuleText[],
  defaults: readonly RuleText[],
): RuleText[] {
  return defaults.filter((d) => !draft.some((r) => r.query.trim() === d.query));
}

/**
 * Printing terms worth knowing, for the page's cheat sheet, since Scryfall's
 * syntax page is long. A test reads each one, so none can go stale.
 */
export const TERMS: readonly (readonly [term: string, what: string])[] = [
  ["is:fullart", "art covers the whole card"],
  ["is:textless", "no rules text printed"],
  ["is:showcase", "showcase frames"],
  ["is:extendedart", "art runs past the frame's sides"],
  ["is:borderless", "no border"],
  ["is:retro", "new cards in the old frame"],
  ["is:ub", "Universes Beyond"],
  ["is:sourcematerial", "printed with art from the source material"],
  ["is:flavorname", "printed under a flavor name"],
  ["is:promo", "promos of any kind"],
  ["is:digital", "Arena and MTGO only"],
  ["lang:en", "language, by Scryfall's code: ja, de, fr, …"],
  ["frame:old", "pre-2003 frames; or a year: 1993, 1997, 2003, 2015"],
  ["border:white", "border colour: black, white, borderless, gold, silver"],
  ["set:mh3", "one set, by its code"],
  ["st:masters", "set type: core, expansion, masters, commander, …"],
];
