import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { loadDeckSync } from "../deck";
import { printingId } from "../scryfall";
import { rankPrintings } from "./preference";
import { parsePrintsPage } from "./prints";

beforeAll(() => {
  loadDeckSync(
    readFileSync(new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url)),
  );
});

const card = (set: string, num: string, released: string, extra = {}) => ({
  object: "card",
  id: `${set}/${num}`,
  name: "Heroic Intervention",
  type_line: "Instant",
  oracle_text:
    "Permanents you control gain hexproof and indestructible until end of turn.",
  lang: "en",
  set,
  set_name: set,
  collector_number: num,
  released_at: released,
  finishes: ["nonfoil"],
  frame: "2015",
  border_color: "black",
  games: ["paper"],
  ...extra,
});

const heroic = parsePrintsPage({
  data: [
    card("mar", "80", "2026-06-26", {
      border_color: "borderless",
      full_art: true,
      promo_types: ["sourcematerial", "universesbeyond"],
    }),
    card("pip", "202", "2024-03-08", { promo_types: ["universesbeyond"] }),
    card("cmm", "295", "2023-08-04"),
    card("omb", "34", "2025-09-23", { digital: true, games: ["mtgo"] }),
  ],
}).printings;

describe("the preferred order of a card's printings", () => {
  it("is the default rules' without a meldweb.toml, each with what moved it", () => {
    const order = rankPrintings(null, heroic);
    expect(order.kind).toBe("ranked");
    expect(order.ranked.map((r) => printingId(r.option))).toEqual([
      "cmm/295",
      "pip/202",
      "mar/80",
      "omb/34",
    ]);
    expect(order.ranked[2]?.matched.map((r) => r.query)).toEqual([
      "is:sourcematerial",
      "is:ub",
    ]);
    expect(order.kind === "ranked" && order.declared).toBe(false);
  });

  it("is the repo's own when it has a meldweb.toml", () => {
    const order = rankPrintings(
      '[printings]\nrank = [{ prefer = "is:fullart" }]\n',
      heroic,
    );
    expect(order.kind === "ranked" && order.declared).toBe(true);
    expect(order.ranked.map((r) => printingId(r.option))[0]).toBe("mar/80");
  });

  it("falls back to newest first and says why when the file is wrong", () => {
    const order = rankPrintings(
      '[printings]\nrank = [{ avoid = "lang:jp" }]\n',
      heroic,
    );
    expect(order.kind).toBe("refused");
    expect(order.kind === "refused" && order.message).toContain("rule 1");
    expect(order.ranked.map((r) => printingId(r.option))).toEqual([
      "mar/80",
      "omb/34",
      "pip/202",
      "cmm/295",
    ]);
  });
});
