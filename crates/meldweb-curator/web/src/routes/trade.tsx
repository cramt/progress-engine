import { createFileRoute } from "@tanstack/react-router";
import { parseCollection } from "../collection";
import { type CardRef, parseDeck } from "../deck";
import { printingNames } from "../deck/archidektNames";
import { loadCollection } from "../github/collection";
import { deckStem } from "../github/decks";
import { openSession } from "../github/session";
import { loadTheirCollection, parseLogin } from "../github/trade";
import { loadDeckFiles, loadWanted } from "../github/wanted";
import { SessionGate } from "../home/SessionGate";
import { fetchPrintings } from "../scryfall";
import { type TradeResult, TradeView } from "../trade/TradeView";
import type { DeckFile } from "../wanted";
import { readTrade, readWanted } from "../wanted";

interface Search {
  /** Their GitHub login. */
  with?: string;
  /** One deck to trade for, by stem, in place of the whole wanted list. */
  deck?: string;
}

const text = (v: unknown) => (typeof v === "string" && v !== "" ? v : null);

// What another player's public collection holds of the wanted list, or of one
// deck (ADR-0035): /trade?with=<their GitHub login>&deck=<stem>.
export const Route = createFileRoute("/trade")({
  validateSearch: (search: Record<string, unknown>): Search => {
    const who = text(search.with);
    // A link from repo version 1 named the deck's path.
    const deck = text(search.deck) && deckStem(text(search.deck) ?? "");
    return { ...(who ? { with: who } : {}), ...(deck ? { deck } : {}) };
  },
  loaderDeps: ({ search }) => search,
  loader: async ({ deps }) => {
    const session = await openSession();
    if (session.kind !== "open") return { kind: "session" as const, session };
    const page = (decks: DeckFile[], result: TradeResult) => ({
      kind: "page" as const,
      data: {
        me: session.me,
        decks: decks.map((d) => {
          const deck = parseDeck(d.text);
          return {
            stem: d.stem,
            name: (deck.kind === "deck" && deck.name) || d.stem,
          };
        }),
        deck: deps.deck,
        result,
      },
    });
    if (deps.with === undefined) {
      return page(await loadDeckFiles(session.repo), { kind: "ask" });
    }
    const who = parseLogin(deps.with);
    if (!who) {
      return page(await loadDeckFiles(session.repo), {
        kind: "not-a-login",
        text: deps.with,
      });
    }
    const [file, collection, decks, theirs] = await Promise.all([
      loadWanted(session.repo),
      loadCollection(session.repo),
      loadDeckFiles(session.repo),
      loadTheirCollection(session.others, who),
    ]);
    if (theirs.kind !== "file") return page(decks, { kind: theirs.kind, who });
    // Every printing the files name, so Rust can tell which card each is
    // where a file left no comment saying so.
    const named: { card: CardRef }[] = [];
    for (const t of [collection.text, theirs.text]) {
      const c = parseCollection(t);
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
    return page(decks, {
      kind: "trade",
      who,
      trade: readTrade(
        file.text,
        collection.text,
        decks,
        theirs.text,
        deps.deck,
        printingNames(printed, printings),
      ),
    });
  },
  // Their collection may have moved since; read it again on every visit.
  gcTime: 0,
  component: TradePage,
});

function TradePage() {
  const loaded = Route.useLoaderData();
  const search = Route.useSearch();
  if (loaded.kind === "session") {
    const q = new URLSearchParams(search as Record<string, string>).toString();
    return (
      <SessionGate
        session={loaded.session}
        returnPath={`/trade${q ? `?${q}` : ""}`}
      />
    );
  }
  return <TradeView data={loaded.data} />;
}
