import type { DeckEdit } from "../deck";
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

/** The actions that change the deck's text. */
export type EditAction = Exclude<
  CardAction,
  { kind: "copy-name" } | { kind: "printing" }
>;

export function isEdit(action: CardAction): action is EditAction {
  return action.kind !== "copy-name" && action.kind !== "printing";
}

/**
 * The edit `action` is, for `editDeckCards`: a board or category to move to
 * is declared there when the deck lacks it, the way the drag strip does.
 */
export function toDeckEdit(action: EditAction): DeckEdit {
  switch (action.kind) {
    case "board":
      return {
        kind: "move",
        to: { kind: "board", board: action.type },
        secondary: false,
      };
    case "category":
      return {
        kind: "move",
        to: { kind: "category", name: action.name },
        secondary: false,
      };
    default:
      return action;
  }
}

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
