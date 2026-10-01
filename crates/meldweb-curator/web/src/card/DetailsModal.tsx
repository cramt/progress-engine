import { useEffect, useRef, useState } from "react";
import {
  type Card,
  type Category,
  declareCategory,
  type Finish,
  removeCard,
  setCardCategories,
  setCardFinish,
  setCardPrinting,
  setCardQty,
} from "../deck";
import { type Printing, printingId } from "../scryfall";
import { ChevronLeft, ChevronRight, CloseIcon } from "../ui/icons";
import { afterRemoval, neighbours } from "./order";
import { PrintingsGrid } from "./PrintingsGrid";
import { FINISHES, type PrintingOption, printsByName } from "./prints";
import { usePrintingOptions } from "./usePrintingOptions";

const NEW_CATEGORY = "\u0000new";

export interface DetailsModalProps {
  card: Card;
  name: string;
  /** The card's printing as the deck view knows it, when Scryfall answered. */
  printing: Printing | undefined;
  categories: readonly Category[];
  /** Every card's index in display order: what prev and next walk. */
  order: readonly number[];
  /** Opens with the printing dropdown focused, as the `P` hotkey asks. */
  focusPrinting: boolean;
  grid: boolean;
  /** Shown inside the modal, since it covers the page's own banner. */
  refusal: string | null;
  onGrid: (open: boolean) => void;
  onStep: (index: number) => void;
  onClose: () => void;
  /** Runs one edit on the latest text; false when it was refused. */
  onEdit: (edit: (text: string) => string) => boolean;
  /** A picked printing's picture, so the deck shows it without asking again. */
  onRemember: (key: string, printing: Printing) => void;
}

/**
 * Archidekt's card details: the card big on the left, its quantity, printing,
 * finish and categories on the right, and prev/next along the bottom walking
 * the deck in the order the stacks show it, without closing. The printing
 * choices come from one request to the card's `prints_search_uri`.
 */
export function DetailsModal(props: DetailsModalProps) {
  const {
    card,
    name,
    printing,
    order,
    grid,
    onGrid,
    onStep,
    onClose,
    onEdit,
    onRemember,
  } = props;
  const uri =
    printing?.prints ??
    (card.card.kind === "name" ? printsByName(card.card.name) : undefined);
  const options = usePrintingOptions(uri);
  const current = card.card.kind === "printing" ? printingId(card.card) : null;
  const currentOption =
    options.status === "done"
      ? options.printings.find((p) => printingId(p) === current)
      : undefined;
  const image = printing?.image ?? currentOption?.image;
  const { prev, next, position } = neighbours(order, card.index);

  const pick = (p: PrintingOption) => {
    if (printingId(p) === current) return;
    onRemember(printingId(p), {
      name: p.name,
      set: p.set,
      num: p.num,
      image: p.image ?? "",
      // Every printing of a card shares its colour identity.
      colorIdentity: printing?.colorIdentity ?? [],
      typeLine: printing?.typeLine ?? "",
      ...(uri ? { prints: uri } : {}),
    });
    onEdit((t) => setCardPrinting(t, card.index, p.set, p.num));
  };
  const setQty = (qty: number) => {
    if (qty >= 1) {
      onEdit((t) => setCardQty(t, card.index, qty));
      return;
    }
    const stay = afterRemoval(order, card.index);
    if (onEdit((t) => removeCard(t, card.index))) {
      if (stay === null) onClose();
      else onStep(stay);
    }
  };

  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      // A text field keeps its own keys, Escape included (it cancels naming),
      // and a dropdown keeps the arrows.
      const t = e.target as HTMLElement | null;
      if (t && (t.closest("input, textarea") || t.isContentEditable)) return;
      if (e.key === "Escape") onClose();
      else if (t?.closest("select")) return;
      else if (e.key === "ArrowLeft" && prev !== null) onStep(prev);
      else if (e.key === "ArrowRight" && next !== null) onStep(next);
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [prev, next, onStep, onClose]);

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the backdrop closes on a click, Escape does it by key
    // biome-ignore lint/a11y/useKeyWithClickEvents: see above
    <div
      className="details-backdrop"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        className={grid ? "details-modal grid" : "details-modal"}
        role="dialog"
        aria-modal="true"
        aria-label={`${name} details`}
      >
        <header className="details-header">
          <h2>{name}</h2>
          {grid && (
            <button type="button" onClick={() => onGrid(false)}>
              Back to card options
            </button>
          )}
          <button
            type="button"
            className="details-close icon ghost"
            aria-label="Close"
            title="Close (Esc)"
            onClick={onClose}
          >
            <CloseIcon />
          </button>
        </header>
        {props.refusal && (
          <p className="refusal details-refusal" role="alert">
            {props.refusal}
          </p>
        )}
        <div className="details-body">
          {grid ? (
            options.status === "done" ? (
              <PrintingsGrid
                printings={options.printings}
                current={current}
                onPick={pick}
              />
            ) : (
              <OptionsStatus options={options} />
            )
          ) : (
            <>
              <div className="details-image">
                {image ? (
                  <img src={image} alt={name} />
                ) : (
                  <div className="card-missing">{name}</div>
                )}
              </div>
              <div className="details-options">
                <Quantity qty={card.qty} onChange={setQty} />
                <PrintingPicker
                  options={options}
                  current={current}
                  focus={props.focusPrinting}
                  onPick={pick}
                  onAll={() => onGrid(true)}
                />
                <FinishToggle
                  finish={card.finish}
                  available={currentOption?.finishes}
                  onChange={(f) =>
                    onEdit((t) => setCardFinish(t, card.index, f))
                  }
                />
                <Categories
                  card={card}
                  declared={props.categories}
                  onEdit={onEdit}
                />
              </div>
            </>
          )}
        </div>
        <footer className="details-footer">
          <button
            type="button"
            disabled={prev === null}
            onClick={() => prev !== null && onStep(prev)}
            title="Previous card (←)"
          >
            <ChevronLeft />
            Previous
          </button>
          <span className="details-position">
            {position + 1} of {order.length}
          </span>
          <button
            type="button"
            disabled={next === null}
            onClick={() => next !== null && onStep(next)}
            title="Next card (→)"
          >
            Next
            <ChevronRight />
          </button>
        </footer>
      </div>
    </div>
  );
}

