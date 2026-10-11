import { useEffect, useState } from "react";
import type { Card } from "../deck";
import { CardCell, SupplySection } from "./SupplyParts";
import {
  type Pile,
  piles,
  printingLabel,
  progress,
  type Row,
  sleeved,
} from "./sourcing";
import type { Supply } from "./useSupply";

/** The lines ticked off while going through the physical deck, per deck. */
const ticksKey = (path: string) => `meldweb-build-ticks:${path}`;

function loadTicks(path: string): ReadonlySet<string> {
  try {
    const raw = JSON.parse(localStorage.getItem(ticksKey(path)) ?? "[]");
    return new Set(Array.isArray(raw) ? raw.map(String) : []);
  } catch {
    return new Set();
  }
}

/** A row's tick outlives a move in the collection, which renumbers its lines. */
const tickOf = (r: Row) => `${r.line.index}:${r.line.name}`;

/** A place not taken in the collection, for the deck's own. */
function freeName(name: string, places: readonly string[]): string {
  const taken = (n: string) =>
    places.some((p) => p.toLowerCase() === n.toLowerCase());
  if (!taken(name)) return name;
  for (let i = 2; ; i++) if (!taken(`${name} ${i}`)) return `${name} ${i}`;
}

/**
 * The physical deck against the list: the copies to pull into it, place by
 * place, the ones to take out, what is missing, and the ones already in it
 * to tick off while going through the deck. A move is a commit of the
 * collection, made at once.
 */
