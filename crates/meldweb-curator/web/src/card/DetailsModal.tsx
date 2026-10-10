import { useEffect, useRef, useState } from "react";
import {
  type Card,
  type CardRef,
  type Category,
  declareCategory,
  editDeckCards,
  type Finish,
  setCardCategories,
  setCardFinish,
  setCardPrinting,
  setCardQty,
} from "../deck";
import { formatPrice, loadCurrency } from "../prices";
import {
  type CardPrices,
  type Currency,
  type Face,
  fetchPrices,
  type Printing,
  printingId,
  printingKey,
} from "../scryfall";
import { ChevronLeft, ChevronRight, CloseIcon, FlipIcon } from "../ui/icons";
import { ManaText } from "./ManaText";
import { neighbours } from "./order";
import { PrintingsGrid } from "./PrintingsGrid";
import { rankPrintings, usePreferredOrder, useSettings } from "./preference";
import {
  cardText,
  type FaceText,
  FINISHES,
  fetchAllPrintings,
  type PrintingOption,
  printsByName,
} from "./prints";
import { usePrintingOptions } from "./usePrintingOptions";

const NEW_CATEGORY = "\u0000new";

export interface DetailsModalProps {
  card: Card;
  name: string;
  /** The card's page on Scryfall, its printing's when it names one. */
  scryfall: string;
  /** Another card's name by its index, for the prev and next buttons. */
  nameAt: (index: number) => string | undefined;
  /** The card's printing as the deck view knows it, when Scryfall answered. */
  printing: Printing | undefined;
  categories: readonly Category[];
  /** Every card's index in display order: what prev and next walk. */
  order: readonly number[];
  /** The next card's printings search, asked ahead while this one is looked at. */
  upcoming: string | undefined;
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
  // A card named by name shows the printing Scryfall picked for it.
  const currentOption =
    options.status === "done"
      ? options.printings.find((p) =>
          current === null ? p.id === printing?.id : printingId(p) === current,
        )
      : undefined;
  const [currency] = useState(loadCurrency);
  const prices = useTodaysPrices(card.card);
  const faces = currentOption ? cardText(currentOption.facts) : [];
  const image = printing?.image ?? currentOption?.image;
  const front: Face | undefined = image
    ? { image, turn: printing?.turn ?? currentOption?.turn ?? "upright" }
    : undefined;
  const back = printing?.back ?? currentOption?.back;
  const { prev, next, position } = neighbours(order, card.index);
  useAhead(grid && options.status === "done" ? props.upcoming : undefined);

