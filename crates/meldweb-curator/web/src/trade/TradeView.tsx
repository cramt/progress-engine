import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { Toolbar } from "../deck/Toolbar";
import type { Login } from "../github/trade";
import { CheckIcon, CopyIcon } from "../ui/icons";
import type { ParsedTrade, TradeOffer, TradePull } from "../wanted";
import "../collection/collection.css";
import "../wanted/wanted.css";
import "./trade.css";

export interface TradeData {
  me: string;
  /** Every deck, to trade for one of them alone. */
  decks: readonly DeckChoice[];
  /** The deck traded for, by path; absent for the whole wanted list. */
  deck: string | undefined;
  result: TradeResult;
}

export interface DeckChoice {
  path: string;
  name: string;
}

export type TradeResult =
  | { kind: "ask" }
  | { kind: "not-a-login"; text: string }
  /** GitHub shows no public `mtg` for them, which a private one also reads as. */
  | { kind: "no-repo"; who: Login }
  | { kind: "no-file"; who: Login }
  | { kind: "trade"; who: Login; trade: ParsedTrade };

/** The last player traded with, offered again on the next visit. */
const LAST_WITH = "meldweb-trade-with";

/** One of their places and the copies to take out of it. */
interface Pile {
  at: string | undefined;
  inDeck: boolean;
  rows: { offer: TradeOffer; pull: TradePull }[];
}

/**
 * The copies to bring, place by place: binders and boxes by name, then the
 * unsorted ones, then the ones sleeved in a deck, which come out last.
 */
function piles(offers: readonly TradeOffer[]): Pile[] {
  const out: Pile[] = [];
  for (const offer of offers) {
    for (const pull of offer.pulls) {
      let pile = out.find((p) => p.at === pull.at);
      if (!pile) {
        pile = { at: pull.at, inDeck: pull.inDeck, rows: [] };
        out.push(pile);
      }
      pile.rows.push({ offer, pull });
    }
  }
  const rank = (p: Pile) => (p.inDeck ? 2 : p.at === undefined ? 1 : 0);
  return out.sort(
    (a, b) => rank(a) - rank(b) || (a.at ?? "").localeCompare(b.at ?? ""),
  );
}

function pileName(p: Pile): string {
  if (p.at === undefined) return "Unsorted";
  return p.inDeck ? `${p.at} (in the deck)` : p.at;
}

/** Their copy, when it says more than the name: its printing and finish. */
function copyOf(pull: TradePull): string {
  const parts: string[] = [];
  if (pull.card.kind === "printing") {
    parts.push(`${pull.card.set.toUpperCase()} ${pull.card.num}`);
  }
  if (pull.finish !== "nonfoil") parts.push(pull.finish);
  return parts.join(", ");
}

/** The list as text to send them, a heading a place. */
export function tradeText(
  me: string,
  who: string,
  offers: TradeOffer[],
  forDeck?: string,
) {
  const blocks = piles(offers).map((p) =>
    [
      pileName(p),
      ...p.rows.map(({ offer, pull }) => {
        const copy = copyOf(pull);
        return `${pull.qty} ${offer.name}${copy ? ` (${copy})` : ""}`;
      }),
    ].join("\n"),
  );
  const wants = forDeck ? `wants for ${forDeck}` : "wants";
  return `Cards of ${who}'s that ${me} ${wants}:\n\n${blocks.join("\n\n")}\n`;
}

