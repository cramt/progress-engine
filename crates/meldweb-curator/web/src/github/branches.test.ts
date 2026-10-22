import { beforeAll, describe, expect, it } from "vitest";
import { ConflictError } from "../repo/api";
import { memoryRepo } from "../repo/memory";
import { MAIN, makeBranch, mergeAsPlanned, planSummary } from "./branches";
import { deckFile } from "./decks";
import { head, revisions, takeSnapshot } from "./history";
import { commitEdit } from "./repoFile";
import { loadWasm } from "./testkit";

// The deck's stem; its line is decks/lantern/main (ADR-0041).
const STEM = "lantern";
const CARDS = [
  "Sol Ring",
  "Mind Stone",
  "Arcane Signet",
  "Command Tower",
  "Brainstorm",
  "Ponder",
  "Counterspell",
  "Island",
];
const deck = (cards: readonly string[]) =>
  `name = "Lantern"\nformat = "commander"\ncards = [\n${cards.map((c) => `  { name = "${c}" },\n`).join("")}]\n`;
const swap = (cards: readonly string[], from: string, to: string) =>
  cards.map((c) => (c === from ? to : c));

beforeAll(loadWasm);

/** Lantern on its main, saved once, with a snapshot of that save. */
async function lantern() {
  let clock = Date.parse("2026-09-01T12:00:00Z");
  const repo = memoryRepo(
    () => ({ name: "Alex", email: "alex@example.com" }),
    () => (clock += 60_000),
  );
  const { path, branch } = deckFile(STEM);
  await commitEdit(repo, path, () => deck(CARDS), branch);
  const made = await head(repo, STEM);
  if (!made) throw new Error("no save");
  await takeSnapshot(repo, STEM, "FNM 14 Sep", made.commit);
  /** Saves `cards` on the deck's branch `on`, as the editor would. */
  const save = (on: string, cards: readonly string[]) => {
    const file = deckFile(STEM, on);
    return commitEdit(repo, file.path, () => deck(cards), file.branch);
  };
  const text = async (on: string) => {
    const file = deckFile(STEM, on);
    return (await repo.getFile(file.path, file.branch))?.text;
  };
  return { repo, save, text, first: made.commit };
}

