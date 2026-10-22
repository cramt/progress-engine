import { describe, expect, it } from "vitest";
import type { BlameCommit, CardBlame } from "../deck.gen";
import {
  addedTouch,
  lastTouch,
  shortDay,
  sortBlame,
  unchangedSinceAdded,
} from "./blame";

const commit = (id: string, day: number): BlameCommit => ({
  commit: id.repeat(40).slice(0, 40),
  deck: "lantern",
  author: "Alex",
  email: "alex@example.com",
  authoredAt: Date.UTC(2026, 9, day) / 1000,
  message: `lantern: ${id}`,
});

const row = (name: string, index: number, blame: CardBlame["blame"]) => ({
  name,
  blame: { index, changes: [], blame },
});

describe("a card's blame", () => {
  it("says where it last changed and where it came in", () => {
    const a = commit("a", 3);
    const c = commit("c", 14);
    expect(lastTouch({ kind: "changed", added: a, changed: c })).toEqual({
      kind: "commit",
      commit: c,
    });
    expect(addedTouch({ kind: "changed", added: a, changed: c })).toEqual({
      kind: "commit",
      commit: a,
    });
    expect(lastTouch({ kind: "changedUnsaved", added: a })).toEqual({
      kind: "unsaved",
    });
    expect(addedTouch({ kind: "changedUnsaved", added: a })).toEqual({
      kind: "commit",
      commit: a,
    });
    expect(addedTouch({ kind: "addedUnsaved" })).toEqual({ kind: "unsaved" });
    expect(unchangedSinceAdded({ kind: "added", added: a })).toBe(true);
    expect(unchangedSinceAdded({ kind: "changedUnsaved", added: a })).toBe(
      false,
    );
  });

  it("dates a commit by its day, with the year only when it is another", () => {
    const now = new Date(Date.UTC(2026, 9, 22));
    expect(shortDay(Date.UTC(2026, 9, 3, 12) / 1000, now)).toBe("3 Oct");
    expect(shortDay(Date.UTC(2025, 9, 3, 12) / 1000, now)).toBe("3 Oct 2025");
  });

  it("sorts the newest change first, unsaved edits above every commit", () => {
    const rows = [
      row("Sol Ring", 0, { kind: "added", added: commit("a", 3) }),
      row("Opt", 1, {
        kind: "changed",
        added: commit("a", 3),
        changed: commit("c", 14),
      }),
      row("Ponder", 2, { kind: "addedUnsaved" }),
      row("Island", 3, { kind: "added", added: commit("b", 9) }),
    ];
    const names = (order: "changed" | "added" | "name") =>
      sortBlame(rows, order).map((r) => r.name);
    expect(names("changed")).toEqual(["Ponder", "Opt", "Island", "Sol Ring"]);
    expect(names("added")).toEqual(["Ponder", "Island", "Sol Ring", "Opt"]);
    expect(names("name")).toEqual(["Island", "Opt", "Ponder", "Sol Ring"]);
  });
});
