import { useEffect, useMemo, useState } from "react";
import { Banners, SaveStatus, Toolbar } from "../deck/Toolbar";
import {
  type CardFile,
  cardPath,
  cardStem,
  listCards,
  newCardText,
} from "../github/cards";
import { commitEdit } from "../github/repoFile";
import { createSaveStore } from "../github/save";
import { useSave } from "../github/useSave";
import {
  type CardLayout,
  checkCard,
  formOf,
  loadMockery,
  type Parts,
  type Reading,
  withLayout,
  writeForm,
} from "../mockery";
import type { Repo } from "../repo/api";
import { Loading } from "../ui/Loading";
import { messageOf, Sheet, useBusy } from "../ui/Sheet";
import { CardForm } from "./CardForm";
import { PrintCards } from "./PrintCards";
import { useCardChecks } from "./useCardChecks";
import "./cards.css";

/**
 * Custom cards: each `cards/*.card.toml` in the repo, edited as a form or
 * as its text, beside the card Mirage Mockery draws of it, and printed on a
 * sheet.
 */
export function CardsPage({
  repo,
  cards: loaded,
  selected,
  onSelect,
}: {
  repo: Repo;
  cards: readonly CardFile[];
  selected: string | undefined;
  onSelect: (path: string | undefined) => void;
}) {
  const [cards, setCards] = useState(loaded);
  const [renderer, setRenderer] = useState<
    { kind: "loading" } | { kind: "ready" } | { kind: "failed"; why: string }
  >({ kind: "loading" });
  const [making, setMaking] = useState(false);
  useEffect(() => {
    loadMockery().then(
      () => setRenderer({ kind: "ready" }),
      (e) => setRenderer({ kind: "failed", why: messageOf(e) }),
    );
  }, []);
  const open = cards.find((c) => c.path === selected) ?? cards[0];

  return (
    <main>
      <Toolbar
        name="Custom cards"
        actions={
          <>
            <button type="button" onClick={() => setMaking(true)}>
              New card
            </button>
            {renderer.kind === "ready" && cards.length > 0 && (
              <PrintCards repo={repo} cards={cards} />
            )}
          </>
        }
      />
      {making && (
        <NewCardDialog
          onClose={() => setMaking(false)}
          onMake={async (name) => {
            const path = cardPath(name);
            const made = await commitEdit(repo, path, (t) =>
              t === null ? newCardText(name) : null,
            );
            if (made.kind === "unchanged")
              return `${path} is taken; name the card something else`;
            setCards(await listCards(repo));
            onSelect(path);
            return null;
          }}
        />
      )}
      {renderer.kind === "failed" ? (
        <p className="refusal" role="alert">
          The card renderer did not load: {renderer.why}
        </p>
      ) : renderer.kind === "loading" ? (
        <Loading />
      ) : !open ? (
        <div className="stacks-empty">
          <h2>No custom cards yet</h2>
          <p>
            Make one with New card. It is a <code>.card.toml</code> in the
            repo's <code>cards/</code>; its art goes beside it.
          </p>
        </div>
      ) : (
        <div className="custom-cards">
          <nav className="custom-card-list" aria-label="Custom cards">
            {cards.map((c) => (
              <button
                key={c.path}
                type="button"
                className={c.path === open.path ? "selected" : undefined}
                onClick={() => onSelect(c.path)}
              >
                {cardStem(c.path)}
              </button>
            ))}
          </nav>
          <CardEditor
            key={open.path}
            repo={repo}
            file={open}
            onSaved={(text) =>
              setCards((cs) =>
                cs.map((c) => (c.path === open.path ? { ...c, text } : c)),
              )
            }
          />
        </div>
      )}
    </main>
  );
}

