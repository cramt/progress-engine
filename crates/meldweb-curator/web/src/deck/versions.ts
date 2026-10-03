/**
 * What the deck page shows beside the deck as it is now, and how the URL
 * says it, so a past version is a link: `?at=<commit>` is the deck as that
 * commit left it, `?vs=<path>` is another deck (a variant or its parent) to
 * compare with, and `?history` keeps the timeline open.
 */
import type { DeckChange } from "../deck";
import type { Revision } from "../github/history";

export type Viewing =
  | { kind: "now" }
  /** The deck as one of its commits left it. */
  | { kind: "revision"; commit: string }
  /** Another deck in the repo, as it is now. */
  | { kind: "deck"; path: string };

export interface DeckSearch {
  history?: true;
  at?: string;
  vs?: string;
}

const COMMIT = /^[0-9a-f]{7,40}$/;

/** The search params a deck page understands; anything else is dropped. */
export function parseSearch(raw: Record<string, unknown>): DeckSearch {
  const at = typeof raw.at === "string" && COMMIT.test(raw.at) ? raw.at : null;
  const vs = typeof raw.vs === "string" && raw.vs !== "" ? raw.vs : null;
  return {
    // The router reads `?history=1` as the number and `?history` as "".
    ...([true, 1, "", "1"].includes(raw.history as never)
      ? { history: true as const }
      : {}),
    // Both at once is no state the page has; the past version wins.
    ...(at ? { at } : vs ? { vs } : {}),
  };
}

export function viewingOf(search: DeckSearch): Viewing {
  if (search.at) return { kind: "revision", commit: search.at };
  if (search.vs) return { kind: "deck", path: search.vs };
  return { kind: "now" };
}

export function searchFor(viewing: Viewing, drawer: boolean): DeckSearch {
  return {
    ...(drawer ? { history: true as const } : {}),
    ...(viewing.kind === "revision" ? { at: viewing.commit } : {}),
    ...(viewing.kind === "deck" ? { vs: viewing.path } : {}),
  };
}

/** A commit message's first line without the `lantern: ` every one starts with. */
export function subject(message: string, stem: string): string {
  const first = message.split("\n", 1)[0] ?? "";
  const prefix = `${stem.toLowerCase()}: `;
  return first.startsWith(prefix) ? first.slice(prefix.length) : first;
}

/** The changes a long commit lists in its body, past the subject's three. */
export function body(message: string): string[] {
  const [, ...rest] = message.split("\n\n");
  return rest
    .join("\n\n")
    .split("\n")
    .filter((l) => l.trim() !== "");
}

const dayKey = (d: Date) => `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;

/** A day's heading on the timeline, in the reader's own time zone. */
export function dayLabel(date: Date, now: Date): string {
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (dayKey(date) === dayKey(now)) return "Today";
  if (dayKey(date) === dayKey(yesterday)) return "Yesterday";
  return date.toLocaleDateString(undefined, {
    weekday: "short",
    day: "numeric",
    month: "short",
    ...(date.getFullYear() === now.getFullYear() ? {} : { year: "numeric" }),
  });
}

/** Revisions, newest first, under the day each was made. */
export function byDay(
  revisions: readonly Revision[],
  now: Date,
): { day: string; revisions: Revision[] }[] {
  const days: { day: string; revisions: Revision[] }[] = [];
  for (const r of revisions) {
    const day = dayLabel(new Date(r.date), now);
    const last = days.at(-1);
    if (last?.day === day) last.revisions.push(r);
    else days.push({ day, revisions: [r] });
  }
  return days;
}

/** How far back the quick jumps go, in days. */
export const JUMPS = [
  { label: "Yesterday", days: 1 },
  { label: "A week ago", days: 7 },
  { label: "A month ago", days: 30 },
  { label: "3 months ago", days: 91 },
] as const;

export type ChangeGroup = "in" | "out" | "changed" | "deck";

/**
 * Where a change sits in the compare panel, read as what taking it does to
 * the deck as it is now: cards that come in, cards that go out, cards that
 * stay but change, and the deck's own settings.
 */
export function groupOf(change: DeckChange): ChangeGroup {
  switch (change.kind) {
    case "add":
      return "in";
    case "remove":
      return "out";
    case "qty":
    case "move":
    case "printing":
    case "finish":
      return "changed";
    case "declare":
    case "undeclare":
    case "retype":
    case "rename":
    case "format":
    case "variantOf":
    case "cover":
    case "description":
      return "deck";
  }
}

export const GROUP_TITLES: Record<ChangeGroup, string> = {
  in: "Comes in",
  out: "Goes out",
  changed: "Changes",
  deck: "The deck",
};

/** A change with its position in the diff, which is how `applyChanges` takes it. */
export interface Takeable {
  at: number;
  change: DeckChange;
}

/**
 * The changes the deck could take from what it is shown beside. From its own
 * past, all of them. From another deck, all but what makes that deck another
 * deck, its name and whose variant it is: taking those would rename this
 * deck, or make it a variant of itself.
 */
export function takeable(
  changes: readonly DeckChange[],
  viewing: Exclude<Viewing, { kind: "now" }>,
): Takeable[] {
  return changes
    .map((change, at) => ({ at, change }))
    .filter(
      ({ change }) =>
        viewing.kind === "revision" ||
        (change.kind !== "rename" && change.kind !== "variantOf"),
    );
}

/** Takeable changes by group, in the order the compare panel lists them. */
export function grouped(
  changes: readonly Takeable[],
): { group: ChangeGroup; changes: Takeable[] }[] {
  const order: ChangeGroup[] = ["in", "out", "changed", "deck"];
  return order
    .map((group) => ({
      group,
      changes: changes.filter(({ change }) => groupOf(change) === group),
    }))
    .filter((g) => g.changes.length > 0);
}
