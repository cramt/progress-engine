/**
 * What a card can be asked to do, from its `...` menu or by a key pressed
 * while it is hovered. The keys are Archidekt's.
 */
export type CardAction =
  | { kind: "increase" }
  | { kind: "decrease" }
  | { kind: "remove" }
  /** Archidekt's "Automatic": no categories, so untyped and in the deck. */
  | { kind: "automatic" }
  /** The deck's maybeboard or sideboard, declared if the deck has none. */
  | { kind: "board"; type: "maybeboard" | "sideboard" }
  /** A category by name, declared (untyped) if the deck has none by that name. */
  | { kind: "category"; name: string }
  | { kind: "commander" }
  | { kind: "copy-name" }
  /** Open the details modal on its printing dropdown. */
  | { kind: "printing" };

/** The parts of a `KeyboardEvent` a hotkey is decided by. */
export interface KeyPress {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
}

/**
 * The action a key asks of the hovered card, or null for a key that is not a
 * hotkey. A chord with Ctrl, Cmd or Alt is never one, so undo (Ctrl+Z) and
 * copy (Ctrl+C) keep their meaning. `+` is also accepted unshifted, as `=`.
 */
export function actionForKey(e: KeyPress): CardAction | null {
  if (e.ctrlKey || e.metaKey || e.altKey) return null;
  switch (e.key.toLowerCase()) {
    case "+":
    case "=":
      return { kind: "increase" };
    case "-":
      return { kind: "decrease" };
    case "r":
      return { kind: "remove" };
    case "a":
      return { kind: "automatic" };
    case "m":
      return { kind: "board", type: "maybeboard" };
    case "s":
      return { kind: "board", type: "sideboard" };
    case "c":
      return { kind: "copy-name" };
    case "p":
      return { kind: "printing" };
    default:
      return null;
  }
}

/** The parts of an event target that say whether the user is typing into it. */
export interface MaybeField {
  tagName?: string;
  isContentEditable?: boolean;
}

/**
 * Whether a key pressed on `target` is text being typed rather than a
 * command: an input, a textarea, a select or anything contenteditable.
 */
export function isTyping(target: unknown): boolean {
  if (typeof target !== "object" || target === null) return false;
  const field = target as MaybeField;
  if (field.isContentEditable) return true;
  const tag = field.tagName?.toUpperCase();
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}
