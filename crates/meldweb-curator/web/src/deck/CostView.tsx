import { useEffect, useMemo, useState } from "react";
import {
  fetchAllPrintings,
  type PrintingOption,
  printsByName,
} from "../card/prints";
import type { Card } from "../deck";
import {
  CURRENCIES,
  formatPrice,
  loadCurrency,
  saveCurrency,
  usePrices,
} from "../prices";
import {
  type Currency,
  cardName,
  type Printings,
  printingKey,
} from "../scryfall";
import {
  atCheapest,
  type Bands,
  bands,
  breakdown,
  type Cheapest,
  cheapest,
  loadBands,
  saveBands,
  TIERS,
  type Tier,
} from "./cost";
import "./cost.css";

/** Puts a line on another printing, in the finish given. */
export type OnUsePrinting = (card: Card, use: Cheapest) => void;

const TITLE: Record<Tier, string> = {
  proxy: "Proxy territory",
  buy: "Worth buying",
  ask: "Ask the playgroup",
};

function range(tier: Tier, at: Bands, currency: Currency): string {
  const p = (n: number) => formatPrice(n, currency);
  switch (tier) {
    case "proxy":
      return `${p(at.proxy)} and up a copy`;
    case "buy":
      return `${p(at.ask)} to ${p(at.proxy)} a copy`;
    case "ask":
      return `under ${p(at.ask)} a copy`;
  }
}

/**
 * Every printing of each card asked about, by its search: what finds the
 * cheapest one. A card is asked about once, and a failed search stays failed.
 */
function usePrintingsOf(
  uris: readonly string[],
): ReadonlyMap<string, PrintingOption[] | "failed"> {
  const [found, setFound] = useState<
    ReadonlyMap<string, PrintingOption[] | "failed">
  >(new Map());
  const key = uris.join("\n");
  // biome-ignore lint/correctness/useExhaustiveDependencies: `key` is `uris`, compared by value
  useEffect(() => {
    const abort = new AbortController();
    for (const uri of uris) {
      if (found.has(uri)) continue;
      fetchAllPrintings(uri, abort.signal).then(
        (all) => setFound((m) => new Map(m).set(uri, all)),
        () => {
          if (!abort.signal.aborted)
            setFound((m) => new Map(m).set(uri, "failed"));
        },
      );
    }
    return () => abort.abort();
  }, [key]);
  return found;
}

/**
 * The deck by what each card costs: tiers split by one copy's price, each
 * line beside its cheapest printing, for deciding what to proxy, what to buy
 * on a cheaper printing and what to ask around for.
 */
