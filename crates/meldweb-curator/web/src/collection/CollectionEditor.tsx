import { Link } from "@tanstack/react-router";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  collectionCommitMessage,
  declarePlace,
  moveOwned,
  type OwnedCard,
  type Place,
  parseCollection,
  setOwnedFinish,
  setOwnedQty,
  undeclarePlace,
} from "../collection";
import type { Finish } from "../deck";
import { Banners, SaveStatus, Toolbar, UndoRedo } from "../deck/Toolbar";
import type { GitHubApi, RepoRef } from "../github/api";
import type { DeckEntry } from "../github/decks";
import { createSaveStore } from "../github/save";
import { useSave } from "../github/useSave";
import { useHistory, useUndoKeys } from "../history";
import { scannerAvailable } from "../probe/scanner";
import { QuickAdd } from "../quickadd/QuickAdd";
import {
  cardName,
  fetchPrintings,
  type Printings,
  printingKey,
} from "../scryfall";
import { LiveScan } from "./LiveScan";
import { addScanned, removeScanned } from "./scanned";
import { addOwnedByName, type Section, sections, UNSORTED } from "./sections";
import "./collection.css";
import { CloseIcon, PlusIcon, ScanIcon, SearchIcon } from "../ui/icons";

export interface CollectionEditorProps {
  /** `collection.toml` in the Magic repo. */
  path: string;
  /** The file as loaded, `""` and no sha while there is none. */
  text: string;
  sha: string | null;
  repo: RepoRef;
  api: GitHubApi;
  printings: Printings;
  /** The repo's decks, which a place can stand for. */
  decks: readonly DeckEntry[];
}

const FINISHES: readonly Finish[] = ["nonfoil", "foil", "etched"];

/**
 * The printings of cards added since the page loaded, looked up as they
 * appear; a card Scryfall cannot find is asked about once.
 */
function useGrowingPrintings(
  loaded: Printings,
  cards: readonly OwnedCard[],
): Printings {
  const [printings, setPrintings] = useState(loaded);
  const asked = useRef(new Set(loaded.keys()));
  useEffect(() => {
    const missing = cards.filter(
      (c) => !asked.current.has(printingKey(c.card)),
    );
    if (missing.length === 0) return;
    for (const c of missing) asked.current.add(printingKey(c.card));
    // Not aborted on cleanup: the keys are already marked asked, so an
    // aborted lookup would never be made again.
    fetchPrintings(missing)
      .then((found) => setPrintings((p) => new Map([...p, ...found])))
      .catch(() => {});
  }, [cards]);
  return printings;
}

/**
 * The collection: every card owned, by the place it is in, saved by itself
 * as a deck is. Mount it with a `key`; it reads its props once and then owns
 * the text.
 */