function CardEditor({
  repo,
  file,
  onSaved,
}: {
  repo: Repo;
  file: CardFile;
  onSaved: (text: string) => void;
}) {
  const [text, setText] = useState(file.text);
  // The page's copy too, so a sheet prints the text as it is now.
  const edit = (next: string) => {
    setText(next);
    onSaved(next);
  };
  const [store] = useState(() =>
    createSaveStore({
      repo,
      path: file.path,
      text: file.text,
      sha: file.sha,
      onMerge: edit,
    }),
  );
  const save = useSave(store);
  const [mode, setMode] = useState(loadMode);
  // The part edited (a card of two has its other face as 1, a meld pair its
  // melded card as 2) and the card image shown, each following the other.
  const [face, setFace] = useState(0);
  const [image, setImage] = useState(0);
  const opened = useMemo(() => {
    const o = formOf(text, face);
    // The second face went away under the edit: back to the front.
    return o.kind === "refused" && face > 0 ? formOf(text, 0) : o;
  }, [text, face]);
  const labels = PART_LABELS[opened.kind === "form" ? opened.parts : "one"];
  const side = labels ? Math.min(face, labels.length - 1) : 0;
  const showing = mode === "form" && opened.kind === "form" ? "form" : "toml";
  useEffect(() => store.edit(text), [store, text]);
  const { checked, preview, missingArt } = useCardChecks(
    repo,
    file.path,
    text,
    image,
  );
  const images = checked.kind === "card" ? checked.images : [];
  // Held while the text is refused, so the frame keeps its shape as you type.
  const [sideways, setSideways] = useState(() => liesSideways(images));
  if (images.length > 0 && liesSideways(images) !== sideways)
    setSideways(liesSideways(images));
  // An aftermath card is held upright, the half cast from the hand readable,
  // and turned to read the half cast from the graveyard: while the form
  // edits that half, or when turned by hand.
  const [turnedBy, setTurnedBy] = useState({ side, turned: side === 1 });
  const turned = turnedBy.side === side ? turnedBy.turned : side === 1;
  const pickPart = (part: number, among = images) => {
    setFace(part);
    const shown = among.findIndex((i) => i.part === part);
    setImage(Math.max(shown, 0));
  };
  const [relayRefusal, setRelayRefusal] = useState<string | null>(null);
  // A new layout lands on the part it added or moved, so it is in view.
  const relay = (layout: CardLayout) => {
    try {
      const relaid = withLayout(text, layout);
      if (
        relaid.lost.length > 0 &&
        !window.confirm(
          `Changing the layout to ${LAYOUT_LABELS[layout]} throws away:\n\n${relaid.lost.map((l) => `• ${l}`).join("\n")}\n\nGo ahead?`,
        )
      )
        return;
      setRelayRefusal(null);
      edit(relaid.text);
      const checked = checkCard(relaid.text);
      pickPart(
        layout === "one" ? 0 : 1,
        checked.kind === "card" ? checked.images : [],
      );
    } catch (e) {
      setRelayRefusal(messageOf(e));
    }
  };

  return (
    <section className="custom-card">
      <div className="custom-card-source">
        <div className="custom-card-head">
          <code>{file.path}</code>
          <SaveStatus save={save} path={file.path} />
          <div className="segmented">
            {MODES.map((m) => (
              <button
                key={m}
                type="button"
                aria-pressed={showing === m}
                title={
                  m === "form" && opened.kind === "refused"
                    ? `The form can't open this file: ${opened.message}`
                    : undefined
                }
                disabled={m === "form" && opened.kind === "refused"}
                onClick={() => {
                  setMode(m);
                  saveMode(m);
                }}
              >
                {m === "form" ? "Form" : "TOML"}
              </button>
            ))}
          </div>
        </div>
        <Banners
          save={save}
          refusal={null}
          onReload={async () => edit(await store.reload())}
          onOverwrite={() => void store.overwrite()}
          onDismiss={() => {}}
        />
        {showing === "form" && opened.kind === "form" ? (
          <>
            <LayoutPicker layout={opened.layout} onPick={relay} />
            {relayRefusal && (
              <p className="refusal" role="alert">
                {relayRefusal}
              </p>
            )}
            {labels && (
              <FacePicker
                face={side}
                labels={labels}
                onPick={(part) => pickPart(part)}
              />
            )}
            <CardForm
              key={side}
              form={opened.form}
              onChange={(form) => edit(writeForm(text, form, side))}
            />
          </>
        ) : (
          <>
            {mode === "form" && opened.kind === "refused" && (
              <p className="hint">
                The form can't open this file until it reads: {opened.message}
              </p>
            )}
            <textarea
              aria-label="The card file"
              spellCheck={false}
              value={text}
              onChange={(e) => edit(e.target.value)}
            />
          </>
        )}
        <ul className="custom-card-checks">
          {checked.kind === "refused" && (
            <li className="refusal">{checked.message}</li>
          )}
          {missingArt.map((a) => (
            <li key={a} className="warning">
              No art at <code>{a}</code> in the repo; drawn without it.
            </li>
          ))}
          {checked.kind === "card" &&
            checked.unread.map((u) => (
              <li key={u.line} className="warning">
                Insight can't read “{u.line}”: it stopped at “{u.at}”. Only a
                warning.
              </li>
            ))}
          {checked.kind === "card" &&
            checked.lines > 0 &&
            checked.unread.length === 0 && (
              <li className="hint">
                Insight reads all {checked.lines} lines of rules text.
              </li>
            )}
        </ul>
      </div>
      <div className="custom-card-preview">
        <div className="custom-card-preview-controls">
          {images.length > 1 && (
            <FacePicker
              face={Math.min(image, images.length - 1)}
              labels={images.map((i) => i.label)}
              onPick={(i) => {
                setImage(i);
                setFace(images[i]?.part ?? 0);
              }}
            />
          )}
          {preview?.kind === "png" &&
            preview.reading.kind === "lowerTurned" && (
              <button
                type="button"
                aria-pressed={turned}
                onClick={() => setTurnedBy({ side, turned: !turned })}
              >
                {turned ? "Hold upright" : "Turn to read the bottom half"}
              </button>
            )}
        </div>
        <div
          className="custom-card-frame"
          data-sideways={sideways || undefined}
        >
          {preview?.kind === "png" ? (
            <img
              crossOrigin="anonymous"
              src={preview.url}
              alt={checked.kind === "card" ? checked.name : "The card"}
              data-turn={turnOf(preview.reading, turned) ?? undefined}
            />
          ) : preview?.kind === "failed" ? (
            <p className="refusal">{preview.why}</p>
          ) : null}
        </div>
        {preview?.kind === "png" && preview.overflows.length > 0 && (
          <ul className="custom-card-checks">
            {preview.overflows.map((o) => (
              <li key={o} className="warning">
                {o} The preview draws it anyway; printing refuses it until it
                fits.
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}

/**
 * Whether every image of a card is read on its side, so the preview's frame
 * lies landscape with no empty band under it. A card with an upright image
 * too, or an aftermath card turned by hand, keeps a frame both fit in.
 */
function liesSideways(images: readonly { reading: Reading }[]): boolean {
  return images.length > 0 && images.every((i) => i.reading.kind === "turned");
}

/**
 * Which way the preview turns a card image so it reads upright, or null to
 * leave it as it prints. An aftermath card turns only when `turned`.
 */
function turnOf(reading: Reading, turned: boolean): string | null {
  switch (reading.kind) {
    case "upright":
      return null;
    case "turned":
      return reading.turn;
    case "lowerTurned":
      return turned ? reading.turn : null;
  }
}

/**
 * What the face picker calls a card's two parts, by the table its second is
 * in; a card of one face has no picker. An Adventure, an Omen, a prepare spell and a
 * flipped half are drawn on the card's one face, so the preview stays one
 * image while the form edits either; a meld pair's melded card is half on
 * each card's back.
 */
const PART_LABELS: Record<Parts, readonly string[] | null> = {
  one: null,
  frontBack: ["Front", "Back"],
  halves: ["Left half", "Right half"],
  adventure: ["Card", "Adventure"],
  omen: ["Card", "Omen"],
  prepare: ["Card", "Prepare spell"],
  flip: ["Upright", "Flipped"],
  meld: ["Card", "Counterpart", "Melded"],
};

/**
 * Every layout the form offers. A fused, aftermath or Room card is a split
 * card its text makes so, and a battle a face its type line makes one, so
 * none of those is picked here.
 */
const LAYOUT_LABELS: Record<CardLayout, string> = {
  one: "One face",
  transform: "Transform",
  modal: "Modal (MDFC)",
  split: "Split",
  adventure: "Adventure",
  omen: "Omen",
  prepare: "Prepare",
  flip: "Flip",
  meld: "Meld",
};
const LAYOUTS = Object.keys(LAYOUT_LABELS) as CardLayout[];

/** How the card is laid out: picking another adds or drops its parts. */
function LayoutPicker({
  layout,
  onPick,
}: {
  layout: CardLayout;
  onPick: (layout: CardLayout) => void;
}) {
  return (
    <label className="field layout-picker">
      Layout
      <select
        value={layout}
        onChange={(e) => {
          const picked = LAYOUTS.find((l) => l === e.target.value);
          if (picked) onPick(picked);
        }}
      >
        {LAYOUTS.map((l) => (
          <option key={l} value={l}>
            {LAYOUT_LABELS[l]}
          </option>
        ))}
      </select>
    </label>
  );
}

/** Which face of a card of two the page shows and edits. */
function FacePicker({
  face,
  labels,
  onPick,
}: {
  face: number;
  labels: readonly string[];
  onPick: (face: number) => void;
}) {
  return (
    <div className="segmented face-picker">
      {labels.map((label, i) => (
        <button
          key={label}
          type="button"
          aria-pressed={face === i}
          onClick={() => onPick(i)}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

type Mode = "form" | "toml";
const MODES: readonly Mode[] = ["form", "toml"];
const MODE_KEY = "meldweb.cards.mode";

/** The way cards were last edited, the form until one is picked. */
function loadMode(): Mode {
  try {
    return globalThis.localStorage?.getItem(MODE_KEY) === "toml"
      ? "toml"
      : "form";
  } catch {
    return "form";
  }
}

function saveMode(mode: Mode) {
  try {
    globalThis.localStorage?.setItem(MODE_KEY, mode);
  } catch {
    // storage blocked: the mode lasts this page
  }
}

function NewCardDialog({
  onClose,
  onMake,
}: {
  onClose: () => void;
  onMake: (name: string) => Promise<string | null>;
}) {
  const [name, setName] = useState("");
  const { busy, refusal, run } = useBusy();
  return (
    <Sheet label="New card" onClose={onClose}>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          if (await run(() => onMake(name.trim()))) onClose();
        }}
      >
        <h2>New custom card</h2>
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
            The file will be <code>{cardPath(name.trim())}</code>.
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
            {busy ? "Making…" : "Make card"}
          </button>
        </div>
      </form>
    </Sheet>
  );
}
