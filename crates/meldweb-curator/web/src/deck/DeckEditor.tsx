import { useEffect, useMemo, useState } from "react";
import { withBoard } from "../card/apply";
import { renderCardResult } from "../card/searchResult";
import { useCardEditor } from "../card/useCardEditor";
import { declareCategory, parseDeck, setCardCategories } from "../deck";
import type { GitHubApi, RepoRef } from "../github/api";
import { deckStem } from "../github/decks";
import { deckText } from "../github/deckText";
import { createSaveStore } from "../github/save";
import { useSave } from "../github/useSave";
import { useHistory, useUndoKeys } from "../history";
import { addPrinting } from "../probe/printings";
import { ScanDialog } from "../probe/ScanDialog";
import { scannerAvailable } from "../probe/scanner";
import { addByName } from "../quickadd/addByName";
import { type AddByName, QuickAdd } from "../quickadd/QuickAdd";
import type { Printings } from "../scryfall";
import { SearchButton, SearchOverlay } from "../search/SearchOverlay";
import { ScanIcon } from "../ui/icons";
import { CopyArchidekt } from "./CopyArchidekt";
import { dropOnto } from "./move";
import { type OnDrop, StacksView } from "./StacksView";
import { Banners, SaveStatus, Toolbar, UndoRedo } from "./Toolbar";

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
  printings: loadedPrintings,
}: DeckEditorProps) {
  const history = useHistory(loaded);
  const [refusal, setRefusal] = useState<string | null>(null);
  const parsed = useMemo(() => parseDeck(history.present), [history.present]);
  const { undo, redo } = history;
  // Each card's menu, hotkeys and details modal, every change an edit here.
  const cards = useCardEditor({
    text: history.present,
    deck: parsed,
    printings: loadedPrintings,
    edit: history.edit,
    refuse: setRefusal,
  });
  const [searching, setSearching] = useState(false);
  const [scanning, setScanning] = useState(false);
  // The deck's printings, grown by every card an edit adds.
  const { printings } = cards;

  const [store] = useState(() =>
    createSaveStore({ api, repo, path, text: loaded, sha, deckText }),
  );
  const save = useSave(store);
  // Every change to the text, edit, undo or redo alike, is what gets saved.
  useEffect(() => store.edit(history.present), [store, history.present]);

  useUndoKeys(undo, redo);

  if (parsed.kind === "refused") {
    return (
      <main>
        <Toolbar name={deckStem(path)} />
        <div className="banners">
          <p className="refusal" role="alert">
            {path} is not a deck Curator can read: {parsed.message}
          </p>
        </div>
        <details className="source" open>
          <summary>The file</summary>
          <pre>{history.present}</pre>
        </details>
      </main>
    );
  }

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
        ({ text, name } = withBoard(text, parsed.categories, to.type));
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

  // Quick add and the search overlay add one copy of a card, undoably.
  const onAdd: AddByName = (name, category) => {
    try {
      history.edit(
        addByName(history.present, parsed.cards, printings, name, category),
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
        name={parsed.name ?? deckStem(path)}
        count={parsed.total}
        search={<SearchButton onClick={() => setSearching(true)} />}
        quickAdd={<QuickAdd categories={parsed.categories} onAdd={onAdd} />}
        actions={
          <>
            {scannerAvailable && (
              <button type="button" onClick={() => setScanning(true)}>
                <ScanIcon />
                Scan
              </button>
            )}
            <CopyArchidekt
              text={history.present}
              cards={parsed.cards}
              printings={printings}
              onRefusal={setRefusal}
            />
          </>
        }
        status={<SaveStatus save={save} path={path} />}
        history={
          <UndoRedo
            onUndo={undo}
            onRedo={redo}
            canUndo={history.past.length > 0}
            canRedo={history.future.length > 0}
          />
        }
      />
      <Banners
        save={save}
        refusal={refusal}
        onReload={() => void reload()}
        onOverwrite={() => void store.overwrite()}
        onDismiss={() => setRefusal(null)}
      />
      {searching && (
        <SearchOverlay
          cards={parsed.cards}
          printings={printings}
          format={parsed.format}
          categories={parsed.categories}
          onAdd={onAdd}
          renderResult={renderCardResult}
          onClose={() => setSearching(false)}
        />
      )}
      {scanning && (
        <ScanDialog
          onAdd={(printing) => {
            try {
              history.edit(
                addPrinting(history.present, parsed.cards, printing),
              );
              setRefusal(null);
            } catch (e) {
              setRefusal(e instanceof Error ? e.message : String(e));
            }
          }}
          onClose={() => setScanning(false)}
        />
      )}
      {parsed.cards.length === 0 && (
        <div className="stacks-empty">
          <h2>No cards yet</h2>
          <p>
            Type a name into quick add (<kbd>Ctrl</kbd> <kbd>'</kbd>), or open
            card search and drag results into the deck.
          </p>
        </div>
      )}
      <StacksView
        categories={parsed.categories}
        cards={parsed.cards}
        printings={cards.printings}
        onDrop={onDrop}
        cardProps={cards.cardProps}
      />
      <details className="source">
        <summary>The file</summary>
        <pre>{history.present}</pre>
      </details>
      {cards.overlay}
    </main>
  );
}
