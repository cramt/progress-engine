import { useState } from "react";
import { branchName } from "../github/branches";
import { deckBranchFor } from "../github/decks";
import { Sheet, useBusy } from "../ui/Sheet";
import { Waiting } from "../ui/Spinner";

/** Names a version of the deck, kept as a git tag (ADR-0024). */
export function SnapshotDialog({
  of,
  onClose,
  onTake,
}: {
  /** Which version, for the reader: "the deck as it is now", or a date. */
  of: string;
  onClose: () => void;
  /** Resolves to why it was refused, or `null` once taken. */
  onTake: (label: string) => Promise<string | null>;
}) {
  const [label, setLabel] = useState("");
  const { busy, refusal, run } = useBusy();
  return (
    <Sheet label="Snapshot" onClose={onClose}>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (await run(() => onTake(label))) onClose();
        }}
      >
        <h2>Snapshot</h2>
        <p className="hint">
          Keeps {of} under a name, so you can find it again however much the
          deck changes after.
        </p>
        <label className="field">
          Name
          <input
            value={label}
            required
            placeholder="FNM 14 Sep"
            // biome-ignore lint/a11y/noAutofocus: the dialog is for this one field
            autoFocus
            onChange={(e) => setLabel(e.target.value)}
          />
        </label>
        {refusal && (
          <p className="refusal" role="alert">
            {refusal}
          </p>
        )}
        <div className="dialog-buttons">
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="primary"
            disabled={busy || label.trim() === ""}
          >
            {busy ? <Waiting>Taking…</Waiting> : "Take snapshot"}
          </button>
        </div>
      </form>
    </Sheet>
  );
}

/**
 * Forks the deck (ADR-0041): a new deck whose line starts on a commit of
 * this one, so its history is this deck's up to the fork.
 */
export function ForkDialog({
  parent,
  of,
  onClose,
  onCreate,
}: {
  /** The parent deck's name. */
  parent: string;
  /** Which version it starts as, for the reader. */
  of: string;
  onClose: () => void;
  /** Resolves to why it was refused, or `null` once made and opened. */
  onCreate: (name: string) => Promise<string | null>;
}) {
  const [name, setName] = useState(`${parent} (budget)`);
  const { busy, refusal, run } = useBusy();
  return (
    <Sheet label="Fork deck" onClose={onClose}>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (await run(() => onCreate(name.trim()))) onClose();
        }}
      >
        <h2>Fork {parent}</h2>
        <p className="hint">
          A deck of its own that starts as {of}. Its history is {parent}'s up to
          here, so either can compare with the other and take its changes.
        </p>
        <label className="field">
          Name
          <input
            value={name}
            required
            // biome-ignore lint/a11y/noAutofocus: the dialog is for this one field
            autoFocus
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        {name.trim() && (
          <p className="hint field-hint">
            The deck will be <code>{deckBranchFor(name.trim())}</code>.
          </p>
        )}
        {refusal && (
          <p className="refusal" role="alert">
            {refusal}
          </p>
        )}
        <div className="dialog-buttons">
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="primary"
            disabled={busy || name.trim() === ""}
          >
            {busy ? "Forking…" : "Fork"}
          </button>
        </div>
      </form>
    </Sheet>
  );
}

/**
 * Makes a branch of the deck (ADR-0041): the same deck with a list to try
 * things on, saved by itself, which can be merged into main later.
 */
export function BranchDialog({
  stem,
  of,
  initial,
  onClose,
  onCreate,
}: {
  stem: string;
  /** Where it starts, for the reader: "main", "FNM 14 Sep", a date. */
  of: string;
  initial: string;
  onClose: () => void;
  /** Resolves to why it was refused, or `null` once made and opened. */
  onCreate: (name: string) => Promise<string | null>;
}) {
  const [name, setName] = useState(initial);
  const { busy, refusal, run } = useBusy();
  const slug = branchName(name.trim());
  return (
    <Sheet label="New branch" onClose={onClose}>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (await run(() => onCreate(name.trim()))) onClose();
        }}
      >
        <h2>New branch from {of}</h2>
        <p className="hint">
          The same deck with a list to try things on. It saves by itself, and
          main stays the deck you build until you merge the branch into it.
        </p>
        <label className="field">
          Name
          <input
            value={name}
            required
            placeholder="budget test"
            // biome-ignore lint/a11y/noAutofocus: the dialog is for this one field
            autoFocus
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        {slug && (
          <p className="hint field-hint">
            The branch will be{" "}
            <code>
              decks/{stem}/{slug}
            </code>
            .
          </p>
        )}
        {refusal && (
          <p className="refusal" role="alert">
            {refusal}
          </p>
        )}
        <div className="dialog-buttons">
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="primary"
            disabled={busy || slug === ""}
          >
            {busy ? "Branching…" : "Create branch"}
          </button>
        </div>
      </form>
    </Sheet>
  );
}
