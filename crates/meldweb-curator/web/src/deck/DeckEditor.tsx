import { useNavigate } from "@tanstack/react-router";
import { useEffect, useMemo, useRef, useState } from "react";
import { withBoard } from "../card/apply";
import { addedLine, bestPrinting } from "../card/bestPrinting";
import { usePins, useSettings } from "../card/preference";
import { renderCardResult } from "../card/searchResult";
import { useCardEditor } from "../card/useCardEditor";
import {
  applyChanges,
  declareCategory,
  parseDeck,
  setCardCategories,
  setCardPrinting,
  setDeckDescription,
} from "../deck";
import type { GitHubApi, RepoRef } from "../github/api";
import { createDeck, deckStem } from "../github/decks";
import { deckText } from "../github/deckText";
import { head, takeSnapshot } from "../github/history";
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
import { BranchIcon, HistoryIcon, ScanIcon } from "../ui/icons";
import { messageOf } from "../ui/Sheet";
import { CopyArchidekt } from "./CopyArchidekt";
import { Description } from "./Description";
import { HistoryDrawer } from "./HistoryDrawer";
import "./history.css";
import { dropOnto } from "./move";
import { PlaytestArchidekt } from "./PlaytestArchidekt";
import { ReplaceArchidekt } from "./ReplaceArchidekt";
import { type OnDrop, StacksView } from "./StacksView";
import { Banners, SaveStatus, Toolbar, UndoRedo } from "./Toolbar";
import { useOther } from "./useOther";
import { SnapshotDialog, VariantDialog } from "./VersionDialogs";
import { VersionView, when } from "./VersionView";
import type { Viewing } from "./versions";

export interface DeckEditorProps {
  /** The deck's path in the Magic repo, e.g. `decks/lantern.deck.toml`. */
  path: string;
  /** The file as loaded from GitHub, and its blob sha. */
  text: string;
  sha: string;
  repo: RepoRef;
  api: GitHubApi;
  printings: Printings;
  /** The deck now, or another version of it shown in place of its stacks. */
  viewing: Viewing;
  /** Whether the history drawer is open. */
  drawer: boolean;
  /** Goes to another version, or opens or shuts the drawer, through the URL. */
  onNavigate: (viewing: Viewing, drawer: boolean) => void;
}

