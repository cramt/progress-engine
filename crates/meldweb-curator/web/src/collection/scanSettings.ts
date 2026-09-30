import type { Finish } from "../deck";
import type { Speed } from "../probe/tracker";

/** How the scan panel was last set, kept per browser for the next session. */
export interface ScanSettings {
  /** A place's name, or null for unsorted. */
  at: string | null;
  finish: Finish;
  keepPrinting: boolean;
  sound: boolean;
  speed: Speed;
}

const KEY = "meldweb.scan-settings";

export const DEFAULT_SCAN_SETTINGS: ScanSettings = {
  at: null,
  finish: "nonfoil",
  keepPrinting: true,
  sound: true,
  speed: "careful",
};

/**
 * The settings last saved, each one checked, so a stale or hand-edited entry
 * costs only that setting. A place no longer declared is unsorted.
 */
export function loadScanSettings(
  places: readonly string[],
  storage: Pick<Storage, "getItem"> | undefined = globalThis.localStorage,
): ScanSettings {
  let raw: Record<string, unknown> = {};
  try {
    const parsed: unknown = JSON.parse(storage?.getItem(KEY) ?? "{}");
    if (typeof parsed === "object" && parsed !== null)
      raw = parsed as Record<string, unknown>;
  } catch {
    // unreadable or blocked storage: the defaults
  }
  const d = DEFAULT_SCAN_SETTINGS;
  const pick = <T>(value: unknown, allowed: readonly T[], fallback: T): T =>
    allowed.includes(value as T) ? (value as T) : fallback;
  return {
    at: pick(raw.at, places, d.at),
    finish: pick(raw.finish, ["nonfoil", "foil", "etched"], d.finish),
    keepPrinting: pick(raw.keepPrinting, [true, false], d.keepPrinting),
    sound: pick(raw.sound, [true, false], d.sound),
    speed: pick(raw.speed, ["careful", "fast"], d.speed),
  };
}

export function saveScanSettings(
  settings: ScanSettings,
  storage: Pick<Storage, "setItem"> | undefined = globalThis.localStorage,
): void {
  try {
    storage?.setItem(KEY, JSON.stringify(settings));
  } catch {
    // a private window, or storage blocked: the settings last this session
  }
}