export function CostView({
  cards,
  printings,
  onUse,
}: {
  cards: readonly Card[];
  printings: Printings;
  onUse: OnUsePrinting;
}) {
  const [currency, setCurrency] = useState(loadCurrency);
  const [at, setAt] = useState(loadBands);
  const inDeck = useMemo(() => cards.filter((c) => c.inDeck), [cards]);
  const prices = usePrices(inDeck);
  const nameOf = (c: Card) => cardName(c, printings);
  const b = breakdown(cards, prices, currency, at, nameOf);

  // Under the ask line a cheaper printing saves pennies, so those cards cost
  // no search; an unpriced card is asked about, as its cheapest may have one.
  const looked = [...b.tiers.proxy.lines, ...b.tiers.buy.lines]
    .map((l) => l.card)
    .concat(b.unpriced);
  const uriOf = (c: Card) =>
    printings.get(printingKey(c.card))?.prints ?? printsByName(nameOf(c));
  const uris = [...new Set(looked.map(uriOf))].sort();
  const found = usePrintingsOf(uris);
  const lookups = (c: Card) => found.get(uriOf(c));
  const cheapestOf = (c: Card): Cheapest | undefined => {
    const all = lookups(c);
    return all && all !== "failed" ? cheapest(all, currency) : undefined;
  };
  const pending = uris.filter((u) => !found.has(u)).length;
  const floor = atCheapest(b, cheapestOf);
  const looking = prices.size === 0 && inDeck.length > 0;

  return (
    <div className="cost">
      <section className="cost-summary" aria-label="What the deck costs">
        <div className="cost-headline">
          {looking ? (
            <span className="muted">Looking up prices…</span>
          ) : (
            <>
              <span>
                <strong>{formatPrice(b.total, currency)}</strong> as printed
              </span>
              <span
                className="muted"
                title="Every card on its cheapest paper printing, in any finish"
              >
                {pending > 0 ? (
                  `finding cheapest printings… ${uris.length - pending} of ${uris.length}`
                ) : (
                  <>
                    <strong>{formatPrice(floor, currency)}</strong> at the
                    cheapest printings
                    {b.total - floor >= 0.01 &&
                      ` · ${formatPrice(b.total - floor, currency)} on the printings picked`}
                  </>
                )}
              </span>
            </>
          )}
        </div>
        <div className="cost-controls">
          <BandInputs
            at={at}
            currency={currency}
            onChange={(next) => {
              setAt(next);
              saveBands(next);
            }}
          />
          <select
            aria-label="Currency"
            value={currency}
            onChange={(e) => {
              const c = e.target.value as Currency;
              setCurrency(c);
              saveCurrency(c);
            }}
          >
            {CURRENCIES.map((c) => (
              <option key={c} value={c}>
                {c.toUpperCase()}
              </option>
            ))}
          </select>
        </div>
        <p className="muted cost-note">
          Scryfall's prices, refreshed daily.
          {b.outside > 0 &&
            ` ${b.outside} ${b.outside === 1 ? "card" : "cards"} outside the deck (maybeboard, sideboard…) not counted.`}
          {b.unpriced.length > 0 &&
            ` ${b.unpriced.length} ${b.unpriced.length === 1 ? "card has" : "cards have"} no price for ${b.unpriced.length === 1 ? "its" : "their"} printing and ${b.unpriced.length === 1 ? "is" : "are"} left out of both totals; see No price below.`}
        </p>
      </section>
      {TIERS.map((tier) => {
        const t = b.tiers[tier];
        if (t.lines.length === 0) return null;
        return (
          <section
            key={tier}
            className={`cost-tier cost-${tier}`}
            aria-label={TITLE[tier]}
          >
            <header className="cost-tier-header">
              <h2>{TITLE[tier]}</h2>
              <span className="muted">{range(tier, at, currency)}</span>
              <span className="badge">{t.qty}</span>
              <span className="badge cost-tier-total">
                {formatPrice(t.total, currency)}
              </span>
            </header>
            <CostTable
              lines={t.lines}
              printings={printings}
              currency={currency}
              nameOf={nameOf}
              cheapestOf={cheapestOf}
              asked={tier !== "ask"}
              failed={(c) => lookups(c) === "failed"}
              onUse={onUse}
            />
          </section>
        );
      })}
      {b.unpriced.length > 0 && (
        <section className="cost-tier cost-unpriced" aria-label="No price">
          <header className="cost-tier-header">
            <h2>No price</h2>
            <span className="muted">
              Scryfall has none for this printing in this finish
            </span>
            <span className="badge">
              {b.unpriced.reduce((n, c) => n + c.qty, 0)}
            </span>
          </header>
          <CostTable
            lines={b.unpriced.map((card) => ({ card, each: undefined }))}
            printings={printings}
            currency={currency}
            nameOf={nameOf}
            cheapestOf={cheapestOf}
            asked
            failed={(c) => lookups(c) === "failed"}
            onUse={onUse}
          />
        </section>
      )}
    </div>
  );
}

/**
 * The two lines between the tiers. A pair where asking around would start
 * above proxying is no pair of bands, so it is shown but not taken.
 */
function BandInputs({
  at,
  currency,
  onChange,
}: {
  at: Bands;
  currency: Currency;
  onChange: (at: Bands) => void;
}) {
  const [ask, setAsk] = useState(String(at.ask));
  const [proxy, setProxy] = useState(String(at.proxy));
  const next = bands(Number(ask), Number(proxy));
  const take = (a: string, p: string) => {
    const b = a.trim() && p.trim() ? bands(Number(a), Number(p)) : null;
    if (b) onChange(b);
  };
  const unit = currency.toUpperCase();
  return (
    <span className="cost-bands">
      <label>
        Ask around under
        <input
          type="number"
          min={0}
          step="any"
          value={ask}
          aria-invalid={next === null}
          onChange={(e) => {
            setAsk(e.target.value);
            take(e.target.value, proxy);
          }}
        />
        {unit}
      </label>
      <label>
        Proxy from
        <input
          type="number"
          min={0}
          step="any"
          value={proxy}
          aria-invalid={next === null}
          onChange={(e) => {
            setProxy(e.target.value);
            take(ask, e.target.value);
          }}
        />
        {unit}
      </label>
      {next === null && (
        <span className="refusal-inline">
          The ask-around line has to be at or under the proxy line.
        </span>
      )}
    </span>
  );
}

