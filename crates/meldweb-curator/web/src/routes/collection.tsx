import { createFileRoute } from "@tanstack/react-router";
import { parseCollection } from "../collection";
import { CollectionEditor } from "../collection/CollectionEditor";
import { COLLECTION_PATH, loadCollection } from "../github/collection";
import { connect } from "../github/connect";
import { listDecks } from "../github/decks";
import { deckText } from "../github/deckText";
import { settled } from "../github/save";
import { openSession } from "../github/session";
import { SessionGate } from "../home/SessionGate";
import { fetchPrintings } from "../scryfall";

// Every card owned, and where each one is (ADR-0023).
export const Route = createFileRoute("/collection")({
  loader: async () => {
    const session = await openSession();
    if (session.kind !== "open") return { kind: "session" as const, session };
    // Leaving the collection just now may still be saving; read after it lands.
    await settled(COLLECTION_PATH);
    const { api } = await connect();
    const [file, decks] = await Promise.all([
      loadCollection(api, session.repo),
      listDecks(api, session.repo, deckText),
    ]);
    const parsed = parseCollection(file.text);
    return {
      kind: "collection" as const,
      file,
      decks,
      repo: session.repo,
      api,
      // As for a deck: an outage costs the names of printings, not the page.
      printings:
        parsed.kind === "collection"
          ? await fetchPrintings(parsed.cards).catch(() => new Map())
          : new Map(),
    };
  },
  // Never reopen from a cached read: its sha would be stale.
  gcTime: 0,
  component: CollectionPage,
});

function CollectionPage() {
  const data = Route.useLoaderData();
  if (data.kind === "session") {
    return <SessionGate session={data.session} returnPath="/collection" />;
  }
  return (
    <CollectionEditor
      key={COLLECTION_PATH}
      path={COLLECTION_PATH}
      text={data.file.text}
      sha={data.file.sha}
      repo={data.repo}
      api={data.api}
      printings={data.printings}
      decks={data.decks}
    />
  );
}
