import { DECK_VIEWS, type DeckView } from "./versions";

const LABEL: Record<DeckView, string> = {
  stacks: "Stacks",
  cost: "Cost",
};

/** Which layout of the deck the page shows; a new view is a new entry in `DECK_VIEWS`. */
export function ViewTabs({
  view,
  onView,
}: {
  view: DeckView;
  onView: (view: DeckView) => void;
}) {
  return (
    <nav className="deck-views segmented" aria-label="Deck view">
      {DECK_VIEWS.map((v) => (
        <button
          key={v}
          type="button"
          aria-pressed={v === view}
          onClick={() => onView(v)}
        >
          {LABEL[v]}
        </button>
      ))}
    </nav>
  );
}
