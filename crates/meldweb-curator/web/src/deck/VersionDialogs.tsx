import { useState } from "react";
import { deckPath } from "../github/decks";
import { Sheet, useBusy } from "../ui/Sheet";

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
            {busy ? "Taking…" : "Take snapshot"}
          </button>
        </div>
      </form>
    </Sheet>
  );
}

/** A new deck that starts as a version of this one and names it as its parent. */
export function VariantDialog({
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
    <Sheet label="New variant" onClose={onClose}>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (await run(() => onCreate(name.trim()))) onClose();
        }}
      >
        <h2>New variant of {parent}</h2>
        <p className="hint">
          A deck of its own that starts as {of}. It remembers {parent} as its
          parent, so either can compare with the other and take its changes.
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
            The file will be <code>{deckPath(name.trim())}</code>.
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
            {busy ? "Making…" : "Make variant"}
          </button>
        </div>
      </form>
    </Sheet>
  );
}
