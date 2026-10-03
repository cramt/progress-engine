import { useEffect, useRef, useState } from "react";
import { isTyping } from "../card/hotkeys";
import type { Place } from "../collection";
import type { Finish, NewCard } from "../deck";
import { beep } from "../probe/beep";
import { ModelPicker, useModel } from "../probe/ModelPicker";
import { printingsById } from "../probe/printings";
import {
  type Boot,
  type Found,
  frameOf,
  scan,
  useScanner,
} from "../probe/scanner";
import { createTracker, type Speed, TRACKING } from "../probe/tracker";
import type { Currency } from "../scryfall";
import { formatPrice, type PriceBook, unitPrice } from "./prices";
import type { ScannedCopy } from "./scanned";
import {
  loadScanSettings,
  type ScanSettings,
  saveScanSettings,
} from "./scanSettings";
import { UNSORTED } from "./sections";
import "../card/card.css";
import "../probe/probe.css";

/** The pause between one frame's scan ending and the next frame's being taken. */
const GAP_MS = 150;

/** A card the session counted, newest first in the log. */
interface Entry {
  id: number;
  name: string;
  /** Null while its printing is looked up, before it goes in. */
  copy: ScannedCopy | null;
  image: string | null;
  /** Why it is not in the collection, when it is not. */
  refused: string | null;
  note: string | null;
  takenBack: boolean;
}

