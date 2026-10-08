import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { Pin, Ranked, RuleText } from "../deck.gen";
import type { FileAt, GitHubApi, RepoRef } from "../github/api";
import { createSaveStore, flushOnLeave } from "../github/save";
import { SETTINGS_PATH } from "../github/settings";
import { readSettings } from "../settings/rules";
import {
  pin_printing,
  rank_printings,
  unpin_printing,
} from "../wasm/pkg/meldweb_wasm.js";
import { byRelease, type PrintingOption } from "./prints";

/**
 * The repo's `meldweb.toml`, null when it has none. Unprovided, as in a test,
 * it is null too, and the default rules rank.
 */
const SettingsContext = createContext<string | null>(null);

/**
 * The cards given a printing of their own, and the heart that sets one.
 * Null where nothing can be pinned, which hides the hearts.
 */
export interface Pins {
  pins: readonly Pin[];
  /** Makes `printing` its card's own, or stops it being, if it already is. */
  toggle: (printing: PrintingOption) => void;
  /** Why the last pin was not saved, if it was not. */
  error: string | null;
}

const PinsContext = createContext<Pins | null>(null);

export function PinsProvider({
  pins,
  children,
}: {
  pins: Pins | null;
  children: ReactNode;
}) {
  return <PinsContext.Provider value={pins}>{children}</PinsContext.Provider>;
}

/** The repo's `meldweb.toml` as a page holds it, and how it saves an edit. */
export interface SettingsFile {
  /** The file's text, null while the repo has none. */
  text: string | null;
  /**
   * Commits `edit` of the file at once. When the file moved on GitHub since,
   * `edit` is made again on what is there now.
   */
  apply: (edit: (text: string | null) => string) => Promise<void>;
  /** Why the last edit was not saved, if it was not. */
  error: string | null;
}

/**
 * `meldweb.toml` for a page that edits it a click at a time (a heart, a deck
 * dragged). An edit is committed at once rather than after the idle wait a
 * deck has: it is one gesture, and what comes next should already see it.
 */
export function useSettingsFile(
  file: FileAt | null,
  api: GitHubApi,
  repo: RepoRef,
): SettingsFile {
  const [text, setTextState] = useState(file?.text ?? null);
  // Two clicks before a render must each build on the other.
  const latest = useRef(text);
  const setText = (next: string) => {
    latest.current = next;
    setTextState(next);
  };
  const [error, setError] = useState<string | null>(null);
  const [store] = useState(() =>
    createSaveStore({
      api,
      repo,
      path: SETTINGS_PATH,
      text: file?.text ?? "",
      sha: file?.sha ?? null,
    }),
  );
  useEffect(() => () => void flushOnLeave(store), [store]);

  const apply = async (edit: (text: string | null) => string) => {
    try {
      const next = edit(latest.current);
      setText(next);
      store.edit(next);
      await flushOnLeave(store);
      if (store.getState().status === "conflict") {
        const fresh = edit(await store.reload());
        setText(fresh);
        store.edit(fresh);
        await flushOnLeave(store);
      }
      const state = store.getState();
      setError(state.status === "error" ? state.message : null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  return { text, apply, error };
}

/**
 * The settings for a page that reads them (a deck, the collection), and a
 * heart on every printing that pins it, committed at once so the next card
 * added already gets it.
 */
export function SettingsProvider({
  file,
  api,
  repo,
  children,
}: {
  /** The repo's `meldweb.toml`, null when it has none. */
  file: FileAt | null;
  api: GitHubApi;
  repo: RepoRef;
  children: ReactNode;
}) {
  const { text, apply, error } = useSettingsFile(file, api, repo);

  const pins = useMemo(() => {
    const read = readSettings(text);
    return read.kind === "read" ? read.pins : [];
  }, [text]);

  const value: Pins = {
    pins,
    error,
    toggle: (p) => {
      const name = p.name;
      void apply(
        pinnedAs(pins, p)
          ? (t) => unpin_printing(t ?? undefined, name)
          : (t) => pin_printing(t ?? undefined, name, p.set, p.num),
      );
    },
  };

  return (
    <SettingsContext.Provider value={text}>
      <PinsContext.Provider value={value}>{children}</PinsContext.Provider>
    </SettingsContext.Provider>
  );
}

/** Whether `printing` is the one pinned for its card. */
export function pinnedAs(
  pins: readonly Pin[],
  printing: Pick<PrintingOption, "name" | "set" | "num">,
): boolean {
  return pins.some(
    (p) =>
      p.name.toLowerCase() === printing.name.toLowerCase() &&
      p.set.toLowerCase() === printing.set.toLowerCase() &&
      p.num === printing.num,
  );
}

/** The cards pinned, and the heart's toggle; null where there are none. */
export function usePins(): Pins | null {
  return useContext(PinsContext);
}

/** A printing in the preferred order, with the rules that put it there. */
export interface RankedOption {
  option: PrintingOption;
  /** The rules it matched, its pin not among them. */
  matched: readonly RuleText[];
  /** Whether a pin put it first. */
  pinned: boolean;
}

export type PreferredOrder =
  | {
      kind: "ranked";
      ranked: readonly RankedOption[];
      /** Whether the rules are the repo's own rather than the default. */
      declared: boolean;
      /** How many printings each rule matched, by its place in the file. */
      hits: readonly number[];
    }
  /** The settings or a printing could not be read; newest first instead. */
  | { kind: "refused"; message: string; ranked: readonly RankedOption[] };

/**
 * `printings` ranked by `settings` in `chip-scryfall`, which reads each rule
 * as the Scryfall query it is. The TypeScript only carries the order back.
 */
export function rankPrintings(
  settings: string | null,
  printings: readonly PrintingOption[],
): PreferredOrder {
  const result = JSON.parse(
    rank_printings(
      settings ?? undefined,
      JSON.stringify(printings.map((p) => p.facts)),
    ),
  ) as Ranked;
  if (result.kind === "refused")
    return {
      kind: "refused",
      message: result.message,
      ranked: byRelease(printings).map((option) => ({
        option,
        matched: [],
        pinned: false,
      })),
    };
  const pins = new Set(result.pins);
  return {
    kind: "ranked",
    declared: result.declared,
    hits: result.rules.map(
      (_, i) => result.order.filter((p) => p.matched.includes(i)).length,
    ),
    ranked: result.order.flatMap(({ index, matched }) => {
      const option = printings[index];
      if (!option) return [];
      return [
        {
          option,
          matched: matched.flatMap((i) =>
            pins.has(i) ? [] : (result.rules[i] ?? []),
          ),
          pinned: matched.some((i) => pins.has(i)),
        },
      ];
    }),
  };
}

/** `printings` in the order the repo's settings prefer, ranked once per list. */
export function usePreferredOrder(
  printings: readonly PrintingOption[],
): PreferredOrder {
  const settings = useContext(SettingsContext);
  return useMemo(
    () => rankPrintings(settings, printings),
    [settings, printings],
  );
}

/** The repo's settings, for ranking outside a component. */
export function useSettings(): string | null {
  return useContext(SettingsContext);
}
