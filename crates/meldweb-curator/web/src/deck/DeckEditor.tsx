import { Link, useNavigate } from "@tanstack/react-router";
import { useEffect, useMemo, useRef, useState } from "react";
import { bestPrinting } from "../card/bestPrinting";
import { useSettings } from "../card/preference";
import { renderCardResult } from "../card/searchResult";
import { useCardEditor } from "../card/useCardEditor";
import {
  applyChanges,
  type Dest,
  deckAdd,
  editDeckCards,
  type Finish,
  parseDeck,
  setCardFinish,
  setCardPrinting,
  setDeckDescription,
} from "../deck";
import type { PublicRepos } from "../github/api";
import { MAIN, makeBranch } from "../github/branches";
import { createDeck, deckFile } from "../github/decks";
import { deckText } from "../github/deckText";
import {
  deckSnapshots,
  head,
  type Snapshot,
  takeSnapshot,
} from "../github/history";
import { createSaveStore } from "../github/save";
import { useSave } from "../github/useSave";
import { useHistory, useUndoKeys } from "../history";
import { ScanDialog } from "../probe/ScanDialog";
import { scannerAvailable } from "../probe/scanner";
import { type AddByName, QuickAdd } from "../quickadd/QuickAdd";
import { ConflictError, type Repo } from "../repo/api";
import { fetchPrintings, type Printings } from "../scryfall";
import { SearchButton, SearchOverlay } from "../search/SearchOverlay";
import { BranchIcon, HistoryIcon, ScanIcon } from "../ui/icons";
import { messageOf } from "../ui/Sheet";
import { printingNames } from "./archidektNames";
import { BuildView } from "./BuildView";
import { CopyDeck } from "./CopyDeck";
import { CostView } from "./CostView";
import { Description } from "./Description";
import { HistoryDrawer } from "./HistoryDrawer";
import { MergeDialog } from "./MergeDialog";
import "./history.css";
import { PlaytestArchidekt } from "./PlaytestArchidekt";
import { PrintProxies } from "./PrintProxies";
import { RefPicker } from "./RefPicker";
import { ReplaceArchidekt } from "./ReplaceArchidekt";
import { applyResolved, namesOnly, resolveNames } from "./resolveNames";
import { SourcingView } from "./SourcingView";
import { type OnDrop, StacksView } from "./StacksView";
import { Banners, SaveStatus, Toolbar, UndoRedo } from "./Toolbar";
import { type ParsedDeck, useOther } from "./useOther";
import { useSupply } from "./useSupply";
import { BranchDialog, ForkDialog, SnapshotDialog } from "./VersionDialogs";
import { VersionView, when } from "./VersionView";
import { ViewTabs } from "./ViewTabs";
import type { DeckPage, Viewing } from "./versions";

/** The deck as its `main` has it: the built deck (ADR-0041). */
export interface BuiltDeck {
  text: string;
  deck: ParsedDeck;
  printings: Printings;
}

/** Main's list, read for a branch's page; null when main holds no deck it reads. */
export async function readBuilt(
  repo: Repo,
  stem: string,
): Promise<BuiltDeck | null> {
  const { path, branch } = deckFile(stem);
  const file = await repo.getFile(path, branch);
  if (!file) return null;
  const parsed = parseDeck(file.text);
  if (parsed.kind !== "deck") return null;
  return {
    text: file.text,
    deck: parsed,
    printings: await fetchPrintings(parsed.cards).catch(() => new Map()),
  };
}

export interface DeckEditorProps {
  /** The deck's stem: it is `decks/<stem>/main`, its file `<stem>.deck.toml`. */
  stem: string;
  /** The branch open for editing, `decks/<stem>/<branch>`: `main` or another. */
  branch: string;
  /**
   * Off main, main's list: what the views about the built deck read. Null on
   * main, whose list is the one being edited.
   */
  built: BuiltDeck | null;
  /** The deck it was forked from, as its history says (ADR-0041). */
  parent?: string;
  /** The file as loaded from GitHub, and its blob sha. */
  text: string;
  sha: string;
  repo: Repo;
  /** Who the user is, to a friend: as the session says. */
  me: string;
  /** Friends' public repos, which Sourcing reads. */
  others: PublicRepos;
  printings: Printings;
  /** The deck now or another version of it, the drawer, and the view, as the URL says. */
  page: DeckPage;
  /** Goes to another version, view or drawer state, through the URL. */
  onNavigate: (change: Partial<DeckPage>) => void;
}

