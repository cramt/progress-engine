import {
  type Card,
  type Category,
  declareCategory,
  removeCard,
  setCardCategories,
  setCardQty,
  setCommander,
} from "../deck";
import { dropOnto } from "../deck/move";
import type { CardAction } from "./hotkeys";

/**
 * A card as the user reached it: its index in the file, and the category of
 * the stack it was reached in (null for the Uncategorized stack, or anywhere
 * that is not a stack). A move replaces `from`, as a drag does.
 */
export interface Target {
  index: number;
  from: string | null;
}

/** The parts of a parsed deck an edit needs to know. */
export interface DeckState {
  categories: readonly Category[];
  cards: readonly Card[];
}

/** The actions that change the deck's text. */
export type EditAction = Exclude<
  CardAction,
  { kind: "copy-name" } | { kind: "printing" }
>;

export function isEdit(action: CardAction): action is EditAction {
  return action.kind !== "copy-name" && action.kind !== "printing";
}

/** The name the drag strip gives a board it has to declare. */
export const BOARD_NAME = {
  maybeboard: "Maybeboard",
  sideboard: "Sideboard",
} as const;

/** A selection: each selected card's index, and the stack it was picked in. */
export type Selection = ReadonlyMap<number, string | null>;

/**
 * The cards an action on `target` applies to: every selected card when the
 * target is one of them, otherwise the target alone.
 */
export function targetsFor(target: Target, selection: Selection): Target[] {
  if (!selection.has(target.index)) return [target];
  return [...selection].map(([index, from]) => ({ index, from }));
}

/** The selection with `target` added, or taken out if it was in it. */
export function toggleSelected(
  selection: Selection,
  target: Target,
): Map<number, string | null> {
  const next = new Map(selection);
  if (next.has(target.index)) next.delete(target.index);
  else next.set(target.index, target.from);
  return next;
}

/**
 * The deck text after `action` on every target, as one edit. Cards are edited
 * from the last line up, so removing one never moves a card still to come.
 * A board or category that is not declared yet is declared first, the way
 * the drag strip does. Throws the first refusal.
 */
export function applyEdit(
  text: string,
  deck: DeckState,
  action: EditAction,
  targets: readonly Target[],
): string {
  let next = text;
  let destination = "";
  if (action.kind === "board") {
    const existing = deck.categories.find((c) => c.kind === action.type);
    destination = existing?.name ?? BOARD_NAME[action.type];
    if (!existing) next = declareCategory(next, destination, action.type);
  } else if (action.kind === "category") {
    destination = action.name;
    if (!deck.categories.some((c) => c.name === destination))
      next = declareCategory(next, destination);
  }
  const byIndex = new Map(targets.map((t) => [t.index, t]));
  const lastFirst = [...byIndex.values()].toSorted((a, b) => b.index - a.index);
  for (const target of lastFirst) {
    const card = deck.cards.find((c) => c.index === target.index);
    if (!card) throw new Error(`There is no card ${target.index} in the deck.`);
    switch (action.kind) {
      case "increase":
        next = setCardQty(next, card.index, card.qty + 1);
        break;
      case "decrease":
        next = setCardQty(next, card.index, card.qty - 1);
        break;
      case "remove":
        next = removeCard(next, card.index);
        break;
      case "automatic":
        next = setCardCategories(next, card.index, []);
        break;
      case "commander":
        next = setCommander(next, card.index);
        break;
      case "board":
      case "category":
        next = setCardCategories(
          next,
          card.index,
          dropOnto(card.categories, target.from, destination, false),
        );
        break;
    }
  }
  return next;
}
