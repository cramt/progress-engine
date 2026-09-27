import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { Card, Category } from "../deck";
import type { AddByName } from "../quickadd/QuickAdd";
import type { Printings } from "../scryfall";
import {
  type ResultCard,
  type SearchBackend,
  type SearchPage,
  SearchRefused,
  scryfallSearch,
} from "./backend";
import {
  composeQuery,
  deckIdentity,
  filterText,
  type SmartFilters,
} from "./query";
import { type RenderResult, renderMinimalResult } from "./ResultCard";
import { startResultDrag, useResultDrops } from "./resultDrag";
import "./search.css";

/** The toolbar's button that opens the overlay. */
export function SearchButton({ onClick }: { onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} title="Card search">
      Card search
    </button>
  );
}

interface Tab {
  id: number;
  query: string;
  /** The query as sent, filters included, once a search has run. */
  sent: string | null;
  loading: boolean;
  cards: ResultCard[];
  total: number;
  warnings: string[];
  error: string | null;
  more: SearchPage["more"];
}

const blankTab = (id: number): Tab => ({
  id,
  query: "",
  sent: null,
  loading: false,
  cards: [],
  total: 0,
  warnings: [],
  error: null,
  more: undefined,
});

const LOCK_KEY = "meldweb.search.locked";

function readLocked(): boolean {
  try {
    return localStorage.getItem(LOCK_KEY) === "1";
  } catch {
    return false;
  }
}

function writeLocked(locked: boolean): void {
  try {
    localStorage.setItem(LOCK_KEY, locked ? "1" : "0");
  } catch {
    // A convenience; the overlay works without it.
  }
}

/**
 * Archidekt's card search: full screen over the deck, or, locked, a panel
 * docked beside it so searching and dragging into the deck happen together.
 * Queries are Scryfall syntax, sent on Enter; smart filters, on by default,
 * keep results to the deck's colour identity and format. Results come from
 * `backend`, and are drawn by `renderResult`.
 */
