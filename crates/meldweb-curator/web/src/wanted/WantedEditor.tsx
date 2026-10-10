import { Link } from "@tanstack/react-router";
import { useEffect, useMemo, useState } from "react";
import { useGrowingPrintings } from "../card/useGrowingPrintings";
import type { CardRef, Finish } from "../deck";
import { printingNames } from "../deck/archidektNames";
import { Banners, SaveStatus, Toolbar, UndoRedo } from "../deck/Toolbar";
import type { GitHubApi, RepoRef } from "../github/api";
import { createSaveStore } from "../github/save";
import { useSave } from "../github/useSave";
import { useHistory, useUndoKeys } from "../history";
import {
  type Copies,
  CURRENCIES,
  formatPrice,
  loadCurrency,
  type PriceBook,
  saveCurrency,
  unitPrice,
  usePrices,
  worth,
} from "../prices";
import { QuickAdd } from "../quickadd/QuickAdd";
import {
  type Currency,
  cardName,
  type Printings,
  printingKey,
} from "../scryfall";
import { CloseIcon } from "../ui/icons";
import {
  addWanted,
  type DeckCopies,
  type DeckFile,
  type MissingCard,
  readWanted,
  setDeckCopies,
  setWantedFinish,
  setWantedQty,
  type WantedCard,
} from "../wanted";
import "../collection/collection.css";
import "./wanted.css";

export interface WantedEditorProps {
  /** `wanted.toml` in the Magic repo. */
  path: string;
  /** The file as loaded, `""` and no sha while there is none. */
  text: string;
  sha: string | null;
  repo: RepoRef;
  api: GitHubApi;
  /** `collection.toml` as loaded, which the page reads and never writes. */
  collection: string;
  decks: readonly DeckFile[];
  printings: Printings;
}

const FINISHES: readonly Finish[] = ["nonfoil", "foil", "etched"];

const COPIES: Record<DeckCopies, string> = {
  each: "Each deck has its own copies",
  shared: "Decks share copies",
};

/**
 * The wanted list: cards wanted by hand, saved by itself as the collection
 * is, above the cards the decks hold that the collection is short of. Mount
 * it with a `key`; it reads its props once and then owns the text.
 */
