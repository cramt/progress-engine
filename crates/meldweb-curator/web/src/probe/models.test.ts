import { describe, expect, it } from "vitest";
import pin from "../../../../gitaxian-probe/assets/pin.json";
import { DEFAULT_MODEL, loadModel, MODELS, saveModel } from "./models";

function memory() {
  const items = new Map<string, string>();
  return {
    getItem: (k: string) => items.get(k) ?? null,
    setItem: (k: string, v: string) => void items.set(k, v),
  };
}

describe("the scanner's model", () => {
  it("is each tier the probe pins, and no other", () => {
    expect(MODELS.map((m) => m.id).sort()).toEqual(
      Object.keys(pin.tiers).sort(),
    );
  });

  it("comes back as it was picked", () => {
    const storage = memory();
    saveModel("gamma", storage);
    expect(loadModel(storage)).toBe("gamma");
  });

  it("is alpha until one is picked, or when the pick is not a model", () => {
    expect(loadModel(memory())).toBe(DEFAULT_MODEL);
    const storage = memory();
    storage.setItem("meldweb.scan-model", "omega");
    expect(loadModel(storage)).toBe("alpha");
  });
});
