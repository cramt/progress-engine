import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { loadDecklistSync, parseDecklist } from "./decklist";

const repo = new URL("../../../../", import.meta.url);

beforeAll(() => {
  loadDecklistSync(
    readFileSync(
      new URL(
        "src/wasm/pkg/meldweb_wasm_bg.wasm",
        new URL("../", import.meta.url),
      ),
    ),
  );
});

describe("the parser, through wasm", () => {
  it("reads lantern.txt as Gauntlet does: 100 cards, Rashmi commanding", () => {
    const parsed = parseDecklist(
      readFileSync(new URL("decks/lantern.txt", repo), "utf8"),
    );
    if (parsed.kind !== "deck") throw new Error(parsed.message);
    expect(parsed.total).toBe(100);
    expect(
      parsed.entries.filter((e) => e.commander).map((e) => e.name),
    ).toEqual(["Rashmi and Ragavan"]);
  });

  it("omits an absent printing rather than sending null", () => {
    const parsed = parseDecklist("1 Sol Ring\n");
    if (parsed.kind !== "deck") throw new Error(parsed.message);
    expect(parsed.entries[0]).not.toHaveProperty("set");
  });

  it("refuses a bad line by number", () => {
    expect(parseDecklist("1 Sol Ring\n0 Island\n")).toMatchObject({
      kind: "refused",
      line: 2,
    });
  });
});