type Dialog =
  /** `commit: null` is the deck as it is now, once saved. */
  | { kind: "snapshot"; commit: string | null; of: string }
  /**
   * A fork of `from`'s deck at `commit` (`null`: its tip, once saved), whose
   * text is `text`; `of` says which version, for the reader.
   */
  | {
      kind: "fork";
      text: string;
      of: string;
      from: string;
      commit: string | null;
    }
  /** A branch at `commit` (`null`: the open branch's tip, once saved). */
  | { kind: "branch"; commit: string | null; of: string; name: string }
  | { kind: "merge"; from: string; into: string };

/**
 * One open deck: the stacks, the toolbar, undo, and saving by itself. Mount it
 * with `key={stem}`; it reads its props once and then owns the text.
 */
export function DeckEditor({
  stem,
  branch,
  built: builtAtLoad,
  parent,
  text: loaded,
  sha,
  repo,
  me,
  others,
  printings: loadedPrintings,
  page,
  onNavigate,
}: DeckEditorProps) {
  const { viewing, drawer, view } = page;
  const history = useHistory(loaded);
  // What an add that finishes later builds on: the text as of then.
  const present = useRef(history.present);
  present.current = history.present;
  const settings = useSettings();
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
    createSaveStore({
      repo,
      ...deckFile(stem, branch),
      text: loaded,
      sha,
      // Another tab's or device's edits, merged in, undo as one step.
      onMerge: (text) => history.edit(text),
    }),
  );
  const save = useSave(store);
  // Every change to the text, edit, undo or redo alike, is what gets saved.
  useEffect(() => store.edit(history.present), [store, history.present]);

  useUndoKeys(undo, redo);

  const navigate = useNavigate();
  const other = useOther(repo, stem, viewing);
  const onMain = branch === MAIN;
  // Main's list off main, read again after a merge into it.
  const [mainDeck, setMainDeck] = useState(builtAtLoad);
  // What is about the built deck reads main only (ADR-0041): on main the deck
  // being edited, on a branch main's list.
  const built: BuiltDeck | null = onMain
    ? parsed.kind === "deck"
      ? { text: history.present, deck: parsed, printings: cards.printings }
      : null
    : mainDeck;
  // The collection and friends, read once a view asks what the deck is made of.
  const supply = useSupply({
    active:
      built !== null &&
      viewing.kind === "now" &&
      (view === "sourcing" || view === "build"),
    repo,
    others,
    stem,
    text: built?.text ?? "",
    printings: built?.printings ?? cards.printings,
  });
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // Bumped when a snapshot or branch is made, merged or deleted, so the
  // drawer and the picker read them again.
  const [snapshots, setSnapshots] = useState(0);
  // The snapshots, so a branch made from one is said to start from its name.
  const [tags, setTags] = useState<Snapshot[]>([]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `snapshots` reads them again
  useEffect(() => {
    let live = true;
    void deckSnapshots(repo, stem).then(
      (t) => live && setTags(t),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [repo, stem, snapshots]);
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 5000);
    return () => clearTimeout(t);
  }, [notice]);
  // A deck opened with cards by name only takes each one's printing, as an
  // added card would, so the file says which copy the deck is. One edit, saved
  // like any other and undone with one Ctrl+Z.
  // biome-ignore lint/correctness/useExhaustiveDependencies: once, on open, with the settings it opened with
  useEffect(() => {
    const names = namesOnly(loaded);
    if (names.length === 0) return;
    let live = true;
    void resolveNames(names, settings).then((resolved) => {
      if (!live) return;
      const { text, count } = applyResolved(present.current, resolved);
      if (count === 0) return;
      history.edit(text);
      setNotice(
        `Gave ${count} card${count === 1 ? "" : "s"} listed by name ${count === 1 ? "its" : "their"} printing. Ctrl+Z undoes it.`,
      );
    });
    return () => {
      live = false;
    };
  }, []);
  const go = (v: Viewing) => onNavigate({ viewing: v });
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
      at = (await head(repo, stem, branch))?.commit ?? null;
    }
    if (at === null) return "The deck has no save to snapshot yet.";
    const taken = await takeSnapshot(repo, stem, label, at);
    if (taken.kind === "refused") return taken.message;
    setSnapshots((n) => n + 1);
    setNotice(`Snapshot “${taken.snapshot.label}” taken.`);
    return null;
  };

  /** Forks `from` (this deck, or the one compared with) at `commit`. */
  const fork = async (
    text: string,
    from: string,
    commit: string | null,
    name: string,
  ) => {
    let at = commit;
    if (at === null && from === stem) {
      if (!(await store.settle()))
        return "Your latest edits are not saved, so there is no version of them to fork yet.";
      at = (await head(repo, stem, branch))?.commit ?? null;
    } else if (at === null) {
      at = (await repo.decks()).find((d) => d.stem === from)?.commit ?? null;
    }
    if (at === null) return "The deck has no save to fork from yet.";
    const made = await createDeck(
      repo,
      name,
      { kind: "fork", text, of: from, at },
      deckText,
    );
    if (made.kind === "refused") return made.message;
    void navigate({
      to: "/deck/$",
      params: { _splat: made.stem },
      search: { history: true, vs: from },
    });
    return null;
  };

  /** Opens the deck's branch `name` for editing. */
  const openBranch = (name: string) =>
    onNavigate({ branch: name, viewing: { kind: "now" } });

  /** Makes a branch at `commit` (the open branch's tip when null) and opens it. */
  const branchFrom = async (commit: string | null, label: string) => {
    let at = commit;
    if (at === null) {
      if (!(await store.settle()))
        return "Your latest edits are not saved, so there is no version of them to branch from yet.";
      at = (await head(repo, stem, branch))?.commit ?? null;
    }
    if (at === null) return "The deck has no save to branch from yet.";
    const made = await makeBranch(repo, stem, label, at);
    if (made.kind === "refused") return made.message;
    setSnapshots((n) => n + 1);
    openBranch(made.branch.name);
    return null;
  };

  /** Lands this branch's pending saves before a merge reads it. */
  const prepare = async () =>
    (await store.settle())
      ? null
      : "Your latest edits are not saved, so they cannot be merged yet.";

  const deleteBranch = async () => {
    if (!(await store.settle())) {
      setRefusal(
        "Your latest edits are not saved, so the branch is kept until they are.",
      );
      return;
    }
    try {
      const it = (await repo.branches(stem)).find((b) => b.name === branch);
      if (!it) return openBranch(MAIN);
      if (
        it.unmerged > 0 &&
        !window.confirm(
          `${branch} has ${it.unmerged} save${it.unmerged === 1 ? "" : "s"} main doesn't. Delete the branch and lose ${it.unmerged === 1 ? "it" : "them"}?`,
        )
      )
        return;
      await repo.deleteBranch(stem, branch, it.commit);
      openBranch(MAIN);
    } catch (e) {
      setRefusal(
        e instanceof ConflictError
          ? `${branch} changed meanwhile, so it was not deleted. Try again.`
          : messageOf(e),
      );
    }
  };

  /** Where a new branch or snapshot would start, as the picker says it. */
  const shownSnapshot =
    viewing.kind === "revision"
      ? tags.find((t) => t.commit === viewing.commit)
      : undefined;
  const here =
    viewing.kind === "revision"
      ? shownSnapshot
        ? shownSnapshot.label
        : other?.kind === "loaded" && other.revision
          ? `the version of ${when(other.revision.date)}`
          : "this version"
      : branch;

  const picker = (
    <RefPicker
      repo={repo}
      stem={stem}
      branch={branch}
      viewing={viewing}
      refresh={snapshots}
      from={here}
      onBranch={openBranch}
      onView={go}
      onNewBranch={(name) =>
        setDialog({
          kind: "branch",
          commit: viewing.kind === "revision" ? viewing.commit : null,
          of: here,
          name,
        })
      }
      onSnapshot={() =>
        setDialog({
          kind: "snapshot",
          commit: viewing.kind === "revision" ? viewing.commit : null,
          of: viewing.kind === "revision" ? here : "the deck as it is now",
        })
      }
    />
  );

  const historyButton = (
    <button
      type="button"
      aria-pressed={drawer}
      onClick={() => onNavigate({ drawer: !drawer })}
      title="Every save of this deck, its snapshots and its forks"
    >
      <HistoryIcon />
      History
    </button>
  );
  const drawerView = drawer && (
    <HistoryDrawer
      repo={repo}
      stem={stem}
      branch={branch}
      name={parsed.kind === "deck" ? (parsed.name ?? stem) : stem}
      parent={parent}
      viewing={viewing}
      saveStatus={save.status}
      refresh={snapshots}
      onView={go}
      onClose={() => onNavigate({ drawer: false })}
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
      onFork={() =>
        setDialog({
          kind: "fork",
          text: history.present,
          of: "the deck as it is now",
          from: stem,
          commit: null,
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
    ) : dialog?.kind === "fork" ? (
      <ForkDialog
        parent={
          dialog.from !== stem
            ? dialog.from
            : parsed.kind === "deck"
              ? (parsed.name ?? stem)
              : stem
        }
        of={dialog.of}
        onClose={() => setDialog(null)}
        onCreate={(name) => fork(dialog.text, dialog.from, dialog.commit, name)}
      />
    ) : dialog?.kind === "branch" ? (
      <BranchDialog
        stem={stem}
        of={dialog.of}
        initial={dialog.name}
        onClose={() => setDialog(null)}
        onCreate={(name) => branchFrom(dialog.commit, name)}
      />
    ) : dialog?.kind === "merge" ? (
      <MergeDialog
        repo={repo}
        stem={stem}
        from={dialog.from}
        into={dialog.into}
        prepare={prepare}
        onClose={() => setDialog(null)}
        onMerged={(done) => {
          const { from, into } = dialog;
          setSnapshots((n) => n + 1);
          if (into === branch) {
            // The open branch moved under the editor: take what it holds now.
            void store
              .reload()
              .then(history.reset, (e) => setRefusal(messageOf(e)));
          }
          if (into === MAIN && !onMain) {
            void readBuilt(repo, stem).then(setMainDeck);
          }
          setNotice(
            done.message === undefined
              ? `${into} fast-forwarded to ${from}.`
              : `Merged ${from} into ${into}: ${done.message.split("\n", 1)[0]}`,
          );
        }}
      />
    ) : null;

  if (parsed.kind === "refused") {
    return (
      <main>
        <Toolbar
          name={
            <>
              {stem}
              {picker}
            </>
          }
          actions={historyButton}
        />
        <div className="banners">
          <p className="refusal" role="alert">
            {deckFile(stem).path} is not a deck Progress Engine can read:{" "}
            {parsed.message}
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
    try {
      // The strip's Sideboard is the deck's sideboard-typed category, and a
      // new category is declared untyped, either made if the deck lacks it.
      const dest: Dest =
        to.kind === "type"
          ? { kind: "board", board: to.type }
          : { kind: "category", name: to.name };
      history.edit(
        editDeckCards(history.present, { kind: "move", to: dest, secondary }, [
          { index: card.index, from },
        ]).text,
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
      const {
        text: after,
        line,
        made,
      } = deckAdd(
        history.present,
        { kind: "name", name },
        category === null
          ? { kind: "automatic" }
          : { kind: "categories", categories: [category] },
        printingNames(parsed.cards, printings),
      );
      history.edit(after);
      setRefusal(null);
      if (!made) return;
      bestPrinting(name, settings)
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

  // A line put on another printing, in the finish given.
  const reprint = (
    index: number,
    printing: { set: string; num: string },
    finish: Finish,
  ) => {
    try {
      const now = history.present;
      const moved = setCardPrinting(now, index, printing.set, printing.num);
      const card = parsed.cards[index];
      // One edit, so one undo puts the line back as it was.
      history.edit(
        finish === card?.finish ? moved : setCardFinish(moved, index, finish),
      );
      setRefusal(null);
    } catch (e) {
      setRefusal(messageOf(e));
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
            {picker}
            {parent && (
              <button
                type="button"
                className="variant-chip"
                onClick={() =>
                  onNavigate({ viewing: { kind: "deck", stem: parent } })
                }
                title="Compare with the deck this was forked from"
              >
                <BranchIcon />
                fork of {parent}
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
            <CopyDeck
              text={history.present}
              cards={parsed.cards}
              printings={printings}
              onRefusal={setRefusal}
            />
            {built && (
              <PrintProxies
                repo={repo}
                stem={stem}
                name={stem}
                text={built.text}
                cards={built.deck.cards}
                printings={built.printings}
                {...(onMain ? {} : { of: MAIN })}
              />
            )}
            <Link
              to="/trade"
              search={{ deck: stem }}
              className="button"
              title="What another player's collection holds of what this deck lacks"
            >
              Trade for…
            </Link>
            <ReplaceArchidekt
              name={parsed.name ?? stem}
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
        status={
          <SaveStatus
            save={save}
            path={`${deckFile(stem, branch).branch}: ${deckFile(stem).path}`}
          />
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
      <Banners
        save={save}
        refusal={refusal}
        onReload={() => void reload()}
        onOverwrite={() => void store.overwrite()}
        onDismiss={() => setRefusal(null)}
        notice={notice}
      />
      {!onMain && (
        <div className="branch-bar">
          <span className="hint">
            Branch <strong>{branch}</strong>. Main is the built deck.
          </span>
          <button
            type="button"
            className="small"
            onClick={() =>
              setDialog({ kind: "merge", from: branch, into: MAIN })
            }
          >
            Merge into main…
          </button>
          <button
            type="button"
            className="small"
            onClick={() =>
              setDialog({ kind: "merge", from: MAIN, into: branch })
            }
            title="Bring main's changes since this branch left it into the branch"
          >
            Update from main…
          </button>
          <button
            type="button"
            className="small ghost"
            onClick={() => void deleteBranch()}
          >
            Delete branch
          </button>
        </div>
      )}
      {version && (
        <VersionView
          viewing={version.viewing}
          other={version.other}
          present={{ text: history.present, deck: parsed, printings }}
          stem={stem}
          onTake={take}
          onBack={() => go({ kind: "now" })}
          onBranch={(commit) =>
            setDialog({ kind: "branch", commit, of: here, name: "" })
          }
          onSnapshot={(commit) =>
            setDialog({ kind: "snapshot", commit, of: otherOf })
          }
          onFork={(text) =>
            setDialog({
              kind: "fork",
              text,
              of: otherOf,
              from: viewing.kind === "deck" ? viewing.stem : stem,
              commit: viewing.kind === "revision" ? viewing.commit : null,
            })
          }
        />
      )}
      {!looking && (
        <>
          <ViewTabs view={view} onView={(v) => onNavigate({ view: v })} />
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
              onAdd={({ set, num, name }) => {
                try {
                  history.edit(
                    deckAdd(history.present, {
                      kind: "printing",
                      set,
                      num,
                      name,
                    }).text,
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
          {view === "stacks" ? (
            <StacksView
              categories={parsed.categories}
              cards={parsed.cards}
              printings={cards.printings}
              onDrop={onDrop}
              cardProps={cards.cardProps}
            />
          ) : view === "cost" ? (
            <CostView
              cards={parsed.cards}
              printings={cards.printings}
              onUse={(card, { printing, finish }) =>
                reprint(card.index, printing, finish)
              }
            />
          ) : !built ? (
            <p className="refusal" role="alert">
              Main holds no deck Progress Engine reads, and{" "}
              {view === "build" ? "Build" : "Sourcing"} is main's.
            </p>
          ) : (
            <>
              {!onMain && (
                <p className="hint built-note">
                  {view === "build" ? "Build" : "Sourcing"} shows main, the deck
                  you build; this branch is not built.{" "}
                  <button
                    type="button"
                    className="small ghost"
                    onClick={() => openBranch(MAIN)}
                  >
                    Open main
                  </button>
                </p>
              )}
              {view === "sourcing" ? (
                <SourcingView
                  state={supply.state}
                  cards={built.deck.cards}
                  deckName={built.deck.name ?? stem}
                  me={me}
                  // A printing picked changes the list it is picked on.
                  {...(onMain ? { onUse: reprint } : {})}
                />
              ) : (
                <BuildView
                  supply={supply}
                  cards={built.deck.cards}
                  stem={stem}
                  deckName={built.deck.name ?? stem}
                  onSourcing={() => onNavigate({ view: "sourcing" })}
                />
              )}
            </>
          )}
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
