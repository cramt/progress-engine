import { useEffect, useState } from "react";
import { fetchAllPrintings, type PrintingOption } from "./prints";

/** How long a card must stay shown before its printings are asked for. */
const SETTLE_MS = 250;

export type PrintingOptions =
  | { status: "none" }
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "done"; printings: PrintingOption[] };

/**
 * Every printing behind `uri`, from one search. Stepping past a card faster
 * than it settles asks nothing, and leaving it cancels a request still queued,
 * so walking a deck spends Scryfall's budget on the cards actually looked at.
 */
export function usePrintingOptions(uri: string | undefined): PrintingOptions {
  const [state, setState] = useState<{
    uri: string;
    options: PrintingOptions;
  } | null>(null);
  useEffect(() => {
    if (!uri) return;
    const abort = new AbortController();
    const load = async (): Promise<void> => {
      try {
        const printings = await fetchAllPrintings(uri, abort.signal);
        if (!abort.signal.aborted)
          setState({ uri, options: { status: "done", printings } });
      } catch (e) {
        if (abort.signal.aborted) return;
        setState({
          uri,
          options: {
            status: "error",
            message: e instanceof Error ? e.message : String(e),
          },
        });
      }
    };
    const timer = setTimeout(() => void load(), SETTLE_MS);
    return () => {
      clearTimeout(timer);
      abort.abort();
    };
  }, [uri]);
  if (!uri) return { status: "none" };
  return state?.uri === uri ? state.options : { status: "loading" };
}