export function WantedEditor({
  path,
  text: loaded,
  sha,
  repo,
  api,
  collection,
  decks,
  printings: loadedPrintings,
}: WantedEditorProps) {
  const history = useHistory(loaded);
  const [refusal, setRefusal] = useState<string | null>(null);
  const { undo, redo } = history;
  useUndoKeys(undo, redo);

  // The loader looked up every printing the three files name; a card added
  // here since is added by name.
  const names = useMemo(
    () =>
      Object.fromEntries(
        [...loadedPrintings.values()].map((p) => [`${p.set}/${p.num}`, p.name]),
      ),
    [loadedPrintings],
  );
  const parsed = useMemo(
    () => readWanted(history.present, collection, decks, names),
    [history.present, collection, decks, names],
  );
  const wants = parsed.kind === "wanted" ? parsed.cards : [];
  const missing = parsed.kind === "wanted" ? parsed.missing : [];
  // A missing card is wanted by name: any printing will do.
  const missingCopies = useMemo<Copies[]>(
    () =>
      missing.map((m) => ({
        card: { kind: "name", name: m.name },
        qty: m.missing,
        finish: "nonfoil",
      })),
    [missing],
  );
  const shown = useMemo(
    () => [...wants, ...missingCopies],
    [wants, missingCopies],
  );
  const printings = useGrowingPrintings(loadedPrintings, shown);
  const prices = usePrices(shown);
  const [currency, setCurrency] = useState(loadCurrency);

  const [store] = useState(() =>
    createSaveStore({ api, repo, path, text: loaded, sha }),
  );
  const save = useSave(store);
  useEffect(() => store.edit(history.present), [store, history.present]);

  if (parsed.kind === "refused") {
    return (
      <main>
        <Toolbar name="Wanted" />
        <div className="banners">
          <p className="refusal" role="alert">
            {path} is not a wanted list this Curator can read: {parsed.message}
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

  const reload = async () => {
    try {
      history.reset(await store.reload());
    } catch (e) {
      setRefusal(e instanceof Error ? e.message : String(e));
    }
  };

  const wantedCopies = wants.reduce((n, c) => n + c.qty, 0);
  const onCurrency = (c: Currency) => {
    setCurrency(c);
    saveCurrency(c);
  };

  return (
    <main>
      <Toolbar
        name="Wanted"
        count={wantedCopies}
        quickAdd={
          <QuickAdd
            categories={[]}
            onAdd={(name) =>
              change(
                (t) =>
                  addWanted(
                    t,
                    { kind: "name", name },
                    1,
                    "nonfoil",
                    printingNames(wants, printings),
                  ).text,
              )
            }
          />
        }
        actions={
          <>
            <Link
              to="/trade"
              className="button"
              title="What another player's collection holds of this list"
            >
              Trade with…
            </Link>
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
      <ByHand
        wants={wants}
        printings={printings}
        prices={prices}
        currency={currency}
        change={change}
      />
      <FromDecks
        missing={missing}
        copies={missingCopies}
        deckCopies={parsed.deckCopies}
        collection={parsed.collection}
        unread={parsed.unread}
        printings={printings}
        prices={prices}
        currency={currency}
        onDeckCopies={(c) => change((t) => setDeckCopies(t, c))}
      />
      <details className="source">
        <summary>The file</summary>
        <pre>{history.present}</pre>
      </details>
    </main>
  );
}

function Worth({
  copies,
  prices,
  currency,
}: {
  copies: readonly Copies[];
  prices: PriceBook;
  currency: Currency;
}) {
  const value = worth(copies, prices, currency);
  if (value.total === 0) return null;
  return (
    <span
      className="badge place-worth"
      title={
        value.unpriced > 0
          ? `Costs, leaving out ${value.unpriced} without a price`
          : "Costs"
      }
    >
      {formatPrice(value.total, currency)}
    </span>
  );
}

function Thumb({ image }: { image: string | undefined }) {
  return (
    <span className="owned-thumb">
      {image && (
        <img crossOrigin="anonymous" src={image} alt="" loading="lazy" />
      )}
    </span>
  );
}

function Price({
  card,
  prices,
  currency,
}: {
  card: Copies;
  prices: PriceBook;
  currency: Currency;
}) {
  const each = unitPrice(card, prices, currency);
  return (
    <td className="wanted-price">
      {each === undefined ? (
        <span className="muted">—</span>
      ) : (
        formatPrice(each, currency)
      )}
    </td>
  );
}

/** Cards wanted for no reason the decks give. */
function ByHand({
  wants,
  printings,
  prices,
  currency,
  change,
}: {
  wants: readonly WantedCard[];
  printings: Printings;
  prices: PriceBook;
  currency: Currency;
  change: (next: (text: string) => string) => boolean;
}) {
  return (
    <section className="place" aria-label="Wanted by hand">
      <header className="place-header">
        <h2>By hand</h2>
        <span className="badge" title="Cards">
          {wants.reduce((n, c) => n + c.qty, 0)}
        </span>
        <Worth copies={wants} prices={prices} currency={currency} />
      </header>
      {wants.length === 0 ? (
        <p className="wanted-empty muted">
          Nothing yet. Quick add puts a card here.
        </p>
      ) : (
        <table className="wanted-table">
          <thead>
            <tr>
              <th className="wanted-qty">Qty</th>
              <th>Card</th>
              <th className="wanted-printing">Printing</th>
              <th className="wanted-finish">Finish</th>
              <th className="wanted-owned">Owned</th>
              <th className="wanted-price">Price</th>
              <th className="wanted-remove">
                <span className="visually-hidden">Remove</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {wants.map((c) => {
              const name = cardName(c, printings);
              return (
                <tr key={c.index}>
                  <td className="wanted-qty">
                    <div className="stepper">
                      <button
                        type="button"
                        aria-label={`One fewer ${name}`}
                        onClick={() =>
                          change((t) => setWantedQty(t, c.index, c.qty - 1))
                        }
                      >
                        −
                      </button>
                      <span>{c.qty}</span>
                      <button
                        type="button"
                        aria-label={`One more ${name}`}
                        onClick={() =>
                          change((t) => setWantedQty(t, c.index, c.qty + 1))
                        }
                      >
                        +
                      </button>
                    </div>
                  </td>
                  <td className="wanted-name">
                    <Thumb image={printings.get(printingKey(c.card))?.image} />
                    {name}
                  </td>
                  <td className="wanted-printing">
                    {c.card.kind === "printing" ? (
                      <span className="set-chip">
                        {c.card.set.toUpperCase()} <span>#{c.card.num}</span>
                      </span>
                    ) : (
                      <span className="muted">any</span>
                    )}
                  </td>
                  <td className="wanted-finish">
                    <select
                      aria-label={`Finish of ${name}`}
                      value={c.finish}
                      onChange={(e) =>
                        change((t) =>
                          setWantedFinish(t, c.index, e.target.value as Finish),
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
                    className="wanted-owned"
                    title="Copies owned, any printing"
                  >
                    {c.owned > 0 ? c.owned : <span className="muted">0</span>}
                  </td>
                  <Price card={c} prices={prices} currency={currency} />
                  <td className="wanted-remove">
                    <button
                      type="button"
                      className="icon ghost"
                      aria-label={`No longer want ${name}`}
                      title="No longer wanted"
                      onClick={() => change((t) => setWantedQty(t, c.index, 0))}
                    >
                      <CloseIcon />
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </section>
  );
}

/** What the decks hold more of than the collection, worked out in Rust. */
function FromDecks({
  missing,
  copies,
  deckCopies,
  collection,
  unread,
  printings,
  prices,
  currency,
  onDeckCopies,
}: {
  missing: readonly MissingCard[];
  /** `missing` as copies to price. */
  copies: readonly Copies[];
  deckCopies: DeckCopies;
  /** Why the collection could not be read, when it could not. */
  collection: string | undefined;
  unread: readonly { path: string; message: string }[];
  printings: Printings;
  prices: PriceBook;
  currency: Currency;
  onDeckCopies: (copies: DeckCopies) => void;
}) {
  return (
    <section className="place" aria-label="Missing from decks">
      <header className="place-header">
        <h2>Missing from decks</h2>
        <span className="badge" title="Cards">
          {missing.reduce((n, m) => n + m.missing, 0)}
        </span>
        <Worth copies={copies} prices={prices} currency={currency} />
        <select
          className="wanted-copies"
          aria-label="How decks count"
          title="Saved in wanted.toml"
          value={deckCopies}
          onChange={(e) => onDeckCopies(e.target.value as DeckCopies)}
        >
          {(Object.keys(COPIES) as DeckCopies[]).map((c) => (
            <option key={c} value={c}>
              {COPIES[c]}
            </option>
          ))}
        </select>
      </header>
      {collection !== undefined && (
        <p className="refusal-inline wanted-note">
          The collection cannot be read, so nothing is worked out: {collection}
        </p>
      )}
      {unread.map((u) => (
        <p key={u.path} className="refusal-inline wanted-note">
          {u.path} is left out: {u.message}
        </p>
      ))}
      {missing.length === 0 ? (
        collection === undefined && (
          <p className="wanted-empty muted">
            You own every card your decks hold.
          </p>
        )
      ) : (
        <table className="wanted-table">
          <thead>
            <tr>
              <th className="wanted-qty">Missing</th>
              <th>Card</th>
              <th>In</th>
              <th className="wanted-owned">Owned</th>
              <th className="wanted-price">Price</th>
            </tr>
          </thead>
          <tbody>
            {missing.map((m) => (
              <tr key={m.name}>
                <td className="wanted-qty">{m.missing}</td>
                <td className="wanted-name">
                  <Thumb
                    image={
                      printings.get(
                        printingKey({ kind: "name", name: m.name } as CardRef),
                      )?.image
                    }
                  />
                  {m.name}
                </td>
                <td className="wanted-decks">
                  {m.decks.map((d) => (
                    <Link
                      key={d.path}
                      to="/deck/$"
                      params={{ _splat: d.path }}
                      className="wanted-deck"
                    >
                      {d.name}
                      {d.qty > 1 && <span className="muted"> ×{d.qty}</span>}
                    </Link>
                  ))}
                </td>
                <td className="wanted-owned">
                  {m.owned > 0 ? m.owned : <span className="muted">0</span>}
                </td>
                <Price
                  card={{
                    card: { kind: "name", name: m.name },
                    qty: m.missing,
                    finish: "nonfoil",
                  }}
                  prices={prices}
                  currency={currency}
                />
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
