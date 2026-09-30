import { useRef, useState } from "react";
import { deckFromArchidekt, type FromArchidekt } from "../github/decks";
import { deckText } from "../github/deckText";

type Unreadable = Extract<FromArchidekt, { kind: "deck" }>["unreadable"];

/**
 * Replace from Archidekt: the pasted text becomes the whole deck, cards,
 * categories and printings alike, keeping only its name and format. It is one
 * edit, so undo brings the old list back and the save's diff says what moved.
 */
export function ReplaceArchidekt({
  name,
  format,
  onReplace,
}: {
  name: string;
  format: string | undefined;
  onReplace: (text: string) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [pasted, setPasted] = useState("");
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [left, setLeft] = useState<Unreadable | null>(null);

  const open = () => {
    setPasted("");
    setRefusal(null);
    setLeft(null);
    dialog.current?.showModal();
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setRefusal(null);
    try {
      const made = await deckFromArchidekt(pasted, name, format, deckText);
      if (made.kind === "refused") {
        setRefusal(made.message);
        return;
      }
      onReplace(made.text);
      if (made.unreadable.length > 0) setLeft(made.unreadable);
      else dialog.current?.close();
    } catch (err) {
      setRefusal(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <button
        type="button"
        onClick={open}
        title="Replace the whole deck with pasted Archidekt text"
      >
        Replace from Archidekt
      </button>
      <dialog
        ref={dialog}
        className="sheet"
        aria-label="Replace from Archidekt"
      >
        {left ? (
          <div>
            <h2>Replaced {name}</h2>
            <p>These lines could not be read and were left out:</p>
            <ul className="unreadable">
              {left.map((u) => (
                <li key={u.line}>
                  <span className="line">line {u.line}</span>{" "}
                  <code>{u.text}</code> — {u.reason}
                </li>
              ))}
            </ul>
            <button
              type="button"
              className="primary"
              onClick={() => dialog.current?.close()}
            >
              Done
            </button>
          </div>
        ) : (
          <form onSubmit={(e) => void submit(e)}>
            <h2>Replace from Archidekt</h2>
            <p className="hint">
              Every card and category becomes what the text says. The name and
              format stay, and undo brings the old list back.
            </p>
            <textarea
              name="archidekt"
              rows={12}
              value={pasted}
              required
              autoFocus
              placeholder={
                "1x Sol Ring (cmm) 410 [Ramp]\n1x Rashmi, Eternities Crafter [Commander{top}]"
              }
              onChange={(e) => setPasted(e.target.value)}
            />
            {refusal && (
              <p className="refusal" role="alert">
                {refusal}
              </p>
            )}
            <div className="dialog-buttons">
              <button type="button" onClick={() => dialog.current?.close()}>
                Cancel
              </button>
              <button type="submit" className="primary" disabled={busy}>
                {busy ? "Replacing…" : "Replace"}
              </button>
            </div>
          </form>
        )}
      </dialog>
    </>
  );
}
