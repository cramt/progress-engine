import { describe, expect, it } from "vitest";
import type { Revision } from "../github/history";
import {
  body,
  byDay,
  DECK_VIEWS,
  grouped,
  pageOf,
  parseSearch,
  searchFor,
  subject,
  takeable,
} from "./versions";

const SHA = "a".repeat(40);
const viewingOf = (raw: Record<string, unknown>) =>
  pageOf(parseSearch(raw)).viewing;

describe("the deck page's URL", () => {
  it("names a past version by its commit, or another deck by its path", () => {
    expect(viewingOf({ at: SHA })).toEqual({
      kind: "revision",
      commit: SHA,
    });
    expect(viewingOf({ vs: "loam" })).toEqual({ kind: "deck", stem: "loam" });
    // A link from repo version 1 named the deck's path.
    expect(viewingOf({ vs: "decks/loam.deck.toml" })).toEqual({
      kind: "deck",
      stem: "loam",
    });
    expect(viewingOf({})).toEqual({ kind: "now" });
  });

  it("drops what is not a commit, and prefers the past to another deck", () => {
    expect(parseSearch({ at: "HEAD~3" })).toEqual({});
    expect(parseSearch({ history: 1 })).toEqual({ history: true });
    expect(parseSearch({ history: false })).toEqual({});
    expect(parseSearch({ at: SHA, vs: "decks/loam.deck.toml" })).toEqual({
      at: SHA,
    });
  });

  it("lays the deck out as stacks unless it names another view", () => {
    expect(pageOf(parseSearch({})).view).toBe("stacks");
    expect(pageOf(parseSearch({ view: "cost" })).view).toBe("cost");
    // The default is no parameter at all, so a link to it stays short.
    expect(parseSearch({ view: "stacks" })).toEqual({});
    expect(parseSearch({ view: "grid" })).toEqual({});
  });

  it("round-trips, with the timeline open or shut, in every view", () => {
    for (const viewing of [
      { kind: "now" as const },
      { kind: "revision" as const, commit: SHA },
      { kind: "deck" as const, stem: "loam" },
    ]) {
      for (const drawer of [true, false]) {
        for (const view of DECK_VIEWS) {
          for (const branch of ["main", "budget-test"]) {
            const page = { branch, viewing, drawer, view };
            expect(
              pageOf(parseSearch(searchFor(page) as Record<string, unknown>)),
            ).toEqual(page);
          }
        }
      }
    }
  });

  it("opens a branch by its name, and main without one", () => {
    expect(pageOf(parseSearch({})).branch).toBe("main");
    expect(parseSearch({ branch: "main" })).toEqual({});
    expect(parseSearch({ branch: "budget" })).toEqual({ branch: "budget" });
    // A branch's name is one component: nothing outside the deck's own refs.
    expect(parseSearch({ branch: "../loam/main" })).toEqual({});
    expect(parseSearch({ branch: "a/b" })).toEqual({});
    // A snapshot on a branch is the same view as a revision.
    expect(parseSearch({ branch: "budget", at: SHA })).toEqual({
      branch: "budget",
      at: SHA,
    });
  });
});

describe("the timeline", () => {
  it("shows a commit by what changed, without the deck's name", () => {
    expect(subject("lantern: +1 Sol Ring, -1 Mind Stone", "Lantern")).toBe(
      "+1 Sol Ring, -1 Mind Stone",
    );
    expect(subject("edit decks/lantern.deck.toml", "lantern")).toBe(
      "edit decks/lantern.deck.toml",
    );
    expect(
      body("lantern: +1 A, +1 B, +1 C, and 1 more\n\n+1 A\n+1 B\n+1 C\n+1 D\n"),
    ).toEqual(["+1 A", "+1 B", "+1 C", "+1 D"]);
    expect(body("lantern: +1 A")).toEqual([]);
  });

  it("puts each revision under its day, newest first", () => {
    const now = new Date(2026, 9, 3, 18);
    const at = (d: Date): Revision => ({
      commit: SHA,
      message: "",
      date: d.toISOString(),
      author: null,
    });
    const days = byDay(
      [
        at(new Date(2026, 9, 3, 9)),
        at(new Date(2026, 9, 3, 8)),
        at(new Date(2026, 9, 2, 23)),
        at(new Date(2025, 11, 24, 12)),
      ],
      now,
    );
    expect(
      days.map((d) => [d.day.startsWith("Wed") || d.day, d.revisions.length]),
    ).toEqual([
      ["Today", 2],
      ["Yesterday", 1],
      [true, 1],
    ]);
    expect(days[2]?.day).toMatch(/2025/);
  });
});

describe("the compare panel", () => {
  it("groups changes by what taking them does, keeping each one's position", () => {
    const groups = grouped(
      takeable(
        [
          { kind: "add", after: 3, text: "+1 Sol Ring" },
          { kind: "remove", before: 1, text: "-1 Mind Stone" },
          { kind: "qty", before: 2, after: 2, text: "Island: 8 → 7" },
          { kind: "rename", text: 'name: "A" → "B"' },
          { kind: "add", after: 4, text: "+1 Arcane Signet" },
        ],
        { kind: "revision", commit: SHA },
      ),
    );
    expect(groups.map((g) => [g.group, g.changes.map((c) => c.at)])).toEqual([
      ["in", [0, 4]],
      ["out", [1]],
      ["changed", [2]],
      ["deck", [3]],
    ]);
  });
});

describe("taking from another deck", () => {
  it("never takes its name", () => {
    const changes = [
      { kind: "remove" as const, before: 0, text: "-1 Volcanic Island" },
      { kind: "rename" as const, text: 'name: none → "Lantern Budget"' },
      { kind: "description" as const, text: "description: added" },
    ];
    expect(
      takeable(changes, { kind: "deck", stem: "b" }).map((c) => c.at),
    ).toEqual([0, 2]);
    // From its own past, a rename is part of what it was.
    expect(
      takeable(changes, { kind: "revision", commit: SHA }).map((c) => c.at),
    ).toEqual([0, 1, 2]);
  });
});
