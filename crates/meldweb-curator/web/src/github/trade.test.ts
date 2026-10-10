import { beforeAll, describe, expect, it } from "vitest";
import { tradeText } from "../trade/TradeView";
import { readTrade } from "../wanted";
import { loadWasm, mockConnection } from "./testkit";
import { loadTheirCollection, parseLogin } from "./trade";

beforeAll(loadWasm);

const THEIRS = `cards = [
  { name = "Sol Ring", at = "Gitrog" },
  { printing = "ltr/451", finish = "foil", at = "Trade binder" },  # The One Ring
  { name = "Sol Ring", at = "Trade binder" },
]

[places]
"Trade binder" = {}
Gitrog = { deck = "decks/gitrog.deck.toml" }
`;

describe("a GitHub login", () => {
  it("is letters, digits and single inner hyphens, an @ allowed in front", () => {
    expect(parseLogin(" @rival-2 ")).toBe("rival-2");
    for (const bad of [
      "",
      "-rival",
      "rival-",
      "ri--val",
      "a/b",
      "x".repeat(40),
    ]) {
      expect(parseLogin(bad)).toBeNull();
    }
  });
});

describe("their collection", () => {
  it("is read from their public repo without the login's token", async () => {
    const { api, mock } = mockConnection({
      others: { Rival: { "collection.toml": THEIRS } },
    });
    const who = parseLogin("rival");
    if (!who) throw new Error("rival is a login");
    expect(await loadTheirCollection(api, who)).toMatchObject({
      kind: "file",
      text: THEIRS,
    });
    expect(mock.requests().at(-1)?.url).toBe(
      "https://api.github.com/repos/rival/mtg/contents/collection.toml",
    );
  });

  it("says whether the repo or only the file is missing", async () => {
    const { api } = mockConnection({ others: { empty: {} } });
    const read = (login: string) => {
      const who = parseLogin(login);
      if (!who) throw new Error(`${login} is a login`);
      return loadTheirCollection(api, who);
    };
    expect(await read("empty")).toEqual({ kind: "no-file" });
    expect(await read("nobody")).toEqual({ kind: "no-repo" });
  });
});

describe("a trade", () => {
  it("brings what is outside their decks first, and reads as a list by place", () => {
    const trade = readTrade(
      `cards = [{ name = "Sol Ring" }, { name = "The One Ring" }]`,
      "",
      [],
      THEIRS,
    );
    if (trade.kind !== "trade") throw new Error(JSON.stringify(trade));
    expect(tradeText("me", "rival", trade.offers)).toBe(
      "Cards of rival's that me wants:\n\nTrade binder\n1 Sol Ring\n1 The One Ring (LTR 451, foil)\n",
    );
  });

  it("for one deck wants only what it lacks, and says so when copied", () => {
    const decks = [
      { path: "decks/a.deck.toml", text: `cards = [{ name = "Sol Ring" }]\n` },
      { path: "decks/b.deck.toml", text: `cards = [{ name = "Lotus" }]\n` },
    ];
    const trade = readTrade(
      `cards = [{ name = "The One Ring" }]`,
      "",
      decks,
      THEIRS,
      "decks/a.deck.toml",
    );
    if (trade.kind !== "trade") throw new Error(JSON.stringify(trade));
    expect(tradeText("me", "rival", trade.offers, "Brew")).toBe(
      "Cards of rival's that me wants for Brew:\n\nTrade binder\n1 Sol Ring\n",
    );
  });

  it("refuses their collection when it cannot be read, and says whose", () => {
    expect(readTrade("", "", [], "cards = 3")).toMatchObject({
      kind: "refused",
      message: expect.stringMatching(/^their collection: /),
    });
  });
});
