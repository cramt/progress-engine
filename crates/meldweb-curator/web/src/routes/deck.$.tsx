import { createFileRoute, Link } from "@tanstack/react-router";
import { SettingsProvider } from "../card/preference";
import { parseDeck } from "../deck";
import { type BuiltDeck, DeckEditor } from "../deck/DeckEditor";
import { pageOf, parseSearch, searchFor } from "../deck/versions";
import { MAIN } from "../github/branches";
import { deckFile, deckStem } from "../github/decks";
import { openFile } from "../github/repoFile";
import { openSession } from "../github/session";
import { loadSettingsFile } from "../github/settings";
import { SessionGate } from "../home/SessionGate";
import { fetchPrintings } from "../scryfall";

// One deck, by its stem: /deck/lantern, the line decks/lantern/main
// (ADR-0041). The URL is the deck, so a reload reopens it; `?branch=<name>`
// opens another of its branches for editing, `?at=<commit>` is the deck as
// that commit left it, and `?vs=<stem>` compares it with another deck. A link
// from repo version 1, /deck/decks/lantern.deck.toml, opens the same deck.
export const Route = createFileRoute("/deck/$")({
  validateSearch: parseSearch,
  // A branch is another file to edit, so changing it reads the deck again.
  loaderDeps: ({ search }) => ({ branch: search.branch ?? MAIN }),
  loader: async ({ params, deps: { branch: on } }) => {
    const stem = deckStem(params._splat ?? "");
    const session = await openSession();
    if (session.kind !== "open")
      return { kind: "session" as const, session, stem };
    const { path, branch } = deckFile(stem, on);
    const [file, settings, lines] = await Promise.all([
      // Leaving this deck just now may still be saving; read after it lands.
      openFile(session.repo, path, branch),
      loadSettingsFile(session.repo),
      session.repo.decks(),
    ]);
    const line = lines.find((d) => d.stem === stem);
    if (!file) return { kind: "missing" as const, stem, branch: on, line };
    const parsed = parseDeck(file.text);
    const parent = line?.parent;
    // Off main, what is about the built deck (Build, Sourcing, proxies)
    // reads main's list, as the collection and the wanted list do.
    const built =
      on === MAIN ? null : await builtDeck(line?.file?.text ?? null);
    return {
      kind: "deck" as const,
      stem,
      branch: on,
      built,
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

async function builtDeck(text: string | null): Promise<BuiltDeck | null> {
  if (text === null) return null;
  const parsed = parseDeck(text);
  if (parsed.kind !== "deck") return null;
  return {
    text,
    deck: parsed,
    printings: await fetchPrintings(parsed.cards).catch(() => new Map()),
  };
}

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
          {data.branch !== MAIN && data.line
            ? `${data.stem} has no branch ${data.branch}.`
            : `There is no deck ${data.stem} in the Magic repo.`}
        </p>
        {data.branch !== MAIN && data.line ? (
          <Link to="/deck/$" params={{ _splat: data.stem }}>
            ← {data.stem}'s main
          </Link>
        ) : (
          <Link to="/">← Decks</Link>
        )}
      </main>
    );
  }
  return (
    <SettingsProvider file={data.settings} repo={data.repo}>
      <DeckEditor
        key={`${data.stem}:${data.branch}`}
        stem={data.stem}
        branch={data.branch}
        built={data.built}
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
