import { createFileRoute } from "@tanstack/react-router";
import lantern from "../../../../../decks/lantern.txt?raw";
import { type Entry, parseDecklist } from "../decklist";

// A smoke test for the pipeline, until the editor reads decks from GitHub:
// the committed lantern list, parsed in the browser by chip-decklist.
export const Route = createFileRoute("/")({
  loader: () => parseDecklist(lantern),
  component: Deck,
});

function premier(entry: Entry): string {
  const top = entry.categories.find((c) => c.flags.includes("top"));
  return (top ?? entry.categories[0])?.name ?? "Uncategorised";
}

function Deck() {
  const parsed = Route.useLoaderData();
  if (parsed.kind === "refused") return <p>{parsed.message}</p>;

  const groups = Map.groupBy(parsed.entries, premier);
  return (
    <main>
      <h1>lantern.txt: {parsed.total} cards</h1>
      {[...groups].map(([category, entries]) => (
        <section key={category}>
          <h2>
            {category} ({entries.reduce((n, e) => n + e.qty, 0)})
          </h2>
          <ul>
            {entries.map((e) => (
              <li key={e.name}>
                {e.qty} {e.name}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </main>
  );
}
