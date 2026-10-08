import { beforeAll, describe, expect, it } from "vitest";
import { addOwned } from "../collection";
import { parseDeck, setDeckCover, setDeckMeta } from "../deck";
import { COLLECTION_PATH } from "./collection";
import {
  createDeck,
  deckFromArchidekt,
  deckPath,
  deleteDeck,
  listDecks,
  slugify,
} from "./decks";
import { type DeckText, deckText, type Imported } from "./deckText";
import { commitEdit } from "./repoFile";
import { createSaveStore, leave } from "./save";
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

describe("a deck from the list", () => {
  const LANTERN = "decks/lantern.deck.toml";

  it("is renamed and given a cover in a commit each, its path staying put", async () => {
    const { api, mock, repo } = mockConnection({ files: seedDecks() });
    await commitEdit(api, repo, LANTERN, (t) =>
      setDeckMeta(t ?? "", "Lantern Control"),
    );
    await commitEdit(api, repo, LANTERN, (t) =>
      setDeckCover(t ?? "", { set: "cmr", num: "304" }),
    );
    expect(mock.commits().map((c) => c.message)).toEqual([
      'lantern: name: none → "Lantern Control"',
      "lantern: cover: none → Codex Shredder",
    ]);
    const [lantern] = await listDecks(api, repo, { parseDeck });
    expect(lantern).toMatchObject({
      path: LANTERN,
      name: "Lantern Control",
      cover: { set: "cmr", num: "304" },
    });
  });

  it("is deleted as the list read it, and refused once it changed since", async () => {
    const { api, mock, repo } = mockConnection({ files: seedDecks() });
    const [lantern, loam] = await listDecks(api, repo, { parseDeck });
    if (!lantern || !loam) throw new Error("seeded");
    expect(await deleteDeck(api, repo, lantern)).toEqual({ kind: "deleted" });
    expect(mock.file(LANTERN)).toBeUndefined();
    expect(mock.commits().at(-1)?.message).toBe("lantern: delete");

    await commitEdit(api, repo, loam.path, (t) => setDeckMeta(t ?? "", "Loam"));
    expect(await deleteDeck(api, repo, loam)).toMatchObject({
      kind: "refused",
    });
    expect(mock.file(loam.path)).toBeDefined();
  });

  it("is not deleted while the collection keeps copies in it, and its empty place goes with it", async () => {
    const collection = (cards: string) =>
      `cards = [\n${cards}]\n\n[places]\nLantern = { deck = "${LANTERN}" }\n`;
    const full = mockConnection({
      files: {
        ...seedDecks(),
        "collection.toml": collection(
          '  { name = "Sol Ring", qty = 2, at = "Lantern" },\n',
        ),
      },
    });
    const [lantern] = await listDecks(full.api, full.repo, { parseDeck });
    if (!lantern) throw new Error("seeded");
    expect(await deleteDeck(full.api, full.repo, lantern)).toEqual({
      kind: "refused",
      message:
        "The collection has 2 copies in Lantern, this deck's place. Move them out in the collection first.",
    });
    expect(full.mock.commits()).toEqual([]);

    const empty = mockConnection({
      files: { ...seedDecks(), "collection.toml": collection("") },
    });
    const [again] = await listDecks(empty.api, empty.repo, { parseDeck });
    if (!again) throw new Error("seeded");
    expect(await deleteDeck(empty.api, empty.repo, again)).toEqual({
      kind: "deleted",
    });
    expect(empty.mock.file("collection.toml")?.text).not.toContain("Lantern");
    expect(empty.mock.commits().map((c) => c.path)).toEqual([
      "collection.toml",
      LANTERN,
    ]);
  });

  const places = `cards = []\n\n[places]\nBulk = {}\nLantern = { deck = "${LANTERN}" }\n`;

  it("is refused when copies are put in its place while it is being deleted", async () => {
    const { api, mock, repo } = mockConnection({
      files: { ...seedDecks(), [COLLECTION_PATH]: places },
    });
    const [lantern] = await listDecks(api, repo, { parseDeck });
    if (!lantern) throw new Error("seeded");
    let first = true;
    const racing = {
      ...api,
      getFile: async (...args: Parameters<typeof api.getFile>) => {
        const file = await api.getFile(...args);
        if (args[1] === COLLECTION_PATH && first) {
          first = false;
          mock.editOnGitHub(
            COLLECTION_PATH,
            addOwned(places, { kind: "name", name: "Sol Ring" }, "Lantern")
              .text,
          );
        }
        return file;
      },
    };
    expect(await deleteDeck(racing, repo, lantern)).toEqual({
      kind: "refused",
      message:
        "The collection has 1 copy in Lantern, this deck's place. Move them out in the collection first.",
    });
    expect(mock.file(LANTERN)).toBeDefined();
    expect(mock.file(COLLECTION_PATH)?.text).toContain("Lantern = ");
  });

  it("drops its place after the collection's own save lands, without a conflict", async () => {
    const { api, mock, repo } = mockConnection({
      files: { ...seedDecks(), [COLLECTION_PATH]: places },
    });
    const [lantern] = await listDecks(api, repo, { parseDeck });
    const loaded = mock.file(COLLECTION_PATH);
    if (!lantern || !loaded) throw new Error("seeded");
    const store = createSaveStore({
      api,
      repo,
      path: COLLECTION_PATH,
      ...loaded,
    });
    store.edit(
      addOwned(places, { kind: "name", name: "Sol Ring" }, "Bulk").text,
    );
    // The collection is left with its edit pending, as on going home.
    void leave(store);
    expect(await deleteDeck(api, repo, lantern)).toEqual({ kind: "deleted" });
    store.dispose();
    const collection = mock.file(COLLECTION_PATH)?.text ?? "";
    expect(collection).toContain("Sol Ring");
    expect(collection).not.toContain("Lantern");
    const puts = mock
      .requests()
      .filter((r) => r.method === "PUT" && r.url.includes(COLLECTION_PATH));
    expect(puts).toHaveLength(2);
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

  it("refuses a slug taken while it was being made, leaving that deck be", async () => {
    const { api, mock, repo } = mockConnection({ files: seedDecks() });
    const path = "decks/burn.deck.toml";
    const theirs = deckText.newDeck("Burn", "modern");
    let checked = false;
    const racing = {
      ...api,
      getFile: async (...args: Parameters<typeof api.getFile>) => {
        const file = await api.getFile(...args);
        if (args[1] === path && !checked) {
          checked = true;
          mock.editOnGitHub(path, theirs);
        }
        return file;
      },
    };
    const made = await createDeck(
      racing,
      repo,
      "Burn",
      { kind: "empty", format: "legacy" },
      fakeDeck(),
    );
    expect(made).toMatchObject({ kind: "refused" });
    expect(mock.file(path)?.text).toBe(theirs);
    expect(mock.commits().filter((c) => !c.outOfBand)).toEqual([]);
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
      {
        inSets: async (wanted) => {
          asked.push(...wanted.map(({ name, set }) => ({ name, set })));
          return wanted.map((w) =>
            w.set === "khm" ? { set: "khm", num: "3" } : null,
          );
        },
        printings: async () => new Map(),
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

describe("replacing a deck from Archidekt", () => {
  it("makes the paste the whole deck under the deck's own name and format", async () => {
    const made = await deckFromArchidekt(
      "1x Sol Ring [Ramp]\n1x Kellan, the Kid [Commander{top}]",
      "lantern",
      "commander",
      deckText,
      { inSets: async () => [], printings: async () => new Map() },
    );
    const parsed = made.kind === "deck" ? parseDeck(made.text) : made;
    expect(parsed).toMatchObject({
      kind: "deck",
      name: "lantern",
      format: "commander",
      total: 2,
    });
    expect(
      parsed.kind === "deck" && parsed.categories.map((c) => c.name).sort(),
    ).toEqual(["Commander", "Ramp"]);
  });
});

describe("an archidekt line with no category", () => {
  it("is filed under its front face's main type, as archidekt's import files it", async () => {
    const typed = (typeLine: string) => ({
      id: "",
      name: "",
      set: "",
      num: "",
      image: "",
      colorIdentity: [],
      typeLine,
    });
    const made = await deckFromArchidekt(
      [
        "1x Adarkar Wastes",
        "1x Invasion of Ixalan // Belligerent Regisaur",
        "1x Dryad Arbor",
        "1x Sol Ring [Ramp]",
        "1x Mystery Card",
      ].join("\n"),
      "Kellan",
      "commander",
      deckText,
      {
        inSets: async () => [],
        printings: async () =>
          new Map([
            ["name:adarkar wastes", typed("Land")],
            [
              "name:invasion of ixalan // belligerent regisaur",
              typed("Battle — Siege"),
            ],
            ["name:dryad arbor", typed("Land Creature — Forest Dryad")],
          ]),
      },
    );
    const parsed = made.kind === "deck" ? parseDeck(made.text) : made;
    expect(
      parsed.kind === "deck" &&
        parsed.cards.map((c) => [
          c.card.kind === "name" && c.card.name,
          c.categories,
        ]),
    ).toEqual([
      ["Adarkar Wastes", ["Land"]],
      ["Invasion of Ixalan // Belligerent Regisaur", ["Battle"]],
      ["Dryad Arbor", ["Land"]],
      ["Sol Ring", ["Ramp"]],
      ["Mystery Card", []],
    ]);
  });
});
