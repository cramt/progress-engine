import { createFileRoute } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import lantern from "../../../../../decks/lantern.txt?raw";
import { dropOnto } from "../deck/move";
import { StacksView } from "../deck/StacksView";
import { parseDecklist, setCategories } from "../decklist";
import { fetchPrintings } from "../scryfall";

// The committed lantern list, until the editor reads decks from GitHub.
export const Route = createFileRoute("/")({
  loader: async () => {
    const parsed = parseDecklist(lantern);
    return {
      printings:
        parsed.kind === "deck"
          ? await fetchPrintings(parsed.entries)
          : new Map(),
    };
  },
  component: Deck,
});

/**
 * The deck is its text. An edit is a new text, which makes undo a stack of
 * texts and every change a line diff against the file as loaded.
 */
interface History {
  past: string[];
  present: string;
  future: string[];
}

function useHistory(initial: string) {
  const [h, setH] = useState<History>({
    past: [],
    present: initial,
    future: [],
  });
  const edit = (next: string) =>
    setH((h) =>
      next === h.present
        ? h
        : { past: [...h.past, h.present], present: next, future: [] },
    );
  const undo = () =>
    setH((h) => {
      const previous = h.past.at(-1);
      return previous === undefined
        ? h
        : {
            past: h.past.slice(0, -1),
            present: previous,
            future: [h.present, ...h.future],
          };
    });
  const redo = () =>
    setH((h) => {
      const [next, ...future] = h.future;
      return next === undefined
        ? h
        : { past: [...h.past, h.present], present: next, future };
    });
  return { ...h, edit, undo, redo };
}

function Deck() {
  const { printings } = Route.useLoaderData();
  const history = useHistory(lantern);
  const parsed = useMemo(
    () => parseDecklist(history.present),
    [history.present],
  );
  const { undo, redo } = history;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.target instanceof HTMLInputElement)
        return;
      const key = e.key.toLowerCase();
      if (key === "z" && !e.shiftKey) undo();
      else if ((key === "z" && e.shiftKey) || key === "y") redo();
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, redo]);

  if (parsed.kind === "refused") return <p>{parsed.message}</p>;
  const changed = history.present
    .split("\n")
    .filter((line, i) => line !== lantern.split("\n")[i]).length;
  return (
    <main>
      <div className="deck-bar">
        <h1>lantern.txt · {parsed.total} cards</h1>
        <span className="muted">
          {changed === 0
            ? "no changes"
            : `${changed} line${changed === 1 ? "" : "s"} changed`}
        </span>
        <button
          type="button"
          onClick={undo}
          disabled={history.past.length === 0}
        >
          Undo
        </button>
        <button
          type="button"
          onClick={redo}
          disabled={history.future.length === 0}
        >
          Redo
        </button>
      </div>
      <StacksView
        entries={parsed.entries}
        printings={printings}
        onDrop={(entry, from, to, secondary) =>
          history.edit(
            setCategories(
              history.present,
              entry.line,
              dropOnto(entry.categories, from, to, secondary),
            ),
          )
        }
      />
    </main>
  );
}
