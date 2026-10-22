import { type ReactNode, useEffect, useMemo, useState } from "react";
import type { Card } from "../deck";
import type { BlameCommit, CardBlame } from "../deck.gen";
import type { BlameRead, Repo } from "../repo/api";
import { artCrop, cardName, type Printings, printingKey } from "../scryfall";
import { messageOf } from "../ui/Sheet";
import { Waiting } from "../ui/Spinner";
import {
  addedTouch,
  type BlameOrder,
  firstLine,
  lastTouch,
  shortDay,
  sortBlame,
  type Touch,
  unchangedSinceAdded,
} from "./blame";
import "./blame.css";

export type BlameState =
  | { kind: "idle" }
  | { kind: "loading" }
  /** `of` is the unsaved text it was worked out on top of, if any. */
  | { kind: "loaded"; blame: BlameRead; of: string | undefined }
  /** No deck Progress Engine reads at that commit. */
  | { kind: "none" }
  | { kind: "failed"; message: string };

/** How long the editor's text sits still before blame is asked again. */
const SETTLE_MS = 400;

/**
 * The deck `stem` blamed at `at` (a commit, or a branch such as
 * `decks/lantern/main`), with `unsaved` the editor's text on top, once
 * `active`. The repo remembers what it settled, so asking again after an
 * edit or a save costs only what changed since.
 */
