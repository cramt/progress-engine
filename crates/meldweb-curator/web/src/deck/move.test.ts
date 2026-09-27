import { describe, expect, it } from "vitest";
import { dropOnto } from "./move";

describe("dropping a card on a category", () => {
  it("moves it into the dragged-from category's place", () => {
    expect(
      dropOnto(["tempo", "self-bounce"], "tempo", "enemy-bounce", false),
    ).toEqual(["enemy-bounce", "self-bounce"]);
  });

  it("does not list a category twice when moving into one it already has", () => {
    expect(
      dropOnto(["tempo", "self-bounce"], "tempo", "self-bounce", false),
    ).toEqual(["self-bounce"]);
  });

  it("adds a category behind the others with Ctrl", () => {
    expect(dropOnto(["tempo"], "tempo", "learnboard", true)).toEqual([
      "tempo",
      "learnboard",
    ]);
  });

  it("gives an uncategorized card its first category", () => {
    expect(dropOnto([], null, "tempo", false)).toEqual(["tempo"]);
  });
});
