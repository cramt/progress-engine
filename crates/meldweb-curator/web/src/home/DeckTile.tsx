import { Link } from "@tanstack/react-router";
import type { DeckEntry } from "../github/decks";
import { artCrop, type Printings, printingKey } from "../scryfall";
import "./home.css";

/** WUBRG order, as Scryfall and every decklist write colour identity. */
const COLOURS = ["W", "U", "B", "R", "G"] as const;

/**
 * One deck in the list: its commanders' art, its name, what it is, and where
 * its file is. A deck the format refuses still gets a tile, saying why.
 */
export function DeckTile({
  deck,
  printings,
}: {
  deck: DeckEntry;
  printings: Printings;
}) {
  const commanders = (deck.commanders ?? []).flatMap((c) => {
    const p = printings.get(printingKey(c));
    return p ? [p] : [];
  });
  const identity = new Set(commanders.flatMap((p) => p.colorIdentity));
  return (
    <Link
      to="/deck/$"
      params={{ _splat: deck.path }}
      className={deck.refused ? "deck-tile refused" : "deck-tile"}
    >
      <div className="deck-art">
        {commanders.map((p) => (
          <img key={p.image} src={artCrop(p.image)} alt="" loading="lazy" />
        ))}
      </div>
      <div className="deck-info">
        <div className="deck-title">
          <h2>{deck.name}</h2>
          {identity.size > 0 && (
            <span
              className="pips"
              title={`Colour identity: ${COLOURS.filter((c) => identity.has(c)).join("")}`}
            >
              {COLOURS.filter((c) => identity.has(c)).map((c) => (
                <span key={c} className={`pip pip-${c}`} />
              ))}
            </span>
          )}
        </div>
        {commanders.length > 0 && (
          <p className="deck-commander">
            {commanders.map((p) => p.name).join(" & ")}
          </p>
        )}
        {deck.refused ? (
          <p className="refusal-inline deck-refused">{deck.refused}</p>
        ) : (
          <p className="deck-meta">
            {deck.format && <span className="badge">{deck.format}</span>}
            {deck.total !== undefined && (
              <span>
                {deck.total} card{deck.total === 1 ? "" : "s"}
              </span>
            )}
          </p>
        )}
        <code className="deck-path">{deck.path}</code>
      </div>
    </Link>
  );
}
