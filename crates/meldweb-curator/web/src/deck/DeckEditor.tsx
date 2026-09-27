import { Link } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { useCardEditor } from "../card/useCardEditor";
import { declareCategory, parseDeck, setCardCategories } from "../deck";
import type { GitHubApi, RepoRef } from "../github/api";
import { deckStem } from "../github/decks";
import { deckText } from "../github/deckText";
import { createSaveStore, type SaveState } from "../github/save";
import { useSave } from "../github/useSave";
import type { Printings } from "../scryfall";
import { dropOnto } from "./move";
import { type OnDrop, StacksView } from "./StacksView";
import { Toolbar, UndoRedo } from "./Toolbar";

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
  /** Starts over from `text` with nothing to undo, as after a reload. */
  const reset = (text: string) => setH({ past: [], present: text, future: [] });
  return { ...h, edit, undo, redo, reset };
}

export interface DeckEditorProps {
  /** The deck's path in the Magic repo, e.g. `decks/lantern.deck.toml`. */
  path: string;
  /** The file as loaded from GitHub, and its blob sha. */
  text: string;
  sha: string;
  repo: RepoRef;
  api: GitHubApi;
  printings: Printings;
}

const statusText: Record<SaveState["status"], string> = {
  unsaved: "Unsaved",
  saving: "Saving…",
  saved: "Saved",
  conflict: "Not saved",
  error: "Not saved",
};

/**
 * One open deck: the stacks, the toolbar, undo, and saving by itself. Mount it
 * with `key={path}`; it reads its props once and then owns the text.
 */
export function DeckEditor({
  path,
  text: loaded,
  sha,
  repo,
  api,
  printings,
}: DeckEditorProps) {
  const history = useHistory(loaded);
  const [refusal, setRefusal] = useState<string | null>(null);
  const parsed = useMemo(() => parseDeck(history.present), [history.present]);
  const { undo, redo } = history;
  // Each card's menu, hotkeys and details modal, every change an edit here.
  const cards = useCardEditor({
    text: history.present,
    deck: parsed,
    printings,
    edit: history.edit,
    refuse: setRefusal,
  });

  const [store] = useState(() =>
    createSaveStore({ api, repo, path, text: loaded, sha, deckText }),
  );
  const save = useSave(store);
  // Every change to the text, edit, undo or redo alike, is what gets saved.
  useEffect(() => store.edit(history.present), [store, history.present]);

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

  const reload = async () => {
    try {
      history.reset(await store.reload());
    } catch (e) {
      setRefusal(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <main>
      <Toolbar
        name={
          <>
            <Link to="/" className="toolbar-back" title="All decks">
              Decks
            </Link>
            <span className="muted"> / </span>
            {parsed.name ?? deckStem(path)}
          </>
        }
        count={parsed.total}
        status={
          <span
            className={`save-status save-${save.status}`}
            title={save.message ?? path}
          >
            {statusText[save.status]}
          </span>
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
      {save.status === "conflict" && (
        <p className="conflict" role="alert">
          Changed on GitHub -{" "}
          <button type="button" onClick={reload}>
            Reload (discard my edits)
          </button>{" "}
          /{" "}
          <button type="button" onClick={() => void store.overwrite()}>
            Overwrite
          </button>
        </p>
      )}
      {save.status === "error" && (
        <p className="refusal" role="alert">
          Saving failed, and will be tried again: {save.message}
        </p>
      )}
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
        printings={cards.printings}
        onDrop={onDrop}
        cardProps={cards.cardProps}
      />
      {cards.overlay}
    </main>
  );
}
