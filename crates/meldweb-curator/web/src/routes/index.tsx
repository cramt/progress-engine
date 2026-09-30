import { createFileRoute, Link } from "@tanstack/react-router";
import { Toolbar } from "../deck/Toolbar";
import { connect } from "../github/connect";
import { type DeckEntry, listDecks } from "../github/decks";
import { deckText } from "../github/deckText";
import { openSession } from "../github/session";
import { DeckTile } from "../home/DeckTile";
import { NewDeckDialog } from "../home/NewDeckDialog";
import { SessionGate } from "../home/SessionGate";
import { fetchPrintings, type Printings } from "../scryfall";

// Login, onboarding, or the Magic repo's decks (#119).
export const Route = createFileRoute("/")({
  loader: async () => {
    const session = await openSession();
    if (session.kind !== "open")
      return { session, decks: [], printings: new Map() as Printings };
    const { api } = await connect();
    const decks = await listDecks(api, session.repo, deckText);
    return { session, decks, printings: await commanderArt(decks) };
  },
  // A deck made or renamed elsewhere shows up on coming back.
  gcTime: 0,
  component: Home,
});

/** Every deck's commanders in one lookup; an outage costs the art, not the list. */
function commanderArt(decks: readonly DeckEntry[]): Promise<Printings> {
  const cards = decks.flatMap((d) =>
    (d.commanders ?? []).map((card) => ({ card })),
  );
  if (cards.length === 0) return Promise.resolve(new Map());
  return fetchPrintings(cards).catch(() => new Map());
}

function Home() {
  const { session, decks, printings } = Route.useLoaderData();
  if (session.kind !== "open") {
    return <SessionGate session={session} returnPath="/" />;
  }
  return (
    <main>
      <Toolbar
        name="Decks"
        up={false}
        actions={
          <>
            <span
              className="home-repo"
              title="Every deck is a file in this repository"
            >
              {session.repo.owner}/{session.repo.name}
            </span>
            <Link to="/collection" className="button">
              Collection
            </Link>
            <NewDeckDialog repo={session.repo} />
          </>
        }
      />
      <div className="home">
        {decks.length === 0 ? (
          <div className="stacks-empty">
            <h2>No decks yet</h2>
            <p>
              Make one with New deck: start empty, or paste a list exported from
              Archidekt.
            </p>
          </div>
        ) : (
          <ul className="deck-grid">
            {decks.map((d) => (
              <li key={d.path}>
                <DeckTile deck={d} printings={printings} />
              </li>
            ))}
          </ul>
        )}
      </div>
    </main>
  );
}
