import { useRef, useState } from "react";
import {
  type CollectionExport,
  type CollectionImported,
  importCollection,
  type Place,
  readCollectionExport,
} from "../collection";
import { fetchAsked } from "../scryfall";

type Read = Extract<CollectionExport, { kind: "export" }>;

/** A destination option for a new place, which no place can be named. */
const NEW_PLACE = "\u0000new";
const UNSORTED_VALUE = "";

const plural = (n: number, one: string, many = `${one}s`) =>
  `${n.toLocaleString()} ${n === 1 ? one : many}`;

/**
 * Import: another app's collection export (ManaBox, Moxfield, Archidekt,
 * Deckbox, Dragon Shield…) or a text list, read by Rust, its printings looked
 * up on Scryfall, and put in as one edit, so undo takes it all back and the
 * save's changelog says what came. Its binders become places; rows with none
 * go where the user says.
 */
export function ImportCollection({
  text,
  places,
  onImport,
}: {
  /** The collection as it is now. */
  text: string;
  places: readonly Place[];
  /** Returns false when the edit was refused, which keeps the dialog open. */
  onImport: (text: string) => boolean;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [source, setSource] = useState("");
  // What was typed or pasted, apart from a file read, which is not shown.
  const [pasted, setPasted] = useState("");
  const [read, setRead] = useState<Read | null>(null);
  const [into, setInto] = useState(UNSORTED_VALUE);
  const [newPlace, setNewPlace] = useState("");
  const [replace, setReplace] = useState(false);
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [done, setDone] = useState<CollectionImported | null>(null);

  const open = () => {
    setSource("");
    setPasted("");
    setRead(null);
    setInto(UNSORTED_VALUE);
    setNewPlace("");
    setReplace(false);
    setRefusal(null);
    setDone(null);
    dialog.current?.showModal();
  };
  const close = () => dialog.current?.close();

  const take = (exported: string) => {
    setSource(exported);
    setRefusal(null);
    const r = readCollectionExport(exported);
    if (r.kind === "refused") {
      setRead(null);
      setRefusal(`Nothing to import: ${r.message}`);
    } else {
      setRead(r);
    }
  };

  const place = into === NEW_PLACE ? newPlace.trim() || null : into || null;
  const filled = read
    ? [...read.places, ...(read.unplaced ? [place ?? "Unsorted"] : [])]
    : [];

  const submit = async () => {
    if (!read) return;
    setBusy(true);
    setRefusal(null);
    try {
      const cards = await fetchAsked(read.asks);
      const imported = importCollection(text, source, cards, replace, place);
      if (onImport(imported.text)) setDone(imported);
    } catch (e) {
      setRefusal(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <button
        type="button"
        onClick={open}
        title="Import a collection exported from ManaBox, Moxfield, Archidekt and others"
      >
        Import
      </button>
      <dialog ref={dialog} className="sheet" aria-label="Import a collection">
        {done ? (
          <ImportDone imported={done} onClose={close} />
        ) : (
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void submit();
            }}
          >
            <h2>Import a collection</h2>
            <p className="hint">
              A CSV exported from ManaBox, Moxfield, Archidekt, Deckbox, Dragon
              Shield, TCGplayer, Helvault or MTGGoldfish, or one card a line
              like <code>4 Sol Ring (CMM) 400 *F*</code>.
            </p>
            <label className="field">
              File
              <input
                type="file"
                accept=".csv,.txt,text/csv,text/plain"
                onChange={(e) => {
                  const file = e.target.files?.[0];
                  if (file) {
                    setPasted("");
                    void file.text().then(take);
                  }
                }}
              />
            </label>
            <textarea
              aria-label="Or paste it"
              rows={6}
              value={pasted}
              placeholder="…or paste it here"
              onChange={(e) => {
                setPasted(e.target.value);
                take(e.target.value);
              }}
            />
            {read && <ImportSummary read={read} />}
            {read?.unplaced && (
              <label className="field">
                {read.places.length > 0
                  ? "Rows with no place go to"
                  : "Put the cards in"}
                <select value={into} onChange={(e) => setInto(e.target.value)}>
                  <option value={UNSORTED_VALUE}>Unsorted</option>
                  {places.map((p) => (
                    <option key={p.name} value={p.name}>
                      {p.name}
                    </option>
                  ))}
                  <option value={NEW_PLACE}>A new place…</option>
                </select>
              </label>
            )}
            {read?.unplaced && into === NEW_PLACE && (
              <label className="field">
                New place
                <input
                  value={newPlace}
                  placeholder={`${read.source} import`}
                  onChange={(e) => setNewPlace(e.target.value)}
                />
              </label>
            )}
            {read && (
              <fieldset className="choice">
                <label>
                  <input
                    type="radio"
                    name="merge"
                    checked={!replace}
                    onChange={() => setReplace(false)}
                  />
                  Add to what is there
                </label>
                <label>
                  <input
                    type="radio"
                    name="merge"
                    checked={replace}
                    onChange={() => setReplace(true)}
                  />
                  Replace what is in {filled.join(", ")}
                </label>
              </fieldset>
            )}
            {refusal && (
              <p className="refusal" role="alert">
                {refusal}
              </p>
            )}
            <div className="dialog-buttons">
              <button type="button" onClick={close}>
                Cancel
              </button>
              <button
                type="submit"
                className="primary"
                disabled={
                  !read || busy || (into === NEW_PLACE && !newPlace.trim())
                }
              >
                {busy
                  ? "Asking Scryfall…"
                  : read
                    ? `Import ${plural(read.copies, "card")}`
                    : "Import"}
              </button>
            </div>
          </form>
        )}
      </dialog>
    </>
  );
}

/** What the export holds, and what it carries that the collection does not keep. */
function ImportSummary({ read }: { read: Read }) {
  return (
    <div className="import-summary">
      <p>
        A {read.source} export: {plural(read.rows, "row")},{" "}
        {plural(read.copies, "card")}
        {read.places.length > 0 &&
          ` in ${plural(read.places.length, "place")}: ${read.places.join(", ")}`}
        .
      </p>
      {read.skipped.map((s) => (
        <p key={s.reason} className="hint">
          {plural(s.rows, "row")} left out: {s.reason}.
        </p>
      ))}
      {read.dropped.length > 0 && (
        <p className="hint">
          Not kept, since the collection has no place for it:{" "}
          {read.dropped
            .map((d) => `${d.column} (${plural(d.rows, "row")})`)
            .join(", ")}
          .
        </p>
      )}
      {read.unreadable.length > 0 && (
        <p className="hint">
          {plural(read.unreadable.length, "row")} could not be read and will be
          listed after the import.
        </p>
      )}
    </div>
  );
}

function ImportDone({
  imported,
  onClose,
}: {
  imported: CollectionImported;
  onClose: () => void;
}) {
  const { notes, unreadable } = imported;
  return (
    <div>
      <h2>Imported</h2>
      {notes.length === 0 && unreadable.length === 0 && (
        <p>Every row went in as its file named it. Undo takes it all back.</p>
      )}
      {unreadable.length > 0 && (
        <>
          <p>These rows could not be read and were left out:</p>
          <ul className="unreadable">
            {unreadable.map((u) => (
              <li key={`${u.line}:${u.text}`}>
                <span className="line">line {u.line}</span>{" "}
                <code>{u.text}</code> — {u.reason}
              </li>
            ))}
          </ul>
        </>
      )}
      {notes.length > 0 && (
        <>
          <p>
            These went in by name, since the printing their file named was not
            one Scryfall agreed with:
          </p>
          <ul className="unreadable">
            {notes.map((n) => (
              <li key={n.line}>
                <span className="line">line {n.line}</span> {n.reason}
              </li>
            ))}
          </ul>
        </>
      )}
      <div className="dialog-buttons">
        <button type="button" className="primary" onClick={onClose}>
          Done
        </button>
      </div>
    </div>
  );
}
