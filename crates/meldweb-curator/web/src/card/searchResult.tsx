import type { ResultCard } from "../search/backend";
import type { RenderResult, ResultActions } from "../search/ResultCard";
import { CardView, type CardViewProps, type MenuEntry } from "./CardView";

/**
 * A search result's menu: Archidekt's for a card, with what only a card in
 * the deck can do greyed out, and "Move to category" become "Add to category".
 */
export function searchResultMenu(
  card: ResultCard,
  actions: ResultActions,
): MenuEntry[] {
  return [
    { label: "Add to deck", hotkey: "+", run: () => actions.add(null) },
    { label: "Decrease quantity", hotkey: "-", disabled: true, run: noop },
    {
      label: "Add to category",
      submenu: [
        { label: "Automatic", run: () => actions.add(null) },
        ...(actions.categories.length > 0 ? ["separator" as const] : []),
        ...actions.categories.map(
          (c): MenuEntry => ({ label: c.name, run: () => actions.add(c.name) }),
        ),
      ],
    },
    "separator",
    {
      label: "Card extras",
      submenu: [
        {
          label: "Copy card name",
          hotkey: "C",
          run: () => void navigator.clipboard.writeText(card.name),
        },
        {
          label: "Scryfall",
          run: () =>
            window.open(
              `https://scryfall.com/card/${card.set}/${card.num}`,
              "_blank",
              "noopener",
            ),
        },
      ],
    },
  ];
}

function noop() {}

/** What `CardView` shows and does for a search result. */
export function searchResultProps(
  card: ResultCard,
  actions: ResultActions,
): CardViewProps {
  const { onDragStart, onDragEnd } = actions.dragProps;
  return {
    name: card.name,
    image: card.image,
    variant: "search",
    menu: searchResultMenu(card, actions),
    onIncrease: () => actions.add(null),
    onDragStart,
    onDragEnd,
  };
}

/**
 * The search overlay's results drawn as the one card component: `+` adds a
 * copy, `...` opens the menu, and the card drags onto a stack.
 */
export const renderCardResult: RenderResult = (card, actions) => (
  <CardView {...searchResultProps(card, actions)} />
);
