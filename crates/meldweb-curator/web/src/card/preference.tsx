import { createContext, type ReactNode, useContext, useMemo } from "react";
import type { Ranked, RuleText } from "../deck.gen";
import { rank_printings } from "../wasm/pkg/meldweb_wasm.js";
import { byRelease, type PrintingOption } from "./prints";

/**
 * The repo's `meldweb.toml`, null when it has none. Unprovided, as in a test,
 * it is null too, and the default rules rank.
 */
const SettingsContext = createContext<string | null>(null);

export function SettingsProvider({
  settings,
  children,
}: {
  settings: string | null;
  children: ReactNode;
}) {
  return (
    <SettingsContext.Provider value={settings}>
      {children}
    </SettingsContext.Provider>
  );
}

/** A printing in the preferred order, with the rules that put it there. */
export interface RankedOption {
  option: PrintingOption;
  matched: readonly RuleText[];
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
      ranked: byRelease(printings).map((option) => ({ option, matched: [] })),
    };
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
          matched: matched.flatMap((i) => result.rules[i] ?? []),
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
