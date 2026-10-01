import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { CardMenu, type MenuEntry } from "../card/CardView";
import type { DeckEntry } from "../github/decks";
import { artCrop, type Printings, printingKey } from "../scryfall";
import { Sigil } from "../ui/Sigil";
import "./home.css";

/**
 * One deck in the list: its cover's art, or its commanders', its name, what it
 * is, and where its file is. A deck the format refuses still gets a tile,
 * saying why. Right-click or `...` opens `menu`, as on a card.
 */
export function DeckTile({
  deck,
  printings,
  menu,
}: {
  deck: DeckEntry;
  printings: Printings;
  menu: MenuEntry[];
}) {
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
  const commanders = (deck.commanders ?? []).flatMap((c) => {
    const p = printings.get(printingKey(c));
    return p ? [p] : [];
  });
  const cover =
    deck.cover &&
    printings.get(printingKey({ kind: "printing", ...deck.cover }));
  const art = cover ? [cover] : commanders;
  const identity = new Set(commanders.flatMap((p) => p.colorIdentity));
  return (
    <div className={menuAt ? "deck-tile-wrap menu-open" : "deck-tile-wrap"}>
      <Link
        to="/deck/$"
        params={{ _splat: deck.path }}
        className={deck.refused ? "deck-tile refused" : "deck-tile"}
        onContextMenu={(e) => {
          e.preventDefault();
          setMenuAt({ x: e.clientX, y: e.clientY });
        }}
      >
        <div className="deck-art">
          {art.map((p) => (
            <img
              crossOrigin="anonymous"
              key={p.image}
              src={artCrop(p.image)}
              alt=""
              loading="lazy"
            />
          ))}
        </div>
        <div className="deck-info">
          <div className="deck-title">
            <h2>{deck.name}</h2>
            {commanders.length > 0 && <Sigil identity={identity} />}
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
      <button
        type="button"
        className="card-edge-button deck-more"
        aria-label={`More for ${deck.name}`}
        title="More"
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setMenuAt({ x: r.right, y: r.bottom });
        }}
      >
        …
      </button>
      {menuAt && (
        <CardMenu
          entries={menu}
          x={menuAt.x}
          y={menuAt.y}
          onClose={() => setMenuAt(null)}
        />
      )}
    </div>
  );
}
