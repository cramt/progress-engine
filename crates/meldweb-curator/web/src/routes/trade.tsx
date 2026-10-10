import { createFileRoute } from "@tanstack/react-router";
import { parseCollection } from "../collection";
import { type CardRef, parseDeck } from "../deck";
import { printingNames } from "../deck/archidektNames";
import { loadCollection } from "../github/collection";
import { connect } from "../github/connect";
import { openSession } from "../github/session";
import { loadTheirCollection, parseLogin } from "../github/trade";
import { loadDeckFiles, loadWanted } from "../github/wanted";
import { SessionGate } from "../home/SessionGate";
import { fetchPrintings } from "../scryfall";
import { TradeView } from "../trade/TradeView";
import { readTrade, readWanted } from "../wanted";

// What another player's public collection holds of the wanted list
// (ADR-0035): /trade?with=<their GitHub login>.
export const Route = createFileRoute("/trade")({
  validateSearch: (search: Record<string, unknown>): { with?: string } =>
    typeof search.with === "string" && search.with !== ""
      ? { with: search.with }
      : {},
  loaderDeps: ({ search }) => ({ with: search.with }),
  loader: async ({ deps }) => {
    const session = await openSession();
    if (session.kind !== "open") return { kind: "session" as const, session };
    const me = session.repo.owner;
    if (deps.with === undefined) return { kind: "ask" as const, me };
    const who = parseLogin(deps.with);
    if (!who) return { kind: "not-a-login" as const, me, text: deps.with };
    const { api } = await connect();
    const [file, collection, decks, theirs] = await Promise.all([
      loadWanted(api, session.repo),
      loadCollection(api, session.repo),
      loadDeckFiles(api, session.repo),
      loadTheirCollection(api, who),
    ]);
    if (theirs.kind !== "file") return { kind: theirs.kind, me, who };
    // Every printing the files name, so Rust can tell which card each is
    // where a file left no comment saying so.
    const named: { card: CardRef }[] = [];
    for (const text of [collection.text, theirs.text]) {
      const c = parseCollection(text);
      if (c.kind === "collection") named.push(...c.cards);
    }
    for (const d of decks) {
      const deck = parseDeck(d.text);
      if (deck.kind === "deck") named.push(...deck.cards);
    }
    const wanted = readWanted(file.text, collection.text, decks);
    if (wanted.kind === "wanted") named.push(...wanted.cards);
    const printed = named.filter((c) => c.card.kind === "printing");
    // An outage costs the names of printings no file comments, not the page.
    const printings = await fetchPrintings(printed).catch(() => new Map());
    return {
      kind: "trade" as const,
      me,
      who,
      trade: readTrade(
        file.text,
        collection.text,
        decks,
        theirs.text,
        printingNames(printed, printings),
      ),
    };
  },
  // Their collection may have moved since; read it again on every visit.
  gcTime: 0,
  component: TradePage,
});

function TradePage() {
  const data = Route.useLoaderData();
  const search = Route.useSearch();
  if (data.kind === "session") {
    const back = search.with ? `?with=${encodeURIComponent(search.with)}` : "";
    return <SessionGate session={data.session} returnPath={`/trade${back}`} />;
  }
  return <TradeView data={data} />;
}