export function TradeView({ data }: { data: TradeData }) {
  const navigate = useNavigate();
  const { result } = data;
  const [login, setLogin] = useState(() =>
    result.kind === "ask"
      ? (localStorage.getItem(LAST_WITH) ?? "")
      : result.kind === "not-a-login"
        ? result.text
        : result.who,
  );
  const offers =
    result.kind === "trade" && result.trade.kind === "trade"
      ? result.trade.offers
      : [];
  const deckName = data.decks.find((d) => d.path === data.deck)?.name;
  const go = (deck: string | undefined) => {
    const who = login.trim();
    if (who) localStorage.setItem(LAST_WITH, who);
    void navigate({
      to: "/trade",
      search: { ...(who ? { with: who } : {}), ...(deck ? { deck } : {}) },
    });
  };
  return (
    <main>
      <Toolbar
        name="Trade"
        actions={
          result.kind === "trade" &&
          offers.length > 0 && (
            <CopyText
              text={tradeText(data.me, result.who, offers, deckName)}
              who={result.who}
            />
          )
        }
      />
      <form
        className="trade-ask"
        onSubmit={(e) => {
          e.preventDefault();
          go(data.deck);
        }}
      >
        <label>
          Trade with
          <input
            value={login}
            onChange={(e) => setLogin(e.target.value)}
            placeholder="their GitHub login"
            aria-label="Their GitHub login"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
          />
        </label>
        <label>
          For
          <select
            aria-label="What to trade for"
            value={data.deck ?? ""}
            onChange={(e) => go(e.target.value || undefined)}
          >
            <option value="">The whole wanted list</option>
            {data.decks.map((d) => (
              <option key={d.path} value={d.path}>
                {d.name}
              </option>
            ))}
          </select>
        </label>
        <button type="submit" className="button primary">
          Compare
        </button>
        <span className="muted">
          {deckName
            ? `What ${deckName} lacks of your collection is checked against theirs.`
            : "Your wanted list is checked against their collection."}{" "}
          Their Curator repo has to be public.
        </span>
      </form>
      <Result result={result} offers={offers} />
    </main>
  );
}

function Result({
  result: data,
  offers,
}: {
  result: TradeResult;
  offers: TradeOffer[];
}) {
  switch (data.kind) {
    case "ask":
      return null;
    case "not-a-login":
      return (
        <p className="refusal trade-note" role="alert">
          “{data.text}” is not a GitHub login.
        </p>
      );
    case "no-repo":
      return (
        <p className="refusal trade-note" role="alert">
          GitHub shows no public {data.who}/mtg. A private repo looks the same
          from here, so it has to be public to trade against.
        </p>
      );
    case "no-file":
      return (
        <p className="trade-note muted">
          {data.who}/mtg has no collection yet.
        </p>
      );
    case "trade":
      break;
  }
  if (data.trade.kind === "refused") {
    return (
      <p className="refusal trade-note" role="alert">
        {data.trade.message}
      </p>
    );
  }
  const bring = offers.reduce(
    (n, o) => n + o.pulls.reduce((m, p) => m + p.qty, 0),
    0,
  );
  return (
    <>
      {data.trade.unread.map((u) => (
        <p key={u.path} className="refusal-inline trade-note">
          {u.path} is left out of what your decks lack: {u.message}
        </p>
      ))}
      {offers.length === 0 ? (
        <p className="trade-note muted">
          {data.who} has none of the cards you want.
        </p>
      ) : (
        <p className="trade-note">
          <a
            href={`https://github.com/${data.who}/mtg`}
            target="_blank"
            rel="noreferrer"
          >
            {data.who}
          </a>{" "}
          has {bring} {bring === 1 ? "copy" : "copies"} of {offers.length}{" "}
          {offers.length === 1 ? "card" : "cards"} you want.
        </p>
      )}
      {piles(offers).map((p) => (
        <section key={p.at ?? ""} className="place" aria-label={pileName(p)}>
          <header className="place-header">
            <h2>{pileName(p)}</h2>
            <span className="badge" title="Copies to bring">
              {p.rows.reduce((n, r) => n + r.pull.qty, 0)}
            </span>
          </header>
          <table className="wanted-table">
            <thead>
              <tr>
                <th className="wanted-qty">Bring</th>
                <th>Card</th>
                <th className="trade-copy">Their copy</th>
                <th className="trade-want" title="By hand and by your decks">
                  You want
                </th>
              </tr>
            </thead>
            <tbody>
              {p.rows.map(({ offer, pull }, i) => (
                // A place can hold a card on two lines, a foil and not.
                // biome-ignore lint/suspicious/noArrayIndexKey: the rows are never reordered in place
                <tr key={`${offer.name}:${i}`}>
                  <td className="wanted-qty">{pull.qty}</td>
                  <td className="wanted-name">{offer.name}</td>
                  <td className="trade-copy">
                    {copyOf(pull) || <span className="muted">—</span>}
                  </td>
                  <td className="trade-want">{offer.short}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      ))}
    </>
  );
}

function CopyText({ text, who }: { text: string; who: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      title={`The list by place, to send ${who}`}
      onClick={() =>
        void navigator.clipboard.writeText(text).then(() => setCopied(true))
      }
    >
      {copied ? <CheckIcon /> : <CopyIcon />}
      {copied ? "Copied" : `Copy for ${who}`}
    </button>
  );
}
