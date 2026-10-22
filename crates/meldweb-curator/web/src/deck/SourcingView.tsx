import { type ReactNode, useMemo, useState } from "react";
import { useSettingsEdit } from "../card/preference";
import type { Card, Finish } from "../deck";
import { parseLogin } from "../github/trade";
import {
  CURRENCIES,
  formatPrice,
  loadCurrency,
  saveCurrency,
  unitPrice,
  usePrices,
} from "../prices";
import type { Currency } from "../scryfall";
import type { SupplyShort } from "../supply";
import { setFriends } from "../supply";
import { tradeText } from "../trade/TradeView";
import { messageOf } from "../ui/Sheet";
import { Spinner, Waiting } from "../ui/Spinner";
import { CardCell, CopyText, SupplySection } from "./SupplyParts";
import {
  offersOf,
  printingLabel,
  type Row,
  sourcing,
  whereLabel,
} from "./sourcing";
import type { FriendMissing, SupplyState } from "./useSupply";

/** Puts deck line `index` on the printing and finish of a copy owned. */
export type OnUseOwned = (
  index: number,
  printing: { set: string; num: string },
  finish: Finish,
) => void;

/**
 * Where each card of the deck is to come from: the copies owned as the list
 * names them, those owned in another printing, what friends have, and what
 * is left to buy.
 */
