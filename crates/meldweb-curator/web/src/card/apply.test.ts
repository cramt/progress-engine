import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { editDeckCards, loadDeckSync } from "../deck";
import { type Target, targetsFor, toDeckEdit, toggleSelected } from "./apply";

// What each edit does to the text is chip-decklist's, tested there
// (tests/edit_cards.rs); these are the page's side of it.

loadDeckSync(
  readFileSync(new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url)),
);

describe("a card action", () => {
  it("is the deck edit it names, a board or category being a move there", () => {
    expect(toDeckEdit({ kind: "remove" })).toEqual({ kind: "remove" });
    expect(toDeckEdit({ kind: "board", type: "maybeboard" })).toEqual({
      kind: "move",
      to: { kind: "board", board: "maybeboard" },
      secondary: false,
    });
    expect(toDeckEdit({ kind: "category", name: "Draw" })).toEqual({
      kind: "move",
      to: { kind: "category", name: "Draw" },
      secondary: false,
    });
  });

  it("is one edit over every target, saying where each line went", () => {
    const text = `cards = [\n  { name = "A" },\n  { name = "B", in = ["Ramp"] },\n]\n\n[categories]\nRamp = {}\n`;
    const edited = editDeckCards(text, toDeckEdit({ kind: "remove" }), [
      { index: 0, from: null },
    ]);
    expect(edited.lines).toEqual([{ kind: "gone" }, { kind: "at", index: 0 }]);
    const moved = editDeckCards(
      text,
      toDeckEdit({ kind: "category", name: "Draw" }),
      [{ index: 1, from: "Ramp" }],
    );
    expect(moved.text).toContain(`{ name = "B", in = ["Draw"] }`);
    expect(() =>
      editDeckCards(text, { kind: "remove" }, [{ index: 9, from: null }]),
    ).toThrow(/no card 9/);
  });
});

describe("multi-select", () => {
  const a: Target = { index: 0, from: "Ramp" };
  const b: Target = { index: 1, from: null };
  const c: Target = { index: 2, from: "Draw" };

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
});
