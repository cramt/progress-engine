import { createFileRoute, Link } from "@tanstack/react-router";
import { parseDeck } from "../deck";
import { DeckEditor } from "../deck/DeckEditor";
import { connect } from "../github/connect";
import { settled } from "../github/save";
import { openSession } from "../github/session";
import { SessionGate } from "../home/SessionGate";
import { fetchPrintings } from "../scryfall";

// One deck, by its path in the Magic repo: /deck/decks/lantern.deck.toml.
// The URL is the deck, so a reload reopens it.
export const Route = createFileRoute("/deck/$")({
  loader: async ({ params }) => {
    const path = params._splat ?? "";
    const session = await openSession();
    if (session.kind !== "open")
      return { kind: "session" as const, session, path };
    // Leaving this deck just now may still be saving; read after it lands.
    await settled(path);
    const { api } = await connect();
    const file = await api.getFile(session.repo, path);
    if (!file) return { kind: "missing" as const, path };
    const parsed = parseDeck(file.text);
    return {
      kind: "deck" as const,
      path,
      file,
      repo: session.repo,
      api,
      // A Scryfall outage costs the pictures, not the deck: each card shows
      // its name instead.
      printings:
        parsed.kind === "deck"
          ? await fetchPrintings(parsed.cards).catch(() => new Map())
          : new Map(),
    };
  },
  // Never reopen a deck from a cached read: its sha would be stale.
  gcTime: 0,
  component: DeckPage,
});

function DeckPage() {
  const data = Route.useLoaderData();
  if (data.kind === "session") {
    return (
      <SessionGate session={data.session} returnPath={`/deck/${data.path}`} />
    );
  }
  if (data.kind === "missing") {
    return (
      <main className="home">
        <p className="refusal" role="alert">
          There is no {data.path} in the Magic repo.
        </p>
        <Link to="/">← Decks</Link>
      </main>
    );
  }
  return (
    <DeckEditor
      key={data.path}
      path={data.path}
      text={data.file.text}
      sha={data.file.sha}
      repo={data.repo}
      api={data.api}
      printings={data.printings}
    />
  );
}
