import { Link } from "@tanstack/react-router";
import { useMemo, useState } from "react";
import type { CardViewProps } from "../card/CardView";
import { type Card, compareDecks, type DeckChange } from "../deck";
import { artCrop, cardName, type Printings, printingKey } from "../scryfall";
import { BranchIcon, HistoryIcon, TagIcon } from "../ui/icons";
import { StacksView } from "./StacksView";
import type { Other, ParsedDeck } from "./useOther";
import {
  GROUP_TITLES,
  grouped,
  subject,
  takeable,
  type Viewing,
} from "./versions";

export interface Present {
  text: string;
  deck: ParsedDeck;
  printings: Printings;
}

/** A commit's time as the reader's clock has it, to the minute. */
export function when(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * Another version of the deck in place of its stacks: a past version
 * (`?at=`), or another deck to compare with (`?vs=`). It says which, lists
 * what differs as changes the deck as it is now could take, and draws that
 * version's stacks with what differs marked. Taking is an edit like any
 * other: it saves by itself and Ctrl+Z undoes it.
 */
export function VersionView({
  viewing,
  other,
  present,
  stem,
  onTake,
  onBack,
  onSnapshot,
  onVariant,
}: {
  viewing: Exclude<Viewing, { kind: "now" }>;
  other: Other;
  present: Present;
  /** The deck's file stem, which every commit message starts with. */
  stem: string;
  /** `all` is restoring the version whole, which leaves it for now. */
  onTake: (take: readonly number[], text: string, all: boolean) => void;
  onBack: () => void;
  onSnapshot: (commit: string) => void;
  onVariant: (text: string) => void;
}) {
  if (other.kind === "loading") {
    return (
      <section className="version-banner" aria-busy="true">
        <HistoryIcon />
        <p className="version-title">Reading that version…</p>
      </section>
    );
  }
  if (other.kind === "missing") {
    return (
      <section className="version-banner">
        <p className="refusal" role="alert">
          {other.message}
        </p>
        <button type="button" onClick={onBack}>
          Back to now
        </button>
      </section>
    );
  }

  const revision = other.revision;
  return (
    <>
      <section className="version-banner">
        {viewing.kind === "revision" ? <HistoryIcon /> : <BranchIcon />}
        <div className="version-heading">
          <p className="version-title">
            {viewing.kind === "revision" ? (
              <>
                {other.name} as it was on{" "}
                <strong>
                  {revision ? when(revision.date) : "that commit"}
                </strong>
              </>
            ) : (
              <>
                Comparing with <strong>{other.name}</strong>
              </>
            )}
          </p>
          {revision && (
            <p className="version-subject">{subject(revision.message, stem)}</p>
          )}
        </div>
        <div className="version-actions">
          {viewing.kind === "revision" && (
            <button
              type="button"
              onClick={() => onSnapshot(viewing.commit)}
              title="Keep this version under a name, such as the event you played it at"
            >
              <TagIcon />
              Snapshot…
            </button>
          )}
          <button
            type="button"
            onClick={() => onVariant(other.text)}
            title="A new deck that starts as this version and remembers this deck as its parent"
          >
            <BranchIcon />
            Variant from this…
          </button>
          {viewing.kind === "deck" && (
            <Link
              to="/deck/$"
              params={{ _splat: viewing.path }}
              className="button"
            >
              Open {other.name}
            </Link>
          )}
          <button type="button" className="primary" onClick={onBack}>
            Back to now
          </button>
        </div>
      </section>
      <Changes
        // Positions shift with every take, so the ticks start again.
        key={present.text}
        present={present}
        other={other}
        viewing={viewing}
        onTake={onTake}
      />
      <StacksView
        categories={other.deck.categories}
        cards={other.deck.cards}
        printings={other.printings}
        cardProps={markFor(present, other)}
      />
    </>
  );
}

/** Each of the other version's cards drawn with what taking it would change. */
function markFor(present: Present, other: Extract<Other, { kind: "loaded" }>) {
  const compared = compareDecks(present.text, other.text);
  // Only card changes mark a card, so which deck it is makes no difference.
  const marks = new Map<number, NonNullable<CardViewProps["mark"]>>();
  if (compared.kind === "diff") {
    for (const c of compared.changes) {
      if (c.kind === "add") {
        marks.set(c.after, { tone: "in", label: "Not in yours" });
      } else if ("after" in c && !marks.has(c.after)) {
        marks.set(c.after, { tone: "changed", label: "Differs" });
      }
    }
  }
  return (c: Card): CardViewProps => {
    const mark = marks.get(c.index);
    return {
      name: cardName(c, other.printings),
      image: other.printings.get(printingKey(c.card))?.image,
      qty: c.qty,
      finish: c.finish,
      ...(mark ? { mark } : {}),
    };
  };
}

function Changes({
  present,
  other,
  viewing,
  onTake,
}: {
  present: Present;
  other: Extract<Other, { kind: "loaded" }>;
  viewing: Exclude<Viewing, { kind: "now" }>;
  onTake: (take: readonly number[], text: string, all: boolean) => void;
}) {
  const compared = useMemo(
    () => compareDecks(present.text, other.text),
    [present.text, other.text],
  );
  const [ticked, setTicked] = useState<ReadonlySet<number>>(() => new Set());
  const [open, setOpen] = useState(true);

  if (compared.kind === "refused") {
    return (
      <p className="refusal" role="alert">
        {compared.message}
      </p>
    );
  }
  const changes = takeable(compared.changes, viewing);
  if (changes.length === 0) {
    return (
      <p className="version-same">
        Your deck is the same as{" "}
        {viewing.kind === "revision" ? "this version" : other.name}, card for
        card.
      </p>
    );
  }

  const toggle = (ats: readonly number[], on: boolean) =>
    setTicked((t) => {
      const next = new Set(t);
      for (const at of ats) on ? next.add(at) : next.delete(at);
      return next;
    });
  const all = changes.map((c) => c.at);
  const source = viewing.kind === "revision" ? "this version" : other.name;

  return (
    <section className="changes" aria-label="What differs">
      <header className="changes-header">
        <button
          type="button"
          className="ghost small changes-toggle"
          aria-expanded={open}
          onClick={() => setOpen((o) => !o)}
        >
          {open ? "▾" : "▸"}
        </button>
        <h2>
          {changes.length} difference{changes.length === 1 ? "" : "s"}
        </h2>
        <p className="hint">
          Tick what to take from {source} into your deck. Taking is an edit: it
          saves by itself, and Ctrl+Z undoes it.
        </p>
        <div className="changes-actions">
          <button
            type="button"
            disabled={ticked.size === 0}
            onClick={() =>
              onTake(
                [...ticked].sort((a, b) => a - b),
                other.text,
                false,
              )
            }
          >
            {ticked.size === 0 ? "Take ticked" : `Take ${ticked.size} ticked`}
          </button>
          <button
            type="button"
            className="primary"
            onClick={() => onTake(all, other.text, true)}
            title={
              viewing.kind === "revision"
                ? "Make your deck this version again, as one undoable edit"
                : `Make your deck match ${other.name}, as one undoable edit`
            }
          >
            {viewing.kind === "revision"
              ? "Restore this version"
              : "Take everything"}
          </button>
        </div>
      </header>
      {open && (
        <div className="changes-groups">
          {grouped(changes).map(({ group, changes: rows }) => {
            const ats = rows.map((r) => r.at);
            const every = ats.every((at) => ticked.has(at));
            return (
              <div key={group} className={`changes-group changes-${group}`}>
                <label className="changes-group-title">
                  <input
                    type="checkbox"
                    checked={every}
                    onChange={(e) => toggle(ats, e.target.checked)}
                  />
                  {GROUP_TITLES[group]}
                  <span className="badge">{rows.length}</span>
                </label>
                <ul>
                  {rows.map(({ at, change }) => (
                    <li key={at}>
                      <label className="change">
                        <input
                          type="checkbox"
                          checked={ticked.has(at)}
                          onChange={(e) => toggle([at], e.target.checked)}
                        />
                        <Thumb
                          change={change}
                          present={present}
                          other={other}
                        />
                        <span className="change-text">{change.text}</span>
                      </label>
                    </li>
                  ))}
                </ul>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

/** The art of the card a change is about, from whichever deck has it. */
function Thumb({
  change,
  present,
  other,
}: {
  change: DeckChange;
  present: Present;
  other: Extract<Other, { kind: "loaded" }>;
}) {
  const card =
    "after" in change
      ? other.deck.cards[change.after]
      : "before" in change
        ? present.deck.cards[change.before]
        : undefined;
  if (!card) return <span className="change-thumb" aria-hidden="true" />;
  const printing =
    other.printings.get(printingKey(card.card)) ??
    present.printings.get(printingKey(card.card));
  return printing ? (
    <img
      crossOrigin="anonymous"
      className="change-thumb"
      src={artCrop(printing.image)}
      alt=""
      loading="lazy"
    />
  ) : (
    <span className="change-thumb" aria-hidden="true" />
  );
}