function OptionsStatus({
  options,
}: {
  options: ReturnType<typeof usePrintingOptions>;
}) {
  switch (options.status) {
    case "none":
      return <p className="details-note">Scryfall does not know this card.</p>;
    case "loading":
      return <p className="details-note">Asking Scryfall for printings…</p>;
    case "error":
      return <p className="details-note">{options.message}</p>;
    case "done":
      return null;
  }
}

function Quantity({
  qty,
  onChange,
}: {
  qty: number;
  onChange: (qty: number) => void;
}) {
  const [typed, setTyped] = useState<string | null>(null);
  const commit = () => {
    if (typed === null) return;
    const n = Number.parseInt(typed, 10);
    setTyped(null);
    if (Number.isInteger(n) && n >= 0 && n !== qty) onChange(n);
  };
  return (
    <fieldset className="details-field details-qty">
      <legend>Quantity</legend>
      <div className="stepper">
        <button
          type="button"
          aria-label="Decrease quantity"
          onClick={() => onChange(qty - 1)}
        >
          −
        </button>
        <input
          type="number"
          min={0}
          aria-label="Quantity"
          value={typed ?? String(qty)}
          onChange={(e) => setTyped(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => e.key === "Enter" && commit()}
        />
        <button
          type="button"
          aria-label="Increase quantity"
          onClick={() => onChange(qty + 1)}
        >
          +
        </button>
      </div>
    </fieldset>
  );
}

function PrintingPicker({
  options,
  current,
  focus,
  onPick,
  onAll,
}: {
  options: ReturnType<typeof usePrintingOptions>;
  current: string | null;
  focus: boolean;
  onPick: (p: PrintingOption) => void;
  onAll: () => void;
}) {
  const select = useRef<HTMLSelectElement>(null);
  const loaded = options.status === "done";
  useEffect(() => {
    if (focus && loaded) select.current?.focus();
  }, [focus, loaded]);
  const printings = loaded ? options.printings : [];
  const known = printings.some((p) => printingId(p) === current);
  return (
    <fieldset className={focus ? "details-field focused" : "details-field"}>
      <legend>Printing</legend>
      {loaded ? (
        <select
          ref={select}
          aria-label="Printing"
          value={current ?? ""}
          onChange={(e) => {
            const p = printings.find((p) => printingId(p) === e.target.value);
            if (p) onPick(p);
          }}
        >
          {current === null && (
            <option value="" disabled>
              By name: Scryfall's choice
            </option>
          )}
          {current !== null && !known && (
            <option value={current}>{current}</option>
          )}
          {printings.map((p) => (
            <option key={printingId(p)} value={printingId(p)}>
              {p.setName} ({p.set.toUpperCase()}) #{p.num} · {p.released}
            </option>
          ))}
        </select>
      ) : (
        <OptionsStatus options={options} />
      )}
      <button type="button" onClick={onAll} disabled={!loaded}>
        All printings
      </button>
    </fieldset>
  );
}

function FinishToggle({
  finish,
  available,
  onChange,
}: {
  finish: Finish;
  /** The finishes the printing comes in, once Scryfall has said. */
  available: readonly Finish[] | undefined;
  onChange: (finish: Finish) => void;
}) {
  return (
    <fieldset className="details-field details-finish">
      <legend>Finish</legend>
      <div className="segmented">
        {FINISHES.map((f) => (
          <button
            key={f}
            type="button"
            aria-pressed={f === finish}
            disabled={
              f !== finish && available !== undefined && !available.includes(f)
            }
            onClick={() => f !== finish && onChange(f)}
          >
            {f}
          </button>
        ))}
      </div>
    </fieldset>
  );
}

function Categories({
  card,
  declared,
  onEdit,
}: {
  card: Card;
  declared: readonly Category[];
  onEdit: (edit: (text: string) => string) => boolean;
}) {
  const [naming, setNaming] = useState(false);
  const kindOf = new Map(declared.map((c) => [c.name, c.kind]));
  const others = declared.filter((c) => !card.categories.includes(c.name));
  const add = (name: string) => {
    const known = kindOf.has(name);
    onEdit((t) =>
      setCardCategories(known ? t : declareCategory(t, name), card.index, [
        ...card.categories,
        name,
      ]),
    );
  };
  return (
    <fieldset className="details-field details-categories">
      <legend>Categories</legend>
      <ul>
        {card.categories.length === 0 && (
          <li className="details-note">None: in the deck, uncategorized</li>
        )}
        {card.categories.map((name) => (
          <li key={name}>
            <span>{name}</span>
            {kindOf.get(name) && (
              <span className="details-kind">{kindOf.get(name)}</span>
            )}
            <button
              type="button"
              aria-label={`Remove from ${name}`}
              title={`Remove from ${name}`}
              onClick={() =>
                onEdit((t) =>
                  setCardCategories(
                    t,
                    card.index,
                    card.categories.filter((c) => c !== name),
                  ),
                )
              }
            >
              <CloseIcon />
            </button>
          </li>
        ))}
      </ul>
      {naming ? (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const name = new FormData(e.currentTarget).get("name");
            if (typeof name === "string" && name.trim()) add(name.trim());
            setNaming(false);
          }}
        >
          <input
            name="name"
            className="details-new-category"
            aria-label="New category"
            placeholder="New category"
            // biome-ignore lint/a11y/noAutofocus: the user just asked to type a name
            autoFocus
            onKeyDown={(e) => e.key === "Escape" && setNaming(false)}
          />
          <button type="submit">Add</button>
        </form>
      ) : (
        <select
          aria-label="Add category"
          value=""
          onChange={(e) => {
            if (e.target.value === NEW_CATEGORY) setNaming(true);
            else if (e.target.value) add(e.target.value);
          }}
        >
          <option value="">Add category…</option>
          {others.map((c) => (
            <option key={c.name} value={c.name}>
              {c.name}
              {c.kind ? ` (${c.kind})` : ""}
            </option>
          ))}
          <option value={NEW_CATEGORY}>New category…</option>
        </select>
      )}
    </fieldset>
  );
}
