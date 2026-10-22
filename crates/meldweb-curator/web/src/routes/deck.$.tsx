import { createFileRoute, Link } from "@tanstack/react-router";
import { SettingsProvider } from "../card/preference";
import { parseDeck } from "../deck";
import { DeckEditor } from "../deck/DeckEditor";
import { pageOf, parseSearch, searchFor } from "../deck/versions";
import { deckFile, deckStem } from "../github/decks";
import { openFile } from "../github/repoFile";
import { openSession } from "../github/session";
import { loadSettingsFile } from "../github/settings";
import { SessionGate } from "../home/SessionGate";
import { fetchPrintings } from "../scryfall";

// One deck, by its stem: /deck/lantern, the line decks/lantern/main
// (ADR-0041). The URL is the deck, so a reload reopens it; `?at=<commit>` is
// the deck as that commit left it, and `?vs=<stem>` compares it with another
// deck. A link from repo version 1, /deck/decks/lantern.deck.toml, opens the
// same deck.
export const Route = createFileRoute("/deck/$")({
  validateSearch: parseSearch,
  loader: async ({ params }) => {
    const stem = deckStem(params._splat ?? "");
    const session = await openSession();
    if (session.kind !== "open")
      return { kind: "session" as const, session, stem };
    const { path, branch } = deckFile(stem);
    const [file, settings, lines] = await Promise.all([
      // Leaving this deck just now may still be saving; read after it lands.
      openFile(session.repo, path, branch),
      loadSettingsFile(session.repo),
      session.repo.decks(),
    ]);
    if (!file) return { kind: "missing" as const, stem };
    const parsed = parseDeck(file.text);
    const parent = lines.find((d) => d.stem === stem)?.parent;
    return {
      kind: "deck" as const,
      stem,
      ...(parent ? { parent } : {}),
      file,
      settings,
      repo: session.repo,
      me: session.me,
      others: session.others,
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
      <SessionGate session={data.session} returnPath={`/deck/${data.stem}`} />
    );
  }
  if (data.kind === "missing") {
    return (
      <main className="home">
        <p className="refusal" role="alert">
          There is no deck {data.stem} in the Magic repo.
        </p>
        <Link to="/">← Decks</Link>
      </main>
    );
  }
  return (
    <SettingsProvider file={data.settings} repo={data.repo}>
      <DeckEditor
        key={data.stem}
        stem={data.stem}
        {...("parent" in data && data.parent ? { parent: data.parent } : {})}
        text={data.file.text}
        sha={data.file.sha}
        repo={data.repo}
        me={data.me}
        others={data.others}
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
