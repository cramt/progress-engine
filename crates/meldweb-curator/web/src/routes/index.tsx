import { createFileRoute } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import lantern from "../../../../../decks/lantern.deck.toml?raw";
import { declareCategory, parseDeck, setCardCategories } from "../deck";
import { dropOnto } from "../deck/move";
import { type OnDrop, StacksView } from "../deck/StacksView";
import { Toolbar, UndoRedo } from "../deck/Toolbar";
import { fetchPrintings } from "../scryfall";

// The committed lantern deck, until the editor reads decks from GitHub.
export const Route = createFileRoute("/")({
  loader: async () => {
    const text = lantern;
    const parsed = parseDeck(text);
    return {
      text,
      printings:
        parsed.kind === "deck" ? await fetchPrintings(parsed.cards) : new Map(),
    };
  },
  component: DeckPage,
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

function DeckPage() {
  const { text: loaded, printings } = Route.useLoaderData();
  const history = useHistory(loaded);
  const [refusal, setRefusal] = useState<string | null>(null);
  const parsed = useMemo(() => parseDeck(history.present), [history.present]);
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

  const onDrop: OnDrop = (card, from, to, secondary) => {
    const declared = (name: string) =>
      parsed.categories.some((c) => c.name === name);
    try {
      let text = history.present;
      let name: string;
      if (to.kind === "category") {
        name = to.name;
      } else if (to.kind === "new") {
        name = to.name;
        if (!declared(name)) text = declareCategory(text, name);
      } else {
        // The strip's Sideboard is the deck's sideboard-typed category, made
        // if the deck has none yet.
        const existing = parsed.categories.find((c) => c.kind === to.type);
        name =
          existing?.name ??
          (to.type === "maybeboard" ? "Maybeboard" : "Sideboard");
        if (!existing) text = declareCategory(text, name, to.type);
      }
      history.edit(
        setCardCategories(
          text,
          card.index,
          dropOnto(card.categories, from, name, secondary),
        ),
      );
      setRefusal(null);
    } catch (e) {
      setRefusal(e instanceof Error ? e.message : String(e));
    }
  };

  const before = loaded.split("\n");
  const changed = history.present
    .split("\n")
    .filter((line, i) => line !== before[i]).length;
  return (
    <main>
      <Toolbar
        name={parsed.name ?? "lantern.deck.toml"}
        count={parsed.total}
        status={
          changed === 0
            ? "no changes"
            : `${changed} line${changed === 1 ? "" : "s"} changed`
        }
        history={
          <UndoRedo
            onUndo={undo}
            onRedo={redo}
            canUndo={history.past.length > 0}
            canRedo={history.future.length > 0}
          />
        }
      />
      {refusal && (
        <p className="refusal" role="alert">
          {refusal}
          <button type="button" onClick={() => setRefusal(null)}>
            ×
          </button>
        </p>
      )}
      <details className="source">
        <summary>The file</summary>
        <pre>{history.present}</pre>
      </details>
      <StacksView
        categories={parsed.categories}
        cards={parsed.cards}
        printings={printings}
        onDrop={onDrop}
      />
    </main>
  );
}
