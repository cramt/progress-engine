import { describe, expect, it } from "vitest";
import { GITHUB_API } from "./api";
import { MAGIC_REPO } from "./repo";
import { mockConnection } from "./testkit";

describe("the GitHub client", () => {
  it("never lets the browser answer from its cache", async () => {
    // GitHub sends `Cache-Control: private, max-age=60`, so a cached GET
    // would hand back what it said a minute ago.
    const { github, mock, others } = mockConnection({
      others: { rival: { "collection.toml": "cards = []\n" } },
    });
    await github.user();
    await github.installations();
    await others.publicFile(
      { owner: "rival", name: MAGIC_REPO },
      "collection.toml",
    );
    const sent = mock.requests();
    const api = sent.filter((r) => r.url.startsWith(GITHUB_API));
    expect(api.length).toBeGreaterThanOrEqual(3);
    expect(api.map((r) => r.cache)).toEqual(api.map(() => "no-store"));
  });
});

describe("reading another player's file", () => {
  it("refuses one over 1 MB rather than reading it as empty", async () => {
    const big = `# ${"x".repeat(1024 * 1024)}\n`;
    const { others } = mockConnection({
      others: { rival: { "collection.toml": big } },
    });
    await expect(
      others.publicFile(
        { owner: "rival", name: MAGIC_REPO },
        "collection.toml",
      ),
    ).rejects.toThrow(/over 1 MB/);
  });
});
