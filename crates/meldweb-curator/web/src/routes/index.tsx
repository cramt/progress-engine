import { createFileRoute } from "@tanstack/react-router";
import lantern from "../../../../../decks/lantern.txt?raw";
import { StacksView } from "../deck/StacksView";
import { parseDecklist } from "../decklist";
import { fetchPrintings } from "../scryfall";

// The committed lantern list, until the editor reads decks from GitHub.
export const Route = createFileRoute("/")({
  loader: async () => {
    const parsed = parseDecklist(lantern);
    if (parsed.kind === "refused") return { parsed, printings: new Map() };
    return { parsed, printings: await fetchPrintings(parsed.entries) };
  },
  component: Deck,
});

function Deck() {
  const { parsed, printings } = Route.useLoaderData();
  if (parsed.kind === "refused") return <p>{parsed.message}</p>;
  return (
    <main>
      <h1>lantern.txt · {parsed.total} cards</h1>
      <StacksView entries={parsed.entries} printings={printings} />
    </main>
  );
}