export function CollectionEditor({
  path,
  text: loaded,
  sha,
  repo,
  api,
  printings: loadedPrintings,
  decks,
}: CollectionEditorProps) {
  const history = useHistory(loaded);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [scanning, setScanning] = useState(false);
  // The text as of the last edit, for the scanner, whose edits land between
  // renders: two cards taken from one frame must each see the other.
  const latest = useRef(loaded);
  latest.current = history.present;
  const parsed = useMemo(
    () => parseCollection(history.present),
    [history.present],
  );
  const cards = parsed.kind === "collection" ? parsed.cards : [];
  const printings = useGrowingPrintings(loadedPrintings, cards);
  const { undo, redo } = history;
  useUndoKeys(undo, redo);

  const [store] = useState(() =>
    createSaveStore({
      api,
      repo,
      path,
      text: loaded,
      sha,
      deckText: { commitMessage: collectionCommitMessage },
    }),
  );
  const save = useSave(store);
  useEffect(() => store.edit(history.present), [store, history.present]);

  if (parsed.kind === "refused") {
    return (
      <main>
        <Toolbar name="Collection" />
        <div className="banners">
          <p className="refusal" role="alert">
            {path} is not a collection this Curator can read: {parsed.message}
          </p>
        </div>
        <details className="source" open>
          <summary>The file</summary>
          <pre>{history.present}</pre>
        </details>
      </main>
    );
  }

  /** Every change is an edit, undoable, and a refusal says why instead. */
  const change = (next: (text: string) => string) => {
    try {
      history.edit(next(history.present));
      setRefusal(null);
    } catch (e) {
      setRefusal(e instanceof Error ? e.message : String(e));
    }
  };

  /** As `change`, on the text as of the last edit; returns the refusal. */
  const scanChange = (next: (text: string) => string): string | null => {
    try {
      const text = next(latest.current);
      latest.current = text;
      history.edit(text);
      return null;
    } catch (e) {
      return e instanceof Error ? e.message : String(e);
    }
  };

  const reload = async () => {
    try {
      history.reset(await store.reload());
    } catch (e) {
      setRefusal(e instanceof Error ? e.message : String(e));
    }
  };

  const nameOf = (c: OwnedCard) => cardName(c, printings);
  const shown = sections(parsed.places, parsed.cards, nameOf, filter);
  const deckNames = new Map(decks.map((d) => [d.path, d.name]));

  return (
    <main>
      <Toolbar
        name="Collection"
        count={parsed.total}
        search={
          <div className="collection-filter">
            <SearchIcon />
            <input
              type="search"
              aria-label="Filter by name"
              placeholder="Filter by name"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>
        }
        quickAdd={
          <QuickAdd
            categories={parsed.places}
            anywhere={UNSORTED}
            onAdd={(name, at) =>
              change((t) =>
                addOwnedByName(t, parsed.cards, printings, name, at),
              )
            }
          />
        }
        actions={
          <>
            {scannerAvailable && (
              <button type="button" onClick={() => setScanning(true)}>
                <ScanIcon />
                Scan
              </button>
            )}
            <NewPlace
              places={parsed.places}
              decks={decks}
              onAdd={(name, deck) => change((t) => declarePlace(t, name, deck))}
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
      {scanning && (
        <LiveScan
          places={parsed.places}
          onAdd={(copy) => scanChange((t) => addScanned(t, copy))}
          onTakeBack={(copy) => scanChange((t) => removeScanned(t, copy))}
          onClose={() => setScanning(false)}
        />
      )}
      {parsed.cards.length === 0 && parsed.places.length === 0 && (
        <div className="stacks-empty">
          <h2>Nothing here yet</h2>
          <p>
            Add cards with quick add, and make places, like a trade binder, a
            bulk box or one of your decks, with New place.
          </p>
        </div>
      )}
      {shown.map((s) => (
        <PlaceSection
          key={s.place?.name ?? ""}
          section={s}
          places={parsed.places}
          printings={printings}
          deckName={s.place?.deck ? deckNames.get(s.place.deck) : undefined}
          change={change}
        />
      ))}
      {filter && shown.length === 0 && (
        <p className="collection-none">No card owned matches “{filter}”.</p>
      )}
      <details className="source">
        <summary>The file</summary>
        <pre>{history.present}</pre>
      </details>
    </main>
  );
}

function PlaceSection({
  section,
  places,
  printings,
  deckName,
  change,
}: {
  section: Section;
  places: readonly Place[];
  printings: Printings;
  /** The name of the deck the place is, when the repo has it. */
  deckName: string | undefined;
  change: (next: (text: string) => string) => void;
}) {
  const { place } = section;
  return (
    <section
      className="place"
      data-place={place?.name ?? undefined}
      aria-label={place?.name ?? UNSORTED}
    >
      <header className="place-header">
        <h2>{place?.name ?? UNSORTED}</h2>
        <span className="badge" title="Cards">
          {section.qty}
        </span>
        {place?.deck &&
          (deckName !== undefined ? (
            <Link
              to="/deck/$"
              params={{ _splat: place.deck }}
              className="place-deck"
            >
              Deck · {deckName}
            </Link>
          ) : (
            <span className="refusal-inline">
              {" "}
              — the deck {place.deck} is not in the repo
            </span>
          ))}
        {place && section.qty === 0 && (
          <button
            type="button"
            className="place-remove small ghost"
            title="Remove this place"
            onClick={() => change((t) => undeclarePlace(t, place.name))}
          >
            Remove place
          </button>
        )}
      </header>
      {section.cards.length > 0 && (
        <table className="owned">
          <thead>
            <tr>
              <th>Qty</th>
              <th>Card</th>
              <th>Printing</th>
              <th>Finish</th>
              <th>Move to</th>
              <th>
                <span className="visually-hidden">Remove</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {section.cards.map((c) => (
              <OwnedRow
                key={c.index}
                card={c}
                places={places}
                printings={printings}
                change={change}
              />
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

function OwnedRow({
  card,
  places,
  printings,
  change,
}: {
  card: OwnedCard;
  places: readonly Place[];
  printings: Printings;
  change: (next: (text: string) => string) => void;
}) {
  const name = cardName(card, printings);
  const image = printings.get(printingKey(card.card))?.image;
  // How many a move takes; all of them unless the user says fewer.
  const [count, setCount] = useState<number | null>(null);
  const moving = Math.min(count ?? card.qty, card.qty);
  const here = card.at ?? null;
  const elsewhere: (string | null)[] = [
    ...(here === null ? [] : [null]),
    ...places.map((p) => p.name).filter((p) => p !== here),
  ];
  return (
    <tr className="owned-row">
      <td className="owned-qty">
        <div className="stepper">
          <button
            type="button"
            aria-label={`One fewer ${name}`}
            onClick={() =>
              change((t) => setOwnedQty(t, card.index, card.qty - 1))
            }
          >
            −
          </button>
          <span>{card.qty}</span>
          <button
            type="button"
            aria-label={`One more ${name}`}
            onClick={() =>
              change((t) => setOwnedQty(t, card.index, card.qty + 1))
            }
          >
            +
          </button>
        </div>
      </td>
      <td className="owned-name">
        <span className="owned-thumb">
          {image && <img src={image} alt="" loading="lazy" />}
        </span>
        <span className="owned-title">{name}</span>
        {image && (
          <img className="owned-preview" src={image} alt="" loading="lazy" />
        )}
      </td>
      <td className="owned-printing">
        {card.card.kind === "printing" ? (
          <span className="set-chip">
            {card.card.set.toUpperCase()} <span>#{card.card.num}</span>
          </span>
        ) : (
          <span className="muted">any</span>
        )}
      </td>
      <td className="owned-finish">
        <select
          aria-label={`Finish of ${name}`}
          value={card.finish}
          onChange={(e) =>
            change((t) =>
              setOwnedFinish(t, card.index, e.target.value as Finish),
            )
          }
        >
          {FINISHES.map((f) => (
            <option key={f} value={f}>
              {f}
            </option>
          ))}
        </select>
      </td>
      <td className="owned-move">
        {card.qty > 1 && (
          <input
            type="number"
            aria-label={`How many ${name} to move`}
            min={1}
            max={card.qty}
            value={moving}
            onChange={(e) => {
              const n = Number(e.target.value);
              if (Number.isInteger(n) && n >= 1) setCount(n);
            }}
          />
        )}
        {elsewhere.length > 0 && (
          <select
            aria-label={`Move ${name} to`}
            value=""
            onChange={(e) => {
              const to = e.target.value === "" ? null : e.target.value;
              setCount(null);
              change((t) => moveOwned(t, card.index, moving, to));
            }}
          >
            <option value="" disabled>
              Move to…
            </option>
            {elsewhere.map((p) => (
              <option key={p ?? ""} value={p ?? ""}>
                {p ?? UNSORTED}
              </option>
            ))}
          </select>
        )}
      </td>
      <td className="owned-remove-cell">
        <button
          type="button"
          className="owned-remove icon ghost small"
          aria-label={`Remove ${name}`}
          title="Remove from the collection"
          onClick={() => change((t) => setOwnedQty(t, card.index, 0))}
        >
          <CloseIcon />
        </button>
      </td>
    </tr>
  );
}

/**
 * New place: a binder, a box, anything cards sit in, or one of the repo's
 * decks, for the cards sleeved in it.
 */
function NewPlace({
  places,
  decks,
  onAdd,
}: {
  places: readonly Place[];
  decks: readonly DeckEntry[];
  onAdd: (name: string, deck: string | undefined) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [name, setName] = useState("");
  const [deck, setDeck] = useState("");
  const placed = new Set(places.flatMap((p) => (p.deck ? [p.deck] : [])));
  const free = decks.filter((d) => !d.refused && !placed.has(d.path));
  const close = () => {
    setName("");
    setDeck("");
    dialog.current?.close();
  };
  return (
    <>
      <button type="button" onClick={() => dialog.current?.showModal()}>
        <PlusIcon />
        New place
      </button>
      <dialog ref={dialog} className="sheet" aria-label="New place">
        <form
          method="dialog"
          onSubmit={(e) => {
            e.preventDefault();
            if (!name.trim()) return;
            onAdd(name.trim(), deck || undefined);
            close();
          }}
        >
          <h2>New place</h2>
          <label className="field">
            Name
            <input
              value={name}
              placeholder="Trade binder"
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          <label className="field">
            It is a deck
            <select
              value={deck}
              onChange={(e) => {
                const path = e.target.value;
                setDeck(path);
                const picked = decks.find((d) => d.path === path);
                if (picked && !name.trim()) setName(picked.name);
              }}
            >
              <option value="">No, somewhere cards are kept</option>
              {free.map((d) => (
                <option key={d.path} value={d.path}>
                  {d.name}
                </option>
              ))}
            </select>
          </label>
          <p className="hint">
            A card is in one place at a time. A deck place holds the copies
            sleeved in that deck.
          </p>
          <div className="dialog-buttons">
            <button type="button" onClick={close}>
              Cancel
            </button>
            <button type="submit" className="primary" disabled={!name.trim()}>
              Add place
            </button>
          </div>
        </form>
      </dialog>
    </>
  );
}
