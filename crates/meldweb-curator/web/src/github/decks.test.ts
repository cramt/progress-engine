import { beforeAll, describe, expect, it } from "vitest";
import { addOwned } from "../collection";
import { parseDeck, setDeckCover, setDeckMeta } from "../deck";
import { COLLECTION_PATH } from "./collection";
import {
  createDeck,
  deckBranchFor,
  deckFile,
  deckFromArchidekt,
  deleteDeck,
  listDecks,
  slugify,
} from "./decks";
import { type DeckText, deckText, type Imported } from "./deckText";
import { commitEdit } from "./repoFile";
import { createSaveStore, leave } from "./save";
import {
  fileOf,
  loadWasm,
  messages,
  mockConnection,
  seedDecks,
} from "./testkit";

beforeAll(loadWasm);

// Everything is the real wasm but the import, which is another agent's and
// stands in with the contract's typed shape.
const fakeDeck = (imported?: Imported): DeckText => ({
  ...deckText,
  importArchidekt: () =>
    imported ?? { kind: "refused", message: "no import in this test" },
});

describe("the deck list", () => {
  it("lists lantern and loam, each a line of its own, by name, the stem where the file names none", async () => {
    const { repo } = mockConnection({
      files: { ...seedDecks(), "notes.md": "x", "README.md": "y" },
    });
    const decks = await listDecks(repo, { parseDeck });
    expect(decks.map((d) => [d.stem, d.name, d.refused])).toEqual([
      ["lantern", "lantern", undefined],
      ["loam", "loam", undefined],
    ]);
    expect(decks[0]?.sha).toMatch(/^[0-9a-f]{40}$/);
    expect(decks[0]?.commit).toMatch(/^[0-9a-f]{40}$/);
    expect(decks[0]?.parent).toBeUndefined();
  });

  it("carries each deck's format, size and commanders, for its tile", async () => {
    const { repo } = mockConnection({
      files: {
        ...seedDecks(),
        "decks/burn.deck.toml": deckText.newDeck("Burn", "modern"),
      },
    });
    const [burn, lantern, loam] = await listDecks(repo, { parseDeck });
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

  it("is empty for a repo with no deck lines", async () => {
    const { repo } = mockConnection({ files: { VERSION: "1\n" } });
    expect(await listDecks(repo, { parseDeck })).toEqual([]);
  });

  it("names a file the format refuses, with why", async () => {
    const { repo } = mockConnection({
      files: { "decks/bad.deck.toml": "cards = 3\n" },
    });
    const [bad] = await listDecks(repo, { parseDeck });
    expect(bad?.name).toBe("bad");
    expect(bad?.refused).toBeTruthy();
  });
});

describe("a deck from the list", () => {
  const LANTERN = deckFile("lantern");

  it("is renamed and given a cover in a commit each on its own line, its stem staying put", async () => {
    const { repo } = mockConnection({ files: seedDecks() });
    await commitEdit(
      repo,
      LANTERN.path,
      (t) => setDeckMeta(t ?? "", "Lantern Control"),
      LANTERN.branch,
    );
    await commitEdit(
      repo,
      LANTERN.path,
      (t) => setDeckCover(t ?? "", { set: "cmr", num: "304" }),
      LANTERN.branch,
    );
    expect(await messages(repo, LANTERN.path, LANTERN.branch)).toEqual([
      "add decks/lantern.deck.toml",
      'lantern: name: none → "Lantern Control"',
      "lantern: cover: none → Codex Shredder",
    ]);
    // main has none of it.
    expect(await fileOf(repo, LANTERN.path)).toBeUndefined();
    const [lantern] = await listDecks(repo, { parseDeck });
    expect(lantern).toMatchObject({
      stem: "lantern",
      name: "Lantern Control",
      cover: { set: "cmr", num: "304" },
    });
  });

  it("is deleted as the list read it, branches and snapshots, and refused once it changed since", async () => {
    const { repo } = mockConnection({ files: seedDecks() });
    const [lantern, loam] = await listDecks(repo, { parseDeck });
    if (!lantern || !loam) throw new Error("seeded");
    await repo.takeSnapshot({
      tag: "decks/lantern/fnm",
      label: "FNM",
      commit: lantern.commit,
    });
    expect(await deleteDeck(repo, lantern)).toEqual({ kind: "deleted" });
    expect(await fileOf(repo, LANTERN.path, LANTERN.branch)).toBeUndefined();
    expect(await repo.snapshots("decks/lantern/")).toEqual([]);
    expect((await repo.decks()).map((d) => d.stem)).toEqual(["loam"]);

    const LOAM = deckFile("loam");
    await commitEdit(
      repo,
      LOAM.path,
      (t) => setDeckMeta(t ?? "", "Loam"),
      LOAM.branch,
    );
    expect(await deleteDeck(repo, loam)).toMatchObject({
      kind: "refused",
    });
    expect(await fileOf(repo, LOAM.path, LOAM.branch)).toBeDefined();
  });

  it("is not deleted while the collection keeps copies in it, and its empty place goes with it", async () => {
    const collection = (cards: string) =>
      `cards = [\n${cards}]\n\n[places]\nLantern = { deck = "lantern" }\n`;
    const full = mockConnection({
      files: {
        ...seedDecks(),
        "collection.toml": collection(
          '  { name = "Sol Ring", qty = 2, at = "Lantern" },\n',
        ),
      },
    });
    const [lantern] = await listDecks(full.repo, { parseDeck });
    if (!lantern) throw new Error("seeded");
    expect(await deleteDeck(full.repo, lantern)).toEqual({
      kind: "refused",
      message:
        "The collection has 2 copies in Lantern, this deck's place. Move them out in the collection first.",
    });
    expect(await messages(full.repo, COLLECTION_PATH)).toEqual([
      "add collection.toml",
    ]);
    expect(await fileOf(full.repo, LANTERN.path, LANTERN.branch)).toBeDefined();

    const empty = mockConnection({
      files: { ...seedDecks(), "collection.toml": collection("") },
    });
    const [again] = await listDecks(empty.repo, { parseDeck });
    if (!again) throw new Error("seeded");
    expect(await deleteDeck(empty.repo, again)).toEqual({
      kind: "deleted",
    });
    expect(await fileOf(empty.repo, COLLECTION_PATH)).not.toContain("Lantern");
    expect(await messages(empty.repo, COLLECTION_PATH)).toEqual([
      "add collection.toml",
      "collection: -place lantern",
    ]);
    expect(await empty.repo.decks()).toHaveLength(1);
  });

  const places = `cards = []\n\n[places]\nBulk = {}\nLantern = { deck = "lantern" }\n`;

  it("is refused when copies are put in its place while it is being deleted", async () => {
    const { repo } = mockConnection({
      files: { ...seedDecks(), [COLLECTION_PATH]: places },
    });
    const [lantern] = await listDecks(repo, { parseDeck });
    if (!lantern) throw new Error("seeded");
    let first = true;
    const racing = {
      ...repo,
      getFile: async (...args: Parameters<typeof repo.getFile>) => {
        const file = await repo.getFile(...args);
        if (args[0] === COLLECTION_PATH && first && file) {
          first = false;
          // Another tab moves copies into the place meanwhile.
          await repo.putFile(COLLECTION_PATH, {
            text: addOwned(
              places,
              { kind: "name", name: "Sol Ring" },
              "Lantern",
            ).text,
            message: "elsewhere",
            sha: file.sha,
          });
        }
        return file;
      },
      putFile: repo.putFile,
      deleteDeck: repo.deleteDeck,
    };
    expect(await deleteDeck(racing, lantern)).toEqual({
      kind: "refused",
      message:
        "The collection has 1 copy in Lantern, this deck's place. Move them out in the collection first.",
    });
    expect(await fileOf(repo, LANTERN.path, LANTERN.branch)).toBeDefined();
    expect(await fileOf(repo, COLLECTION_PATH)).toContain("Lantern = ");
  });

  it("drops its place after the collection's own save lands, without a conflict", async () => {
    const { repo } = mockConnection({
      files: { ...seedDecks(), [COLLECTION_PATH]: places },
    });
    const [lantern] = await listDecks(repo, { parseDeck });
    const loaded = await repo.getFile(COLLECTION_PATH);
    if (!lantern || !loaded) throw new Error("seeded");
    const store = createSaveStore({
      repo,
      path: COLLECTION_PATH,
      ...loaded,
    });
    store.edit(
      addOwned(places, { kind: "name", name: "Sol Ring" }, "Bulk").text,
    );
    // The collection is left with its edit pending, as on going home.
    void leave(store);
    expect(await deleteDeck(repo, lantern)).toEqual({ kind: "deleted" });
    store.dispose();
    const collection = (await fileOf(repo, COLLECTION_PATH)) ?? "";
    expect(collection).toContain("Sol Ring");
    expect(collection).not.toContain("Lantern");
    // The edit, then the place dropped on top of it: no conflict between.
    expect(await messages(repo, COLLECTION_PATH)).toEqual([
      "add collection.toml",
      "collection: +1 Sol Ring to bulk",
      "collection: -place lantern",
    ]);
  });
});

describe("a deck's stem", () => {
  it("is slugged from its name, and names its line and its file", () => {
    expect(slugify("Rashmi's Lantern")).toBe("rashmi-s-lantern");
    expect(slugify("  Lórien — Loam!! ")).toBe("lorien-loam");
    expect(slugify("!!!")).toBe("");
    expect(deckBranchFor("Lantern")).toBe("decks/lantern/main");
    expect(deckFile("lantern")).toEqual({
      path: "lantern.deck.toml",
      branch: "decks/lantern/main",
    });
  });
});

describe("a new deck", () => {
  it("starts empty, as the first commit of its own line", async () => {
    const { repo } = mockConnection({ files: seedDecks() });
    const made = await createDeck(
      repo,
      "Sol Ring Tribal",
      { kind: "empty", format: "commander" },
      fakeDeck(),
    );
    expect(made).toMatchObject({ kind: "created", stem: "sol-ring-tribal" });
    const { path, branch } = deckFile("sol-ring-tribal");
    const file = await repo.getFile(path, branch);
    expect(file?.text).toContain('name = "Sol Ring Tribal"');
    expect(made.kind === "created" && made.sha).toBe(file?.sha);
    // The first save's message is #124's diff against nothing.
    expect(await messages(repo, path, branch)).toEqual([
      'sol-ring-tribal: name: none → "Sol Ring Tribal", format: none → commander',
    ]);
    const parsed = parseDeck(file?.text ?? "");
    expect(parsed).toMatchObject({
      kind: "deck",
      name: "Sol Ring Tribal",
      total: 0,
    });
  });

  it("refuses a slug that already exists, and writes nothing", async () => {
    const { repo } = mockConnection({ files: seedDecks() });
    const before = (await repo.decks()).map((d) => d.commit);
    const made = await createDeck(
      repo,
      "LANTERN",
      { kind: "empty", format: "commander" },
      fakeDeck(),
    );
    expect(made.kind).toBe("refused");
    if (made.kind === "refused")
      expect(made.message).toMatch(/a deck lantern already/);
    expect((await repo.decks()).map((d) => d.commit)).toEqual(before);
  });

  it("refuses a slug taken while it was being made, leaving that deck be", async () => {
    const { repo } = mockConnection({ files: seedDecks() });
    const { path, branch } = deckFile("burn");
    const theirs = deckText.newDeck("Burn", "modern");
    let checked = false;
    const racing = {
      ...repo,
      getFile: async (...args: Parameters<typeof repo.getFile>) => {
        const file = await repo.getFile(...args);
        if (args[0] === path && !checked) {
          checked = true;
          // Another tab makes it first.
          await repo.putFile(path, {
            text: theirs,
            message: "burn: made elsewhere",
            sha: null,
            branch,
          });
        }
        return file;
      },
      decks: repo.decks,
      putFile: repo.putFile,
    };
    const made = await createDeck(
      racing,
      "Burn",
      { kind: "empty", format: "legacy" },
      fakeDeck(),
    );
    expect(made).toMatchObject({ kind: "refused" });
    expect(await fileOf(repo, path, branch)).toBe(theirs);
    expect(await messages(repo, path, branch)).toEqual([
      "burn: made elsewhere",
    ]);
  });

  it("refuses a name with nothing to slug", async () => {
    const { repo } = mockConnection();
    const made = await createDeck(
      repo,
      "???",
      { kind: "empty", format: "" },
      fakeDeck(),
    );
    expect(made.kind).toBe("refused");
  });

  it("from Archidekt text takes the import's toml, named, and keeps its unreadable lines", async () => {
    const { repo } = mockConnection();
    const toml =
      'cards = [\n  { name = "Sol Ring", in = ["Ramp"] },\n]\n\n[categories]\nRamp = {}\n';
    const unreadable = [{ line: 3, text: "banana", reason: "no quantity" }];
    const made = await createDeck(
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
    const ramp = deckFile("ramp-pile");
    const text = (await fileOf(repo, ramp.path, ramp.branch)) ?? "";
    expect(parseDeck(text)).toMatchObject({
      kind: "deck",
      name: "Ramp Pile",
      format: "commander",
      total: 1,
    });
    expect(text.endsWith(toml)).toBe(true);
  });

  it("names a card whose line gave only a set by the printing Scryfall has there, and says which it could not", async () => {
    const { repo } = mockConnection();
    const asked: { name: string; set: string }[] = [];
    const made = await createDeck(
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
    const sets = deckFile("sets-only");
    const parsed = parseDeck(
      (await fileOf(repo, sets.path, sets.branch)) ?? "",
    );
    expect(parsed.kind === "deck" && parsed.cards.map((c) => c.card)).toEqual([
      { kind: "printing", set: "khm", num: "3" },
      { kind: "name", name: "Sol Ring" },
    ]);
  });

  it("says why an import was refused, and writes nothing", async () => {
    const { repo } = mockConnection();
    const made = await createDeck(
      repo,
      "Nothing",
      { kind: "archidekt", text: "" },
      fakeDeck({ kind: "refused", message: "no card lines" }),
    );
    expect(made).toEqual({ kind: "refused", message: "no card lines" });
    expect(await repo.decks()).toEqual([]);
  });
});

describe("a fork", () => {
  it("is a deck whose line starts on the commit it was forked from, and the list says whose", async () => {
    const { repo } = mockConnection({ files: seedDecks() });
    const [lantern] = await listDecks(repo, { parseDeck });
    if (!lantern) throw new Error("seeded");
    const made = await createDeck(
      repo,
      "Lantern (budget)",
      { kind: "fork", text: lantern.text, of: "lantern", at: lantern.commit },
      fakeDeck(),
    );
    expect(made).toMatchObject({ kind: "created", stem: "lantern-budget" });
    const budget = deckFile("lantern-budget");
    expect(await messages(repo, budget.path, budget.branch)).toEqual([
      "lantern-budget: forked from lantern",
    ]);
    const text = (await fileOf(repo, budget.path, budget.branch)) ?? "";
    expect(parseDeck(text)).toMatchObject({ name: "Lantern (budget)" });
    const decks = await listDecks(repo, { parseDeck });
    expect(decks.map((d) => [d.stem, d.parent])).toEqual([
      ["lantern", undefined],
      ["lantern-budget", "lantern"],
      ["loam", undefined],
    ]);
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