export function useBlame(
  repo: Repo,
  stem: string,
  at: string,
  unsaved: string | undefined,
  active: boolean,
): BlameState {
  const [state, setState] = useState<BlameState>({ kind: "idle" });
  useEffect(() => {
    if (!active) return;
    let live = true;
    // Kept while the text settles, so an edit does not blank the table.
    setState((s) => (s.kind === "loaded" ? s : { kind: "loading" }));
    const t = setTimeout(() => {
      repo.blame(stem, at, unsaved).then(
        (blame) =>
          live &&
          setState(
            blame ? { kind: "loaded", blame, of: unsaved } : { kind: "none" },
          ),
        (e) => live && setState({ kind: "failed", message: messageOf(e) }),
      );
    }, SETTLE_MS);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [repo, stem, at, unsaved, active]);
  // Blame of other text than this has other card indices: until it is
  // asked again, it is still being read.
  return state.kind === "loaded" && state.of !== unsaved
    ? { kind: "loading" }
    : state;
}

/** Each card's blame by its index in the deck, once loaded. */
export function blameByIndex(
  state: BlameState,
): ReadonlyMap<number, CardBlame> {
  return state.kind === "loaded"
    ? new Map(state.blame.cards.map((c) => [c.index, c]))
    : new Map();
}

/** Opens the revision a commit made, of this deck or the one it was forked from. */
export type OpenCommit = (commit: BlameCommit) => void;

function CommitLink({
  touch,
  stem,
  onOpen,
  children,
}: {
  touch: Extract<Touch, { kind: "commit" }>;
  stem: string;
  onOpen: OpenCommit;
  children: ReactNode;
}) {
  const c = touch.commit;
  return (
    <button
      type="button"
      className="link blame-commit"
      title={`${firstLine(c.message)}\n${c.author} <${c.email}>, ${new Date(c.authoredAt * 1000).toLocaleString()}${c.deck === stem ? "" : `\nin ${c.deck}, before it was forked`}`}
      onClick={() => onOpen(c)}
    >
      {children}
    </button>
  );
}

/**
 * One card's blame as a sentence: "Added 3 Oct by Alex in 'lantern: +1 Sol
 * Ring' · last changed 14 Oct by Bo: Sol Ring: 1 → 2", each commit a link to
 * the deck as it left it.
 */
export function BlameLine({
  blame,
  stem,
  onOpen,
  now = new Date(),
}: {
  blame: CardBlame;
  stem: string;
  onOpen: OpenCommit;
  now?: Date;
}) {
  const added = addedTouch(blame.blame);
  const last = lastTouch(blame.blame);
  const what = blame.changes.join(", ");
  const from = (c: BlameCommit) => (c.deck === stem ? "" : ` (${c.deck})`);
  return (
    <p className="blame-line">
      {added.kind === "unsaved" ? (
        <>Added in edits not saved yet</>
      ) : (
        <>
          Added {shortDay(added.commit.authoredAt, now)} by{" "}
          {added.commit.author} in{" "}
          <CommitLink touch={added} stem={stem} onOpen={onOpen}>
            ‘{firstLine(added.commit.message)}’
          </CommitLink>
          {from(added.commit)}
        </>
      )}
      {!unchangedSinceAdded(blame.blame) && (
        <>
          {" · "}
          {last.kind === "unsaved" ? (
            <>changed in edits not saved yet: {what}</>
          ) : (
            <>
              last changed{" "}
              <CommitLink touch={last} stem={stem} onOpen={onOpen}>
                {shortDay(last.commit.authoredAt, now)}
              </CommitLink>{" "}
              by {last.commit.author}: {what}
              {from(last.commit)}
            </>
          )}
        </>
      )}
    </p>
  );
}

/** The blame line for a card in the details modal, or what stands in for it. */
export function CardBlamePanel({
  state,
  blame,
  stem,
  onOpen,
}: {
  state: BlameState;
  blame: CardBlame | undefined;
  stem: string;
  onOpen: OpenCommit;
}) {
  return (
    <section className="details-history" aria-label="Blame">
      <h3>Blame</h3>
      {blame ? (
        <BlameLine blame={blame} stem={stem} onOpen={onOpen} />
      ) : state.kind === "failed" ? (
        <p className="hint">{state.message}</p>
      ) : state.kind === "none" ? (
        <p className="hint">This deck has no history yet.</p>
      ) : (
        <p className="hint">
          <Waiting>Reading the deck's history…</Waiting>
        </p>
      )}
    </section>
  );
}

const ORDERS: { order: BlameOrder; label: string }[] = [
  { order: "name", label: "Card" },
  { order: "changed", label: "Last changed" },
  { order: "added", label: "Added" },
];

/**
 * Every card with its last change, newest first, so what changed recently
 * is one glance; sortable by when each was added or by name. Each commit
 * opens the deck as it left it.
 */
export function BlameView({
  state,
  cards,
  printings,
  stem,
  onOpen,
}: {
  state: BlameState;
  cards: readonly Card[];
  printings: Printings;
  stem: string;
  onOpen: OpenCommit;
}) {
  const [order, setOrder] = useState<BlameOrder>("changed");
  const now = useMemo(() => new Date(), []);
  const rows = useMemo(() => {
    const byIndex = blameByIndex(state);
    return cards.flatMap((card) => {
      const blame = byIndex.get(card.index);
      return blame ? [{ card, blame, name: cardName(card, printings) }] : [];
    });
  }, [state, cards, printings]);
  if (state.kind === "failed")
    return (
      <p className="refusal" role="alert">
        {state.message}
      </p>
    );
  if (state.kind === "none")
    return <p className="hint">There is no history of this deck here.</p>;
  if (state.kind !== "loaded")
    return (
      <p className="blame-waiting">
        <Waiting>Reading the deck's history…</Waiting>
      </p>
    );
  return (
    <section className="blame-view" aria-label="Blame">
      <table className="blame-table">
        <thead>
          <tr>
            <th aria-label="Art" />
            {ORDERS.map(({ order: o, label }) => (
              <th
                key={o}
                aria-sort={
                  o === order
                    ? o === "name"
                      ? "ascending"
                      : "descending"
                    : "none"
                }
              >
                <button
                  type="button"
                  className="ghost small"
                  aria-pressed={o === order}
                  onClick={() => setOrder(o)}
                >
                  {label}
                </button>
              </th>
            ))}
            <th>What changed</th>
          </tr>
        </thead>
        <tbody>
          {sortBlame(rows, order).map(({ card, blame, name }) => {
            const last = lastTouch(blame.blame);
            const added = addedTouch(blame.blame);
            const art = printings.get(printingKey(card.card))?.image;
            return (
              <tr key={card.index} data-card-name={name}>
                <td>
                  {art ? (
                    <img
                      crossOrigin="anonymous"
                      className="change-thumb"
                      src={artCrop(art)}
                      alt=""
                      loading="lazy"
                    />
                  ) : (
                    <span className="change-thumb" aria-hidden="true" />
                  )}
                </td>
                <td className="blame-name">
                  {card.qty > 1 ? `${card.qty} ` : ""}
                  {name}
                </td>
                <td>
                  <When touch={last} stem={stem} onOpen={onOpen} now={now} />
                </td>
                <td>
                  <When touch={added} stem={stem} onOpen={onOpen} now={now} />
                </td>
                <td className="blame-what">
                  {blame.changes.join(", ")}
                  {last.kind === "commit" && (
                    <span className="blame-subject">
                      {firstLine(last.commit.message)}
                    </span>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </section>
  );
}

function When({
  touch,
  stem,
  onOpen,
  now,
}: {
  touch: Touch;
  stem: string;
  onOpen: OpenCommit;
  now: Date;
}) {
  if (touch.kind === "unsaved") return <span className="badge">Unsaved</span>;
  return (
    <span className="blame-when">
      <CommitLink touch={touch} stem={stem} onOpen={onOpen}>
        {shortDay(touch.commit.authoredAt, now)}
      </CommitLink>
      <span className="blame-by">
        {touch.commit.author} ·{" "}
        {new Date(touch.commit.authoredAt * 1000).toLocaleTimeString([], {
          hour: "2-digit",
          minute: "2-digit",
        })}
        {touch.commit.deck === stem ? "" : ` · ${touch.commit.deck}`}
      </span>
    </span>
  );
}
