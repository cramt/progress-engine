import { createFileRoute, Link } from "@tanstack/react-router";
import { Toolbar } from "../deck/Toolbar";
import type { RepoRef } from "../github/api";
import { connect } from "../github/connect";
import { type DeckEntry, listDecks } from "../github/decks";
import { deckText } from "../github/deckText";
import { openSession } from "../github/session";
import { useDeckActions } from "../home/DeckActions";
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

/** Every deck's cover and commanders in one lookup; an outage costs the art, not the list. */
function commanderArt(decks: readonly DeckEntry[]): Promise<Printings> {
  const cards = decks.flatMap((d) => [
    ...(d.commanders ?? []).map((card) => ({ card })),
    ...(d.cover ? [{ card: { kind: "printing" as const, ...d.cover } }] : []),
  ]);
  if (cards.length === 0) return Promise.resolve(new Map());
  return fetchPrintings(cards).catch(() => new Map());
}

function Home() {
  const { session, decks, printings } = Route.useLoaderData();
  if (session.kind !== "open") {
    return <SessionGate session={session} returnPath="/" />;
  }
  return <Decks repo={session.repo} decks={decks} printings={printings} />;
}

function Decks({
  repo,
  decks,
  printings,
}: {
  repo: RepoRef;
  decks: DeckEntry[];
  printings: Printings;
}) {
  const { menuFor, dialogs } = useDeckActions(repo);
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
              {repo.owner}/{repo.name}
            </span>
            <Link to="/collection" className="button">
              Collection
            </Link>
            <NewDeckDialog repo={repo} />
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
                <DeckTile deck={d} printings={printings} menu={menuFor(d)} />
              </li>
            ))}
          </ul>
        )}
      </div>
      {dialogs}
    </main>
  );
}
