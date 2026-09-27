import {
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { Card, Parsed } from "../deck";
import {
  fetchPrintings,
  type Printing,
  type Printings,
  printingKey,
} from "../scryfall";
import {
  applyEdit,
  isEdit,
  type Selection,
  type Target,
  targetsFor,
  toggleSelected,
} from "./apply";
import type { CardViewProps, MenuEntry } from "./CardView";
import { DetailsModal } from "./DetailsModal";
import { actionForKey, type CardAction, isTyping } from "./hotkeys";
import { displayOrder } from "./order";

/** A card's name: the file's, or Scryfall's for a card named by printing. */
export function cardName(card: Card, printings: Printings): string {
  if (card.card.kind === "name") return card.card.name;
  return (
    printings.get(printingKey(card.card))?.name ??
    `${card.card.set}/${card.card.num}`
  );
}

/** Scryfall's page for the card, the one card extra Curator keeps. */
function scryfallPage(card: Card, name: string): string {
  return card.card.kind === "printing"
    ? `https://scryfall.com/card/${card.card.set}/${card.card.num}`
    : `https://scryfall.com/search?q=${encodeURIComponent(`!"${name}"`)}`;
}

/**
 * The printings the deck shows: the page's, plus any it lacks, looked up as
 * cards gain printings (by a pick, an undo or a paste), plus pictures handed
 * over by whoever already has them.
 */
function useLivePrintings(initial: Printings, cards: readonly Card[]) {
  const [known, setKnown] = useState<ReadonlyMap<string, Printing>>(
    () => new Map(),
  );
  const asked = useRef(new Set<string>());
  const printings = useMemo<Printings>(
    () => (known.size === 0 ? initial : new Map([...initial, ...known])),
    [initial, known],
  );
  useEffect(() => {
    const missing = cards.filter((c) => {
      const key = printingKey(c.card);
      return !printings.has(key) && !asked.current.has(key);
    });
    if (missing.length === 0) return;
    for (const c of missing) asked.current.add(printingKey(c.card));
    fetchPrintings(missing)
      .then((found) => {
        if (found.size > 0) setKnown((k) => new Map([...k, ...found]));
      })
      // The card shows its name instead; nothing else depends on the picture.
      .catch(() => undefined);
  }, [cards, printings]);
  const remember = useCallback((key: string, printing: Printing) => {
    setKnown((k) => new Map([...k, [key, printing]]));
  }, []);
  return { printings, remember };
}

interface Details {
  index: number;
  /**
   * The walk prev/next follows: the display order as the modal opened, kept
   * while it is open, so moving a card to another category does not send
   * the walk back over cards already seen.
   */
  order: readonly number[];
  focusPrinting: boolean;
  grid: boolean;
}

export interface CardEditor {
  /** The deck's printings, kept up with printings picked since it loaded. */
  printings: Printings;
  /** What `CardView` shows and does for `card`, met in the stack for `from`. */
  cardProps: (card: Card, from: string | null) => CardViewProps;
  /** The details modal, the new-category prompt and the copy notice. */
  overlay: ReactNode;
}

/**
 * Everything a deck card does beyond being drawn: its `...` menu, the hover
 * hotkeys, multi-select and the details modal. Every change is one edit
 * through `chip-decklist`, handed to `edit` for the page's undo history; a
 * refusal goes to `refuse`, and `refuse(null)` clears it after a success.
 */
export function useCardEditor({
  text,
  deck,
  printings: initial,
  edit,
  refuse,
}: {
  text: string;
  deck: Parsed;
  printings: Printings;
  edit: (next: string) => void;
  refuse: (message: string | null) => void;
}): CardEditor {
  const cards = deck.kind === "deck" ? deck.cards : [];
  const categories = deck.kind === "deck" ? deck.categories : [];
  const { printings, remember } = useLivePrintings(initial, cards);
  const nameOf = useCallback((c: Card) => cardName(c, printings), [printings]);
  const order = useMemo(
    () => displayOrder(categories, cards, nameOf),
    [categories, cards, nameOf],
  );

  const hovered = useRef<Target | null>(null);
  const [selection, setSelection] = useState<Selection>(() => new Map());
  const [details, setDetails] = useState<Details | null>(null);
  const [naming, setNaming] = useState<Target[] | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [modalRefusal, setModalRefusal] = useState<string | null>(null);
  const openDetails = (index: number, focusPrinting: boolean) =>
    setDetails({ index, order, focusPrinting, grid: false });

  // Indices move when a card is added or removed (an undo included), so what
  // was hovered or selected by index may now be another card.
  const count = cards.length;
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs on the count changing
  useEffect(() => {
    hovered.current = null;
    setSelection(new Map());
    setDetails((d) => d && { ...d, order });
  }, [count]);
  useEffect(() => {
    if (!notice) return;
    const t = setTimeout(() => setNotice(null), 1800);
    return () => clearTimeout(t);
  }, [notice]);

  const fail = (e: unknown) => {
    const message = e instanceof Error ? e.message : String(e);
    refuse(message);
    setModalRefusal(message);
  };
  const tryEdit = (make: (text: string) => string): boolean => {
    try {
      edit(make(text));
      refuse(null);
      setModalRefusal(null);
      return true;
    } catch (e) {
      fail(e);
      return false;
    }
  };

  const run = (action: CardAction, targets: Target[]) => {
    if (targets.length === 0) return;
    if (action.kind === "copy-name") {
      const names = targets.flatMap((t) => {
        const c = cards.find((c) => c.index === t.index);
        return c ? [nameOf(c)] : [];
      });
      navigator.clipboard
        .writeText(names.join("\n"))
        .then(() => setNotice(`Copied ${names.join(", ")}`))
        .catch(fail);
      return;
    }
    if (action.kind === "printing") {
      const first = targets[0];
      if (first) openDetails(first.index, true);
      return;
    }
    if (isEdit(action))
      tryEdit((t) => applyEdit(t, { categories, cards }, action, targets));
  };

  // The hotkeys read the latest render through this, so the listener is
  // installed once.
  const latest = useRef({ run, selection, blocked: false });
  latest.current = {
    run,
    selection,
    blocked: details !== null || naming !== null,
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const now = latest.current;
      if (e.defaultPrevented || now.blocked || isTyping(e.target)) return;
      if (e.key === "Escape" && now.selection.size > 0) {
        setSelection(new Map());
        return;
      }
      const target = hovered.current;
      const action = actionForKey(e);
      if (!target || !action) return;
      e.preventDefault();
      now.run(action, targetsFor(target, now.selection));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const menuFor = (card: Card, target: Target): MenuEntry[] => {
    const targets = targetsFor(target, selection);
    const on = (action: CardAction) => () => run(action, targets);
    const many = targets.length > 1;
    const name = nameOf(card);
    return [
      ...(many
        ? [
            {
              label: `${targets.length} selected cards`,
              disabled: true,
              run: () => undefined,
            },
            "separator" as const,
          ]
        : []),
      {
        label: "Open details",
        hotkey: "Click",
        run: () => openDetails(card.index, false),
      },
      "separator",
      {
        label: "Increase quantity",
        hotkey: "+",
        run: on({ kind: "increase" }),
      },
      {
        label: "Decrease quantity",
        hotkey: "-",
        run: on({ kind: "decrease" }),
      },
      {
        label: "Switch card printing",
        hotkey: "P",
        run: () => openDetails(card.index, true),
      },
      { label: "Set as commander", run: on({ kind: "commander" }) },
      {
        label: "Move to category",
        submenu: [
          { label: "Automatic", hotkey: "A", run: on({ kind: "automatic" }) },
          {
            label: "Maybeboard",
            hotkey: "M",
            run: on({ kind: "board", type: "maybeboard" }),
          },
          {
            label: "Sideboard",
            hotkey: "S",
            run: on({ kind: "board", type: "sideboard" }),
          },
          { label: "Create new category", run: () => setNaming(targets) },
          "separator",
          ...categories.map(
            (c): MenuEntry => ({
              label: c.name,
              disabled: !many && card.categories.includes(c.name),
              run: on({ kind: "category", name: c.name }),
            }),
          ),
        ],
      },
      { label: "Remove card", hotkey: "R", run: on({ kind: "remove" }) },
      "separator",
      {
        label: selection.has(card.index) ? "Deselect" : "Multi-select",
        hotkey: "Ctrl+Click",
        run: () => setSelection((s) => toggleSelected(s, target)),
      },
      ...(selection.size > 0
        ? [
            {
              label: "Clear selection",
              hotkey: "Esc",
              run: () => setSelection(new Map()),
            },
          ]
        : []),
      {
        label: "Card extras",
        submenu: [
          {
            label: "Copy card name",
            hotkey: "C",
            run: on({ kind: "copy-name" }),
          },
          {
            label: "Scryfall",
            run: () =>
              window.open(scryfallPage(card, name), "_blank", "noopener"),
          },
        ],
      },
    ];
  };

  const cardProps = (card: Card, from: string | null): CardViewProps => {
    const target = { index: card.index, from };
    const printing = printings.get(printingKey(card.card));
    return {
      name: nameOf(card),
      image: printing?.image,
      qty: card.qty,
      finish: card.finish,
      variant: "deck",
      selected: selection.has(card.index),
      menu: menuFor(card, target),
      onOpen: () => openDetails(card.index, false),
      onSelect: () => setSelection((s) => toggleSelected(s, target)),
      onHover: (on) => {
        if (on) hovered.current = target;
        else if (
          hovered.current?.index === target.index &&
          hovered.current.from === target.from
        )
          hovered.current = null;
      },
      onIncrease: () =>
        run({ kind: "increase" }, targetsFor(target, selection)),
      onDecrease: () =>
        run({ kind: "decrease" }, targetsFor(target, selection)),
    };
  };

  const shown = details && cards.find((c) => c.index === details.index);
  const overlay = (
    <>
      {details && shown && (
        <DetailsModal
          card={shown}
          name={nameOf(shown)}
          printing={printings.get(printingKey(shown.card))}
          categories={categories}
          order={details.order.includes(shown.index) ? details.order : order}
          focusPrinting={details.focusPrinting}
          grid={details.grid}
          refusal={modalRefusal}
          onGrid={(grid) => setDetails({ ...details, grid })}
          onStep={(index) => {
            setModalRefusal(null);
            setDetails({ ...details, index, focusPrinting: false });
          }}
          onClose={() => {
            setModalRefusal(null);
            setDetails(null);
          }}
          onEdit={tryEdit}
          onRemember={remember}
        />
      )}
      {naming && (
        <NamePrompt
          onDone={(name) => {
            setNaming(null);
            if (name) run({ kind: "category", name }, naming);
          }}
        />
      )}
      {notice && (
        <p className="card-notice" role="status">
          {notice}
        </p>
      )}
    </>
  );

  return { printings, cardProps, overlay };
}

function NamePrompt({ onDone }: { onDone: (name: string | null) => void }) {
  return (
    <form
      className="new-category"
      onSubmit={(e) => {
        e.preventDefault();
        const name = new FormData(e.currentTarget).get("name");
        onDone(typeof name === "string" && name.trim() ? name.trim() : null);
      }}
    >
      <label>
        New category
        <input
          name="name"
          // biome-ignore lint/a11y/noAutofocus: the user just asked to type a name
          autoFocus
          onKeyDown={(e) => e.key === "Escape" && onDone(null)}
        />
      </label>
      <button type="submit">Add</button>
      <button type="button" onClick={() => onDone(null)}>
        Cancel
      </button>
    </form>
  );
}
