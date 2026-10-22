import { beforeAll, describe, expect, it } from "vitest";
import { compareDecks, parseDeck } from "../deck";
import { memoryRepo } from "../repo/memory";
import { createDeck, deckFile } from "./decks";
import { deckText } from "./deckText";
import {
  deckSnapshots,
  fileAt,
  head,
  revisionAt,
  revisions,
  snapshotTag,
  takeSnapshot,
} from "./history";
import { fileOf, loadWasm, messages } from "./testkit";

// The deck's stem; its line is decks/lantern/main (ADR-0041).
const STEM = "lantern";
const FILE = deckFile(STEM);
const v = (n: number) =>
  `name = "Lantern"\ncards = [\n  { name = "Island", qty = ${n} },\n]\n`;
const day = (d: number) =>
  `2026-09-${String(d).padStart(2, "0")}T12:00:00.000Z`;

beforeAll(loadWasm);

/** Lantern saved on the 1st, 10th and 20th of September with 1, 2 and 3 Islands. */
async function lantern() {
  let clock = 0;
  const repo = memoryRepo(
    () => ({ name: "Alex", email: "alex@example.com" }),
    () => clock,
  );
  let sha: string | null = null;
  for (const [i, d] of [1, 10, 20].entries()) {
    clock = Date.parse(day(d));
    ({ sha } = await repo.putFile(FILE.path, {
      text: v(i + 1),
      message: `lantern: Island: ${i} → ${i + 1}`,
      sha,
      branch: FILE.branch,
    }));
  }
  clock = Date.parse(day(25));
  return { repo, sha };
}

describe("a deck's history", () => {
  it("is the commits that touched its path, newest first", async () => {
    const { repo } = await lantern();
    const log = await revisions(repo, STEM);
    expect(log.map((r) => r.message)).toEqual([
      "lantern: Island: 2 → 3",
      "lantern: Island: 1 → 2",
      "lantern: Island: 0 → 1",
    ]);
    expect(await revisions(repo, "loam")).toEqual([]);
  });

  it("answers how the deck looked on a day", async () => {
    const { repo } = await lantern();
    const at = await revisionAt(repo, STEM, new Date(day(15)));
    expect(at?.date).toBe(day(10));
    const file = at && (await fileAt(repo, STEM, at.commit));
    expect(file?.text).toBe(v(2));
    // The day of a save is that save's version.
    expect((await revisionAt(repo, STEM, new Date(day(20))))?.date).toBe(
      day(20),
    );
  });

  it("before the deck existed, there is no version of it", async () => {
    const { repo } = await lantern();
    expect(await revisionAt(repo, STEM, new Date("2026-08-01T00:00:00Z"))).toBe(
      null,
    );
  });

  it("grows by every save, and a version's file never changes", async () => {
    const { repo, sha } = await lantern();
    const [oldest] = (await revisions(repo, STEM)).slice(-1);
    await repo.putFile(FILE.path, {
      text: v(4),
      message: "four",
      sha,
      branch: FILE.branch,
    });
    const [latest] = await revisions(repo, STEM);
    expect(latest?.message).toBe("four");
    expect((await fileAt(repo, STEM, latest?.commit ?? ""))?.text).toBe(v(4));
    expect((await fileAt(repo, STEM, oldest?.commit ?? ""))?.text).toBe(v(1));
  });
});

describe("snapshots", () => {
  it("are tags under the deck's stem, listed with their name", async () => {
    const { repo } = await lantern();
    const at = await head(repo, STEM);
    const taken = await takeSnapshot(
      repo,
      STEM,
      "FNM 14 Sep",
      at?.commit ?? "",
    );
    expect(taken.kind).toBe("taken");
    const snaps = await deckSnapshots(repo, STEM);
    expect(snaps.map((s) => [s.tag, s.label, s.commit])).toEqual([
      ["decks/lantern/fnm-14-sep", "FNM 14 Sep", at?.commit],
    ]);
    // Another deck's snapshots are its own.
    expect(await deckSnapshots(repo, "lantern-budget")).toEqual([]);
  });

  it("refuses a name the deck already used", async () => {
    const { repo } = await lantern();
    const at = await head(repo, STEM);
    await takeSnapshot(repo, STEM, "Before the swap", at?.commit ?? "");
    const again = await takeSnapshot(
      repo,
      STEM,
      "before the swap!",
      at?.commit ?? "",
    );
    expect(again).toEqual({
      kind: "refused",
      message: "This deck already has a snapshot named before-the-swap.",
    });
  });

  it("without anything to slug, are named for the day", () => {
    expect(snapshotTag(STEM, "!!", new Date(day(14)))).toBe(
      "decks/lantern/2026-09-14",
    );
  });
});

describe("a fork", () => {
  it("is a new deck whose line starts on any version of the deck, which its history says", async () => {
    const { repo } = await lantern();
    const [, middle] = await revisions(repo, STEM);
    const old = await fileAt(repo, STEM, middle?.commit ?? "");
    const made = await createDeck(
      repo,
      "Lantern Budget",
      {
        kind: "fork",
        text: old?.text ?? "",
        of: STEM,
        at: middle?.commit ?? "",
      },
      deckText,
    );
    expect(made.kind).toBe("created");
    const budget = deckFile("lantern-budget");
    const text = (await fileOf(repo, budget.path, budget.branch)) ?? "";
    expect(parseDeck(text)).toMatchObject({ name: "Lantern Budget" });
    // It differs from the version it came from by its name alone.
    const diff = compareDecks(old?.text ?? "", text);
    expect(diff.kind === "diff" && diff.changes.map((c) => c.text)).toEqual([
      'name: "Lantern" → "Lantern Budget"',
    ]);
    expect(await messages(repo, budget.path, budget.branch)).toEqual([
      "lantern-budget: forked from lantern",
    ]);
    const lines = await repo.decks();
    expect(lines.map((d) => [d.stem, d.parent])).toEqual([
      ["lantern", undefined],
      ["lantern-budget", "lantern"],
    ]);
  });
});
