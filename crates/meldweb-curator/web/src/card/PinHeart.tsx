import { HeartIcon } from "../ui/icons";
import { pinnedAs, usePins } from "./preference";
import type { PrintingOption } from "./prints";

/**
 * The heart in a printing's corner: filled when it is its card's own printing
 * (a pin in `meldweb.toml`), and a click to make it so or to stop it being.
 * Nothing where pins cannot be saved.
 */
export function PinHeart({ printing }: { printing: PrintingOption }) {
  const pins = usePins();
  if (!pins) return null;
  const pinned = pinnedAs(pins.pins, printing);
  const where = `${printing.setName} #${printing.num}`;
  return (
    <button
      type="button"
      className={pinned ? "pin-heart pinned" : "pin-heart"}
      aria-pressed={pinned}
      aria-label={
        pinned
          ? `${where} is your ${printing.name}; stop`
          : `Make ${where} your ${printing.name}`
      }
      title={
        pinned
          ? `Your ${printing.name}: offered first and added to decks. Click to stop.`
          : `Make this your ${printing.name}: offered first, and what adding it to a deck gives`
      }
      onClick={(e) => {
        e.stopPropagation();
        pins.toggle(printing);
      }}
    >
      <HeartIcon filled={pinned} />
    </button>
  );
}