export function BuildView({
  supply,
  cards,
  path,
  deckName,
  onSourcing,
}: {
  supply: Supply;
  cards: readonly Card[];
  path: string;
  deckName: string;
  onSourcing: () => void;
}) {
  const [ticks, setTicks] = useState(() => loadTicks(path));
  useEffect(() => {
    localStorage.setItem(ticksKey(path), JSON.stringify([...ticks]));
  }, [path, ticks]);
  const { state } = supply;

  if (state.kind === "loading")
    return <p className="muted supply-status">Reading your collection…</p>;
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
  const { printings, saving } = state;
  const lineOf = (r: Row) => cards[r.line.index];

  const toolbar = (state.refusal || state.canUndo || saving) && (
    <div className="supply-moves">
      {saving && <span className="muted">Saving your collection…</span>}
      {state.canUndo && !saving && (
        <button type="button" className="small" onClick={supply.undo}>
          Undo the last move
        </button>
      )}
      {state.refusal && (
        <span className="refusal-inline" role="alert">
          {state.refusal}{" "}
          <button type="button" className="small" onClick={supply.dismiss}>
            Dismiss
          </button>
        </span>
      )}
    </div>
  );

  const place = s.place;
  if (place === undefined) {
    const name = freeName(deckName, state.places);
    return (
      <div className="supply">
        {toolbar}
        <div className="supply-no-place">
          <p>
            {deckName} has no place in your collection, so there is no telling
            which copies are sleeved in it.
          </p>
          <button
            type="button"
            className="button primary"
            disabled={saving}
            onClick={() => supply.makePlace(name)}
          >
            Make “{name}” a place for this deck
          </button>
          <p className="muted">
            Then pull its cards into it from your binders here, and the
            collection keeps track of where each copy went.
          </p>
        </div>
      </div>
    );
  }

  const p = progress(s);
  const inDeck = sleeved(s);
  const ticked = inDeck.filter((r) => ticks.has(tickOf(r))).length;
  const all = piles(s);
  const moveIn = (rows: readonly Row[]) =>
    supply.move(
      rows.map((r) => ({ line: r.copies.line, qty: r.copies.qty })),
      place,
    );
  const toggle = (r: Row) =>
    setTicks((t) => {
      const next = new Set(t);
      if (!next.delete(tickOf(r))) next.add(tickOf(r));
      return next;
    });

  return (
    <div className="supply">
      <div className="supply-summary">
        <progress
          className="supply-progress"
          max={p.need}
          value={p.inDeck}
          aria-label="Copies in the deck"
        />
        <span>
          <strong>{p.inDeck}</strong> of {p.need} in {place}
          {p.standIns > 0 && ` (${p.standIns} in another printing)`}
          {p.toPull > 0 && ` · ${p.toPull} to pull`}
          {p.toTakeOut > 0 && ` · ${p.toTakeOut} to take out`}
          {p.missing > 0 && (
            <>
              {" · "}
              <button type="button" className="link" onClick={onSourcing}>
                {p.missing} missing
              </button>
            </>
          )}
        </span>
      </div>
      {toolbar}

      {all.map((pile) => (
        <PileSection
          key={`${pile.whereabouts}:${pile.at ?? ""}`}
          pile={pile}
          place={place}
          saving={saving}
          lineOf={lineOf}
          printings={printings}
          onMove={moveIn}
        />
      ))}

      {s.extra.length > 0 && (
        <SupplySection
          title="Take out"
          count={p.toTakeOut}
          hint={`In ${place}, but not on the list.`}
          actions={
            <button
              type="button"
              className="small"
              disabled={saving}
              onClick={() =>
                supply.move(
                  s.extra.map((e) => ({ line: e.line, qty: e.qty })),
                  null,
                )
              }
            >
              Take all out
            </button>
          }
        >
          <thead>
            <tr>
              <th>Qty</th>
              <th>Card</th>
              <th>Why</th>
              <th aria-label="Move" />
            </tr>
          </thead>
          <tbody>
            {s.extra.map((e) => (
              <tr key={e.line}>
                <td>{e.qty}</td>
                <CardCell
                  name={e.name}
                  card={e.card}
                  finish={e.finish}
                  line={undefined}
                  printings={printings}
                />
                <td className="muted">
                  {e.listed
                    ? "A copy too many, or not the printing listed"
                    : "Not in the deck"}
                </td>
                <td>
                  <button
                    type="button"
                    className="small"
                    disabled={saving}
                    title="Move to unsorted"
                    onClick={() =>
                      supply.move([{ line: e.line, qty: e.qty }], null)
                    }
                  >
                    Take out
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </SupplySection>
      )}

      {inDeck.length > 0 && (
        <SupplySection
          title={`Check ${place}`}
          count={p.inDeck}
          hint={`Go through the deck and tick each card you find: ${ticked} of ${inDeck.length} ticked.`}
          actions={
            ticks.size > 0 && (
              <button
                type="button"
                className="small"
                onClick={() => setTicks(new Set())}
              >
                Clear ticks
              </button>
            )
          }
        >
          <thead>
            <tr>
              <th aria-label="Found" />
              <th>Card</th>
              <th>Qty</th>
            </tr>
          </thead>
          <tbody>
            {inDeck.map((r) => {
              const id = tickOf(r);
              const line = lineOf(r);
              return (
                <tr
                  key={`${id}:${r.copies.line}`}
                  className={ticks.has(id) ? "supply-ticked" : undefined}
                >
                  <td>
                    <input
                      type="checkbox"
                      checked={ticks.has(id)}
                      onChange={() => toggle(r)}
                      aria-label={`Found ${r.line.name}`}
                    />
                  </td>
                  <CardCell
                    name={r.line.name}
                    card={r.copies.card}
                    finish={r.copies.finish}
                    line={line}
                    printings={printings}
                    note={
                      !r.copies.exact && (
                        <span className="supply-standin">
                          stands in for{" "}
                          {line
                            ? printingLabel(line.card, line.finish) ||
                              "the card"
                            : "the card"}
                        </span>
                      )
                    }
                  />
                  <td>
                    {r.copies.qty}
                    {r.copies.qty < r.line.need && (
                      <span className="muted"> of {r.line.need}</span>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </SupplySection>
      )}
    </div>
  );
}

function PileSection({
  pile,
  place,
  saving,
  lineOf,
  printings,
  onMove,
}: {
  pile: Pile;
  place: string;
  saving: boolean;
  lineOf: (r: Row) => Card | undefined;
  printings: Parameters<typeof CardCell>[0]["printings"];
  onMove: (rows: readonly Row[]) => void;
}) {
  return (
    <SupplySection
      title={`Pull from ${pile.at ?? "unsorted"}`}
      count={pile.rows.reduce((n, r) => n + r.copies.qty, 0)}
      hint={
        pile.whereabouts === "otherDeck"
          ? "Sleeved in another deck: take it apart last."
          : undefined
      }
      actions={
        <button
          type="button"
          className="small"
          disabled={saving}
          onClick={() => onMove(pile.rows)}
        >
          Move all into {place}
        </button>
      }
    >
      <thead>
        <tr>
          <th>Qty</th>
          <th>Card</th>
          <th aria-label="Move" />
        </tr>
      </thead>
      <tbody>
        {pile.rows.map((r) => {
          const line = lineOf(r);
          return (
            <tr key={`${r.line.index}:${r.copies.line}`}>
              <td>{r.copies.qty}</td>
              <CardCell
                name={r.line.name}
                card={r.copies.card}
                finish={r.copies.finish}
                line={line}
                printings={printings}
                note={
                  !r.copies.exact && (
                    <span className="supply-standin">
                      the list names{" "}
                      {line
                        ? printingLabel(line.card, line.finish) || "the card"
                        : "the card"}
                    </span>
                  )
                }
              />
              <td>
                <button
                  type="button"
                  className="small"
                  disabled={saving}
                  onClick={() => onMove([r])}
                >
                  Move in
                </button>
              </td>
            </tr>
          );
        })}
      </tbody>
    </SupplySection>
  );
}
