import { createFileRoute } from "@tanstack/react-router";
import { parseCollection } from "../collection";
import { type CardRef, parseDeck } from "../deck";
import { loadCollection } from "../github/collection";
import { connect } from "../github/connect";
import { openSession } from "../github/session";
import { loadDeckFiles, loadWanted, WANTED_PATH } from "../github/wanted";
import { SessionGate } from "../home/SessionGate";
import { fetchPrintings } from "../scryfall";
import { readWanted } from "../wanted";
import { WantedEditor } from "../wanted/WantedEditor";

// What is wanted, by hand and by the decks (ADR-0034).
export const Route = createFileRoute("/wanted")({
  loader: async () => {
    const session = await openSession();
    if (session.kind !== "open") return { kind: "session" as const, session };
    const { api } = await connect();
    const [file, collection, decks] = await Promise.all([
      loadWanted(api, session.repo),
      loadCollection(api, session.repo),
      loadDeckFiles(api, session.repo),
    ]);
    // Every printing the three files name, so Rust can tell which card each
    // is where a file left no comment saying so.
    const named: { card: CardRef }[] = [];
    const owned = parseCollection(collection.text);
    if (owned.kind === "collection") named.push(...owned.cards);
    for (const d of decks) {
      const deck = parseDeck(d.text);
      if (deck.kind === "deck") named.push(...deck.cards);
    }
    const wanted = readWanted(file.text, collection.text, decks);
    if (wanted.kind === "wanted") named.push(...wanted.cards);
    const printings = await fetchPrintings(
      named.filter((c) => c.card.kind === "printing"),
    ).catch(() => new Map());
    return {
      kind: "wanted" as const,
      file,
      collection: collection.text,
      decks,
      repo: session.repo,
      api,
      // As for the collection: an outage costs the names, not the page.
      printings,
    };
  },
  // Never reopen from a cached read: its sha would be stale.
  gcTime: 0,
  component: WantedPage,
});

function WantedPage() {
  const data = Route.useLoaderData();
  if (data.kind === "session") {
    return <SessionGate session={data.session} returnPath="/wanted" />;
  }
  return (
    <WantedEditor
      key={WANTED_PATH}
      path={WANTED_PATH}
      text={data.file.text}
      sha={data.file.sha}
      repo={data.repo}
      api={data.api}
      collection={data.collection}
      decks={data.decks}
      printings={data.printings}
    />
  );
}
