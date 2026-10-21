/**
 * The few icons the app draws, as inline SVG in the current colour. Each is
 * decorative: the button holding it carries the label.
 */
import { type ReactNode, useId } from "react";

function Icon({ children }: { children: ReactNode }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

export const UndoIcon = () => (
  <Icon>
    <path d="M9 14 4 9l5-5" />
    <path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" />
  </Icon>
);

export const RedoIcon = () => (
  <Icon>
    <path d="m15 14 5-5-5-5" />
    <path d="M20 9H9.5a5.5 5.5 0 0 0 0 11H13" />
  </Icon>
);

export const SearchIcon = () => (
  <Icon>
    <circle cx="11" cy="11" r="7" />
    <path d="m20 20-3.5-3.5" />
  </Icon>
);

export const PlusIcon = () => (
  <Icon>
    <path d="M12 5v14M5 12h14" />
  </Icon>
);

export const CloseIcon = () => (
  <Icon>
    <path d="M18 6 6 18M6 6l12 12" />
  </Icon>
);

export const CopyIcon = () => (
  <Icon>
    <rect x="9" y="9" width="12" height="12" rx="2" />
    <path d="M5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1" />
  </Icon>
);

export const CheckIcon = () => (
  <Icon>
    <path d="M20 6 9 17l-5-5" />
  </Icon>
);

export const LockIcon = ({ open = false }: { open?: boolean }) => (
  <Icon>
    <rect x="4" y="11" width="16" height="10" rx="2" />
    <path d={open ? "M8 11V7a4 4 0 0 1 7.5-2" : "M8 11V7a4 4 0 0 1 8 0v4"} />
  </Icon>
);

export const ChevronLeft = () => (
  <Icon>
    <path d="m15 18-6-6 6-6" />
  </Icon>
);

export const ChevronRight = () => (
  <Icon>
    <path d="m9 18 6-6-6-6" />
  </Icon>
);

export const ScanIcon = () => (
  <Icon>
    <path d="M3 7V5a2 2 0 0 1 2-2h2M17 3h2a2 2 0 0 1 2 2v2M21 17v2a2 2 0 0 1-2 2h-2M7 21H5a2 2 0 0 1-2-2v-2" />
    <rect x="8" y="7" width="8" height="10" rx="1" />
  </Icon>
);

/**
 * The mark: the splash's three sutures at rest, two in polished steel and one
 * in flesh thread. Drawn rather than loaded, so it is there before anything
 * else is; the gradient's id is per instance, as a page can hold two marks.
 */
export function BrandMark() {
  const steel = useId();
  return (
    <svg className="brand-mark" viewBox="0 0 68 68" aria-hidden="true">
      <defs>
        <linearGradient id={steel} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#5b6274" />
          <stop offset=".5" stopColor="#ffffff" />
          <stop offset="1" stopColor="#5b6274" />
        </linearGradient>
      </defs>
      <g fill="none" strokeWidth="4" strokeLinecap="round">
        <path
          stroke={`url(#${steel})`}
          d="M34 6 C 50 14, 50 30, 34 34 C 18 38, 18 54, 34 62"
        />
        <path
          stroke="var(--flesh)"
          d="M8 34 C 16 18, 30 18, 34 34 C 38 50, 52 50, 60 34"
        />
        <path
          stroke={`url(#${steel})`}
          d="M14 14 C 26 20, 28 28, 34 34 C 40 40, 42 48, 54 54"
        />
      </g>
    </svg>
  );
}

export const FlipIcon = () => (
  <Icon>
    <path d="M3 12a9 9 0 0 1 15-6.7L21 8" />
    <path d="M21 3v5h-5" />
    <path d="M21 12a9 9 0 0 1-15 6.7L3 16" />
    <path d="M3 21v-5h5" />
  </Icon>
);

/** A clock turned back: the deck's history. */
export const HistoryIcon = () => (
  <Icon>
    <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
    <path d="M3 3v5h5" />
    <path d="M12 7v5l3 2" />
  </Icon>
);

/** A fork in a line: a variant of the deck. */
export const BranchIcon = () => (
  <Icon>
    <circle cx="6" cy="5" r="2" />
    <circle cx="6" cy="19" r="2" />
    <circle cx="18" cy="7" r="2" />
    <path d="M6 7v10" />
    <path d="M18 9c0 5-12 3-12 8" />
  </Icon>
);

/** A tag: a snapshot of the deck kept under a name. */
export const TagIcon = () => (
  <Icon>
    <path d="M3 12V4a1 1 0 0 1 1-1h8l9 9-9 9z" />
    <circle cx="8" cy="8" r="1.5" />
  </Icon>
);

/** Filled, a card's own printing; outlined, one that could be. */
export const HeartIcon = ({ filled = false }: { filled?: boolean }) => (
  <svg
    viewBox="0 0 24 24"
    fill={filled ? "currentColor" : "none"}
    stroke="currentColor"
    strokeWidth="2"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M12 20.5s-7.5-4.6-9.3-9.2C1.4 7.9 3.6 4.5 7 4.5c2 0 3.6 1.1 5 2.9 1.4-1.8 3-2.9 5-2.9 3.4 0 5.6 3.4 4.3 6.8-1.8 4.6-9.3 9.2-9.3 9.2Z" />
  </svg>
);

export const PrintIcon = () => (
  <Icon>
    <path d="M6 9V3h12v6" />
    <rect x="3" y="9" width="18" height="8" rx="2" />
    <path d="M7 14h10v7H7z" />
  </Icon>
);
