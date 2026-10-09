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
    expect(viewingOf({ vs: "decks/loam.deck.toml" })).toEqual({
      kind: "deck",
      path: "decks/loam.deck.toml",
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
      { kind: "deck" as const, path: "decks/loam.deck.toml" },
    ]) {
      for (const drawer of [true, false]) {
        for (const view of DECK_VIEWS) {
          const page = { viewing, drawer, view };
          expect(
            pageOf(parseSearch(searchFor(page) as Record<string, unknown>)),
          ).toEqual(page);
        }
      }
    }
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
  it("never takes its name or whose variant it is", () => {
    const changes = [
      { kind: "remove" as const, before: 0, text: "-1 Volcanic Island" },
      { kind: "rename" as const, text: 'name: none → "Lantern Budget"' },
      { kind: "variantOf" as const, text: "variant of: none → lantern" },
      { kind: "description" as const, text: "description: added" },
    ];
    expect(
      takeable(changes, { kind: "deck", path: "decks/b.deck.toml" }).map(
        (c) => c.at,
      ),
    ).toEqual([0, 3]);
    // From its own past, a rename is part of what it was.
    expect(
      takeable(changes, { kind: "revision", commit: SHA }).map((c) => c.at),
    ).toEqual([0, 1, 2, 3]);
  });
});
