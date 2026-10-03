import { beforeAll, describe, expect, it } from "vitest";
import { compareDecks, parseDeck } from "../deck";
import { createDeck } from "./decks";
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
import { loadWasm, mockConnection } from "./testkit";

const PATH = "decks/lantern.deck.toml";
const v = (n: number) =>
  `name = "Lantern"\ncards = [\n  { name = "Island", qty = ${n} },\n]\n`;
const day = (d: number) => `2026-09-${String(d).padStart(2, "0")}T12:00:00Z`;

beforeAll(loadWasm);

/** Lantern saved on the 1st, 10th and 20th of September with 1, 2 and 3 Islands. */
async function lantern() {
  const conn = mockConnection({
    history: [1, 10, 20].map((d, i) => ({
      path: PATH,
      text: v(i + 1),
      message: `lantern: Island: ${i} → ${i + 1}`,
      date: day(d),
    })),
  });
  await conn.auth.token();
  return conn;
}

describe("a deck's history", () => {
  it("is the commits that touched its path, newest first", async () => {
    const { api, repo } = await lantern();
    const log = await revisions(api, repo, PATH);
    expect(log.map((r) => r.message)).toEqual([
      "lantern: Island: 2 → 3",
      "lantern: Island: 1 → 2",
      "lantern: Island: 0 → 1",
    ]);
    expect(await revisions(api, repo, "decks/loam.deck.toml")).toEqual([]);
  });

  it("answers how the deck looked on a day", async () => {
    const { api, repo } = await lantern();
    const at = await revisionAt(api, repo, PATH, new Date(day(15)));
    expect(at?.date).toBe(day(10));
    const file = at && (await fileAt(api, repo, PATH, at.commit));
    expect(file?.text).toBe(v(2));
    // The day of a save is that save's version.
    expect((await revisionAt(api, repo, PATH, new Date(day(20))))?.date).toBe(
      day(20),
    );
  });

  it("before the deck existed, there is no version of it", async () => {
    const { api, repo } = await lantern();
    expect(
      await revisionAt(api, repo, PATH, new Date("2026-08-01T00:00:00Z")),
    ).toBe(null);
  });

  it("grows by every save, and a version's file never changes", async () => {
    const { api, repo, mock } = await lantern();
    const [oldest] = (await revisions(api, repo, PATH)).slice(-1);
    const sha = mock.file(PATH)?.sha ?? null;
    await api.putFile(repo, PATH, { text: v(4), message: "four", sha });
    const [latest] = await revisions(api, repo, PATH);
    expect(latest?.message).toBe("four");
    expect((await fileAt(api, repo, PATH, latest?.commit ?? ""))?.text).toBe(
      v(4),
    );
    expect((await fileAt(api, repo, PATH, oldest?.commit ?? ""))?.text).toBe(
      v(1),
    );
  });
});

describe("snapshots", () => {
  it("are tags under the deck's path, listed with their name", async () => {
    const { api, repo } = await lantern();
    const at = await head(api, repo, PATH);
    const taken = await takeSnapshot(
      api,
      repo,
      PATH,
      "FNM 14 Sep",
      at?.commit ?? "",
    );
    expect(taken.kind).toBe("taken");
    const snaps = await deckSnapshots(api, repo, PATH);
    expect(snaps.map((s) => [s.tag, s.label, s.commit])).toEqual([
      ["decks/lantern/fnm-14-sep", "FNM 14 Sep", at?.commit],
    ]);
    // Another deck's snapshots are its own.
    expect(
      await deckSnapshots(api, repo, "decks/lantern-budget.deck.toml"),
    ).toEqual([]);
  });

  it("refuses a name the deck already used", async () => {
    const { api, repo } = await lantern();
    const at = await head(api, repo, PATH);
    await takeSnapshot(api, repo, PATH, "Before the swap", at?.commit ?? "");
    const again = await takeSnapshot(
      api,
      repo,
      PATH,
      "before the swap!",
      at?.commit ?? "",
    );
    expect(again).toEqual({
      kind: "refused",
      message: "This deck already has a snapshot named before-the-swap.",
    });
  });

  it("without anything to slug, are named for the day", () => {
    expect(snapshotTag(PATH, "!!", new Date(day(14)))).toBe(
      "decks/lantern/2026-09-14",
    );
  });
});

describe("a variant", () => {
  it("is a new deck that names its parent, from any version of it", async () => {
    const { api, repo, mock } = await lantern();
    const [, middle] = await revisions(api, repo, PATH);
    const old = await fileAt(api, repo, PATH, middle?.commit ?? "");
    const made = await createDeck(
      api,
      repo,
      "Lantern Budget",
      { kind: "variant", text: old?.text ?? "", of: PATH },
      deckText,
    );
    expect(made.kind).toBe("created");
    const path = "decks/lantern-budget.deck.toml";
    const text = mock.file(path)?.text ?? "";
    const parsed = parseDeck(text);
    expect(parsed.kind === "deck" && [parsed.name, parsed.variantOf]).toEqual([
      "Lantern Budget",
      PATH,
    ]);
    // It differs from the version it came from by what makes it a variant.
    const diff = compareDecks(old?.text ?? "", text);
    expect(diff.kind === "diff" && diff.changes.map((c) => c.text)).toEqual([
      'name: "Lantern" → "Lantern Budget"',
      "variant of: none → lantern",
    ]);
  });
});
