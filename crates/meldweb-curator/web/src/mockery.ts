/**
 * Mirage Mockery on the custom cards page (ADR-0039): `mockery-core` compiled
 * to its own wasm, apart from `meldweb-wasm`, because Typst and its fonts
 * are most of its size and only this page needs them. Call `loadMockery`
 * before anything else here.
 */
import type {
  CardSheet,
  Checked,
  Form,
  Opened,
  SheetCard,
} from "./mockery.gen";
import init, {
  check,
  form_of,
  render_png,
  sheet_pdf,
  write_form,
} from "./wasm/mockery/mockery_wasm.js";
import packed from "./wasm/mockery/mockery_wasm_bg.wasm-gz?url";

export type {
  CardSheet,
  Checked,
  Form,
  FormCorner,
  Opened,
  Parts,
  SheetCard,
} from "./mockery.gen";

let loading: Promise<unknown> | null = null;

/**
 * Fetches and starts the renderer, once a page. It is shipped gzipped, being
 * past what Cloudflare serves as one file raw (scripts/build-wasm.mjs).
 */
export function loadMockery(): Promise<void> {
  loading ??= (async () => {
    const r = await fetch(packed);
    if (!r.ok) throw new Error(`fetching the card renderer: ${r.status}`);
    const bytes = new Uint8Array(await r.arrayBuffer());
    // A server that unpacked it on the way, as for a `.gz`, sent it raw.
    const gzipped = bytes[0] === 0x1f && bytes[1] === 0x8b;
    const wasm = gzipped
      ? await new Response(
          new Blob([bytes])
            .stream()
            .pipeThrough(new DecompressionStream("gzip")),
        ).arrayBuffer()
      : bytes;
    await init({ module_or_path: wasm });
  })().catch((e) => {
    loading = null;
    throw e;
  });
  return loading.then(() => undefined);
}

export function checkCard(text: string): Checked {
  return JSON.parse(check(text)) as Checked;
}

/**
 * One face of the file as the form's fields, or why the form can't hold
 * them: face 0 is the front (or left half), 1 the back (or right half).
 */
export function formOf(text: string, face = 0): Opened {
  return JSON.parse(form_of(text, face)) as Opened;
}

/** `text` with the form's fields written into one face, the rest as it was. */
export function writeForm(text: string, form: Form, face = 0): string {
  return write_form(text, JSON.stringify(form), face);
}

/**
 * One face of the card as a PNG: 0 the front, 1 a double-faced card's back.
 * `art` holds the arts it names, by path; a face whose art is missing is
 * drawn without it.
 */
export function renderPng(
  text: string,
  art: ReadonlyMap<string, Uint8Array>,
  face: number,
  ppi: number,
): Uint8Array {
  const { paths, all, ends } = pack(art);
  return render_png(text, JSON.stringify(paths), all, ends, face, ppi);
}

/** A sheet to print and cut; `art` holds every art the cards name, by path. */
export function sheetPdf(
  cards: readonly SheetCard[],
  layout: CardSheet,
  art: ReadonlyMap<string, Uint8Array>,
): Uint8Array {
  const { paths, all, ends } = pack(art);
  return sheet_pdf(
    JSON.stringify(cards),
    JSON.stringify(layout),
    JSON.stringify(paths),
    all,
    ends,
  );
}

/** Arts end to end, the `n`th ending at `ends[n]`, as the wasm takes them. */
function pack(art: ReadonlyMap<string, Uint8Array>) {
  const paths = [...art.keys()];
  const ends = new Uint32Array(paths.length);
  let total = 0;
  paths.forEach((p, i) => {
    total += (art.get(p) as Uint8Array).length;
    ends[i] = total;
  });
  const all = new Uint8Array(total);
  paths.forEach((p, i) => {
    const b = art.get(p) as Uint8Array;
    all.set(b, (ends[i] as number) - b.length);
  });
  return { paths, all, ends };
}
