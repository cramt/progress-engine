import { describe, expect, it } from "vitest";
import type { Category } from "../decklist";
import { dropOnto } from "./move";

const c = (name: string, ...flags: string[]): Category => ({ name, flags });

describe("dropping a card on a category", () => {
  it("moves it, keeping the premier flag on the new category", () => {
    expect(
      dropOnto(
        [c("Artifact Count", "top"), c("Ramp")],
        "Artifact Count",
        "Draw",
        false,
      ),
    ).toEqual([c("Draw", "top"), c("Ramp")]);
  });

  it("does not list a category twice when moving into one it already has", () => {
    expect(
      dropOnto(
        [c("Artifact Count"), c("Draw")],
        "Artifact Count",
        "Draw",
        false,
      ),
    ).toEqual([c("Draw")]);
  });

  it("adds a secondary category behind the others with Ctrl", () => {
    expect(
      dropOnto([c("Artifact Count")], "Artifact Count", "Draw", true),
    ).toEqual([c("Artifact Count"), c("Draw")]);
  });

  it("gives an uncategorized card its first category", () => {
    expect(dropOnto([], "Uncategorized", "Land", false)).toEqual([c("Land")]);
  });
});