function CostTable({
  lines,
  printings,
  currency,
  nameOf,
  cheapestOf,
  asked,
  failed,
  onUse,
}: {
  lines: readonly { card: Card; each: number | undefined }[];
  printings: Printings;
  currency: Currency;
  nameOf: (card: Card) => string;
  cheapestOf: (card: Card) => Cheapest | undefined;
  /** Whether these lines' cheapest printings are looked up at all. */
  asked: boolean;
  failed: (card: Card) => boolean;
  onUse: OnUsePrinting;
}) {
  const price = (n: number | undefined) =>
    n === undefined ? (
      <span className="muted">—</span>
    ) : (
      formatPrice(n, currency)
    );
  return (
    <table className="cost-table">
      <thead>
        <tr>
          <th>Qty</th>
          <th>Card</th>
          <th className="cost-num">This printing</th>
          <th className="cost-num">Total</th>
          <th>Cheapest printing</th>
        </tr>
      </thead>
      <tbody>
        {lines.map(({ card, each }) => {
          const shown = printings.get(printingKey(card.card));
          const name = nameOf(card);
          return (
            <tr key={card.index}>
              <td>{card.qty}</td>
              <td className="cost-name">
                <span className="cost-thumb" aria-hidden="true">
                  {shown?.image && (
                    <img
                      crossOrigin="anonymous"
                      src={shown.image}
                      alt=""
                      loading="lazy"
                    />
                  )}
                </span>
                <span className="cost-title">{name}</span>
                <span className="muted cost-printing">
                  {card.card.kind === "printing"
                    ? `${card.card.set.toUpperCase()} ${card.card.num}`
                    : "Scryfall's usual printing"}
                  {card.finish !== "nonfoil" && ` · ${card.finish}`}
                </span>
              </td>
              <td className="cost-num">{price(each)}</td>
              <td className="cost-num">
                {card.qty > 1 && each !== undefined
                  ? price(each * card.qty)
                  : null}
              </td>
              <td>
                {asked ? (
                  <CheapestCell
                    card={card}
                    each={each}
                    best={cheapestOf(card)}
                    failed={failed(card)}
                    currency={currency}
                    onUse={onUse}
                  />
                ) : (
                  <span className="muted">—</span>
                )}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function CheapestCell({
  card,
  each,
  best,
  failed,
  currency,
  onUse,
}: {
  card: Card;
  each: number | undefined;
  best: Cheapest | undefined;
  failed: boolean;
  currency: Currency;
  onUse: OnUsePrinting;
}) {
  if (failed) return <span className="muted">Scryfall did not answer</span>;
  if (!best) return <span className="muted">…</span>;
  const same =
    card.card.kind === "printing" &&
    card.card.set === best.printing.set &&
    card.card.num === best.printing.num &&
    card.finish === best.finish;
  // Within a cent is the same price: nothing to switch for.
  if (same || (each !== undefined && each - best.each < 0.01))
    return (
      <span className="muted cost-cheapest-here">This is the cheapest</span>
    );
  const label = `${best.printing.set.toUpperCase()} ${best.printing.num}${best.finish === "nonfoil" ? "" : ` ${best.finish}`}`;
  return (
    <span className="cost-cheapest">
      <span title={best.printing.setName}>
        {formatPrice(best.each, currency)} · {label}
      </span>
      {each !== undefined && (
        <span className="cost-saving">
          −{formatPrice((each - best.each) * card.qty, currency)}
        </span>
      )}
      <button
        type="button"
        className="small"
        title={`Put this line on ${best.printing.setName} #${best.printing.num}, ${best.finish}`}
        onClick={() => onUse(card, best)}
      >
        Use
      </button>
    </span>
  );
}