  /**
   * `advance` is the grid's walk: a pick there is the answer for this card,
   * so it moves on to the next, the same printing included, which is how a
   * printing already right is confirmed.
   */
  const pick = (p: PrintingOption, advance: boolean) => {
    const step = () => advance && next !== null && onStep(next);
    if (printingId(p) === current) return step();
    onRemember(printingId(p), {
      id: p.id,
      name: p.name,
      set: p.set,
      num: p.num,
      image: p.image ?? "",
      // Every printing of a card shares its colour identity.
      colorIdentity: printing?.colorIdentity ?? [],
      typeLine: printing?.typeLine ?? "",
      ...(uri ? { prints: uri } : {}),
      ...(p.turn ? { turn: p.turn } : {}),
      ...(p.back ? { back: p.back } : {}),
    });
    if (onEdit((t) => setCardPrinting(t, card.index, p.set, p.num))) step();
  };
  const setQty = (qty: number) => {
    if (qty >= 1) {
      onEdit((t) => setCardQty(t, card.index, qty));
      return;
    }
    // Then the card after it in the stacks, else the one before, at its line
    // in the next text.
    const stay = next ?? prev;
    let now: number | null = null;
    const removed = onEdit((t) => {
      const edited = editDeckCards(t, { kind: "remove" }, [
        { index: card.index, from: null },
      ]);
      const line = stay === null ? undefined : edited.lines[stay];
      now = line?.kind === "at" ? line.index : null;
      return edited.text;
    });
    if (removed) {
      if (now === null) onClose();
      else onStep(now);
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
      // Plain arrows in the grid move between pictures; Shift steps the deck.
      else if (grid && !e.shiftKey && t?.closest(".printings-grid")) return;
      else if (e.key === "ArrowLeft" && prev !== null) onStep(prev);
      else if (e.key === "ArrowRight" && next !== null) onStep(next);
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [prev, next, grid, onStep, onClose]);

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the backdrop closes on a click, Escape does it by key
    // biome-ignore lint/a11y/useKeyWithClickEvents: see above
    <div
      className="details-backdrop"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        className={
          grid
            ? "details-modal grid"
            : front?.turn === "sideways"
              ? "details-modal wide"
              : "details-modal"
        }
        role="dialog"
        aria-modal="true"
        aria-label={`${name} details`}
      >
        <header className="details-header">
          <h2>{name}</h2>
          {grid ? (
            <button type="button" onClick={() => onGrid(false)}>
              Back to card options
            </button>
          ) : (
            <a
              className="button"
              href={props.scryfall}
              target="_blank"
              rel="noopener"
            >
              Scryfall
            </a>
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
                onPick={(p) => pick(p, true)}
              />
            ) : (
              <OptionsStatus options={options} />
            )
          ) : (
            <>
              <Picture key={card.index} name={name} front={front} back={back} />
              <div className="details-options">
                {faces.length > 0 && <TextBox faces={faces} />}
                <div className="details-row">
                  <Quantity qty={card.qty} onChange={setQty} />
                  <FinishToggle
                    finish={card.finish}
                    available={currentOption?.finishes}
                    prices={prices?.[currency]}
                    currency={currency}
                    onChange={(f) =>
                      onEdit((t) => setCardFinish(t, card.index, f))
                    }
                  />
                </div>
                <PrintingPicker
                  options={options}
                  current={current}
                  focus={props.focusPrinting}
                  onPick={(p) => pick(p, false)}
                  onAll={() => onGrid(true)}
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
            title={grid ? "Previous card (Shift+←)" : "Previous card (←)"}
          >
            <ChevronLeft />
            <span className="details-step">
              {prev === null ? "Previous" : (props.nameAt(prev) ?? "Previous")}
            </span>
          </button>
          <span className="details-position">
            {position + 1} of {order.length}
            {grid && (
              <span className="details-hint">
                {next === null
                  ? " · the last card"
                  : " · a pick moves to the next card"}
              </span>
            )}
          </span>
          <button
            type="button"
            disabled={next === null}
            onClick={() => next !== null && onStep(next)}
            title={
              grid
                ? "Next card, keeping this printing (Shift+→)"
                : "Next card (→)"
            }
          >
            <span className="details-step">
              {next === null ? "Next" : (props.nameAt(next) ?? "Next")}
            </span>
            <ChevronRight />
          </button>
        </footer>
      </div>
    </div>
  );
}

/**
 * Today's price of the card shown. The deck's own lookup cached it for today
 * already, so this asks Scryfall only for a card added since.
 */
function useTodaysPrices(ref: CardRef): CardPrices | undefined {
  const key = printingKey(ref);
  const [found, setFound] = useState<{ key: string; prices?: CardPrices }>();
  // The key alone says which card: a new ref for the same card is not a new
  // question.
  // biome-ignore lint/correctness/useExhaustiveDependencies: see above
  useEffect(() => {
    let gone = false;
    fetchPrices([{ card: ref }])
      .then((book) => {
        const prices = book.get(key);
        if (!gone) setFound(prices ? { key, prices } : { key });
      })
      // A price is extra: without one the finishes show none.
      .catch(() => {});
    return () => {
      gone = true;
    };
  }, [key]);
  return found?.key === key ? found.prices : undefined;
}

/** How many of the next card's best printings to fetch the pictures of. */
const AHEAD_PICTURES = 8;

/**
 * The next card's printings, and the pictures it will show first, asked for
 * while this card is looked at, so a pick lands on a grid already drawn.
 */
function useAhead(uri: string | undefined) {
  const settings = useSettings();
  useEffect(() => {
    if (!uri) return;
    let gone = false;
    fetchAllPrintings(uri)
      .then((printings) => {
        if (gone) return;
        for (const { option } of rankPrintings(
          settings,
          printings,
        ).ranked.slice(0, AHEAD_PICTURES)) {
          if (!option.image) continue;
          const preload = new Image();
          preload.crossOrigin = "anonymous";
          preload.src = option.image;
        }
      })
      // Only ahead of time: the card itself asks again when it is shown.
      .catch(() => {});
    return () => {
      gone = true;
    };
  }, [uri, settings]);
}

/**
 * The card big, turned the way it is read, with a button to turn it over
 * when it has a back. The frame stays card-shaped either way, so turning a
 * battle over does not move the options beside it.
 */
function Picture({
  name,
  front,
  back,
}: {
  name: string;
  front: Face | undefined;
  back: Face | undefined;
}) {
  const [showBack, setShowBack] = useState(false);
  const face = showBack && back ? back : front;
  // Fetched ahead, so turning the card over shows the back at once.
  const backImage = back?.image;
  useEffect(() => {
    if (!backImage) return;
    // In CORS mode, as the image element asks, or the page blocks it.
    const preload = new Image();
    preload.crossOrigin = "anonymous";
    preload.src = backImage;
  }, [backImage]);
  return (
    <div className="details-image">
      <div
        className={
          face?.turn === "sideways" ? "details-frame sideways" : "details-frame"
        }
      >
        {face ? (
          // Keyed by picture, so a new face never shows the old one at its new turn
          // while it loads.
          <img
            crossOrigin="anonymous"
            key={face.image}
            src={face.image}
            alt={name}
            className={`turn-${face.turn}`}
          />
        ) : (
          <div className="card-missing">{name}</div>
        )}
      </div>
      {back && (
        <button type="button" onClick={() => setShowBack((b) => !b)}>
          <FlipIcon />
          {showBack ? "Show front" : "Show back"}
        </button>
      )}
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

const NONE: readonly PrintingOption[] = [];

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
  const printings = loaded ? options.printings : NONE;
  const preferred = usePreferredOrder(printings).ranked;
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
          {preferred.map(({ option: p }) => (
            <option key={printingId(p)} value={printingId(p)}>
              {p.set.toUpperCase()} #{p.num} · {p.setName} ·{" "}
              {p.released.slice(0, 4)}
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
  prices,
  currency,
  onChange,
}: {
  finish: Finish;
  /** The finishes the printing comes in, once Scryfall has said. */
  available: readonly Finish[] | undefined;
  prices: Readonly<Partial<Record<Finish, number>>> | undefined;
  currency: Currency;
  onChange: (finish: Finish) => void;
}) {
  return (
    <fieldset className="details-field details-finish">
      <legend>Finish</legend>
      <div className="segmented">
        {FINISHES.map((f) => {
          const price = prices?.[f];
          return (
            <button
              key={f}
              type="button"
              aria-pressed={f === finish}
              disabled={
                f !== finish &&
                available !== undefined &&
                !available.includes(f)
              }
              onClick={() => f !== finish && onChange(f)}
            >
              {f}
              {price !== undefined && (
                <span className="details-price">
                  {formatPrice(price, currency)}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </fieldset>
  );
}

/** The card's text as its text box reads, each face of a two-faced card. */
function TextBox({ faces }: { faces: readonly FaceText[] }) {
  return (
    <section className="details-text" aria-label="Card text">
      {faces.map((f) => (
        <div key={f.name} className="details-face">
          <div className="details-face-head">
            {faces.length > 1 && <strong>{f.name}</strong>}
            <span className="details-type">{f.type}</span>
            {f.mana && (
              <span className="details-cost">
                <ManaText text={f.mana} />
              </span>
            )}
          </div>
          {f.text.split("\n").map((para, i) => (
            // Paragraphs of one fixed text, in its order.
            // biome-ignore lint/suspicious/noArrayIndexKey: see above
            <p key={i}>
              <ManaText text={para} />
            </p>
          ))}
          {f.corner && <span className="details-corner">{f.corner}</span>}
        </div>
      ))}
    </section>
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
