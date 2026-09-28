import { describe, expect, it } from "vitest";
import { GITHUB_API } from "./api";
import { mockConnection } from "./testkit";

const PATH = "decks/lantern.deck.toml";

describe("the GitHub client", () => {
  it("never lets the browser answer from its cache", async () => {
    // GitHub sends `Cache-Control: private, max-age=60`, so a cached GET after
    // a save would hand back the old sha and conflict with our own commit.
    const { api, mock, repo } = mockConnection({ files: { [PATH]: "a" } });
    const file = await api.getFile(repo, PATH);
    await api.putFile(repo, PATH, {
      text: "b",
      message: "m",
      sha: file?.sha ?? null,
    });
    await api.getFile(repo, PATH);
    await api.listDir(repo, "decks");
    await api.user();
    const github = mock.requests().filter((r) => r.url.startsWith(GITHUB_API));
    expect(github.length).toBeGreaterThan(4);
    expect(github.map((r) => r.cache)).toEqual(github.map(() => "no-store"));
  });
});
