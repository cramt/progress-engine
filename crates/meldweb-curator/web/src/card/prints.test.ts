import { afterEach, describe, expect, it, vi } from "vitest";
import { printingId } from "../scryfall";
import { SEARCH_GATE } from "../scryfallQueue";
import {
  byRelease,
  fetchAllPrintings,
  filterBySet,
  type PrintingOption,
  parsePrintsPage,
} from "./prints";

const raw = (set: string, num: string, released: string, extra = {}) => ({
  object: "card",
  id: `${set}/${num}`,
  name: "Sol Ring",
  set,
  set_name: `Set ${set.toUpperCase()}`,
  collector_number: num,
  released_at: released,
  finishes: ["nonfoil", "foil"],
  image_uris: { small: `s/${set}`, normal: `n/${set}` },
  ...extra,
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("scryfall's printings", () => {
  it("read a page into set, number, set name, date, pictures and finishes", () => {
    const page = parsePrintsPage({
      data: [
        raw("CMR", "472", "2020-11-20"),
        raw("znr", "180", "2020-09-25", {
          image_uris: undefined,
          card_faces: [{ image_uris: { normal: "front" } }, {}],
          finishes: ["nonfoil", "glossy"],
        }),
        { name: "broken" },
      ],
      has_more: true,
      next_page: "https://api.scryfall.com/next",
    });
    expect(page.next).toBe("https://api.scryfall.com/next");
    expect(page.printings).toEqual([
      {
        id: "CMR/472",
        name: "Sol Ring",
        set: "cmr",
        num: "472",
        setName: "Set CMR",
        released: "2020-11-20",
        image: "n/CMR",
        small: "s/CMR",
        finishes: ["nonfoil", "foil"],
        facts: {
          name: "Sol Ring",
          set: "CMR",
          collector_number: "472",
          released_at: "2020-11-20",
        },
      },
      {
        id: "znr/180",
        name: "Sol Ring",
        set: "znr",
        num: "180",
        setName: "Set ZNR",
        released: "2020-09-25",
        image: "front",
        finishes: ["nonfoil"],
        facts: {
          name: "Sol Ring",
          set: "znr",
          collector_number: "180",
          released_at: "2020-09-25",
          card_faces: [{ image_uris: { normal: "front" } }, {}],
        },
      },
    ]);
  });

  it("keep what ranking reads and nothing it does not", () => {
    const [p] = parsePrintsPage({
      data: [
        raw("mar", "80", "2026-06-26", {
          promo_types: ["sourcematerial", "universesbeyond"],
          full_art: true,
          flavor_name: null,
          prices: { usd: "3.00" },
        }),
      ],
    }).printings;
    expect(p?.facts).toMatchObject({
      promo_types: ["sourcematerial", "universesbeyond"],
      full_art: true,
    });
    expect(p?.facts).not.toHaveProperty("prices");
    expect(p?.facts).not.toHaveProperty("flavor_name");
  });

  it("read which way each face is turned, and the back where there is one", () => {
    const turned = (extra: object) => {
      const [p] = parsePrintsPage({
        data: [raw("x", "1", "2023-04-21", extra)],
      }).printings;
      return { turn: p?.turn, back: p?.back };
    };
    const dfc = {
      image_uris: undefined,
      card_faces: [
        { image_uris: { normal: "front" } },
        { image_uris: { normal: "back" } },
      ],
    };
    expect(
      turned({
        layout: "transform",
        ...dfc,
        card_faces: [
          { type_line: "Battle — Siege", image_uris: { normal: "front" } },
          { type_line: "Creature — Angel", image_uris: { normal: "back" } },
        ],
      }),
    ).toEqual({
      turn: "sideways",
      back: { image: "back", turn: "upright" },
    });
    expect(turned({ layout: "modal_dfc", ...dfc })).toEqual({
      turn: undefined,
      back: { image: "back", turn: "upright" },
    });
    expect(turned({ layout: "split" })).toEqual({
      turn: "sideways",
      back: undefined,
    });
    expect(turned({ layout: "split", keywords: ["Aftermath"] })).toEqual({
      turn: undefined,
      back: undefined,
    });
    expect(turned({ layout: "flip" })).toEqual({
      turn: undefined,
      back: { image: "n/x", turn: "upside-down" },
    });
    expect(turned({ layout: "normal" })).toEqual({
      turn: undefined,
      back: undefined,
    });
  });

  it("order by release date, newest first unless asked otherwise, and filter by set name", () => {
    const list = parsePrintsPage({
      data: [
        raw("lea", "270", "1993-08-05"),
        raw("cmr", "472", "2020-11-20"),
        raw("c21", "263", "2021-04-23"),
      ],
    }).printings;
    expect(byRelease(list).map(printingId)).toEqual([
      "c21/263",
      "cmr/472",
      "lea/270",
    ]);
    expect(byRelease(list, true).map(printingId)).toEqual([
      "lea/270",
      "cmr/472",
      "c21/263",
    ]);
    expect(filterBySet(list, "set c").map(printingId)).toEqual([
      "cmr/472",
      "c21/263",
    ]);
    expect(filterBySet(list, "LEA")).toHaveLength(1);
    expect(filterBySet(list, "  ")).toHaveLength(3);
  });

  it("are every page of one search, fetched at scryfall's 2 a second", async () => {
    vi.useFakeTimers();
    const started: number[] = [];
    const pages: Record<string, unknown> = {
      "https://api/prints": {
        data: [raw("cmr", "472", "2020-11-20")],
        has_more: true,
        next_page: "https://api/prints?page=2",
      },
      "https://api/prints?page=2": {
        data: [raw("lea", "270", "1993-08-05")],
        has_more: false,
      },
    };
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) => {
        started.push(Date.now());
        return new Response(JSON.stringify(pages[url]));
      }),
    );
    const done = fetchAllPrintings("https://api/prints");
    await vi.runAllTimersAsync();
    const all: PrintingOption[] = await done;
    expect(all.map(printingId)).toEqual(["cmr/472", "lea/270"]);
    expect(started).toHaveLength(2);
    expect((started[1] ?? 0) - (started[0] ?? 0)).toBeGreaterThanOrEqual(
      SEARCH_GATE.intervalMs,
    );
    // Asked again, it is answered without a request.
    expect(await fetchAllPrintings("https://api/prints")).toBe(all);
    expect(started).toHaveLength(2);
  });

  it("are still fetched for one view when another view of the card gives up", async () => {
    vi.useFakeTimers();
    let requests = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        init?.signal?.throwIfAborted();
        requests++;
        return new Response(
          JSON.stringify({ data: [raw("m21", "1", "2020-07-03")] }),
        );
      }),
    );
    const leaving = new AbortController();
    const left = fetchAllPrintings("https://api/shared", leaving.signal).then(
      () => "done",
      () => "aborted",
    );
    const staying = fetchAllPrintings(
      "https://api/shared",
      new AbortController().signal,
    );
    leaving.abort();
    await vi.runAllTimersAsync();
    expect(await left).toBe("aborted");
    expect((await staying).map(printingId)).toEqual(["m21/1"]);
    expect(requests).toBe(1);
  });
});
