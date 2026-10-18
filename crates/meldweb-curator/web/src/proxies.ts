/**
 * Phyrexian Metamorph in the browser (ADR-0038): what a deck has left to
 * print, counted by `chip-decklist`'s `supply` as the CLI counts it, and the
 * sheet from `metamorph-sheet`. Scryfall's JPEGs are fetched here and handed
 * over as they are. Load `deck.ts` first; this module shares its wasm.
 */
import type { CardRef } from "./deck";
import type { SheetLayout, ToPrint } from "./deck.gen";
import { type Printings, printingKey } from "./scryfall";
import {
  proxies_per_page,
  proxies_to_print,
  proxy_sheet,
} from "./wasm/pkg/meldweb_wasm.js";

export type { Paper, SheetLayout, ToPrint } from "./deck.gen";

/** `metamorph`'s defaults: A4, no gap, a 5 mm margin, cut marks. */
export const DEFAULT_LAYOUT: SheetLayout = {
  paper: "a4",
  gapMm: 0,
  marginMm: 5,
  cutMarks: true,
};

/** Which sides of a card with two get a proxy. */
export type Sides = "both" | "front";

/**
 * The proxies the deck at `path` needs: all of it with no `collection`, or
 * less what that text owns, and with `noBasics`, less the basic lands.
 */
export function toPrint(
  path: string,
  text: string,
  collection: string | null,
  names: Readonly<Record<string, string>>,
  noBasics: boolean,
): ToPrint {
  return JSON.parse(
    proxies_to_print(
      path,
      text,
      collection ?? "",
      JSON.stringify(names),
      noBasics,
    ),
  ) as ToPrint;
}

/** Cards `layout` fits on a page; throws for a layout that fits none. */
export function perPage(layout: SheetLayout): number {
  return proxies_per_page(JSON.stringify(layout));
}

/**
 * Scryfall's `large` image of each side to print, front first. A flip card's
 * two halves share one picture, so it prints once.
 */
export function sideImages(
  printing: { image: string; back?: { image: string } },
  sides: Sides,
): string[] {
  const large = (normal: string) => normal.replace("/normal/", "/large/");
  const back = printing.back?.image;
  return sides === "both" && back && back !== printing.image
    ? [large(printing.image), large(back)]
    : [large(printing.image)];
}

export interface Proxy {
  name: string;
  qty: number;
  images: string[];
}

/**
 * Each line to print with its pictures, or the cards Scryfall has not
 * answered for, which stop the sheet: a missing proxy goes unnoticed until
 * the deck is sleeved.
 */
export function proxiesOf(
  lines: readonly { index: number; qty: number }[],
  cards: readonly { index: number; card: CardRef }[],
  printings: Printings,
  sides: Sides,
):
  | { kind: "proxies"; proxies: Proxy[] }
  | { kind: "unknown"; names: string[] } {
  const proxies: Proxy[] = [];
  const unknown: string[] = [];
  for (const line of lines) {
    const ref = cards.find((c) => c.index === line.index)?.card;
    const printing = ref && printings.get(printingKey(ref));
    if (!printing) {
      unknown.push(
        !ref
          ? `line ${line.index + 1}`
          : ref.kind === "name"
            ? ref.name
            : `${ref.set}/${ref.num}`,
      );
      continue;
    }
    proxies.push({
      name: printing.name,
      qty: line.qty,
      images: sideImages(printing, sides),
    });
  }
  return unknown.length > 0
    ? { kind: "unknown", names: unknown }
    : { kind: "proxies", proxies };
}

/** Same as `metamorph`: a fetch is nearly all round trip. */
const IMAGE_WORKERS = 8;

/**
 * The PDF of `proxies`, each image fetched once however many copies print.
 * `onFetched` counts the images as they arrive.
 */
export async function proxySheet(
  layout: SheetLayout,
  proxies: readonly Proxy[],
  onFetched: (done: number, of: number) => void = () => {},
): Promise<Uint8Array> {
  const links = [...new Set(proxies.flatMap((p) => p.images))];
  const bytes: Uint8Array[] = new Array(links.length);
  let next = 0;
  let done = 0;
  onFetched(0, links.length);
  const worker = async () => {
    while (next < links.length) {
      const i = next++;
      const link = links[i] as string;
      const res = await fetch(link);
      if (!res.ok) throw new Error(`fetching ${link}: ${res.status}`);
      bytes[i] = new Uint8Array(await res.arrayBuffer());
      onFetched(++done, links.length);
    }
  };
  await Promise.all(
    Array.from({ length: Math.min(IMAGE_WORKERS, links.length) }, worker),
  );

  const ends = new Uint32Array(links.length);
  let total = 0;
  bytes.forEach((b, i) => {
    total += b.length;
    ends[i] = total;
  });
  const all = new Uint8Array(total);
  bytes.forEach((b, i) => {
    all.set(b, (ends[i] as number) - b.length);
  });
  const at = new Map(links.map((l, i) => [l, i]));
  const order = proxies.flatMap((p) =>
    Array.from({ length: p.qty }, () =>
      p.images.map((l) => at.get(l) as number),
    ).flat(),
  );
  return proxy_sheet(
    JSON.stringify(layout),
    all,
    ends,
    Uint32Array.from(order),
  );
}
