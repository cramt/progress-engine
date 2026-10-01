import { describe, expect, it } from "vitest";

// The site is cross-origin isolated with COEP `require-corp` (vite.config.ts),
// under which a cross-origin image loaded without CORS is blocked. Nothing
// fails until it reaches a browser, so this reads the source instead.
const sources = Object.entries(
  import.meta.glob<string>(["./**/*.tsx", "./**/*.ts", "!./**/*.test.ts"], {
    query: "?raw",
    import: "default",
    eager: true,
  }),
);

describe("every image loads in CORS mode", () => {
  it("an <img> carries crossOrigin", () => {
    const bare = sources.flatMap(([path, source]) =>
      [...source.matchAll(/<img\b(?!\s+crossOrigin="anonymous")/g)].map(
        (m) => `${path}:${source.slice(0, m.index).split("\n").length}`,
      ),
    );
    expect(bare).toEqual([]);
  });

  it("an Image() preload sets crossOrigin", () => {
    const bare = sources
      .filter(
        ([, source]) =>
          source.includes("new Image(") &&
          !source.includes('.crossOrigin = "anonymous"'),
      )
      .map(([path]) => path);
    expect(bare).toEqual([]);
  });
});
