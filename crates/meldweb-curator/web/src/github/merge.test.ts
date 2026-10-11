import { describe, expect, it } from "vitest";
import { mergeLines } from "./merge";

const lines = (...l: string[]) => `${l.join("\n")}\n`;
const BASE = lines("cards = [", "  a,", "  b,", "  c,", "  d,", "]");

describe("merging two clients' edits by lines", () => {
  it("keeps both when they changed different lines", () => {
    const ours = lines("cards = [", "  A,", "  b,", "  c,", "  d,", "]");
    const theirs = lines("cards = [", "  a,", "  b,", "  c,", "  D,", "]");
    expect(mergeLines(BASE, ours, theirs)).toBe(
      lines("cards = [", "  A,", "  b,", "  c,", "  D,", "]"),
    );
  });

  it("keeps an add on one side and a removal on the other", () => {
    const ours = lines(
      "cards = [",
      "  a,",
      "  aa,",
      "  b,",
      "  c,",
      "  d,",
      "]",
    );
    const theirs = lines("cards = [", "  a,", "  b,", "  d,", "]");
    expect(mergeLines(BASE, ours, theirs)).toBe(
      lines("cards = [", "  a,", "  aa,", "  b,", "  d,", "]"),
    );
  });

  it("takes the one side that changed, and an edit both made alike", () => {
    const changed = lines("cards = [", "  a,", "  B,", "  c,", "  d,", "]");
    expect(mergeLines(BASE, BASE, changed)).toBe(changed);
    expect(mergeLines(BASE, changed, BASE)).toBe(changed);
    expect(mergeLines(BASE, changed, changed)).toBe(changed);
  });

  it("gives up when both changed the same line differently", () => {
    const ours = lines("cards = [", "  a,", "  B1,", "  c,", "  d,", "]");
    const theirs = lines("cards = [", "  a,", "  B2,", "  c,", "  d,", "]");
    expect(mergeLines(BASE, ours, theirs)).toBeNull();
  });

  it("gives up when both added different lines at the same place", () => {
    const ours = lines(
      "cards = [",
      "  a,",
      "  b,",
      "  c,",
      "  d,",
      "  e,",
      "]",
    );
    const theirs = lines(
      "cards = [",
      "  a,",
      "  b,",
      "  c,",
      "  d,",
      "  f,",
      "]",
    );
    expect(mergeLines(BASE, ours, theirs)).toBeNull();
  });
});
