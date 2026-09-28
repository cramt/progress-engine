import { describe, expect, it } from "vitest";
import { ConflictError, createGitHubApi, GITHUB_API, GitHubError } from "./api";
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

describe("a refused commit", () => {
  /** The mock, but every PUT answered with `status` and `message`. */
  function refusing(status: number, message: string) {
    const conn = mockConnection({ files: { [PATH]: "a" } });
    const api = createGitHubApi({
      auth: conn.auth,
      fetch: (async (input: RequestInfo | URL, init?: RequestInit) =>
        init?.method === "PUT"
          ? new Response(JSON.stringify({ message }), { status })
          : conn.mock.fetch(input, init)) as typeof fetch,
    });
    const put = () =>
      api.putFile(conn.repo, PATH, { text: "b", message: "m", sha: "0" });
    return put;
  }

  it("is a conflict on 409", async () => {
    await expect(refusing(409, "is at 1 but expected 0")()).rejects.toThrow(
      ConflictError,
    );
  });

  it("is a conflict on the 422 GitHub sends for a missing sha", async () => {
    const { api, repo } = mockConnection({ files: { [PATH]: "a" } });
    await expect(
      api.putFile(repo, PATH, { text: "b", message: "m", sha: null }),
    ).rejects.toThrow(ConflictError);
  });

  it("is a conflict on a 422 naming a sha mismatch", async () => {
    await expect(refusing(422, `${PATH} does not match 0`)()).rejects.toThrow(
      ConflictError,
    );
  });

  it("is an error, not a conflict, on any other 422", async () => {
    const e = await refusing(
      422,
      "Invalid request.\n\nFor 'properties/content', nil is not a string.",
    )().catch((e: unknown) => e);
    expect(e).toBeInstanceOf(GitHubError);
    expect(e).not.toBeInstanceOf(ConflictError);
    expect((e as GitHubError).status).toBe(422);
    expect((e as Error).message).toContain("nil is not a string");
  });
});
