import { useNavigate } from "@tanstack/react-router";
import { use, useRef, useState } from "react";
import type { RepoRef } from "../github/api";
import { connect } from "../github/connect";
import {
  type CreatedDeck,
  createDeck,
  deckPath,
  slugify,
} from "../github/decks";
import { deckText } from "../github/deckText";
import { PlusIcon } from "../ui/icons";

const FORMATS = [
  "commander",
  "standard",
  "pioneer",
  "modern",
  "legacy",
  "vintage",
  "pauper",
  "brawl",
  "oathbreaker",
];

type Unreadable = Extract<CreatedDeck, { kind: "created" }>["unreadable"];

/**
 * New deck (#121): a name and a format, then empty or pasted from Archidekt.
 * The file is `decks/<slug>.deck.toml`, committed at once, and then opened.
 */
export function NewDeckDialog({ repo }: { repo: RepoRef }) {
  const { api } = use(connect());
  const navigate = useNavigate();
  const dialog = useRef<HTMLDialogElement>(null);
  const [name, setName] = useState("");
  const [format, setFormat] = useState("commander");
  const [source, setSource] = useState<"empty" | "archidekt">("empty");
  const [pasted, setPasted] = useState("");
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [made, setMade] = useState<{
    path: string;
    unreadable: Unreadable;
  } | null>(null);

  const open = (path: string) =>
    void navigate({ to: "/deck/$", params: { _splat: path } });

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setRefusal(null);
    try {
      const r = await createDeck(
        api,
        repo,
        name.trim(),
        source === "empty"
          ? { kind: "empty", format }
          : { kind: "archidekt", text: pasted, format },
        deckText,
      );
      if (r.kind === "refused") setRefusal(r.message);
      else if (r.unreadable.length > 0)
        setMade({ path: r.path, unreadable: r.unreadable });
      else open(r.path);
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
        className="primary"
        onClick={() => dialog.current?.showModal()}
      >
        <PlusIcon />
        New deck
      </button>
      <dialog ref={dialog} className="sheet" aria-label="New deck">
        {made ? (
          <div>
            <h2>Created {made.path}</h2>
            <p>These lines could not be read and were left out:</p>
            <ul className="unreadable">
              {made.unreadable.map((u) => (
                <li key={u.line}>
                  <span className="line">line {u.line}</span>{" "}
                  <code>{u.text}</code> — {u.reason}
                </li>
              ))}
            </ul>
            <button
              type="button"
              className="primary"
              onClick={() => open(made.path)}
            >
              Open deck
            </button>
          </div>
        ) : (
          <form onSubmit={submit}>
            <h2>New deck</h2>
            <label className="field">
              Name
              <input
                name="name"
                value={name}
                required
                autoFocus
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            <p className="hint field-hint">
              {slugify(name) ? (
                <code>{deckPath(name)}</code>
              ) : (
                "The file is named from this."
              )}
            </p>
            <label className="field">
              Format
              <select
                name="format"
                value={format}
                onChange={(e) => setFormat(e.target.value)}
              >
                {FORMATS.map((f) => (
                  <option key={f} value={f}>
                    {f}
                  </option>
                ))}
              </select>
            </label>
            <fieldset className="choice">
              <legend className="visually-hidden">Start from</legend>
              <label>
                <input
                  type="radio"
                  name="source"
                  checked={source === "empty"}
                  onChange={() => setSource("empty")}
                />
                Empty
              </label>
              <label>
                <input
                  type="radio"
                  name="source"
                  checked={source === "archidekt"}
                  onChange={() => setSource("archidekt")}
                />
                Paste Archidekt text
              </label>
            </fieldset>
            {source === "archidekt" && (
              <textarea
                name="archidekt"
                rows={12}
                value={pasted}
                placeholder={
                  "1x Sol Ring (cmm) 410 [Ramp]\n1x Rashmi, Eternities Crafter [Commander{top}]"
                }
                onChange={(e) => setPasted(e.target.value)}
              />
            )}
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
                {busy ? "Creating…" : "Create"}
              </button>
            </div>
          </form>
        )}
      </dialog>
    </>
  );
}
