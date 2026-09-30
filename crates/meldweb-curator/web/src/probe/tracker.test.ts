import { describe, expect, it } from "vitest";
import { createTracker } from "./tracker";

/** What each frame scanned, for frames of these names in turn. */
function run(frames: string[][], confirm = 2, clear = 2): string[][] {
  const tracker = createTracker({ confirm, clear });
  return frames.map((f) => tracker.frame(f));
}

describe("scanning a stream of frames", () => {
  it("takes a card once it holds for two frames", () => {
    expect(run([["Sol Ring"], ["Sol Ring"]])).toEqual([[], ["Sol Ring"]]);
  });

  it("counts a card lying under the camera once, however long it lies", () => {
    const frames = Array.from({ length: 10 }, () => ["Sol Ring"]);
    expect(run(frames).flat()).toEqual(["Sol Ring"]);
  });

  it("does not take a card read once while it slid into place", () => {
    expect(run([["Llanowar Elves"], ["Sol Ring"], ["Sol Ring"]])).toEqual([
      [],
      [],
      ["Sol Ring"],
    ]);
  });

  it("does not count a card twice for one frame that missed it", () => {
    expect(run([["Sol Ring"], ["Sol Ring"], [], ["Sol Ring"]]).flat()).toEqual([
      "Sol Ring",
    ]);
  });

  it("counts the next copy once the first was taken away", () => {
    expect(
      run([["Sol Ring"], ["Sol Ring"], [], [], ["Sol Ring"], ["Sol Ring"]]),
    ).toEqual([[], ["Sol Ring"], [], [], [], ["Sol Ring"]]);
  });

  it("counts a different card straight after, with no gap", () => {
    expect(
      run([["Sol Ring"], ["Sol Ring"], ["Mana Crypt"], ["Mana Crypt"]]).flat(),
    ).toEqual(["Sol Ring", "Mana Crypt"]);
  });

  it("counts a card that comes back after another card, as a new copy", () => {
    const frames = [
      ["Sol Ring"],
      ["Sol Ring"],
      ["Mana Crypt"],
      ["Mana Crypt"],
      ["Sol Ring"],
      ["Sol Ring"],
    ];
    expect(run(frames).flat()).toEqual(["Sol Ring", "Mana Crypt", "Sol Ring"]);
  });

  it("takes each of several cards in one frame, in its order", () => {
    expect(
      run([
        ["Sol Ring", "Mana Crypt"],
        ["Mana Crypt", "Sol Ring"],
      ]),
    ).toEqual([[], ["Mana Crypt", "Sol Ring"]]);
  });

  it("counts two copies in one frame as one", () => {
    expect(
      run([
        ["Island", "Island"],
        ["Island", "Island"],
      ]).flat(),
    ).toEqual(["Island"]);
  });

  it("takes on the first frame when told to", () => {
    expect(run([["Sol Ring"]], 1)).toEqual([["Sol Ring"]]);
  });

  it("says which cards it is holding", () => {
    const tracker = createTracker({ confirm: 2, clear: 2 });
    tracker.frame(["Sol Ring"]);
    expect(tracker.holding("Sol Ring")).toBe(false);
    tracker.frame(["Sol Ring"]);
    expect(tracker.holding("Sol Ring")).toBe(true);
    tracker.frame([]);
    expect(tracker.holding("Sol Ring")).toBe(true);
    tracker.frame([]);
    expect(tracker.holding("Sol Ring")).toBe(false);
  });
});
