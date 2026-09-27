import { createFileRoute, Link } from "@tanstack/react-router";
import { connect } from "../github/connect";
import { listDecks } from "../github/decks";
import { deckText } from "../github/deckText";
import { openSession } from "../github/session";
import { NewDeckDialog } from "../home/NewDeckDialog";
import { SessionGate } from "../home/SessionGate";

// Login, onboarding, or the Magic repo's decks (#119).
export const Route = createFileRoute("/")({
  loader: async () => {
    const session = await openSession();
    if (session.kind !== "open") return { session, decks: [] };
    const { api } = await connect();
    return { session, decks: await listDecks(api, session.repo, deckText) };
  },
  // A deck made or renamed elsewhere shows up on coming back.
  gcTime: 0,
  component: Home,
});

function Home() {
  const { session, decks } = Route.useLoaderData();
  if (session.kind !== "open") {
    return <SessionGate session={session} returnPath="/" />;
  }
  return (
    <main className="home">
      <header className="home-header">
        <h1>Decks</h1>
        <span className="muted">
          {session.repo.owner}/{session.repo.name}
        </span>
        <div className="toolbar-spacer" />
        <NewDeckDialog repo={session.repo} />
      </header>
      {decks.length === 0 ? (
        <p className="muted">No decks yet. Make one with New deck.</p>
      ) : (
        <ul className="deck-list">
          {decks.map((d) => (
            <li key={d.path}>
              <Link to="/deck/$" params={{ _splat: d.path }}>
                {d.name}
              </Link>{" "}
              <code className="muted">{d.path}</code>
              {d.refused && (
                <span className="refusal-inline"> — {d.refused}</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </main>
  );
}
