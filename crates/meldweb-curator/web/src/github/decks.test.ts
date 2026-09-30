import { beforeAll, describe, expect, it } from "vitest";
import { parseDeck } from "../deck";
import { createDeck, deckPath, listDecks, slugify } from "./decks";
import { type DeckText, deckText, type Imported } from "./deckText";
import { loadWasm, mockConnection, seedDecks } from "./testkit";

beforeAll(loadWasm);

// Everything is the real wasm but the import, which is another agent's and
// stands in with the contract's typed shape.
const fakeDeck = (imported?: Imported): DeckText => ({
  ...deckText,
  importArchidekt: () =>
    imported ?? { kind: "refused", message: "no import in this test" },
});

describe("the deck list", () => {
  it("lists lantern and loam by name, the stem where the file names none", async () => {
    const { api, repo } = mockConnection({
      files: { ...seedDecks(), "decks/notes.md": "x", "README.md": "y" },
    });
    const decks = await listDecks(api, repo, { parseDeck });
    expect(decks.map((d) => [d.path, d.name, d.refused])).toEqual([
      ["decks/lantern.deck.toml", "lantern", undefined],
      ["decks/loam.deck.toml", "loam", undefined],
    ]);
    expect(decks[0]?.sha).toMatch(/^[0-9a-f]{40}$/);
  });

  it("carries each deck's format, size and commanders, for its tile", async () => {
    const { api, repo } = mockConnection({
      files: {
        ...seedDecks(),
        "decks/burn.deck.toml": deckText.newDeck("Burn", "modern"),
      },
    });
    const [burn, lantern, loam] = await listDecks(api, repo, { parseDeck });
    expect([burn?.format, burn?.total, burn?.commanders]).toEqual([
      "modern",
      0,
      [],
    ]);
    // The committed decks declare no format.
    expect([lantern?.format, lantern?.total]).toEqual([undefined, 100]);
    expect(loam?.total).toBe(99);
    expect(lantern?.commanders).toEqual([
      { kind: "printing", set: "moc", num: "94" },
    ]);
    expect(loam?.commanders).toHaveLength(1);
  });

  it("is empty for a repo with no decks directory", async () => {
    const { api, repo } = mockConnection({ files: { VERSION: "1\n" } });
    expect(await listDecks(api, repo, { parseDeck })).toEqual([]);
  });

  it("names a file the format refuses, with why", async () => {
    const { api, repo } = mockConnection({
      files: { "decks/bad.deck.toml": "cards = 3\n" },
    });
    const [bad] = await listDecks(api, repo, { parseDeck });
    expect(bad?.name).toBe("bad");
    expect(bad?.refused).toBeTruthy();
  });
});

describe("a deck's path", () => {
  it("is slugged from its name", () => {
    expect(slugify("Rashmi's Lantern")).toBe("rashmi-s-lantern");
    expect(slugify("  Lórien — Loam!! ")).toBe("lorien-loam");
    expect(slugify("!!!")).toBe("");
    expect(deckPath("Lantern")).toBe("decks/lantern.deck.toml");
  });
});

describe("a new deck", () => {
  it("starts empty, as its first commit", async () => {
    const { api, mock, repo } = mockConnection({ files: seedDecks() });
    const made = await createDeck(
      api,
      repo,
      "Sol Ring Tribal",
      { kind: "empty", format: "commander" },
      fakeDeck(),
    );
    expect(made).toMatchObject({
      kind: "created",
      path: "decks/sol-ring-tribal.deck.toml",
    });
    const file = mock.file("decks/sol-ring-tribal.deck.toml");
    expect(file?.text).toContain('name = "Sol Ring Tribal"');
    expect(made.kind === "created" && made.sha).toBe(file?.sha);
    // The first save's message is #124's diff against nothing.
    expect(mock.commits().at(-1)?.message).toBe(
      'sol-ring-tribal: name: none → "Sol Ring Tribal", format: none → commander',
    );
    const parsed = parseDeck(file?.text ?? "");
    expect(parsed).toMatchObject({
      kind: "deck",
      name: "Sol Ring Tribal",
      total: 0,
    });
  });

  it("refuses a slug that already exists, and writes nothing", async () => {
    const { api, mock, repo } = mockConnection({ files: seedDecks() });
    const made = await createDeck(
      api,
      repo,
      "LANTERN",
      { kind: "empty", format: "commander" },
      fakeDeck(),
    );
    expect(made.kind).toBe("refused");
    if (made.kind === "refused")
      expect(made.message).toMatch(/lantern\.deck\.toml already exists/);
    expect(mock.commits()).toHaveLength(0);
  });

  it("refuses a name with nothing to slug", async () => {
    const { api, repo } = mockConnection();
    const made = await createDeck(
      api,
      repo,
      "???",
      { kind: "empty", format: "" },
      fakeDeck(),
    );
    expect(made.kind).toBe("refused");
  });

  it("from Archidekt text takes the import's toml, named, and keeps its unreadable lines", async () => {
    const { api, mock, repo } = mockConnection();
    const toml =
      'cards = [\n  { name = "Sol Ring", in = ["Ramp"] },\n]\n\n[categories]\nRamp = {}\n';
    const unreadable = [{ line: 3, text: "banana", reason: "no quantity" }];
    const made = await createDeck(
      api,
      repo,
      "Ramp Pile",
      {
        kind: "archidekt",
        text: "1x Sol Ring [Ramp]\nbanana",
        format: "commander",
      },
      fakeDeck({ kind: "imported", toml, unreadable, setOnly: [] }),
    );
    expect(made).toMatchObject({ kind: "created", unreadable });
    const text = mock.file("decks/ramp-pile.deck.toml")?.text ?? "";
    expect(parseDeck(text)).toMatchObject({
      kind: "deck",
      name: "Ramp Pile",
      format: "commander",
      total: 1,
    });
    expect(text.endsWith(toml)).toBe(true);
  });

  it("names a card whose line gave only a set by the printing Scryfall has there, and says which it could not", async () => {
    const { api, mock, repo } = mockConnection();
    const asked: { name: string; set: string }[] = [];
    const made = await createDeck(
      api,
      repo,
      "Sets Only",
      {
        kind: "archidekt",
        text: "1x Doomskar (khm) [Board Wipe]\n1x Sol Ring (zzz) [Ramp]",
        format: "commander",
      },
      deckText,
      async (wanted) => {
        asked.push(...wanted.map(({ name, set }) => ({ name, set })));
        return wanted.map((w) =>
          w.set === "khm" ? { set: "khm", num: "3" } : null,
        );
      },
    );
    expect(asked).toEqual([
      { name: "Doomskar", set: "khm" },
      { name: "Sol Ring", set: "zzz" },
    ]);
    expect(made).toMatchObject({
      kind: "created",
      unreadable: [{ line: 2, text: "1x Sol Ring (zzz) [Ramp]" }],
    });
    const parsed = parseDeck(
      mock.file("decks/sets-only.deck.toml")?.text ?? "",
    );
    expect(parsed.kind === "deck" && parsed.cards.map((c) => c.card)).toEqual([
      { kind: "printing", set: "khm", num: "3" },
      { kind: "name", name: "Sol Ring" },
    ]);
  });

  it("says why an import was refused, and writes nothing", async () => {
    const { api, mock, repo } = mockConnection();
    const made = await createDeck(
      api,
      repo,
      "Nothing",
      { kind: "archidekt", text: "" },
      fakeDeck({ kind: "refused", message: "no card lines" }),
    );
    expect(made).toEqual({ kind: "refused", message: "no card lines" });
    expect(mock.commits()).toHaveLength(0);
  });
});