type Dialog =
  /** `commit: null` is the deck as it is now, once saved. */
  | { kind: "snapshot"; commit: string | null; of: string }
  | { kind: "variant"; text: string; of: string };

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
  viewing,
  drawer,
  onNavigate,
}: DeckEditorProps) {
  const history = useHistory(loaded);
  // What an add that finishes later builds on: the text as of then.
  const present = useRef(history.present);
  present.current = history.present;
  const settings = useSettings();
  const pins = usePins()?.pins ?? [];
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

  const navigate = useNavigate();
  const other = useOther(api, repo, path, viewing);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // Bumped when a snapshot is taken, so the drawer reads them again.
  const [snapshots, setSnapshots] = useState(0);
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 5000);
    return () => clearTimeout(t);
  }, [notice]);
  const go = (v: Viewing) => onNavigate(v, drawer);
  const stem = deckStem(path);
  const otherOf =
    other?.kind === "loaded" && other.revision
      ? `the deck as it was on ${when(other.revision.date)}`
      : other?.kind === "loaded"
        ? other.name
        : "the deck as it is now";

  const snapshot = async (commit: string | null, label: string) => {
    let at = commit;
    if (at === null) {
      if (!(await store.settle()))
        return "Your latest edits are not saved, so there is no version of them to snapshot yet.";
      at = (await head(api, repo, path))?.commit ?? null;
    }
    if (at === null) return "The deck has no save to snapshot yet.";
    const taken = await takeSnapshot(api, repo, path, label, at);
    if (taken.kind === "refused") return taken.message;
    setSnapshots((n) => n + 1);
    setNotice(`Snapshot “${taken.snapshot.label}” taken.`);
    return null;
  };

  const variant = async (text: string, name: string) => {
    const made = await createDeck(
      api,
      repo,
      name,
      { kind: "variant", text, of: path },
      deckText,
    );
    if (made.kind === "refused") return made.message;
    void navigate({
      to: "/deck/$",
      params: { _splat: made.path },
      search: { history: true, vs: path },
    });
    return null;
  };

  const historyButton = (
    <button
      type="button"
      aria-pressed={drawer}
      onClick={() => onNavigate(viewing, !drawer)}
      title="Every save of this deck, its snapshots and its variants"
    >
      <HistoryIcon />
      History
    </button>
  );
  const drawerView = drawer && (
    <HistoryDrawer
      api={api}
      repo={repo}
      path={path}
      stem={parsed.kind === "deck" ? (parsed.name ?? stem) : stem}
      variantOf={parsed.kind === "deck" ? parsed.variantOf : undefined}
      viewing={viewing}
      saveStatus={save.status}
      refresh={snapshots}
      onView={go}
      onClose={() => onNavigate(viewing, false)}
      onSnapshot={(commit) =>
        setDialog({
          kind: "snapshot",
          commit,
          of:
            commit === null
              ? "the deck as it is now"
              : "that version of the deck",
        })
      }
      onVariant={() =>
        setDialog({
          kind: "variant",
          text: history.present,
          of: "the deck as it is now",
        })
      }
    />
  );
  const dialogView =
    dialog?.kind === "snapshot" ? (
      <SnapshotDialog
        of={dialog.of}
        onClose={() => setDialog(null)}
        onTake={(label) => snapshot(dialog.commit, label)}
      />
    ) : dialog?.kind === "variant" ? (
      <VariantDialog
        parent={parsed.kind === "deck" ? (parsed.name ?? stem) : stem}
        of={dialog.of}
        onClose={() => setDialog(null)}
        onCreate={(name) => variant(dialog.text, name)}
      />
    ) : null;

  if (parsed.kind === "refused") {
    return (
      <main>
        <Toolbar name={deckStem(path)} actions={historyButton} />
        <div className="banners">
          <p className="refusal" role="alert">
            {path} is not a deck Curator can read: {parsed.message}
          </p>
        </div>
        <details className="source" open>
          <summary>The file</summary>
          <pre>{history.present}</pre>
        </details>
        {drawerView}
        {dialogView}
      </main>
    );
  }

  const take = (take: readonly number[], text: string, all: boolean) => {
    try {
      history.edit(applyChanges(history.present, text, take));
      setRefusal(null);
      if (all) {
        go({ kind: "now" });
        setNotice(
          viewing.kind === "revision"
            ? "Restored that version. Ctrl+Z undoes it."
            : "Took every change. Ctrl+Z undoes it.",
        );
      } else {
        setNotice(
          `Took ${take.length} change${take.length === 1 ? "" : "s"}. Ctrl+Z undoes ${take.length === 1 ? "it" : "them"}.`,
        );
      }
    } catch (e) {
      setRefusal(messageOf(e));
    }
  };
  // Another version in place of the stacks, once the URL names one.
  const version =
    viewing.kind !== "now" && other !== null ? { viewing, other } : null;
  const looking = version !== null;

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

  // Quick add and the search overlay add one copy of a card, undoably. A new
  // line goes in by name at once, then takes the user's printing of the card:
  // its pin, or the one the rules rank first.
  const onAdd: AddByName = (name, category) => {
    try {
      const after = addByName(
        history.present,
        parsed.cards,
        printings,
        name,
        category,
      );
      history.edit(after);
      setRefusal(null);
      const added = parseDeck(after);
      const line =
        added.kind === "deck"
          ? addedLine(parsed.cards, added.cards, name)
          : null;
      if (line === null) return;
      bestPrinting(name, settings, pins)
        .then((p) => {
          if (!p) return;
          const now = present.current;
          const still = parseDeck(now);
          const card = still.kind === "deck" ? still.cards[line] : undefined;
          // An edit since may have moved or named the line; then leave it.
          if (card?.card.kind !== "name" || card.card.name !== name) return;
          // Untouched since, the printing is part of the add, one undo.
          history.edit(setCardPrinting(now, line, p.set, p.num), now === after);
        })
        // Scryfall out: the line stays by name, as it always could be.
        .catch(() => {});
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
    <main className={drawer ? "with-drawer" : undefined}>
      <Toolbar
        name={
          <>
            {parsed.name ?? stem}
            {parsed.variantOf && (
              <button
                type="button"
                className="variant-chip"
                onClick={() =>
                  onNavigate(
                    { kind: "deck", path: parsed.variantOf ?? "" },
                    drawer,
                  )
                }
                title="Compare with the deck this is a variant of"
              >
                <BranchIcon />
                variant of {deckStem(parsed.variantOf)}
              </button>
            )}
          </>
        }
        count={parsed.total}
        search={
          looking ? undefined : (
            <SearchButton onClick={() => setSearching(true)} />
          )
        }
        quickAdd={
          looking ? undefined : (
            <QuickAdd categories={parsed.categories} onAdd={onAdd} />
          )
        }
        actions={
          <>
            {historyButton}
            {scannerAvailable && !looking && (
              <button type="button" onClick={() => setScanning(true)}>
                <ScanIcon />
                Scan
              </button>
            )}
            <PlaytestArchidekt
              cards={parsed.cards}
              printings={printings}
              onRefusal={setRefusal}
            />
            <CopyArchidekt
              text={history.present}
              cards={parsed.cards}
              printings={printings}
              onRefusal={setRefusal}
            />
            <ReplaceArchidekt
              name={parsed.name ?? deckStem(path)}
              format={parsed.format}
              onReplace={(text) => {
                // Archidekt has no description to replace it with.
                history.edit(
                  setDeckDescription(text, parsed.description ?? ""),
                );
                setRefusal(null);
              }}
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
        notice={notice}
      />
      {version && (
        <VersionView
          viewing={version.viewing}
          other={version.other}
          present={{ text: history.present, deck: parsed, printings }}
          stem={stem}
          onTake={take}
          onBack={() => go({ kind: "now" })}
          onSnapshot={(commit) =>
            setDialog({ kind: "snapshot", commit, of: otherOf })
          }
          onVariant={(text) =>
            setDialog({ kind: "variant", text, of: otherOf })
          }
        />
      )}
      {!looking && (
        <>
          <Description
            text={parsed.description}
            onSave={(description) => {
              try {
                history.edit(setDeckDescription(history.present, description));
                setRefusal(null);
              } catch (e) {
                setRefusal(e instanceof Error ? e.message : String(e));
              }
            }}
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
                Type a name into quick add (<kbd>Ctrl</kbd> <kbd>'</kbd>), or
                open card search and drag results into the deck.
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
        </>
      )}
      {cards.overlay}
      {drawerView}
      {dialogView}
    </main>
  );
}
