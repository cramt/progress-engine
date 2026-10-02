/**
 * Delver X's three model tiers, which Gitaxian Probe boots alike
 * (crates/gitaxian-probe/engine/README.md, *Scope*). The download is each
 * tier's packed weights, on top of the ~19 MB engine and catalogue every tier
 * shares.
 */
export const MODELS = [
  { id: "alpha", label: "Alpha", download: "23 MB" },
  { id: "lambda", label: "Lambda", download: "28 MB" },
  { id: "gamma", label: "Gamma", download: "40 MB" },
] as const;

export type Model = (typeof MODELS)[number]["id"];

/**
 * Alpha until another is picked: on the probe's six fixture cards it names
 * one more exact printing than lambda or gamma (5/6 against 4/6), and it is
 * the smallest download.
 */
export const DEFAULT_MODEL: Model = "alpha";

const KEY = "meldweb.scan-model";

/** The model last picked in this browser, for the deck's scan and the collection's alike. */
export function loadModel(
  storage: Pick<Storage, "getItem"> | undefined = globalThis.localStorage,
): Model {
  try {
    const saved = storage?.getItem(KEY);
    return MODELS.find((m) => m.id === saved)?.id ?? DEFAULT_MODEL;
  } catch {
    return DEFAULT_MODEL;
  }
}

export function saveModel(
  model: Model,
  storage: Pick<Storage, "setItem"> | undefined = globalThis.localStorage,
): void {
  try {
    storage?.setItem(KEY, model);
  } catch {
    // blocked storage: the pick lasts as long as the page
  }
}
