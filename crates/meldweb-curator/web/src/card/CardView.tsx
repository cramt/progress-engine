import {
  type DragEvent,
  type MouseEvent,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import type { Finish } from "../deck";
import "./card.css";

/**
 * One entry of a card's `...` menu: an action, a submenu, or a rule between
 * groups. The hotkey is only shown; the key itself is handled where the card
 * is hovered.
 */
export type MenuEntry =
  | {
      label: string;
      hotkey?: string;
      disabled?: boolean;
      run: () => void;
    }
  | { label: string; submenu: MenuEntry[] }
  | "separator";

export interface CardViewProps {
  name: string;
  /** The picture, or none while Scryfall has not answered or knows no such card. */
  image?: string | undefined;
  /** Copies in the deck; a search result has none. */
  qty?: number;
  finish?: Finish;
  /**
   * `deck` is a card in the deck, with `+`, `−` and `...` down its right edge.
   * `search` is one that is not in it yet: `+` adds it, and it can be dragged.
   */
  variant?: "deck" | "search";
  selected?: boolean;
  /** Everything the `...` menu offers, in order. */
  menu?: MenuEntry[];
  /** A click: open the details. */
  onOpen?: () => void;
  /** A Ctrl+click (Cmd on a Mac): multi-select. */
  onSelect?: () => void;
  /** The pointer came onto the card (true) or left it (false). */
  onHover?: (hovering: boolean) => void;
  onIncrease?: () => void;
  onDecrease?: () => void;
  /** Makes the card itself draggable, for a card with no draggable container. */
  onDragStart?: (e: DragEvent<HTMLDivElement>) => void;
}

/**
 * A card, as Archidekt has one card everywhere: in a stack, in search
 * results, in the details modal and in a text view it is this component with
 * this menu, and only which entries it offers changes.
 */
export function CardView({
  name,
  image,
  qty,
  finish,
  variant = "deck",
  selected = false,
  menu,
  onOpen,
  onSelect,
  onHover,
  onIncrease,
  onDecrease,
  onDragStart,
}: CardViewProps) {
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
  const openMenu = (x: number, y: number) => {
    if (menu && menu.length > 0) setMenuAt({ x, y });
  };
  const click = (e: MouseEvent) => {
    if ((e.ctrlKey || e.metaKey) && onSelect) {
      e.preventDefault();
      onSelect();
    } else onOpen?.();
  };
  const classes = [
    "card-view",
    `card-view-${variant}`,
    selected ? "selected" : "",
    menuAt ? "menu-open" : "",
    finish && finish !== "nonfoil" ? `finish-${finish}` : "",
  ].filter(Boolean);
  return (
    // biome-ignore lint/a11y/useSemanticElements: a card holds buttons, so it cannot be one
    <div
      className={classes.join(" ")}
      data-card-name={name}
      role="button"
      tabIndex={-1}
      aria-pressed={selected}
      onClick={click}
      onKeyDown={(e) => e.key === "Enter" && onOpen?.()}
      onMouseEnter={() => onHover?.(true)}
      onMouseLeave={() => onHover?.(false)}
      onContextMenu={(e) => {
        if (!menu) return;
        e.preventDefault();
        openMenu(e.clientX, e.clientY);
      }}
      draggable={onDragStart ? true : undefined}
      onDragStart={onDragStart}
    >
      {image ? (
        <img src={image} alt={name} loading="lazy" draggable={false} />
      ) : (
        <div className="card-missing">{name}</div>
      )}
      {qty !== undefined && <span className="card-qty">{qty}</span>}
      {finish && finish !== "nonfoil" && (
        <span className="card-finish">{finish}</span>
      )}
      <div className="card-edge">
        {onIncrease && (
          <EdgeButton
            label={variant === "search" ? "Add to deck" : "Increase quantity"}
            onClick={onIncrease}
          >
            +
          </EdgeButton>
        )}
        {variant === "deck" && onDecrease && (
          <EdgeButton label="Decrease quantity" onClick={onDecrease}>
            −
          </EdgeButton>
        )}
        {menu && menu.length > 0 && (
          <EdgeButton
            label="More"
            onClick={(e) => {
              const r = e.currentTarget.getBoundingClientRect();
              openMenu(r.right, r.bottom);
            }}
          >
            …
          </EdgeButton>
        )}
      </div>
      {menuAt && menu && (
        <CardMenu
          entries={menu}
          x={menuAt.x}
          y={menuAt.y}
          onClose={() => setMenuAt(null)}
        />
      )}
    </div>
  );
}

function EdgeButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: (e: MouseEvent<HTMLButtonElement>) => void;
  children: string;
}) {
  return (
    <button
      type="button"
      className="card-edge-button"
      title={label}
      aria-label={label}
      onClick={(e) => {
        e.stopPropagation();
        onClick(e);
      }}
    >
      {children}
    </button>
  );
}

