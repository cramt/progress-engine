import { useRouter } from "@tanstack/react-router";
import { type ReactNode, use, useEffect, useState } from "react";
import type { MenuEntry } from "../card/CardView";
import {
  type Card,
  exportDeck,
  parseDeck,
  setDeckCover,
  setDeckMeta,
} from "../deck";
import { printingNames } from "../deck/archidektNames";
import { EXPORT_TARGETS } from "../deck/exportTargets";
import type { RepoRef } from "../github/api";
import { connect } from "../github/connect";
import { type DeckEntry, deleteDeck, editDeckFile } from "../github/decks";
import { deckText } from "../github/deckText";
import {
  artCrop,
  fetchPrintings,
  type Printing,
  printingKey,
} from "../scryfall";
import { messageOf, Sheet, useBusy } from "../ui/Sheet";

type Open =
  | { kind: "rename"; deck: DeckEntry }
  | { kind: "cover"; deck: DeckEntry }
  | { kind: "delete"; deck: DeckEntry };

/**
 * What a deck tile's menu does without opening the deck: each action reads
 * the file as GitHub has it now and commits once, and the list reloads after.
 */
export function useDeckActions(repo: RepoRef): {
  menuFor: (deck: DeckEntry) => MenuEntry[];
  dialogs: ReactNode;
} {
  const { api } = use(connect());
  const router = useRouter();
  const [open, setOpen] = useState<Open | null>(null);
  const [notice, setNotice] = useState<
    { kind: "done" | "refusal"; text: string } | undefined
  >();
  useEffect(() => {
    if (notice?.kind !== "done") return;
    const t = setTimeout(() => setNotice(undefined), 2500);
    return () => clearTimeout(t);
  }, [notice]);

  const reload = () => void router.invalidate();
  const close = () => setOpen(null);

  const copyAs = async (
    deck: DeckEntry,
    to: (typeof EXPORT_TARGETS)[number],
  ) => {
    const exported = (async () => {
      const { text, cards } = await readDeck(deck);
      const printings = await fetchPrintings(cards).catch(() => new Map());
      return exportDeck(text, to.target, printingNames(cards, printings));
    })();
    try {
      // The text is fetched after the click, and Safari and Firefox let the
      // clipboard be written only within it, so it is handed a promise.
      if (typeof ClipboardItem === "undefined") {
        await navigator.clipboard.writeText(await exported);
      } else {
        await navigator.clipboard.write([
          new ClipboardItem({
            "text/plain": exported.then(
              (t) => new Blob([t], { type: "text/plain" }),
            ),
          }),
        ]);
      }
      setNotice({
        kind: "done",
        text: `Copied ${deck.name} for ${to.label}`,
      });
    } catch (e) {
      setNotice({ kind: "refusal", text: messageOf(e) });
    }
  };

  const readDeck = async (deck: DeckEntry) => {
    const file = await api.getFile(repo, deck.path);
    if (!file) throw new Error(`${deck.path} is no longer in the repo`);
    const parsed = parseDeck(file.text);
    if (parsed.kind === "refused") throw new Error(parsed.message);
    return { text: file.text, cards: parsed.cards };
  };

  const edit = async (deck: DeckEntry, change: (text: string) => string) => {
    await editDeckFile(api, repo, deck.path, change, deckText);
    reload();
  };

  const github = (deck: DeckEntry) =>
    window.open(
      `https://github.com/${repo.owner}/${repo.name}/blob/HEAD/${deck.path}`,
      "_blank",
      "noopener",
    );

  const menuFor = (deck: DeckEntry): MenuEntry[] => {
    // A file the format refuses can only be looked at or thrown away.
    const readable = !deck.refused;
    return [
      ...EXPORT_TARGETS.map((to) => ({
        label: `Copy for ${to.label}`,
        disabled: !readable,
        run: () => void copyAs(deck, to),
      })),
      "separator",
      {
        label: "Rename…",
        disabled: !readable,
        run: () => setOpen({ kind: "rename", deck }),
      },
      {
        label: "Set cover…",
        disabled: !readable,
        run: () => setOpen({ kind: "cover", deck }),
      },
      ...(deck.cover
        ? [
            {
              label: "Use commanders as cover",
              run: () =>
                void edit(deck, (t) => setDeckCover(t, null)).catch((e) =>
                  setNotice({ kind: "refusal", text: messageOf(e) }),
                ),
            },
          ]
        : []),
      "separator",
      { label: "Open on GitHub", run: () => github(deck) },
      "separator",
      { label: "Delete…", run: () => setOpen({ kind: "delete", deck }) },
    ];
  };

  const dialogs = (
    <>
      {open?.kind === "rename" && (
        <RenameDialog
          deck={open.deck}
          onClose={close}
          onRename={(name) => edit(open.deck, (t) => setDeckMeta(t, name))}
        />
      )}
      {open?.kind === "cover" && (
        <CoverDialog
          deck={open.deck}
          cards={() => readDeck(open.deck).then((d) => d.cards)}
          onClose={close}
          onPick={(p) => edit(open.deck, (t) => setDeckCover(t, p))}
        />
      )}
      {open?.kind === "delete" && (
        <DeleteDialog
          deck={open.deck}
          onClose={close}
          onDelete={async () => {
            const r = await deleteDeck(api, repo, open.deck);
            if (r.kind === "refused") return r.message;
            reload();
            return null;
          }}
        />
      )}
      {notice && (
        <div
          className={`home-notice ${notice.kind}`}
          role={notice.kind === "refusal" ? "alert" : "status"}
        >
          <span>{notice.text}</span>
          {notice.kind === "refusal" && (
            <button
              type="button"
              className="ghost small"
              onClick={() => setNotice(undefined)}
            >
              Dismiss
            </button>
          )}
        </div>
      )}
    </>
  );

  return { menuFor, dialogs };
}

