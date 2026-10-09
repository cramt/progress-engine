import { createFileRoute, Link } from "@tanstack/react-router";
import { SettingsProvider } from "../card/preference";
import { parseDeck } from "../deck";
import { DeckEditor } from "../deck/DeckEditor";
import { pageOf, parseSearch, searchFor } from "../deck/versions";
import { connect } from "../github/connect";
import { openFile } from "../github/repoFile";
import { openSession } from "../github/session";
import { loadSettingsFile } from "../github/settings";
import { SessionGate } from "../home/SessionGate";
import { fetchPrintings } from "../scryfall";

// One deck, by its path in the Magic repo: /deck/decks/lantern.deck.toml.
// The URL is the deck, so a reload reopens it; `?at=<commit>` is the deck as
// that commit left it, and `?vs=<path>` compares it with another deck.
export const Route = createFileRoute("/deck/$")({
  validateSearch: parseSearch,
  loader: async ({ params }) => {
    const path = params._splat ?? "";
    const session = await openSession();
    if (session.kind !== "open")
      return { kind: "session" as const, session, path };
    const { api } = await connect();
    const [file, settings] = await Promise.all([
      // Leaving this deck just now may still be saving; read after it lands.
      openFile(api, session.repo, path),
      loadSettingsFile(api, session.repo),
    ]);
    if (!file) return { kind: "missing" as const, path };
    const parsed = parseDeck(file.text);
    return {
      kind: "deck" as const,
      path,
      file,
      settings,
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
  // Moving through the deck's history changes only the search, and the open
  // editor owns the text from its first load on; reading the file again for
  // it would throw the read away.
  shouldReload: false,
  component: DeckPage,
});

function DeckPage() {
  const data = Route.useLoaderData();
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
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
    <SettingsProvider file={data.settings} api={data.api} repo={data.repo}>
      <DeckEditor
        key={data.path}
        path={data.path}
        text={data.file.text}
        sha={data.file.sha}
        repo={data.repo}
        api={data.api}
        printings={data.printings}
        page={pageOf(search)}
        onNavigate={(change) =>
          void navigate({
            search: searchFor({ ...pageOf(search), ...change }),
          })
        }
      />
    </SettingsProvider>
  );
}
