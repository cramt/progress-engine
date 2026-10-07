import { useEffect, useState } from "react";
import type { Pin } from "../deck.gen";
import { fetchPrintings, type Printings, printingId } from "../scryfall";
import { HeartIcon } from "../ui/icons";

/**
 * The cards with a printing of their own, as the cards they are. In the file
 * each is a rule like the others; here it is a picture to unheart.
 */
export function Pinned({
  pins,
  onUnpin,
  onPreview,
}: {
  pins: readonly Pin[];
  onUnpin: (pin: Pin) => void;
  /** Shows the card's printings in the preview, to pick another. */
  onPreview: (name: string) => void;
}) {
  const [pictures, setPictures] = useState<Printings>(new Map());
  const wanted = pins.map(printingId).join(" ");
  // biome-ignore lint/correctness/useExhaustiveDependencies: refetched when the set of printings changes
  useEffect(() => {
    if (pins.length === 0) return;
    const abort = new AbortController();
    fetchPrintings(
      pins.map((p) => ({ card: { kind: "printing", set: p.set, num: p.num } })),
      abort.signal,
    )
      .then((found) => {
        if (!abort.signal.aborted) setPictures(found);
      })
      // A missing picture shows the name; the pin is still the pin.
      .catch(() => {});
    return () => abort.abort();
  }, [wanted]);

  return (
    <section className="pinned" aria-labelledby="pinned-heading">
      <h2 id="pinned-heading">Your printings</h2>
      <p className="settings-lede">
        The printing you get when you add one of these cards to a deck, and the
        one offered first when you pick. Heart any printing, in a deck or in the
        preview, to add it here.
      </p>
      {pins.length === 0 ? (
        <p className="settings-empty pinned-empty">
          <HeartIcon /> No favourites yet. Try Sol Ring in the preview.
        </p>
      ) : (
        <ul className="pinned-list">
          {pins.map((pin) => {
            const picture = pictures.get(printingId(pin));
            return (
              <li key={pin.name.toLowerCase()}>
                <button
                  type="button"
                  className="printing pinned-card"
                  title={`Show every ${pin.name} in the preview`}
                  onClick={() => onPreview(pin.name)}
                >
                  {picture?.image ? (
                    <img
                      crossOrigin="anonymous"
                      src={picture.image}
                      alt={pin.name}
                      loading="lazy"
                    />
                  ) : (
                    <span className="card-missing">{pin.name}</span>
                  )}
                  <span className="printing-set">{pin.name}</span>
                  <span className="printing-meta">
                    {pin.set.toUpperCase()} #{pin.num}
                  </span>
                </button>
                <button
                  type="button"
                  className="pin-heart pinned"
                  aria-pressed="true"
                  aria-label={`Stop ${pin.set.toUpperCase()} #${pin.num} being your ${pin.name}`}
                  title="Unheart: the rules decide again"
                  onClick={() => onUnpin(pin)}
                >
                  <HeartIcon filled />
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