function RenameDialog({
  deck,
  onClose,
  onRename,
}: {
  deck: DeckEntry;
  onClose: () => void;
  onRename: (name: string) => Promise<void>;
}) {
  const [name, setName] = useState(deck.name);
  const { busy, refusal, run } = useBusy();
  return (
    <Sheet label="Rename deck" onClose={onClose}>
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          const ok = await run(async () => {
            await onRename(name.trim());
            return null;
          });
          if (ok) onClose();
        }}
      >
        <h2>Rename deck</h2>
        <label className="field">
          Name
          <input
            value={name}
            required
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <p className="hint field-hint">
          The file stays <code>{deck.path}</code>: a deck's path never follows
          its name.
        </p>
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
            disabled={busy || name.trim() === "" || name.trim() === deck.name}
          >
            {busy ? "Renaming…" : "Rename"}
          </button>
        </div>
      </form>
    </Sheet>
  );
}

interface CoverChoice {
  key: string;
  printing: Printing;
}

/**
 * Every card in the deck as its art, to pick the one that stands for it. A
 * card named by name stands as the printing Scryfall shows for it.
 */
function CoverDialog({
  deck,
  cards,
  onClose,
  onPick,
}: {
  deck: DeckEntry;
  cards: () => Promise<Card[]>;
  onClose: () => void;
  onPick: (p: { set: string; num: string }) => Promise<void>;
}) {
  const [choices, setChoices] = useState<CoverChoice[] | null>(null);
  const [filter, setFilter] = useState("");
  const { busy, refusal, run } = useBusy();
  const current = deck.cover && `${deck.cover.set}/${deck.cover.num}`;

  // biome-ignore lint/correctness/useExhaustiveDependencies: read once, as the dialog opens
  useEffect(() => {
    let live = true;
    void run(async () => {
      const list = await cards();
      const printings = await fetchPrintings(list);
      const seen = new Set<string>();
      const found: CoverChoice[] = [];
      for (const c of list) {
        const p = printings.get(printingKey(c.card));
        if (!p) continue;
        const key = `${p.set}/${p.num}`;
        if (seen.has(key)) continue;
        seen.add(key);
        found.push({ key, printing: p });
      }
      found.sort((a, b) => a.printing.name.localeCompare(b.printing.name));
      if (live) setChoices(found);
      return null;
    });
    return () => {
      live = false;
    };
  }, []);

  const shown = (choices ?? []).filter((c) =>
    c.printing.name.toLowerCase().includes(filter.trim().toLowerCase()),
  );

  return (
    <Sheet label="Set cover" onClose={onClose} wide>
      <div>
        <h2>Cover for {deck.name}</h2>
        <input
          type="search"
          placeholder="Filter cards"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        {choices === null && !refusal && <p className="hint">Loading art…</p>}
        <ul className="cover-grid">
          {shown.map((c) => (
            <li key={c.key}>
              <button
                type="button"
                className={
                  c.key === current ? "cover-choice current" : "cover-choice"
                }
                disabled={busy}
                title={c.printing.name}
                onClick={async () => {
                  const ok = await run(async () => {
                    await onPick(c.printing);
                    return null;
                  });
                  if (ok) onClose();
                }}
              >
                <img
                  crossOrigin="anonymous"
                  src={artCrop(c.printing.image)}
                  alt=""
                  loading="lazy"
                />
                <span>{c.printing.name}</span>
              </button>
            </li>
          ))}
        </ul>
        {refusal && (
          <p className="refusal" role="alert">
            {refusal}
          </p>
        )}
        <div className="dialog-buttons">
          <button type="button" onClick={onClose}>
            Cancel
          </button>
        </div>
      </div>
    </Sheet>
  );
}

function DeleteDialog({
  deck,
  onClose,
  onDelete,
}: {
  deck: DeckEntry;
  onClose: () => void;
  /** The refusal, or `null` once deleted. */
  onDelete: () => Promise<string | null>;
}) {
  const { busy, refusal, run } = useBusy();
  return (
    <Sheet label="Delete deck" onClose={onClose}>
      <div>
        <h2>Delete {deck.name}?</h2>
        <p>
          <code>{deck.path}</code> is removed in a commit of its own. Its
          history stays in the repo, so it can be brought back from there.
        </p>
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
            type="button"
            className="danger"
            disabled={busy}
            onClick={async () => {
              if (await run(onDelete)) onClose();
            }}
          >
            {busy ? "Deleting…" : "Delete"}
          </button>
        </div>
      </div>
    </Sheet>
  );
}
