import type { Card } from "../deck";
import type { Printings } from "../scryfall";
import { playtestUrl } from "./playtest";

/**
 * Opens the deck in Archidekt's playtester in a new tab, as it stands in the
 * editor, saved or not. A deck it cannot send whole goes to `onRefusal`.
 */
export function PlaytestArchidekt({
  cards,
  printings,
  onRefusal,
}: {
  cards: readonly Card[];
  printings: Printings;
  onRefusal: (message: string) => void;
}) {
  const open = () => {
    const playtest = playtestUrl(cards, printings);
    if (playtest.kind === "refused") onRefusal(playtest.message);
    else window.open(playtest.url, "_blank", "noopener");
  };

  return (
    <button
      type="button"
      onClick={open}
      title="Open the deck in Archidekt's playtester"
    >
      Playtest
    </button>
  );
}