/**
 * The `...` menu, drawn over the page at the pointer rather than inside the
 * card, so a stack's later cards cannot cover it. Any click outside it, Escape
 * or scrolling closes it.
 */
export function CardMenu({
  entries,
  x,
  y,
  onClose,
}: {
  entries: MenuEntry[];
  x: number;
  y: number;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState({ x, y });
  // Kept inside the window: flipped left or up when it would run off it.
  useLayoutEffect(() => {
    const r = ref.current?.getBoundingClientRect();
    if (!r) return;
    setAt({
      x: x + r.width > window.innerWidth ? Math.max(0, x - r.width) : x,
      y:
        y + r.height > window.innerHeight
          ? Math.max(0, window.innerHeight - r.height - 4)
          : y,
    });
  }, [x, y]);
  useEffect(() => {
    const away = (e: Event) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("pointerdown", away, true);
    window.addEventListener("scroll", onClose, true);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("pointerdown", away, true);
      window.removeEventListener("scroll", onClose, true);
      window.removeEventListener("keydown", key);
    };
  }, [onClose]);
  return createPortal(
    <div
      ref={ref}
      className="card-menu"
      role="menu"
      style={{ left: at.x, top: at.y }}
      // Events in a portal still bubble to the card in React's tree.
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => e.stopPropagation()}
    >
      <MenuList entries={entries} onClose={onClose} />
    </div>,
    document.body,
  );
}

function MenuList({
  entries,
  onClose,
}: {
  entries: MenuEntry[];
  onClose: () => void;
}) {
  const [open, setOpen] = useState<string | null>(null);
  return (
    <ul className="card-menu-list">
      {entries.map((entry, i) => {
        if (entry === "separator")
          return (
            <li key={`after ${labelOf(entries[i - 1])}`}>
              <hr className="card-menu-separator" />
            </li>
          );
        if ("submenu" in entry)
          return (
            <li
              key={entry.label}
              className="card-menu-parent"
              onMouseEnter={() => setOpen(entry.label)}
              onMouseLeave={() => setOpen(null)}
            >
              <button
                type="button"
                role="menuitem"
                aria-haspopup="menu"
                aria-expanded={open === entry.label}
                onClick={() => setOpen(entry.label)}
              >
                <span>{entry.label}</span>
                <span className="card-menu-key">▸</span>
              </button>
              {open === entry.label && (
                <div className="card-menu card-submenu" role="menu">
                  <MenuList entries={entry.submenu} onClose={onClose} />
                </div>
              )}
            </li>
          );
        return (
          <li key={entry.label}>
            <button
              type="button"
              role="menuitem"
              disabled={entry.disabled}
              onClick={() => {
                onClose();
                entry.run();
              }}
            >
              <span>{entry.label}</span>
              {entry.hotkey && (
                <kbd className="card-menu-key">{entry.hotkey}</kbd>
              )}
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function labelOf(entry: MenuEntry | undefined): string {
  return entry === undefined || entry === "separator" ? "" : entry.label;
}
