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
import { atCheapest, breakdown, type Cheapest, cheapest } from "./cost";
import "./cost.css";

/** Puts a line on another printing, in the finish given. */
export type OnUsePrinting = (card: Card, use: Cheapest) => void;

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
 * The deck by what each card costs, dearest first, each line beside its
 * cheapest printing: for deciding what to proxy, what to buy on a cheaper
 * printing and what to ask around for.
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
  const inDeck = useMemo(() => cards.filter((c) => c.inDeck), [cards]);
  const prices = usePrices(inDeck);
  const nameOf = (c: Card) => cardName(c, printings);
  const b = breakdown(cards, prices, currency, nameOf);

  const uriOf = (c: Card) =>
    printings.get(printingKey(c.card))?.prints ?? printsByName(nameOf(c));
  const uris = [...new Set(inDeck.map(uriOf))].sort();
  const found = usePrintingsOf(uris);
  const lookups = (c: Card) => found.get(uriOf(c));
  const cheapestOf = (c: Card): Cheapest | undefined => {
    const all = lookups(c);
    return all && all !== "failed" ? cheapest(all, currency) : undefined;
  };
  const pending = uris.filter((u) => !found.has(u)).length;
  const floor = atCheapest(b, cheapestOf);
  const looking = prices.size === 0 && inDeck.length > 0;
  const n = b.unpriced.length;

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
        <select
          className="cost-currency"
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
        <p className="muted cost-note">
          Scryfall's prices, refreshed daily.
          {b.outside > 0 &&
            ` ${b.outside} ${b.outside === 1 ? "card" : "cards"} outside the deck (maybeboard, sideboard…) not counted.`}
          {n > 0 &&
            ` ${n} ${n === 1 ? "card has" : "cards have"} no price for ${n === 1 ? "its" : "their"} printing, listed last and left out of both totals.`}
        </p>
      </section>
      {(b.lines.length > 0 || n > 0) && (
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
            {[
              ...b.lines,
              ...b.unpriced.map((card) => ({ card, each: undefined })),
            ].map(({ card, each }) => (
              <CostRow
                key={card.index}
                card={card}
                each={each}
                name={nameOf(card)}
                image={printings.get(printingKey(card.card))?.image}
                best={cheapestOf(card)}
                failed={lookups(card) === "failed"}
                currency={currency}
                onUse={onUse}
              />
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

/** A card picture that shows while its cell is hovered. */
function Preview({ image }: { image: string | undefined }) {
  return image ? (
    <img
      crossOrigin="anonymous"
      className="cost-preview"
      src={image}
      alt=""
      loading="lazy"
    />
  ) : null;
}

function CostRow({
  card,
  each,
  name,
  image,
  best,
  failed,
  currency,
  onUse,
}: {
  card: Card;
  /** One copy on its own printing, or undefined for no price. */
  each: number | undefined;
  name: string;
  image: string | undefined;
  best: Cheapest | undefined;
  failed: boolean;
  currency: Currency;
  onUse: OnUsePrinting;
}) {
  return (
    <tr>
      <td>{card.qty}</td>
      <td className="cost-name cost-hover">
        <span className="cost-thumb" aria-hidden="true">
          {image && (
            <img crossOrigin="anonymous" src={image} alt="" loading="lazy" />
          )}
        </span>
        <span className="cost-title">{name}</span>
        <span className="muted cost-printing">
          {card.card.kind === "printing"
            ? `${card.card.set.toUpperCase()} ${card.card.num}`
            : "Scryfall's usual printing"}
          {card.finish !== "nonfoil" && ` · ${card.finish}`}
        </span>
        <Preview image={image} />
      </td>
      <td className="cost-num">
        {each === undefined ? (
          <span className="muted">—</span>
        ) : (
          formatPrice(each, currency)
        )}
      </td>
      <td className="cost-num">
        {card.qty > 1 && each !== undefined
          ? formatPrice(each * card.qty, currency)
          : null}
      </td>
      <CheapestCell
        card={card}
        each={each}
        best={best}
        failed={failed}
        currency={currency}
        onUse={onUse}
      />
    </tr>
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
  if (failed)
    return (
      <td>
        <span className="muted">Scryfall did not answer</span>
      </td>
    );
  if (!best)
    return (
      <td>
        <span className="muted">…</span>
      </td>
    );
  const same =
    card.card.kind === "printing" &&
    card.card.set === best.printing.set &&
    card.card.num === best.printing.num &&
    card.finish === best.finish;
  // Within a cent is the same price: nothing to switch for.
  if (same || (each !== undefined && each - best.each < 0.01))
    return (
      <td>
        <span className="muted cost-cheapest-here">This is the cheapest</span>
      </td>
    );
  const label = `${best.printing.set.toUpperCase()} ${best.printing.num}${best.finish === "nonfoil" ? "" : ` ${best.finish}`}`;
  return (
    <td className="cost-hover">
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
      <Preview image={best.printing.image} />
    </td>
  );
}
