import { createFileRoute, Link } from "@tanstack/react-router";
import { useSettingsFile } from "../card/preference";
import { Toolbar } from "../deck/Toolbar";
import type { FileAt, GitHubApi, RepoRef } from "../github/api";
import { connect } from "../github/connect";
import { type DeckEntry, listDecks } from "../github/decks";
import { deckText } from "../github/deckText";
import { openSession } from "../github/session";
import { loadSettingsFile } from "../github/settings";
import { useDeckActions } from "../home/DeckActions";
import { DeckGrid } from "../home/DeckGrid";
import { NewDeckDialog } from "../home/NewDeckDialog";
import { SessionGate } from "../home/SessionGate";
import { fetchPrintings, type Printings } from "../scryfall";
import { readSettings } from "../settings/rules";
import { order_decks } from "../wasm/pkg/meldweb_wasm.js";

// Login, onboarding, or the Magic repo's decks (#119).
export const Route = createFileRoute("/")({
  loader: async () => {
    const session = await openSession();
    if (session.kind !== "open") return { kind: "gate" as const, session };
    const { api } = await connect();
    const [decks, settings] = await Promise.all([
      listDecks(api, session.repo, deckText),
      loadSettingsFile(api, session.repo),
    ]);
    return {
      kind: "decks" as const,
      repo: session.repo,
      decks,
      printings: await commanderArt(decks),
      settings,
      api,
    };
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
  const data = Route.useLoaderData();
  if (data.kind === "gate") {
    return <SessionGate session={data.session} returnPath="/" />;
  }
  return <Decks {...data} />;
}

function Decks({
  api,
  repo,
  decks,
  printings,
  settings,
}: {
  api: GitHubApi;
  repo: RepoRef;
  decks: DeckEntry[];
  printings: Printings;
  settings: FileAt | null;
}) {
  const { menuFor, dialogs } = useDeckActions(repo);
  // The order is meldweb.toml's (ADR-0028), so it follows the repo to any device.
  const file = useSettingsFile(settings, api, repo);
  const read = readSettings(file.text);
  const order = read.kind === "read" ? read.decks : [];
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
            <Link
              to="/settings"
              className="button"
              title="Which printing of a card is offered first"
            >
              Settings
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
          <DeckGrid
            decks={decks}
            order={order}
            printings={printings}
            menuFor={menuFor}
            onOrder={(paths) =>
              void file.apply((t) =>
                order_decks(t ?? undefined, JSON.stringify(paths)),
              )
            }
          />
        )}
      </div>
      {dialogs}
      {file.error && (
        <div className="home-notice refusal" role="alert">
          <span>The deck order was not saved: {file.error}</span>
        </div>
      )}
    </main>
  );
}
