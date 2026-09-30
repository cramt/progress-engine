import { useEffect, useRef, useState } from "react";
import { printingsById, type ScannedPrinting } from "./printings";
import { type Found, frameOf, scan, useScanner } from "./scanner";
import "./probe.css";

/** One card found, with Scryfall's printing for each of its candidates. */
interface Result {
  found: Found;
  printings: ReadonlyMap<string, ScannedPrinting>;
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * Scan a physical card into the deck: a camera frame or an image file through
 * Gitaxian Probe, then each printing the engine considered, with an Add for
 * each. The engine reads no collector number, so its pick is a guess between
 * same-art reprints and the runners-up are shown beside it rather than hidden.
 */
export function ScanDialog({
  onAdd,
  onClose,
}: {
  onAdd: (printing: ScannedPrinting) => void;
  onClose: () => void;
}) {
  const boot = useScanner();
  const [camera, setCamera] = useState<MediaStream | null>(null);
  const video = useRef<HTMLVideoElement>(null);
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<Result[] | null>(null);
  const [took, setTook] = useState<number | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [added, setAdded] = useState<string[]>([]);

  useEffect(() => {
    if (video.current) video.current.srcObject = camera;
    return () => {
      for (const t of camera?.getTracks() ?? []) t.stop();
    };
  }, [camera]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const startCamera = async () => {
    try {
      setCamera(
        await navigator.mediaDevices.getUserMedia({
          video: { facingMode: "environment", width: { ideal: 1920 } },
        }),
      );
    } catch (e) {
      setRefusal(`No camera: ${message(e)}`);
    }
  };

  const recognise = async (frame: () => ImageData | Promise<ImageData>) => {
    if (boot.phase !== "ready") return;
    setBusy(true);
    setRefusal(null);
    // A frame's results are never shown beside another frame's Add buttons.
    setResults(null);
    try {
      const t0 = performance.now();
      const found = await scan(boot.scanner, await frame());
      setTook(performance.now() - t0);
      const ids = found.flatMap((f) =>
        [f.card, ...f.alternatives].flatMap((c) =>
          c.scryfall_id ? [c.scryfall_id] : [],
        ),
      );
      // Without Scryfall the scan still says what the card is; it just
      // cannot name a printing the deck can hold.
      const printings = await (ids.length
        ? printingsById(ids)
        : Promise.resolve(new Map())
      ).catch((e: unknown) => {
        setRefusal(`Scryfall could not name the printings: ${message(e)}`);
        return new Map<string, ScannedPrinting>();
      });
      setResults(found.map((f) => ({ found: f, printings })));
    } catch (e) {
      setRefusal(message(e));
    } finally {
      setBusy(false);
    }
  };

  const capture = () => {
    const v = video.current;
    if (!v?.videoWidth) return;
    void recognise(() => frameOf(v, v.videoWidth, v.videoHeight));
  };

  const fromFile = (file: File) =>
    void recognise(async () => {
      const bitmap = await createImageBitmap(file);
      try {
        return frameOf(bitmap, bitmap.width, bitmap.height);
      } finally {
        bitmap.close();
      }
    });

  const add = (p: ScannedPrinting) => {
    onAdd(p);
    setAdded((a) => [...a, `${p.name} (${p.set.toUpperCase()} ${p.num})`]);
  };

  const ready = boot.phase === "ready" && !busy;
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the backdrop closes on a click, Escape does it by key
    // biome-ignore lint/a11y/useKeyWithClickEvents: Escape is handled on window
    <div
      className="details-backdrop"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="details-modal scan-modal" role="dialog" aria-label="Scan">
        <div className="details-header">
          <h2>Scan a card</h2>
          {boot.phase === "ready" && (
            <span className="muted">Delver X {boot.scanner.version}</span>
          )}
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
            {camera && <video ref={video} autoPlay playsInline muted />}
            <div className="scan-controls">
              {camera ? (
                <button type="button" onClick={capture} disabled={!ready}>
                  Capture
                </button>
              ) : (
                <button type="button" onClick={() => void startCamera()}>
                  Use camera
                </button>
              )}
              <label className="scan-file">
                Image…
                <input
                  type="file"
                  accept="image/*"
                  disabled={!ready}
                  onChange={(e) => {
                    const file = e.target.files?.[0];
                    if (file) fromFile(file);
                    e.target.value = "";
                  }}
                />
              </label>
            </div>
            <p className="scan-status" role="status">
              {boot.phase === "booting" &&
                `Starting the scanner: ${boot.stage} ${boot.percent}%`}
              {boot.phase === "failed" && `The scanner failed: ${boot.message}`}
              {boot.phase === "ready" &&
                (busy
                  ? "Scanning…"
                  : took === null
                    ? "Whole card in frame, on a plain background."
                    : `Scanned in ${Math.round(took)} ms.`)}
            </p>
            {refusal && (
              <p className="refusal" role="alert">
                {refusal}
              </p>
            )}
            {added.length > 0 && (
              <p className="scan-added">Added {added.join(", ")}.</p>
            )}
          </div>
          <div className="scan-results">
            {results?.length === 0 && <p>No card found in that frame.</p>}
            {results?.map((r, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: one frame's cards, in the engine's order
              <FoundCard key={i} result={r} onAdd={add} />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

function FoundCard({
  result: { found, printings },
  onAdd,
}: {
  result: Result;
  onAdd: (p: ScannedPrinting) => void;
}) {
  // The runners-up are the embedding index's nearest artworks. Those with the
  // pick's name are its other printings; the rest are different cards that
  // look alike, worth a button only in case the pick's name is wrong.
  const seen = new Set<string>();
  const candidates = [found.card, ...found.alternatives].filter((c) => {
    const key = c.scryfall_id ?? `${c.name}|${c.edition}|${c.number}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
  const same = candidates.filter((c) => c.name === found.card.name);
  const others = new Map<string, ScannedPrinting>();
  for (const c of candidates) {
    const p = c.scryfall_id ? printings.get(c.scryfall_id) : undefined;
    if (p && c.name !== found.card.name && !others.has(c.name))
      others.set(c.name, p);
  }
  return (
    <section className="scan-found">
      <h3>{found.card.name}</h3>
      <p className="muted">
        Card {found.rec_conf}%, printing {found.set_conf}%.
        {same.length > 1 &&
          " The pick first, then its other printings the engine ranked."}
      </p>
      <ul className="scan-candidates">
        {same.map((c, i) => {
          const p = c.scryfall_id ? printings.get(c.scryfall_id) : undefined;
          return (
            <li key={c.scryfall_id ?? i} className={i === 0 ? "pick" : ""}>
              {p?.image ? (
                <img src={p.image} alt={c.name} />
              ) : (
                <div className="scan-noimage">{c.name}</div>
              )}
              <span>
                {c.edition} #{c.number}
              </span>
              <button
                type="button"
                disabled={!p}
                title={
                  p
                    ? `${p.set.toUpperCase()} ${p.num}`
                    : "Scryfall named no printing for it"
                }
                onClick={() => p && onAdd(p)}
              >
                Add
              </button>
            </li>
          );
        })}
      </ul>
      {others.size > 0 && (
        <p className="scan-others">
          Not {found.card.name}? It also looked like{" "}
          {[...others.values()].map((p) => (
            <button
              key={p.name}
              type="button"
              title={`Add ${p.name} (${p.set.toUpperCase()} ${p.num})`}
              onClick={() => onAdd(p)}
            >
              {p.name}
            </button>
          ))}
        </p>
      )}
    </section>
  );
}
