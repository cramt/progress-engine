import { type DragEvent, useState } from "react";
import type { Category } from "../deck";
import type { ResultCard } from "./backend";

/**
 * What a result can do, handed to whatever draws it. `add` puts one copy in
 * `category` (`null` is Automatic); the drag handlers make it droppable on
 * the deck's stacks.
 */
export interface ResultActions {
  add: (category: string | null) => void;
  categories: readonly Category[];
  dragProps: {
    draggable: true;
    onDragStart: (e: DragEvent) => void;
    onDragEnd: () => void;
  };
}

/**
 * The seam for #112's card component: the overlay draws each result with
 * this, so the shared card replaces `MinimalResult` by being passed in.
 */
export type RenderResult = (
  card: ResultCard,
  actions: ResultActions,
) => React.ReactNode;

/** A stand-in for the shared card: the image, `+` to add, `...` for a category. */
export function MinimalResult({
  card,
  actions,
}: {
  card: ResultCard;
  actions: ResultActions;
}) {
  const [menu, setMenu] = useState(false);
  return (
    <div className="result" title={card.name} {...actions.dragProps}>
      {card.image ? (
        <img
          src={card.image}
          alt={card.name}
          loading="lazy"
          draggable={false}
        />
      ) : (
        <div className="result-missing">{card.name}</div>
      )}
      <div className="result-buttons">
        <button
          type="button"
          className="result-add"
          aria-label={`Add ${card.name}`}
          title="Add one copy"
          onClick={() => actions.add(null)}
        >
          +
        </button>
        <button
          type="button"
          className="result-more"
          aria-label={`More for ${card.name}`}
          aria-expanded={menu}
          onClick={() => setMenu((m) => !m)}
        >
          ...
        </button>
      </div>
      {menu && (
        <>
          <button
            type="button"
            className="result-menu-backdrop"
            aria-label="Close menu"
            onClick={() => setMenu(false)}
          />
          <div className="result-menu" role="menu">
            <div className="result-menu-title">Add to category</div>
            {actions.categories.map((c) => (
              <button
                type="button"
                role="menuitem"
                key={c.name}
                onClick={() => {
                  setMenu(false);
                  actions.add(c.name);
                }}
              >
                {c.name}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

export const renderMinimalResult: RenderResult = (card, actions) => (
  <MinimalResult card={card} actions={actions} />
);
