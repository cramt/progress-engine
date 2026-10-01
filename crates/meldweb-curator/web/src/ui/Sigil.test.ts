import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { sigilFor } from "./Sigil";

// The sigil page is the signed-off look, so every tile it draws is one the app
// must draw the same.
const page = readFileSync(
  new URL("../../../../../assets/index.html", import.meta.url),
  "utf8",
);
const tiles = [
  ...page.matchAll(
    /style="background:([^"]+)"><img src="\.\/([^"]+)\.svg".*?<span>(\w*)<\/span>/g,
  ),
].map(([, background, file, order]) => ({ background, file, order }));

describe("sigilFor", () => {
  it("covers every tile on the sigil page", () => {
    expect(tiles).toHaveLength(32);
  });

  it.each(tiles)(
    "draws $file as the page does",
    ({ background, file, order }) => {
      const sigil = sigilFor(new Set(order));
      expect(sigil?.file).toBe(file);
      expect(sigil?.src).toBeTruthy();
      expect(sigil?.background.replaceAll(", ", ",")).toBe(background);
    },
  );
});