describe("a deck's branches", () => {
  it("edit apart from main and merge back into it as a two-parent merge", async () => {
    const { repo, save, text, first } = await lantern();
    const made = await makeBranch(repo, STEM, "Budget test", first);
    expect(made).toMatchObject({
      kind: "made",
      branch: { name: "budget-test", unmerged: 0, behind: 0 },
    });

    // Two cards changed on the branch, apart from the one main changes: two
    // edits to neighbouring lines are one stretch, which clashes.
    const budget = swap(
      swap(CARDS, "Mind Stone", "Fellwar Stone"),
      "Brainstorm",
      "Opt",
    );
    await save("budget-test", budget);
    // Main, which the deck list, Build and the collection read, is as it was.
    const [line] = await repo.decks();
    expect(line?.file?.text).toBe(deck(CARDS));
    expect(
      (await revisions(repo, STEM, 1, "budget-test")).map((r) => r.message),
    ).toEqual([
      "lantern: +1 Fellwar Stone, +1 Opt, -1 Brainstorm, and 1 more\n\n+1 Fellwar Stone\n+1 Opt\n-1 Brainstorm\n-1 Mind Stone",
      expect.stringMatching(/^lantern: /),
    ]);

    // Main moves on another card; the merge keeps both.
    const onMain = swap(CARDS, "Island", "Snow-Covered Island");
    await save(MAIN, onMain);
    expect(await repo.branches(STEM)).toMatchObject([
      { name: "main", unmerged: 0, behind: 0 },
      { name: "budget-test", unmerged: 1, behind: 1 },
    ]);
    const ask = { from: "budget-test", into: MAIN };
    const plan = await repo.planMerge(STEM, ask);
    expect(plan.kind).toBe("merge");
    if (plan.kind !== "merge") return;
    expect(plan.message.split("\n", 1)[0]).toBe(
      "lantern: merge budget-test: +1 Fellwar Stone, +1 Opt, -1 Brainstorm, and 1 more",
    );
    expect(planSummary(plan, ask)).toMatch(/merge commit/);
    const both = swap(budget, "Island", "Snow-Covered Island");
    expect(plan.after).toBe(deck(both));

    const done = await mergeAsPlanned(repo, STEM, ask, plan);
    expect(done.kind).toBe("merged");
    expect(await text(MAIN)).toBe(deck(both));
    const [top] = await revisions(repo, STEM);
    expect(top?.message.split("\n", 1)[0]).toBe(plan.message.split("\n", 1)[0]);
    expect(await repo.branches(STEM)).toMatchObject([
      { name: "main" },
      { name: "budget-test", unmerged: 0, behind: 2 },
    ]);

    // Updating the branch from main is now a fast-forward.
    const update = { from: MAIN, into: "budget-test" };
    const forward = await repo.planMerge(STEM, update);
    expect(forward.kind).toBe("fastForward");
    expect(await mergeAsPlanned(repo, STEM, update, forward)).toMatchObject({
      kind: "merged",
    });
    expect(await text("budget-test")).toBe(deck(both));
  });

  it("stop at a clash, show both sides' lines, and merge once a side is picked", async () => {
    const { repo, save, text, first } = await lantern();
    await makeBranch(repo, STEM, "clash", first);
    await save("clash", swap(CARDS, "Ponder", "Preordain"));
    await save(MAIN, swap(CARDS, "Ponder", "Serum Visions"));
    const ask = { from: "clash", into: MAIN };
    const plan = await repo.planMerge(STEM, ask);
    expect(plan).toMatchObject({
      kind: "conflict",
      hunks: [
        {
          path: "lantern.deck.toml",
          ours: ['  { name = "Serum Visions" },'],
          theirs: ['  { name = "Preordain" },'],
        },
      ],
    });
    // Nothing merges until a side is picked.
    expect(await mergeAsPlanned(repo, STEM, ask, plan)).toMatchObject({
      kind: "stopped",
    });
    const theirs = { ...ask, prefer: "theirs" as const };
    const picked = await repo.planMerge(STEM, theirs);
    expect(picked.kind).toBe("merge");
    await mergeAsPlanned(repo, STEM, theirs, picked);
    expect(await text(MAIN)).toBe(deck(swap(CARDS, "Ponder", "Preordain")));
  });

  it("start from a snapshot, refuse a taken name, and delete only at their tip", async () => {
    const { repo, save, first } = await lantern();
    await save(MAIN, swap(CARDS, "Island", "Snow-Covered Island"));
    const [fnm] = await repo.snapshots(`decks/${STEM}/`);
    expect(fnm?.commit).toBe(first);
    const made = await makeBranch(repo, STEM, "from fnm", first);
    expect(made).toMatchObject({
      kind: "made",
      branch: { name: "from-fnm", unmerged: 0, behind: 1 },
    });
    expect(await makeBranch(repo, STEM, "From FNM", first)).toEqual({
      kind: "refused",
      message: "This deck already has a branch from-fnm.",
    });
    expect((await makeBranch(repo, STEM, "Main", first)).kind).toBe("refused");

    await save("from-fnm", swap(CARDS, "Brainstorm", "Opt"));
    const [, branch] = await repo.branches(STEM);
    expect(branch).toMatchObject({ name: "from-fnm", unmerged: 1 });
    // A tip read before the branch moved deletes nothing.
    await expect(
      repo.deleteBranch(STEM, "from-fnm", first),
    ).rejects.toBeInstanceOf(ConflictError);
    await repo.deleteBranch(STEM, "from-fnm", branch?.commit ?? "");
    expect((await repo.branches(STEM)).map((b) => b.name)).toEqual([MAIN]);
  });
});
