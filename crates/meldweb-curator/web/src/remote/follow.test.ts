import { describe, expect, it } from "vitest";
import { freeStem, ownerOf, theirRepoUrl } from "./follow";

describe("another player's repo", () => {
  it("is their GitHub login's mtg repo, or a git URL as given", () => {
    expect(theirRepoUrl("alice")).toBe("https://github.com/alice/mtg.git");
    expect(theirRepoUrl(" @alice ")).toBe("https://github.com/alice/mtg.git");
    expect(theirRepoUrl("https://codeberg.org/bob/decks.git/")).toBe(
      "https://codeberg.org/bob/decks.git",
    );
    for (const bad of [
      "http://github.com/a/mtg",
      "https://github.com/a",
      "https://u:p@github.com/a/mtg",
      "a b",
      "-alice",
    ])
      expect([bad, theirRepoUrl(bad)]).toEqual([bad, null]);
    expect(ownerOf("https://github.com/alice/mtg.git")).toBe("alice");
  });

  it("is kept under their stem unless a deck here has it", () => {
    expect(freeStem("lantern", "alice", new Set())).toBe("lantern");
    expect(freeStem("lantern", "Alice", new Set(["lantern"]))).toBe(
      "lantern-alice",
    );
    expect(
      freeStem("lantern", "alice", new Set(["lantern", "lantern-alice"])),
    ).toBe("lantern-alice-2");
  });
});
