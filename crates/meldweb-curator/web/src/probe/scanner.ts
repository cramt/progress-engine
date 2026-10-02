import probe, { type ScannerHandle } from "virtual:gitaxian-probe";
import { useEffect, useState } from "react";
import type { Model } from "./models";

/** Whether this build carries the card scanner: every site build, and `MELDWEB_PROBE=1 pnpm dev`. */
export const scannerAvailable = probe !== null;

/** A catalogue printing, as the probe resolves it from Delver's own SQLite. */
export interface ProbeCard {
  name: string;
  /** Delver's edition name, e.g. "Limited Edition Alpha": not a set code. */
  edition: string;
  number: string;
  /** The join key to Scryfall, and so to how the deck names a printing. */
  scryfall_id: string | null;
}

/** One card found in a frame: the engine's pick and its runners-up. */
export interface Found {
  card: ProbeCard;
  /** The engine's confidence, 0-100, in the card and in the printing. */
  rec_conf: number;
  set_conf: number;
  /** Best first. A same-art reprint is usually here rather than in `card`. */
  alternatives: ProbeCard[];
}

export type Progress = (stage: string, percent: number) => void;

interface Opening {
  scanner: Promise<ScannerHandle>;
  /** Everyone waiting on this boot, so a second caller sees its progress too. */
  listeners: Set<Progress>;
}

const opening = new Map<Model, Opening>();

/**
 * The page's scanner for `model`, booted on its first use. Boot costs a few
 * seconds, tens of MB of downloads and unpacking the model, then 32 workers
 * that live as long as the page whether the scanner is closed or not. So each
 * model's scanner is shared by every scan and never closed, and going back to
 * a model already booted is instant. A failed boot is forgotten, so the next
 * call tries again.
 */
export function openScanner(
  model: Model,
  progress?: Progress,
): Promise<ScannerHandle> {
  if (!probe) return Promise.reject(new Error("this build has no scanner"));
  if (!crossOriginIsolated) {
    return Promise.reject(
      new Error(
        "the page is not cross-origin isolated, so the scanner cannot start",
      ),
    );
  }
  const { init, Scanner, base, files } = probe;
  let boot = opening.get(model);
  if (!boot) {
    const listeners = new Set<Progress>();
    boot = {
      listeners,
      scanner: init()
        .then(() =>
          Scanner.open(
            base,
            model,
            (stage, percent) => {
              for (const l of listeners) l(stage, percent);
            },
            files,
          ),
        )
        .catch((e: unknown) => {
          opening.delete(model);
          throw e;
        })
        .finally(() => listeners.clear()),
    };
    opening.set(model, boot);
  }
  if (progress) boot.listeners.add(progress);
  return boot.scanner;
}

/** The cards in one RGBA frame. */
export async function scan(
  scanner: ScannerHandle,
  frame: ImageData,
): Promise<Found[]> {
  const rgba = new Uint8Array(
    frame.data.buffer,
    frame.data.byteOffset,
    frame.data.byteLength,
  );
  return JSON.parse(await scanner.scan(rgba, frame.width, frame.height));
}

/** The longest side a frame is scanned at: a phone photo is ~4000 px. */
const MAX_SIDE = 1280;

/**
 * `source` as a frame the engine can read: scaled down to at most
 * `MAX_SIDE`, and set on a dark border. The detector finds a card by its
 * edge against the background, so a tight crop - a Scryfall image, a
 * screenshot - finds nothing without one, and a camera frame loses nothing
 * by having one.
 */
export function frameOf(
  source: CanvasImageSource,
  width: number,
  height: number,
): ImageData {
  const scale = Math.min(1, MAX_SIDE / Math.max(width, height));
  const w = Math.round(width * scale);
  const h = Math.round(height * scale);
  const margin = Math.round(Math.max(w, h) * 0.15);
  const canvas = document.createElement("canvas");
  canvas.width = w + 2 * margin;
  canvas.height = h + 2 * margin;
  const g = canvas.getContext("2d", { willReadFrequently: true });
  if (!g) throw new Error("no 2d canvas");
  g.fillStyle = "#2b2b30";
  g.fillRect(0, 0, canvas.width, canvas.height);
  g.drawImage(source, margin, margin, w, h);
  return g.getImageData(0, 0, canvas.width, canvas.height);
}

export type Boot =
  | { phase: "booting"; stage: string; percent: number }
  | { phase: "ready"; scanner: ScannerHandle }
  | { phase: "failed"; message: string };

/** The page's scanner for `model`, booted when first asked for. */
export function useScanner(model: Model): Boot {
  const [boot, setBoot] = useState<Boot>({
    phase: "booting",
    stage: "load",
    percent: 0,
  });
  useEffect(() => {
    let live = true;
    setBoot({ phase: "booting", stage: "load", percent: 0 });
    openScanner(model, (stage, percent) => {
      if (live) setBoot({ phase: "booting", stage, percent });
    }).then(
      (scanner) => live && setBoot({ phase: "ready", scanner }),
      (e: unknown) =>
        live &&
        setBoot({
          phase: "failed",
          message: e instanceof Error ? e.message : String(e),
        }),
    );
    return () => {
      live = false;
    };
  }, [model]);
  return boot;
}