export function SourcingView({
  state,
  cards,
  deckName,
  me,
  onUse,
}: {
  state: SupplyState;
  cards: readonly Card[];
  deckName: string;
  me: string;
  onUse: OnUseOwned;
}) {
  const [currency, setCurrency] = useState(loadCurrency);
  // The deck's own printings, what buying a card means, and the friends'
  // copies, what asking for one is worth.
  const priced = useMemo(() => {
    const out: { card: Card["card"] }[] = [...cards];
    if (state.kind === "ready" && state.supply.kind === "supply")
      for (const s of state.supply.short)
        for (const f of s.friends) out.push(...f.pulls);
    return out;
  }, [cards, state]);
  const prices = usePrices(priced);

  if (state.kind === "loading")
    return (
      <p className="muted supply-status">
        <Waiting>Reading your collection…</Waiting>
      </p>
    );
  if (state.kind === "failed")
    return (
      <p className="refusal" role="alert">
        Your collection could not be read: {state.message}
      </p>
    );
  if (state.supply.kind === "refused")
    return (
      <p className="refusal" role="alert">
        {state.supply.message}
      </p>
    );
  const s = state.supply;
  const b = sourcing(s);
  const lineOf = (r: Row) => cards[r.line.index];
  const { printings } = state;
  const buyText = b.buy.map((c) => `${c.short} ${c.name}`).join("\n");
  // A short card is priced as the list names it: what buying it would cost.
  const shortEach = (c: SupplyShort) => {
    const line = cards[c.line];
    return line && unitPrice(line, prices, currency);
  };
  const buy = sum(b.buy.map((c) => [shortEach(c), c.short]));
  const fromFriends = sum(
    b.friends.flatMap((c) =>
      c.friends.flatMap((f) =>
        f.pulls.map((p) => [unitPrice(p, prices, currency), p.qty] as const),
      ),
    ),
  );
  const toBuy = [...b.buy].sort(
    (x, y) => (shortEach(y) ?? -1) * y.short - (shortEach(x) ?? -1) * x.short,
  );

  return (
    <div className="supply">
      <p className="supply-summary">
        <strong>{b.copies.have}</strong> of {b.copies.need} copies owned as
        listed
        {b.copies.alike > 0 && (
          <>
            {" "}
            · <strong>{b.copies.alike}</strong> in another printing
          </>
        )}
        {b.copies.friends > 0 && (
          <>
            {" "}
            · <strong>{b.copies.friends}</strong> a friend has
          </>
        )}
        {b.copies.buy > 0 && (
          <>
            {" "}
            · <strong>{b.copies.buy}</strong> to get
            {buy.total > 0 && (
              <>
                , <strong>{formatPrice(buy.total, currency)}</strong>
                {buy.unpriced > 0 && (
                  <span className="muted"> + {buy.unpriced} unpriced</span>
                )}
              </>
            )}
          </>
        )}
        {b.copies.basics > 0 && (
          <span className="muted"> · {b.copies.basics} basic lands</span>
        )}
        <CurrencyPick currency={currency} onPick={setCurrency} />
      </p>
      <Friends
        friends={state.friends}
        reading={state.reading}
        missing={state.missing}
      />
      {state.supply.unread.map((u) => (
        <p key={u.who} className="refusal-inline">
          {u.who}'s collection is left out: {u.message}
        </p>
      ))}

      {b.alike.length > 0 && (
        <SupplySection
          title="Owned in another printing"
          count={b.alike.reduce((n, r) => n + r.copies.qty, 0)}
          hint="Play the copy you have, or put the list on it."
        >
          <thead>
            <tr>
              <th>Qty</th>
              <th>Your copy</th>
              <th>The list names</th>
              <th>Where</th>
              <th
                className="cost-num"
                title="One copy of the printing the list names"
              >
                Listed costs
              </th>
              <th aria-label="Use" />
            </tr>
          </thead>
          <tbody>
            {b.alike.map((r) => {
              const line = lineOf(r);
              const { copies } = r;
              // Only a whole line on one printing can take it; a line made
              // of several copies keeps the printing it names.
              const usable =
                copies.card.kind === "printing" &&
                r.line.copies.length === 1 &&
                copies.qty === r.line.need;
              return (
                <tr key={`${r.line.index}:${copies.line}`}>
                  <td>{copies.qty}</td>
                  <CardCell
                    name={r.line.name}
                    card={copies.card}
                    finish={copies.finish}
                    line={line}
                    printings={printings}
                    note={
                      copies.card.kind === "name" && (
                        <span className="muted">logged by name</span>
                      )
                    }
                  />
                  <td className="muted">
                    {line
                      ? printingLabel(line.card, line.finish) || "Any printing"
                      : ""}
                  </td>
                  <td>{whereLabel(copies)}</td>
                  <PriceCell
                    each={line && unitPrice(line, prices, currency)}
                    currency={currency}
                  />
                  <td>
                    {usable && copies.card.kind === "printing" && (
                      <button
                        type="button"
                        className="small"
                        title="Put this line on the printing you own"
                        onClick={() =>
                          copies.card.kind === "printing" &&
                          onUse(r.line.index, copies.card, copies.finish)
                        }
                      >
                        Use mine
                      </button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </SupplySection>
      )}

      {b.friends.length > 0 && (
        <SupplySection
          title="A friend has"
          count={b.copies.friends}
          hint={
            fromFriends.total > 0 &&
            `Their copies are worth ${formatPrice(fromFriends.total, currency)}.`
          }
          actions={state.friends
            .filter((who) => offersOf(s, who).length > 0)
            .map((who) => (
              <CopyText
                key={who}
                label={`Copy for ${who}`}
                title={`What ${who} has of what ${deckName} lacks, by their place`}
                text={tradeText(me, who, offersOf(s, who), deckName)}
              />
            ))}
        >
          <thead>
            <tr>
              <th>Short</th>
              <th>Card</th>
              <th>Who has it</th>
            </tr>
          </thead>
          <tbody>
            {b.friends.map((c) => (
              <ShortRow
                key={c.name}
                short={c}
                cards={cards}
                printings={printings}
              >
                <td>
                  <ul className="supply-friends">
                    {c.friends.map((f) =>
                      f.pulls.map((p, i) => {
                        const label = printingLabel(p.card, p.finish);
                        return (
                          // biome-ignore lint/suspicious/noArrayIndexKey: a friend's pulls are never reordered
                          <li key={`${f.who}:${i}`}>
                            <strong>{f.who}</strong> {p.qty}×{" "}
                            {p.at ?? "unsorted"}
                            {p.inDeck && " (in a deck)"}
                            {label && <span className="muted"> · {label}</span>}
                            <PullPrice
                              each={unitPrice(p, prices, currency)}
                              qty={p.qty}
                              currency={currency}
                            />
                          </li>
                        );
                      }),
                    )}
                  </ul>
                </td>
              </ShortRow>
            ))}
          </tbody>
        </SupplySection>
      )}

      {b.buy.length > 0 && (
        <SupplySection
          title="To get"
          count={b.copies.buy}
          hint="Neither you nor a friend has these."
          actions={
            <CopyText
              label="Copy list"
              title="One line a card, as a store's wants list takes it"
              text={`${buyText}\n`}
            />
          }
        >
          <thead>
            <tr>
              <th>Short</th>
              <th>Card</th>
              <th className="cost-num">Each</th>
              <th className="cost-num">Total</th>
            </tr>
          </thead>
          <tbody>
            {toBuy.map((c) => (
              <ShortRow
                key={c.name}
                short={c}
                cards={cards}
                printings={printings}
              >
                <PriceCell each={shortEach(c)} currency={currency} />
                <PriceCell
                  each={shortEach(c)}
                  qty={c.short}
                  currency={currency}
                />
              </ShortRow>
            ))}
          </tbody>
        </SupplySection>
      )}

      {b.basics.length > 0 && (
        <p className="muted supply-basics">
          Basic lands not in your collection:{" "}
          {b.basics.map((c) => `${c.short} ${c.name}`).join(", ")}. Most people
          don't log basics, so they're left off the lists above.
        </p>
      )}

      {b.have.length > 0 && (
        <SupplySection title="Owned as listed" count={b.copies.have}>
          <thead>
            <tr>
              <th>Qty</th>
              <th>Card</th>
              <th>Where</th>
            </tr>
          </thead>
          <tbody>
            {b.have.map((r) => (
              <tr key={`${r.line.index}:${r.copies.line}`}>
                <td>{r.copies.qty}</td>
                <CardCell
                  name={r.line.name}
                  card={r.copies.card}
                  finish={r.copies.finish}
                  line={lineOf(r)}
                  printings={printings}
                />
                <td>{whereLabel(r.copies)}</td>
              </tr>
            ))}
          </tbody>
        </SupplySection>
      )}
    </div>
  );
}

function ShortRow({
  short,
  cards,
  printings,
  children,
}: {
  short: SupplyShort;
  cards: readonly Card[];
  printings: Parameters<typeof CardCell>[0]["printings"];
  children: ReactNode;
}) {
  const line = cards[short.line];
  return (
    <tr>
      <td>{short.short}</td>
      <CardCell
        name={short.name}
        card={line?.card ?? { kind: "name", name: short.name }}
        finish={line?.finish ?? "nonfoil"}
        line={line}
        printings={printings}
      />
      {children}
    </tr>
  );
}

const WHY: Record<FriendMissing["why"], string> = {
  "no-repo": "has no public mtg repo",
  "no-file": "has no collection yet",
  "not-a-login": "is not a GitHub login",
  failed: "could not be read",
};

/** The friends whose collections are asked, kept in `meldweb.toml`. */
function Friends({
  friends,
  reading,
  missing,
}: {
  friends: readonly string[];
  reading: readonly string[];
  missing: readonly FriendMissing[];
}) {
  const settings = useSettingsEdit();
  const [login, setLogin] = useState("");
  const [refusal, setRefusal] = useState<string | null>(null);
  if (!settings) return null;
  const save = (next: readonly string[]) => {
    try {
      // Checked now, so a typo is said here rather than by the commit.
      setFriends(settings.text, next);
      setRefusal(null);
      void settings.apply((t) => setFriends(t, next));
    } catch (e) {
      setRefusal(messageOf(e));
    }
  };
  return (
    <form
      className="supply-friends-edit"
      onSubmit={(e) => {
        e.preventDefault();
        const who = parseLogin(login);
        if (!who) {
          setRefusal(`“${login.trim()}” is not a GitHub login.`);
          return;
        }
        save([...friends, who]);
        setLogin("");
      }}
    >
      <span className="muted">Friends</span>
      {friends.map((f) => {
        const gone = missing.find((m) => m.who === f);
        return (
          <span
            key={f}
            className={`chip${gone ? " chip-warn" : ""}`}
            title={gone ? `${f} ${WHY[gone.why]}` : undefined}
          >
            {f}
            {reading.includes(f) && <Spinner />}
            {gone && " ⚠"}
            <button
              type="button"
              className="chip-remove"
              aria-label={`Stop asking ${f}`}
              onClick={() => save(friends.filter((g) => g !== f))}
            >
              ×
            </button>
          </span>
        );
      })}
      <input
        value={login}
        onChange={(e) => setLogin(e.target.value)}
        placeholder="a friend's GitHub login"
        aria-label="A friend's GitHub login"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
      />
      <button type="submit" className="small">
        Add
      </button>
      {(refusal ?? settings.error) && (
        <span className="refusal-inline">{refusal ?? settings.error}</span>
      )}
      {friends.length === 0 && (
        <span className="muted">
          Their mtg repo has to be public; what they hold of what this deck
          lacks shows below.
        </span>
      )}
    </form>
  );
}

/** The copies' total and how many of them have no price. */
function sum(rows: readonly (readonly [number | undefined, number])[]) {
  let total = 0;
  let unpriced = 0;
  for (const [each, qty] of rows) {
    if (each === undefined) unpriced += qty;
    else total += each * qty;
  }
  return { total, unpriced };
}

/** `qty` copies at `each`, or a dash for a card Scryfall has no price for. */
function PriceCell({
  each,
  qty = 1,
  currency,
}: {
  each: number | undefined;
  qty?: number;
  currency: Currency;
}) {
  return (
    <td className="cost-num">
      {each === undefined ? (
        <span className="muted">—</span>
      ) : (
        formatPrice(each * qty, currency)
      )}
    </td>
  );
}

/** The currency, shared with the cost view. */
function CurrencyPick({
  currency,
  onPick,
}: {
  currency: Currency;
  onPick: (c: Currency) => void;
}) {
  return (
    <select
      className="cost-currency"
      aria-label="Currency"
      value={currency}
      onChange={(e) => {
        const c = e.target.value as Currency;
        onPick(c);
        saveCurrency(c);
      }}
    >
      {CURRENCIES.map((c) => (
        <option key={c} value={c}>
          {c.toUpperCase()}
        </option>
      ))}
    </select>
  );
}

function PullPrice({
  each,
  qty,
  currency,
}: {
  each: number | undefined;
  qty: number;
  currency: Currency;
}) {
  return each === undefined ? null : (
    <span className="muted"> · {formatPrice(each * qty, currency)}</span>
  );
}
