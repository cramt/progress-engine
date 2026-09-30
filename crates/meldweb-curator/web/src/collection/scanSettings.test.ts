import { describe, expect, it } from "vitest";
import {
  DEFAULT_SCAN_SETTINGS,
  loadScanSettings,
  saveScanSettings,
} from "./scanSettings";

function memory() {
  const items = new Map<string, string>();
  return {
    getItem: (k: string) => items.get(k) ?? null,
    setItem: (k: string, v: string) => void items.set(k, v),
  };
}

describe("the scan panel's settings", () => {
  it("come back as they were saved", () => {
    const storage = memory();
    const saved = {
      at: "Trade binder",
      finish: "foil",
      keepPrinting: false,
      sound: false,
      speed: "fast",
    } as const;
    saveScanSettings(saved, storage);
    expect(loadScanSettings(["Bulk", "Trade binder"], storage)).toEqual(saved);
  });

  it("put cards in unsorted when the place is gone", () => {
    const storage = memory();
    saveScanSettings({ ...DEFAULT_SCAN_SETTINGS, at: "Old box" }, storage);
    expect(loadScanSettings(["Bulk"], storage).at).toBeNull();
  });

  it("drop only the setting that is not one", () => {
    const storage = memory();
    storage.setItem(
      "meldweb.scan-settings",
      JSON.stringify({ finish: "gold", sound: false }),
    );
    expect(loadScanSettings([], storage)).toEqual({
      ...DEFAULT_SCAN_SETTINGS,
      sound: false,
    });
  });

  it("are the defaults when storage cannot be read", () => {
    const broken = {
      getItem: () => {
        throw new Error("blocked");
      },
    };
    expect(loadScanSettings([], broken)).toEqual(DEFAULT_SCAN_SETTINGS);
  });

  it("are kept for the session when storage cannot be written", () => {
    const full = {
      setItem: () => {
        throw new Error("full");
      },
    };
    expect(() => saveScanSettings(DEFAULT_SCAN_SETTINGS, full)).not.toThrow();
  });
});