export function SearchOverlay({
  cards,
  printings,
  format,
  categories,
  onAdd,
  onClose,
  backend = scryfallSearch,
  renderResult = renderMinimalResult,
}: {
  cards: readonly Card[];
  printings: Printings;
  format: string | undefined;
  categories: readonly Category[];
  onAdd: AddByName;
  onClose: () => void;
  backend?: SearchBackend;
  renderResult?: RenderResult;
}) {
  const [locked, setLocked] = useState(readLocked);
  const [smart, setSmart] = useState(true);
  const [tabs, setTabs] = useState<Tab[]>([blankTab(0)]);
  const [current, setCurrent] = useState(0);
  const [history, setHistory] = useState<string[]>([]);
  const [dragging, setDragging] = useState(false);
  const nextId = useRef(1);
  const controllers = useRef(new Map<number, AbortController>());
  const input = useRef<HTMLInputElement>(null);
  const historyId = useId();

  const filters: SmartFilters = useMemo(
    () => ({
      identity: deckIdentity(cards, printings),
      format: format ?? null,
    }),
    [cards, printings, format],
  );
  const tab = tabs.find((t) => t.id === current) ?? tabs[0] ?? blankTab(0);

  useResultDrops(onAdd);

  useEffect(() => {
    document.body.classList.toggle("search-docked", locked);
    return () => document.body.classList.remove("search-docked");
  }, [locked]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !locked) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [locked, onClose]);

  useEffect(() => {
    const all = controllers.current;
    return () => {
      for (const c of all.values()) c.abort();
    };
  }, []);

  // `current` changes focus to the tab's box.
  // biome-ignore lint/correctness/useExhaustiveDependencies: see above
  useEffect(() => input.current?.focus(), [current]);

  const update = (id: number, change: (t: Tab) => Partial<Tab>) =>
    setTabs((all) =>
      all.map((t) => (t.id === id ? { ...t, ...change(t) } : t)),
    );

  const fail = (id: number, e: unknown) => {
    if (e instanceof DOMException && e.name === "AbortError") return;
    update(id, () => ({
      loading: false,
      error: e instanceof Error ? e.message : String(e),
      warnings: e instanceof SearchRefused ? e.warnings : [],
    }));
  };

  const run = (id: number, query: string, withFilters: boolean) => {
    const sent = composeQuery(query, withFilters ? filters : null);
    if (sent === "") return;
    controllers.current.get(id)?.abort();
    const controller = new AbortController();
    controllers.current.set(id, controller);
    setHistory((h) =>
      [query.trim(), ...h.filter((q) => q !== query.trim())].slice(0, 20),
    );
    update(id, () => ({ ...blankTab(id), query, sent, loading: true }));
    backend.search(sent, controller.signal).then(
      (page) =>
        update(id, () => ({
          loading: false,
          cards: page.cards,
          total: page.total,
          warnings: page.warnings,
          more: page.more,
        })),
      (e: unknown) => fail(id, e),
    );
  };

  const loadMore = (t: Tab) => {
    if (!t.more) return;
    update(t.id, () => ({ loading: true }));
    t.more().then(
      (page) =>
        update(t.id, (now) => ({
          loading: false,
          cards: [...now.cards, ...page.cards],
          warnings: [...new Set([...now.warnings, ...page.warnings])],
          more: page.more,
        })),
      (e: unknown) => fail(t.id, e),
    );
  };

  const addTab = () => {
    const id = nextId.current++;
    setTabs((all) => [...all, blankTab(id)]);
    setCurrent(id);
  };

  const closeTab = (id: number) => {
    controllers.current.get(id)?.abort();
    controllers.current.delete(id);
    const rest = tabs.filter((t) => t.id !== id);
    if (rest.length === 0) {
      const fresh = nextId.current++;
      setTabs([blankTab(fresh)]);
      setCurrent(fresh);
      return;
    }
    setTabs(rest);
    if (id === current) setCurrent(rest[rest.length - 1]?.id ?? 0);
  };

  const filterNote = filterText(filters);

  return (
    <div
      className={[
        "search-overlay",
        locked ? "docked" : "full",
        dragging && !locked ? "passing-drag" : "",
      ].join(" ")}
      role="dialog"
      aria-label="Card search"
    >
      <div className="search-head">
        <div className="search-tabs" role="tablist">
          {tabs.map((t, i) => (
            <div
              key={t.id}
              className={t.id === tab.id ? "search-tab active" : "search-tab"}
            >
              <button
                type="button"
                role="tab"
                aria-selected={t.id === tab.id}
                onClick={() => setCurrent(t.id)}
              >
                {t.query.trim() || `Search ${i + 1}`}
                {t.sent !== null && !t.loading && (
                  <span className="search-tab-count">{t.total}</span>
                )}
              </button>
              {tabs.length > 1 && (
                <button
                  type="button"
                  className="search-tab-close"
                  aria-label="Close tab"
                  onClick={() => closeTab(t.id)}
                >
                  ×
                </button>
              )}
            </div>
          ))}
          <button type="button" aria-label="New search tab" onClick={addTab}>
            +
          </button>
        </div>
        <div className="search-head-actions">
          <button
            type="button"
            aria-pressed={locked}
            title={
              locked
                ? "Unlock: search over the whole page"
                : "Lock: dock beside the deck"
            }
            onClick={() => {
              setLocked(!locked);
              writeLocked(!locked);
            }}
          >
            {locked ? "Unlock" : "Lock"}
          </button>
          <button type="button" aria-label="Close search" onClick={onClose}>
            ×
          </button>
        </div>
      </div>

      <form
        className="search-form"
        onSubmit={(e) => {
          e.preventDefault();
          run(tab.id, tab.query, smart);
        }}
      >
        <input
          ref={input}
          type="search"
          aria-label="Scryfall syntax"
          placeholder="Scryfall syntax, e.g. t:artifact mv<=2 o:draw"
          list={historyId}
          autoComplete="off"
          spellCheck={false}
          value={tab.query}
          onChange={(e) => {
            const query = e.target.value;
            update(tab.id, () => ({ query }));
          }}
        />
        <datalist id={historyId}>
          {history.map((q) => (
            <option key={q} value={q} />
          ))}
        </datalist>
        <button type="submit">Search</button>
        <label
          className="search-smart"
          title="Keep results to the deck's colour identity and format"
        >
          <input
            type="checkbox"
            checked={smart}
            onChange={(e) => {
              setSmart(e.target.checked);
              if (tab.sent !== null) run(tab.id, tab.query, e.target.checked);
            }}
          />
          Smart filters
          <code>{filterNote || "(the deck has no commander or format)"}</code>
        </label>
        <a
          className="search-guide"
          href="https://scryfall.com/docs/syntax"
          target="_blank"
          rel="noreferrer"
        >
          Syntax guide
        </a>
      </form>

      <div className="search-body">
        {tab.sent !== null && (
          <p className="search-sent">
            <code>{tab.sent}</code>
            {!tab.loading && tab.error === null && (
              <>
                {" "}
                · {tab.total} card{tab.total === 1 ? "" : "s"}
              </>
            )}
          </p>
        )}
        {tab.warnings.length > 0 && (
          <ul className="search-warnings" role="alert">
            {tab.warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        )}
        {tab.error !== null && (
          <p className="search-error" role="alert">
            {tab.error}
          </p>
        )}
        {tab.sent !== null &&
          !tab.loading &&
          tab.error === null &&
          tab.total === 0 && <p className="search-empty">No cards found.</p>}
        <div className="search-results">
          {tab.cards.map((card) => (
            <div className="search-result" key={card.id}>
              {renderResult(card, {
                add: (category) => onAdd(card.name, category),
                categories,
                dragProps: {
                  draggable: true,
                  onDragStart: (e) => {
                    startResultDrag(e, card.name);
                    // Deferred: changing the DOM inside dragstart makes
                    // Chrome cancel the drag it has just begun.
                    setTimeout(() => setDragging(true));
                  },
                  onDragEnd: () => setDragging(false),
                },
              })}
            </div>
          ))}
        </div>
        {tab.loading && <p className="search-loading">Searching…</p>}
        {!tab.loading && tab.more && (
          <button
            type="button"
            className="search-more"
            onClick={() => loadMore(tab)}
          >
            More results ({tab.cards.length} of {tab.total})
          </button>
        )}
      </div>
    </div>
  );
}
