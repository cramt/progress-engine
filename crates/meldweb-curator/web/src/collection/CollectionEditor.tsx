import { Link } from "@tanstack/react-router";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  collectionCommitMessage,
  declarePlace,
  moveOwned,
  moveOwnedLines,
  type OwnedCard,
  type Place,
  parseCollection,
  reprintOwned,
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
  type Currency,
  cardName,
  fetchPrices,
  fetchPrintings,
  type Printings,
  printingKey,
} from "../scryfall";
import { LiveScan } from "./LiveScan";
import {
  CURRENCIES,
  formatPrice,
  loadCurrency,
  type PriceBook,
  saveCurrency,
  unitPrice,
  worth,
} from "./prices";
import { ReprintDialog } from "./ReprintDialog";
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

/** A move's option for unsorted, which no place can be named. */
const TO_UNSORTED = "\u0000unsorted";

const NOTHING: ReadonlySet<number> = new Set();

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
 * Today's price of every card owned, looked up after the page has opened, so
 * a slow or failed lookup costs the prices and nothing else.
 */
function usePrices(cards: readonly OwnedCard[]): PriceBook {
  const [prices, setPrices] = useState<PriceBook>(new Map());
  const asked = useRef(new Set<string>());
  useEffect(() => {
    const missing = cards.filter(
      (c) => !asked.current.has(printingKey(c.card)),
    );
    if (missing.length === 0) return;
    for (const c of missing) asked.current.add(printingKey(c.card));
    fetchPrices(missing)
      .then((found) => setPrices((p) => new Map([...p, ...found])))
      .catch(() => {});
  }, [cards]);
  return prices;
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
  const [reprinting, setReprinting] = useState<number | null>(null);
  // The lines ticked, by index, as of the text they were ticked in: an edit
  // moves indices (an undo included), so any edit clears them.
  const [ticked, setTicked] = useState<{
    text: string;
    lines: ReadonlySet<number>;
  }>({ text: loaded, lines: NOTHING });
  const selected = ticked.text === history.present ? ticked.lines : NOTHING;
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
  const prices = usePrices(cards);
  const [currency, setCurrency] = useState(loadCurrency);
  const { undo, redo } = history;
  useUndoKeys(undo, redo);

  const hasSelection = selected.size > 0;
  const dialogOpen = scanning || reprinting !== null;
  useEffect(() => {
    if (!hasSelection || dialogOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setTicked({ text: "", lines: NOTHING });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [hasSelection, dialogOpen]);

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

  /**
   * Every change is an edit, undoable, and a refusal says why instead;
   * returns whether it went in.
   */
  const change = (next: (text: string) => string): boolean => {
    try {
      history.edit(next(history.present));
      setRefusal(null);
      return true;
    } catch (e) {
      setRefusal(e instanceof Error ? e.message : String(e));
      return false;
    }
  };

  const tick = (indices: readonly number[], on: boolean) => {
    const lines = new Set(selected);
    for (const i of indices) {
      if (on) lines.add(i);
      else lines.delete(i);
    }
    setTicked({ text: history.present, lines });
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
  const reprinted =
    reprinting === null
      ? undefined
      : parsed.cards.find((c) => c.index === reprinting);
  const selectedCopies = parsed.cards
    .filter((c) => selected.has(c.index))
    .reduce((n, c) => n + c.qty, 0);

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
      {parsed.cards.length > 0 && (
        <CollectionWorth
          cards={parsed.cards}
          prices={prices}
          currency={currency}
          onCurrency={(c) => {
            setCurrency(c);
            saveCurrency(c);
          }}
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
          prices={prices}
          currency={currency}
          deckName={s.place?.deck ? deckNames.get(s.place.deck) : undefined}
          selected={selected}
          onTick={tick}
          onReprint={setReprinting}
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
      {hasSelection && (
        <section className="bulk-bar" aria-label="Selected cards">
          <span>
            {selected.size} {selected.size === 1 ? "line" : "lines"},{" "}
            {selectedCopies} {selectedCopies === 1 ? "card" : "cards"}
          </span>
          <select
            aria-label="Move the selected cards to"
            value=""
            onChange={(e) => {
              const to = e.target.value === TO_UNSORTED ? null : e.target.value;
              change((t) => moveOwnedLines(t, [...selected], to));
            }}
          >
            <option value="" disabled>
              Move to…
            </option>
            <option value={TO_UNSORTED}>{UNSORTED}</option>
            {parsed.places.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name}
              </option>
            ))}
          </select>
          <button
            type="button"
            className="ghost"
            title="Clear the selection (Esc)"
            onClick={() => setTicked({ text: "", lines: NOTHING })}
          >
            Clear
          </button>
        </section>
      )}
      {reprinted && (
        <ReprintDialog
          key={reprinted.index}
          card={reprinted}
          name={nameOf(reprinted)}
          printing={printings.get(printingKey(reprinted.card))}
          onApply={(qty, p, finish) =>
            change((t) => reprintOwned(t, reprinted.index, qty, p, finish))
          }
          onClose={() => setReprinting(null)}
        />
      )}
    </main>
  );
}

/** What the whole collection is worth, and the currency it is priced in. */
function CollectionWorth({
  cards,
  prices,
  currency,
  onCurrency,
}: {
  cards: readonly OwnedCard[];
  prices: PriceBook;
  currency: Currency;
  onCurrency: (currency: Currency) => void;
}) {
  const { total, unpriced } = worth(cards, prices, currency);
  const copies = cards.reduce((n, c) => n + c.qty, 0);
  return (
    <section className="collection-worth" aria-label="Collection value">
      <span>
        Worth <strong>{formatPrice(total, currency)}</strong>
        {prices.size === 0 ? (
          <span className="muted"> · looking up prices…</span>
        ) : (
          unpriced > 0 && (
            <span className="muted">
              {" "}
              · {unpriced} of {copies} {copies === 1 ? "card" : "cards"} without
              a price
            </span>
          )
        )}
      </span>
      <select
        aria-label="Currency"
        value={currency}
        onChange={(e) => onCurrency(e.target.value as Currency)}
      >
        {CURRENCIES.map((c) => (
          <option key={c} value={c}>
            {c.toUpperCase()}
          </option>
        ))}
      </select>
      <span className="muted collection-worth-source">
        Scryfall's prices, refreshed daily
      </span>
    </section>
  );
}

function PlaceSection({
  section,
  places,
  printings,
  prices,
  currency,
  deckName,
  selected,
  onTick,
  onReprint,
  change,
}: {
  section: Section;
  places: readonly Place[];
  printings: Printings;
  prices: PriceBook;
  currency: Currency;
  /** The name of the deck the place is, when the repo has it. */
  deckName: string | undefined;
  selected: ReadonlySet<number>;
  onTick: (indices: readonly number[], on: boolean) => void;
  onReprint: (index: number) => void;
  change: (next: (text: string) => string) => boolean;
}) {
  const { place } = section;
  const here = section.cards.map((c) => c.index);
  const all = here.length > 0 && here.every((i) => selected.has(i));
  const value = worth(section.cards, prices, currency);
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
        {value.total > 0 && (
          <span
            className="badge place-worth"
            title={
              value.unpriced > 0
                ? `Value, leaving out ${value.unpriced} without a price`
                : "Value"
            }
          >
            {formatPrice(value.total, currency)}
          </span>
        )}
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
              <th className="owned-tick">
                <input
                  type="checkbox"
                  aria-label={`Select every card in ${place?.name ?? UNSORTED}`}
                  checked={all}
                  onChange={(e) => onTick(here, e.target.checked)}
                />
              </th>
              <th>Qty</th>
              <th>Card</th>
              <th>Printing</th>
              <th>Finish</th>
              <th className="owned-price">Price</th>
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
                price={unitPrice(c, prices, currency)}
                currency={currency}
                selected={selected.has(c.index)}
                onTick={(on) => onTick([c.index], on)}
                onReprint={() => onReprint(c.index)}
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
  price,
  currency,
  selected,
  onTick,
  onReprint,
  change,
}: {
  card: OwnedCard;
  places: readonly Place[];
  printings: Printings;
  /** One copy's price, absent where Scryfall has none for its finish. */
  price: number | undefined;
  currency: Currency;
  selected: boolean;
  onTick: (on: boolean) => void;
  onReprint: () => void;
  change: (next: (text: string) => string) => boolean;
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
    <tr className={selected ? "owned-row selected" : "owned-row"}>
      <td className="owned-tick">
        <input
          type="checkbox"
          aria-label={`Select ${name}`}
          checked={selected}
          onChange={(e) => onTick(e.target.checked)}
        />
      </td>
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
          {image && (
            <img crossOrigin="anonymous" src={image} alt="" loading="lazy" />
          )}
        </span>
        <button
          type="button"
          className="owned-title"
          title="Pick the printing, or the finish of some copies"
          onClick={onReprint}
        >
          {name}
        </button>
        {image && (
          <img
            crossOrigin="anonymous"
            className="owned-preview"
            src={image}
            alt=""
            loading="lazy"
          />
        )}
      </td>
      <td className="owned-printing">
        <button
          type="button"
          className="owned-printing-pick"
          title="Pick the printing, or the finish of some copies"
          aria-label={`Printing of ${name}`}
          onClick={onReprint}
        >
          {card.card.kind === "printing" ? (
            <span className="set-chip">
              {card.card.set.toUpperCase()} <span>#{card.card.num}</span>
            </span>
          ) : (
            <span className="muted">any</span>
          )}
        </button>
      </td>
      <td className="owned-finish">
        <select
          aria-label={`Finish of ${name}`}
          value={card.finish}
          onChange={(e) =>
            change((t) =>
              reprintOwned(
                t,
                card.index,
                card.qty,
                null,
                e.target.value as Finish,
              ),
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
      <td
        className="owned-price"
        title={
          price === undefined
            ? undefined
            : `${formatPrice(price, currency)} each${card.card.kind === "name" ? ", as Scryfall's usual printing" : ""}`
        }
      >
        {price === undefined ? (
          <span className="muted">—</span>
        ) : (
          formatPrice(price * card.qty, currency)
        )}
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