/** What the camera sees now: each card in view, and whether it is counted. */
interface InView {
  names: { name: string; counted: boolean }[];
  took: number;
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/**
 * Scanning cards into the collection, one after another: the camera is read a
 * frame at a time, and each card that holds still under it goes in once,
 * with a beep. The same card goes in again only after it has left the frame,
 * so a card left lying there is one copy (../probe/tracker.ts), and Same
 * again adds another for a stack of one card. Each copy is logged with a
 * Take back, since the printing is the scanner's guess.
 *
 * `onAdd`, `onTakeBack` and `onMove` apply to the collection as it is when called and
 * return a refusal, or null.
 */
export function LiveScan({
  places,
  prices,
  currency,
  onAdd,
  onTakeBack,
  onMove,
  onClose,
}: {
  places: readonly Place[];
  /**
   * The collection's prices, which pick a scanned card up once it is in, so
   * the log prices each copy without a lookup of its own.
   */
  prices: PriceBook;
  currency: Currency;
  onAdd: (copy: ScannedCopy) => string | null;
  onTakeBack: (copy: ScannedCopy) => string | null;
  onMove: (copy: ScannedCopy, to: string | null) => string | null;
  onClose: () => void;
}) {
  const [model, setModel] = useModel();
  const boot = useScanner(model);
  const video = useRef<HTMLVideoElement>(null);
  const [camera, setCamera] = useState<MediaStream | null>(null);
  const [audio, setAudio] = useState<AudioContext | null>(null);
  const [running, setRunning] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [inView, setInView] = useState<InView | null>(null);
  const [log, setLog] = useState<Entry[]>([]);
  // Open until scanning starts, so on a phone the camera and the log are what
  // fills the screen while cards go in.
  const [settingsOpen, setSettingsOpen] = useState(true);

  const nextId = useRef(0);

  const [prefs, setPrefs] = useState(() =>
    loadScanSettings(places.map((p) => p.name)),
  );
  const set = (change: Partial<ScanSettings>) =>
    setPrefs((p) => {
      const next = { ...p, ...change };
      saveScanSettings(next);
      return next;
    });
  const { at, finish, keepPrinting, sound, speed } = prefs;
  // The loop reads these as they are when a card is taken, rather than
  // restarting, and forgetting what it holds, whenever one changes.
  const settings = useRef({ ...prefs, audio, onAdd });
  settings.current = { ...prefs, audio, onAdd };

  useEffect(() => {
    if (video.current) video.current.srcObject = camera;
    return () => {
      for (const t of camera?.getTracks() ?? []) t.stop();
    };
  }, [camera]);

  useEffect(() => () => void audio?.close(), [audio]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const scanner = boot.phase === "ready" ? boot.scanner : null;
  useEffect(() => {
    if (!running || !scanner || !camera) return;
    let stopped = false;
    const tracker = createTracker(TRACKING[settings.current.speed]);

    const update = (id: number, change: Partial<Entry>) =>
      setLog((l) => l.map((e) => (e.id === id ? { ...e, ...change } : e)));

    /**
     * Logs and beeps for a card the tracker took as soon as it is taken, then
     * puts it into the collection once its printing is known.
     */
    const take = async (found: Found) => {
      const s = settings.current;
      if (s.sound && s.audio) beep(s.audio);
      const name = found.card.name;
      nextId.current += 1;
      const id = nextId.current;
      setLog((l) => [
        {
          id,
          name,
          copy: null,
          image: null,
          refused: null,
          note: null,
          takenBack: false,
        },
        ...l,
      ]);
      let card: NewCard = { kind: "name", name };
      let image: string | null = null;
      let note: string | null = null;
      const scryfallId = found.card.scryfall_id;
      if (s.keepPrinting && scryfallId) {
        try {
          const p = (await printingsById([scryfallId])).get(scryfallId);
          if (p) {
            card = { kind: "printing", set: p.set, num: p.num, name: p.name };
            image = p.image;
          } else note = "Scryfall has no printing for it, so by name";
        } catch (e) {
          note = `Scryfall could not name the printing (${message(e)}), so by name`;
        }
      } else if (s.keepPrinting) note = "no printing scanned, so by name";
      // Where it goes is decided when it was taken, not when Scryfall answered.
      const copy: ScannedCopy = { card, at: s.at, finish: s.finish };
      const refused = settings.current.onAdd(copy);
      if (refused && s.sound && s.audio) beep(s.audio, 220, 250);
      update(id, { copy, image, note, refused });
    };

    void (async () => {
      while (!stopped) {
        const v = video.current;
        if (document.hidden || !v?.videoWidth) {
          await sleep(250);
          continue;
        }
        const t0 = performance.now();
        let found: Found[];
        try {
          found = await scan(scanner, frameOf(v, v.videoWidth, v.videoHeight));
        } catch (e) {
          if (stopped) return;
          setRefusal(`The scanner failed on a frame: ${message(e)}`);
          await sleep(1000);
          continue;
        }
        if (stopped) return;
        const took = performance.now() - t0;
        const names = found.map((f) => f.card.name);
        tracker.setOptions(TRACKING[settings.current.speed]);
        for (const name of tracker.frame(names)) {
          const f = found.find((f) => f.card.name === name);
          if (f) void take(f);
        }
        setInView({
          names: [...new Set(names)].map((name) => ({
            name,
            counted: tracker.holding(name),
          })),
          took,
        });
        await sleep(GAP_MS);
      }
    })();
    return () => {
      stopped = true;
      setInView(null);
    };
  }, [running, scanner, camera]);

  const start = async () => {
    setRefusal(null);
    setSettingsOpen(false);
    // Made in the click, since a page may only start sound from a gesture.
    if (!audio) setAudio(new AudioContext());
    if (!camera) {
      try {
        setCamera(
          await navigator.mediaDevices.getUserMedia({
            video: { facingMode: "environment", width: { ideal: 1920 } },
          }),
        );
      } catch (e) {
        setRefusal(`No camera: ${message(e)}`);
        return;
      }
    }
    setRunning(true);
  };

  // The last card counted, when it went in: what Same again adds another of.
  const last =
    log[0]?.copy && !log[0].refused && !log[0].takenBack ? log[0] : null;

  /**
   * One more copy of the last card, without lifting it out of frame: for a
   * stack of the same card, which the tracker counts once.
   */
  const sameAgain = () => {
    if (!last?.copy) return;
    const { copy } = last;
    const refused = onAdd(copy);
    if (audio && sound) beep(audio, refused ? 220 : 1320, refused ? 250 : 90);
    nextId.current += 1;
    const entry: Entry = {
      ...last,
      id: nextId.current,
      refused,
      note: "Same again",
      takenBack: false,
    };
    setLog((l) => [entry, ...l]);
  };
  const sameAgainRef = useRef(sameAgain);
  sameAgainRef.current = sameAgain;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== " " || e.repeat || isTyping(e.target)) return;
      e.preventDefault();
      sameAgainRef.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const takeBack = (entry: Entry) => {
    if (!entry.copy) return;
    const refused = onTakeBack(entry.copy);
    if (refused) setRefusal(`Could not take back ${entry.name}: ${refused}`);
    else {
      setRefusal(null);
      setLog((l) =>
        l.map((e) => (e.id === entry.id ? { ...e, takenBack: true } : e)),
      );
    }
  };

  /** Moves a logged copy elsewhere, say a pricey one to the trade binder. */
  const move = (entry: Entry, to: string | null) => {
    if (!entry.copy) return;
    const refused = onMove(entry.copy, to);
    if (refused) setRefusal(`Could not move ${entry.name}: ${refused}`);
    else {
      setRefusal(null);
      const copy = { ...entry.copy, at: to };
      setLog((l) => l.map((e) => (e.id === entry.id ? { ...e, copy } : e)));
    }
  };

  const counted = log.filter((e) => e.copy && !e.refused && !e.takenBack);
  const added = counted.length;
  const priceOf = (e: Entry) =>
    e.copy ? unitPrice(e.copy, prices, currency) : undefined;
  const priced = counted.flatMap((e) => priceOf(e) ?? []);
  const sessionWorth = priced.reduce((sum, p) => sum + p, 0);
  // How many of each card went in, most first: the session at a glance.
  const tally = [
    ...counted
      .reduce(
        (m, e) => m.set(e.name, (m.get(e.name) ?? 0) + 1),
        new Map<string, number>(),
      )
      .entries(),
  ].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the backdrop closes on a click, Escape does it by key
    // biome-ignore lint/a11y/useKeyWithClickEvents: Escape is handled on window
    <div
      className="details-backdrop"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        className="details-modal scan-modal"
        role="dialog"
        aria-label="Scan cards"
      >
        <div className="details-header">
          <h2>Scan cards</h2>
          {scanner && <span className="muted">Delver X {scanner.version}</span>}
          <button
            type="button"
            className="details-close"
            aria-label="Close"
            onClick={onClose}
          >
            ×
          </button>
        </div>
        <div className="scan-body">
          <div className="scan-source">
            <div className={`scan-view${running ? " live" : ""}`}>
              <video ref={video} autoPlay playsInline muted hidden={!camera} />
              {!camera && (
                <p className="muted">
                  One card at a time, whole in frame, on a plain background.
                </p>
              )}
            </div>
            <div className="scan-controls">
              {running ? (
                <button type="button" onClick={() => setRunning(false)}>
                  Pause
                </button>
              ) : (
                <button
                  type="button"
                  className="primary"
                  disabled={!scanner}
                  onClick={() => void start()}
                >
                  {camera ? "Resume" : "Start scanning"}
                </button>
              )}
              <button
                type="button"
                disabled={!last}
                title={
                  last
                    ? `One more ${last.name} (Space), for a stack of the same card`
                    : "Adds another of the last card scanned (Space)"
                }
                onClick={sameAgain}
              >
                Same again
              </button>
              <label className="scan-toggle">
                <input
                  type="checkbox"
                  checked={sound}
                  onChange={(e) => set({ sound: e.target.checked })}
                />
                Beep
              </label>
            </div>
            <ScanStatus boot={boot} running={running} inView={inView} />
            {refusal && (
              <p className="refusal" role="alert">
                {refusal}
              </p>
            )}
            <details
              className="scan-settings-fold"
              open={settingsOpen}
              onToggle={(e) => setSettingsOpen(e.currentTarget.open)}
            >
              <summary>
                Settings
                {!settingsOpen && (
                  <span className="muted">
                    {" "}
                    · into {at ?? UNSORTED}, {finish}, {speed}
                    {keepPrinting ? "" : ", by name"}, {model}
                  </span>
                )}
              </summary>
              <fieldset className="scan-settings">
                {/* A new model is a new tracker, which would count the card in view again. */}
                <ModelPicker
                  model={model}
                  onChange={setModel}
                  disabled={running}
                />
                <label className="field">
                  Into
                  <select
                    value={at ?? ""}
                    onChange={(e) => set({ at: e.target.value || null })}
                  >
                    <option value="">{UNSORTED}</option>
                    {places.map((p) => (
                      <option key={p.name} value={p.name}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="field">
                  Finish
                  <select
                    className="scan-finish"
                    value={finish}
                    onChange={(e) => set({ finish: e.target.value as Finish })}
                  >
                    <option value="nonfoil">nonfoil</option>
                    <option value="foil">foil</option>
                    <option value="etched">etched</option>
                  </select>
                </label>
                <label className="field">
                  Speed
                  <select
                    value={speed}
                    onChange={(e) => set({ speed: e.target.value as Speed })}
                  >
                    <option value="careful">
                      Careful: a card holds for two frames
                    </option>
                    <option value="fast">
                      Fast: the first frame that reads it
                    </option>
                  </select>
                </label>
                <label className="scan-toggle">
                  <input
                    type="checkbox"
                    checked={keepPrinting}
                    onChange={(e) => set({ keepPrinting: e.target.checked })}
                  />
                  Keep the printing it guesses
                </label>
                <p className="hint">
                  The card's name is reliable; which printing it is, is a guess
                  between reprints with the same art. Unticked, cards go in by
                  name.
                </p>
              </fieldset>
            </details>
          </div>
          <div className="scan-results">
            <h3>
              {added === 0
                ? "Nothing scanned yet"
                : `${added} ${added === 1 ? "copy" : "copies"} added`}
              {priced.length > 0 && (
                <span
                  className="muted"
                  title={
                    priced.length < added
                      ? `${added - priced.length} not priced: still being looked up, or Scryfall has no price for them`
                      : undefined
                  }
                >
                  {" "}
                  · {formatPrice(sessionWorth, currency)}
                  {priced.length < added ? "+" : ""}
                </span>
              )}
            </h3>
            {tally.length > 1 && (
              <ul className="scan-tally" aria-label="Copies of each card">
                {tally.map(([name, n]) => (
                  <li key={name}>
                    {name} <span className="muted">×{n}</span>
                  </li>
                ))}
              </ul>
            )}
            <ol className="scan-log">
              {log.map((e) => (
                <LogRow
                  key={e.id}
                  entry={e}
                  price={priceOf(e)}
                  currency={currency}
                  places={places}
                  onMove={(to) => move(e, to)}
                  onTakeBack={() => takeBack(e)}
                />
              ))}
            </ol>
          </div>
        </div>
      </div>
    </div>
  );
}

function ScanStatus({
  boot,
  running,
  inView,
}: {
  boot: Boot;
  running: boolean;
  inView: InView | null;
}) {
  let text: string;
  if (boot.phase === "booting")
    text = `Starting the scanner: ${boot.stage} ${boot.percent}%`;
  else if (boot.phase === "failed")
    text = `The scanner failed: ${boot.message}`;
  else if (!running) text = "Paused.";
  else if (!inView) text = "Looking…";
  else if (inView.names.length === 0)
    text = `No card in view (${Math.round(inView.took)} ms a frame).`;
  else {
    const held = inView.names.filter((n) => n.counted).map((n) => n.name);
    const reading = inView.names.filter((n) => !n.counted).map((n) => n.name);
    text = [
      held.length > 0 &&
        `${held.join(", ")} counted: take it away before the next copy.`,
      reading.length > 0 && `Reading ${reading.join(", ")}…`,
    ]
      .filter(Boolean)
      .join(" ");
  }
  return (
    <p className="scan-status" role="status">
      {text}
    </p>
  );
}

function LogRow({
  entry,
  price,
  currency,
  places,
  onMove,
  onTakeBack,
}: {
  entry: Entry;
  price: number | undefined;
  currency: Currency;
  places: readonly Place[];
  onMove: (to: string | null) => void;
  onTakeBack: () => void;
}) {
  const live = entry.copy !== null && !entry.refused && !entry.takenBack;
  const { copy } = entry;
  let detail = "Naming the printing…";
  if (copy) {
    const printing =
      copy.card.kind === "printing"
        ? `${copy.card.set.toUpperCase()} #${copy.card.num}`
        : "any printing";
    const finish = copy.finish === "nonfoil" ? "" : `, ${copy.finish}`;
    detail = `${printing}${finish}`;
    if (!live) detail += ` → ${copy.at ?? UNSORTED}`;
  }
  return (
    <li
      className={
        entry.refused || entry.takenBack ? "scan-entry gone" : "scan-entry"
      }
    >
      {entry.image ? (
        <img crossOrigin="anonymous" src={entry.image} alt="" />
      ) : (
        <div className="scan-noimage" />
      )}
      <div className="scan-entry-text">
        <strong>{entry.name}</strong>
        <span className="muted">{detail}</span>
        {entry.note && <span className="muted">{entry.note}</span>}
        {entry.refused && (
          <span className="refusal-inline">Not added: {entry.refused}</span>
        )}
        {entry.takenBack && <span className="muted">Taken back</span>}
      </div>
      {price !== undefined && (
        <span
          className="scan-price"
          title={
            copy?.card.kind === "name"
              ? "As Scryfall's usual printing"
              : undefined
          }
        >
          {formatPrice(price, currency)}
        </span>
      )}
      {live && copy && (
        <>
          <select
            className="scan-move"
            aria-label={`Where ${entry.name} goes`}
            value={copy.at ?? ""}
            onChange={(e) => onMove(e.target.value || null)}
          >
            <option value="">{UNSORTED}</option>
            {places.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name}
              </option>
            ))}
          </select>
          <button type="button" className="small" onClick={onTakeBack}>
            Take back
          </button>
        </>
      )}
    </li>
  );
}
