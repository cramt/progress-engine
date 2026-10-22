import { useEffect, useMemo, useRef, useState } from "react";
import { useSettings } from "../card/preference";
import { declarePlace, parseCollection } from "../collection";
import type { CardRef } from "../deck";
import type { PublicRepos } from "../github/api";
import { COLLECTION_PATH, loadCollection } from "../github/collection";
import { commitEdit } from "../github/repoFile";
import { loadTheirCollection, parseLogin } from "../github/trade";
import type { Repo } from "../repo/api";
import { fetchPrintings, type Printings, printingKey } from "../scryfall";
import { readSettings } from "../settings/rules";
import {
  type FriendFile,
  moveSome,
  type ParsedSupply,
  readSupply,
} from "../supply";
import { messageOf } from "../ui/Sheet";
import { printingNames } from "./archidektNames";

/** A friend whose collection could not be had, and why. */
export interface FriendMissing {
  who: string;
  why: "no-repo" | "no-file" | "not-a-login" | "failed";
}

export type SupplyState =
  | { kind: "loading" }
  | { kind: "failed"; message: string }
  | {
      kind: "ready";
      supply: ParsedSupply;
      /** The collection's places, for the deck's own to be named after it. */
      places: readonly string[];
      /** The deck's printings and the collection's and friends', for pictures. */
      printings: Printings;
      friends: readonly string[];
      /** Friends still being read. */
      reading: readonly string[];
      missing: readonly FriendMissing[];
      /** Whether a move is on its way to GitHub. */
      saving: boolean;
      /** Why the last change to the collection was not made. */
      refusal: string | null;
      /** A move to take back, the newest last. */
      canUndo: boolean;
    };

export interface Supply {
  state: SupplyState;
  /** `qty` copies of each collection `line` moved to `to`, unsorted for null. */
  move: (
    moves: readonly { line: number; qty: number }[],
    to: string | null,
  ) => void;
  /** Declares a place in the collection that is this deck. */
  makePlace: (name: string) => void;
  undo: () => void;
  dismiss: () => void;
}

/** Their collections, read once a session: GitHub limits unsigned reads. */
const theirs = new Map<string, Promise<FriendFile | FriendMissing>>();

function readFriend(others: PublicRepos, who: string) {
  const key = who.toLowerCase();
  let read = theirs.get(key);
  if (!read) {
    const login = parseLogin(who);
    read = !login
      ? Promise.resolve({ who, why: "not-a-login" as const })
      : loadTheirCollection(others, login).then(
          (f) =>
            f.kind === "file" ? { who, text: f.text } : { who, why: f.kind },
          () => ({ who, why: "failed" as const }),
        );
    theirs.set(key, read);
  }
  return read;
}

/**
 * The deck at `path` against the collection and the friends in the
 * settings, read once the deck page shows a view that needs them. A move is
 * committed to `collection.toml` at once, made only on the collection as it
 * was read; when it moved on GitHub since, the page takes what is there and
 * says so rather than move a line that may now be another card.
 */
export function useSupply({
  active,
  repo,
  others,
  path,
  text,
  printings,
}: {
  active: boolean;
  repo: Repo;
  others: PublicRepos;
  path: string;
  text: string;
  printings: Printings;
}): Supply {
  const settings = useSettings();
  const friends = useMemo(() => {
    const read = readSettings(settings);
    return read.kind === "read" ? read.friends : [];
  }, [settings]);

  const [collection, setCollection] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [found, setFound] = useState<
    ReadonlyMap<string, FriendFile | FriendMissing>
  >(new Map());
  const [extra, setExtra] = useState<Printings>(new Map());
  const [saving, setSaving] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const past = useRef<string[]>([]);

  useEffect(() => {
    if (!active || collection !== null) return;
    let live = true;
    loadCollection(repo).then(
      (c) => live && setCollection(c.text),
      (e) => live && setFailed(messageOf(e)),
    );
    return () => {
      live = false;
    };
  }, [active, collection, repo]);

  useEffect(() => {
    if (!active) return;
    let live = true;
    for (const who of friends) {
      if (found.has(who)) continue;
      void readFriend(others, who).then(
        (f) => live && setFound((m) => new Map(m).set(who, f)),
      );
    }
    return () => {
      live = false;
    };
  }, [active, friends, found, others]);

  // The names of printings no file comments, and pictures for the copies.
  const files = useMemo(() => {
    const out: FriendFile[] = [];
    for (const who of friends) {
      const f = found.get(who);
      if (f && "text" in f) out.push(f);
    }
    return out;
  }, [friends, found]);
  const printed = useMemo(() => {
    const cards: { card: CardRef }[] = [];
    for (const t of [collection ?? "", ...files.map((f) => f.text)]) {
      const c = parseCollection(t);
      if (c.kind === "collection")
        cards.push(...c.cards.filter((o) => o.card.kind === "printing"));
    }
    return cards;
  }, [collection, files]);
  useEffect(() => {
    const wanted = printed.filter((c) => !extra.has(printingKey(c.card)));
    if (wanted.length === 0) return;
    let live = true;
    // An outage costs pictures and the names of uncommented printings.
    fetchPrintings(wanted).then(
      (p) => live && setExtra((m) => new Map([...m, ...p])),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [printed, extra]);

  const all = useMemo(
    () => new Map([...extra, ...printings]),
    [extra, printings],
  );
  const supply = useMemo(() => {
    if (collection === null) return null;
    return readSupply(
      path,
      text,
      collection,
      files,
      printingNames(printed, all),
    );
  }, [path, text, collection, files, printed, all]);

  // `undone` is the move to take back again if this one fails.
  const commit = (from: string, to: string, undone?: string) => {
    setSaving(true);
    setCollection(to);
    commitEdit(repo, COLLECTION_PATH, (now) =>
      (now ?? "") === from ? to : null,
    )
      .then((made) => {
        if (made.kind === "unchanged" && (made.text ?? "") !== to) {
          past.current = [];
          setCollection(made.text ?? "");
          setRefusal(
            "Your collection changed on GitHub since this page read it, so nothing moved. This is the collection as it is now.",
          );
        }
      })
      .catch((e) => {
        setCollection(from);
        if (undone === undefined) past.current.pop();
        else past.current.push(undone);
        setRefusal(messageOf(e));
      })
      .finally(() => setSaving(false));
  };
  const change = (edit: (text: string) => string) => {
    if (collection === null || saving) return;
    try {
      const next = edit(collection);
      if (next === collection) return;
      past.current.push(collection);
      setRefusal(null);
      commit(collection, next);
    } catch (e) {
      setRefusal(messageOf(e));
    }
  };

  const places = useMemo(() => {
    const c = parseCollection(collection ?? "");
    return c.kind === "collection" ? c.places.map((p) => p.name) : [];
  }, [collection]);

  const state: SupplyState = failed
    ? { kind: "failed", message: failed }
    : supply === null
      ? { kind: "loading" }
      : {
          kind: "ready",
          supply,
          places,
          printings: all,
          friends,
          reading: friends.filter((f) => !found.has(f)),
          missing: friends.flatMap((f) => {
            const got = found.get(f);
            return got && "why" in got ? [got] : [];
          }),
          saving,
          refusal,
          canUndo: past.current.length > 0,
        };

  return {
    state,
    move: (moves, to) => change((t) => moveSome(t, moves, to)),
    makePlace: (name) => change((t) => declarePlace(t, name, path)),
    undo: () => {
      const back = past.current.at(-1);
      if (collection === null || back === undefined || saving) return;
      past.current.pop();
      setRefusal(null);
      commit(collection, back, back);
    },
    dismiss: () => setRefusal(null),
  };
}
