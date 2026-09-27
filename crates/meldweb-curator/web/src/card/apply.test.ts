import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { loadDeckSync, type Parsed, parseDeck } from "../deck";
import { applyEdit, type Target, targetsFor, toggleSelected } from "./apply";

const repo = new URL("../../../../../", import.meta.url);
const lantern = readFileSync(new URL("decks/lantern.deck.toml", repo), "utf8");

// Loaded before the describes run, since they look cards up as they are declared.
loadDeckSync(
  readFileSync(new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url)),
);

function deck(text: string): Extract<Parsed, { kind: "deck" }> {
  const parsed = parseDeck(text);
  if (parsed.kind !== "deck") throw new Error(parsed.message);
  return parsed;
}

/** The lines of `after` that differ from `before`, when neither gained lines. */
function changedLines(before: string, after: string): string[] {
  const a = before.split("\n");
  return after.split("\n").filter((line, i) => line !== a[i]);
}

/** The card whose comment names it, and the stack it sits in. */
function find(name: string): Target & { line: string } {
  const d = deck(lantern);
  const lines = lantern.split("\n");
  const line = lines.find((l) => l.endsWith(`# ${name}`));
  if (!line) throw new Error(`no ${name} in lantern`);
  const index = lines.filter((l) => l.startsWith("  {")).indexOf(line);
  const card = d.cards[index];
  if (!card) throw new Error(`no card ${index}`);
  return { index, from: card.categories[0] ?? null, line };
}

describe("a card action is one chip-decklist edit", () => {
  const solRing = find("Sol Ring");

  it("+ and - change only that card's line, and - at one removes it", () => {
    const up = applyEdit(lantern, deck(lantern), { kind: "increase" }, [
      solRing,
    ]);
    expect(changedLines(lantern, up)).toEqual([
      '  { printing = "cmr/472", qty = 2, in = ["Artifact Count"] },  # Sol Ring',
    ]);
    const gone = applyEdit(lantern, deck(lantern), { kind: "decrease" }, [
      solRing,
    ]);
    expect(gone.split("\n")).toEqual(
      lantern.split("\n").filter((l) => l !== solRing.line),
    );
  });

  it("R removes it and A leaves it in no category", () => {
    const removed = applyEdit(lantern, deck(lantern), { kind: "remove" }, [
      solRing,
    ]);
    expect(deck(removed).total).toBe(99);
    const auto = applyEdit(lantern, deck(lantern), { kind: "automatic" }, [
      solRing,
    ]);
    expect(changedLines(lantern, auto)).toEqual([
      '  { printing = "cmr/472" },  # Sol Ring',
    ]);
  });

  it("M moves it to a maybeboard the deck declares for it, as the drag strip does", () => {
    const next = applyEdit(
      lantern,
      deck(lantern),
      { kind: "board", type: "maybeboard" },
      [solRing],
    );
    const d = deck(next);
    expect(d.categories).toContainEqual({
      name: "Maybeboard",
      kind: "maybeboard",
    });
    expect(d.cards[solRing.index]?.categories).toEqual(["Maybeboard"]);
    expect(d.total).toBe(99);
    // Sol Ring's line changed; the rest is the declaration, added below it.
    const added = next.split("\n").length - lantern.split("\n").length;
    expect(added).toBe(1);
    expect(
      next.split("\n").filter((l) => !lantern.split("\n").includes(l)),
    ).toEqual([
      '  { printing = "cmr/472", in = ["Maybeboard"] },  # Sol Ring',
      'Maybeboard = { type = "maybeboard" }',
    ]);
    // A second card goes to the same board rather than declaring another.
    const again = applyEdit(next, d, { kind: "board", type: "maybeboard" }, [
      find("Counterspell"),
    ]);
    expect(changedLines(next, again)).toHaveLength(1);
  });

  it("moving to a category replaces the stack it was reached from, and a new one is declared", () => {
    const next = applyEdit(
      lantern,
      deck(lantern),
      { kind: "category", name: "Draw" },
      [solRing],
    );
    expect(changedLines(lantern, next)).toEqual([
      '  { printing = "cmr/472", in = ["Draw"] },  # Sol Ring',
    ]);
    const fresh = applyEdit(
      lantern,
      deck(lantern),
      { kind: "category", name: "Fast Mana" },
      [solRing],
    );
    expect(deck(fresh).cards[solRing.index]?.categories).toEqual(["Fast Mana"]);
    expect(deck(fresh).categories).toContainEqual({ name: "Fast Mana" });
  });

  it("set as commander puts it in the commander category first, keeping its labels", () => {
    const next = applyEdit(lantern, deck(lantern), { kind: "commander" }, [
      solRing,
    ]);
    expect(changedLines(lantern, next)).toEqual([
      '  { printing = "cmr/472", in = ["Commander", "Artifact Count"] },  # Sol Ring',
    ]);
  });

  it("refuses the way chip-decklist does, and changes nothing", () => {
    expect(() =>
      applyEdit(lantern, deck(lantern), { kind: "remove" }, [
        { index: 999, from: null },
      ]),
    ).toThrow();
  });
});

describe("multi-select", () => {
  const a = find("Sol Ring");
  const b = find("Arcane Signet");
  const c = find("Counterspell");

  it("an action on a selected card applies to every selected card", () => {
    let selection = toggleSelected(new Map(), a);
    selection = toggleSelected(selection, c);
    expect(targetsFor(a, selection).map((t) => t.index)).toEqual([
      a.index,
      c.index,
    ]);
    // A card outside the selection is acted on alone.
    expect(targetsFor(b, selection)).toEqual([b]);
    // Toggling again deselects.
    expect(toggleSelected(selection, a).has(a.index)).toBe(false);
  });

  it("removes every selected card, whatever order they were picked in", () => {
    const next = applyEdit(lantern, deck(lantern), { kind: "remove" }, [
      b,
      c,
      a,
    ]);
    expect(next.split("\n")).toEqual(
      lantern
        .split("\n")
        .filter((l) => l !== a.line && l !== b.line && l !== c.line),
    );
  });

  it("raises every selected card by one, each on its own line", () => {
    const next = applyEdit(lantern, deck(lantern), { kind: "increase" }, [
      a,
      b,
    ]);
    expect(changedLines(lantern, next)).toHaveLength(2);
    expect(deck(next).total).toBe(102);
  });
});
