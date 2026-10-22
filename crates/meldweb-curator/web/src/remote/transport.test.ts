import { describe, expect, it } from "vitest";
import {
  basicToken,
  type Credentials,
  NeedsLogin,
  RemoteError,
  relayPath,
  relayTransport,
} from "./transport";

const URL_ = "https://github.com/alex/mtg.git";

function host(answers: Response[]) {
  const seen: { url: string; init: RequestInit }[] = [];
  const fetchImpl = (async (url: string, init: RequestInit) => {
    seen.push({ url, init });
    return answers.shift() ?? new Response("", { status: 500 });
  }) as unknown as typeof fetch;
  return { seen, fetchImpl };
}

const advert = (body = "0000") =>
  new Response(body, {
    headers: { "Content-Type": "application/x-git-upload-pack-advertisement" },
  });

const login = (tokens: string[]): Credentials => ({
  authorization: async () => (tokens[0] ? basicToken(tokens[0]) : null),
  refreshed: async () => {
    tokens.shift();
    return tokens[0] ? basicToken(tokens[0]) : null;
  },
});

describe("the relay transport", () => {
  it("names the remote by the relay's path on this origin", () => {
    expect(relayPath(URL_)).toBe("/api/git/github.com/alex/mtg.git");
    expect(relayPath("https://gitlab.com/g/sub/mtg.git/")).toBe(
      "/api/git/gitlab.com/g/sub/mtg.git",
    );
    expect(() => relayPath("http://github.com/a/b")).toThrow();
    expect(() => relayPath("https://u:p@github.com/a/b")).toThrow();
  });

  it("sends the token as basic auth with git's content types", async () => {
    const { seen, fetchImpl } = host([
      advert("ads"),
      new Response("pack", {
        headers: { "Content-Type": "application/x-git-upload-pack-result" },
      }),
    ]);
    const git = relayTransport({
      credentials: login(["ghu_1"]),
      fetch: fetchImpl,
    });
    expect(
      new TextDecoder().decode(await git.advertise(URL_, "git-upload-pack")),
    ).toBe("ads");
    await git.post(URL_, "git-upload-pack", new Uint8Array([1, 2]));
    expect(seen.map((s) => s.url)).toEqual([
      "/api/git/github.com/alex/mtg.git/info/refs?service=git-upload-pack",
      "/api/git/github.com/alex/mtg.git/git-upload-pack",
    ]);
    const headers = new Headers(seen[1]?.init.headers);
    expect(headers.get("Authorization")).toBe(
      `Basic ${btoa("x-access-token:ghu_1")}`,
    );
    expect(headers.get("Content-Type")).toBe(
      "application/x-git-upload-pack-request",
    );
    // The site's own cookies stay home.
    expect(seen[1]?.init.credentials).toBe("omit");
  });

  it("refreshes once on a 401, then says a login is wanted", async () => {
    const { seen, fetchImpl } = host([
      new Response("", { status: 401 }),
      advert(),
    ]);
    const tokens = ["stale", "fresh"];
    const git = relayTransport({
      credentials: login(tokens),
      fetch: fetchImpl,
    });
    await git.advertise(URL_, "git-upload-pack");
    expect(new Headers(seen[1]?.init.headers).get("Authorization")).toBe(
      basicToken("fresh"),
    );

    const { fetchImpl: refusing } = host([
      new Response("", { status: 401 }),
      new Response("", { status: 401 }),
    ]);
    const again = relayTransport({
      credentials: login(["a", "b"]),
      fetch: refusing,
    });
    await expect(
      again.advertise(URL_, "git-upload-pack"),
    ).rejects.toBeInstanceOf(NeedsLogin);

    // GitHub hides a private repo from a request with no login.
    const { fetchImpl: hidden } = host([new Response("", { status: 404 })]);
    const anonymous = relayTransport({ credentials: login([]), fetch: hidden });
    await expect(
      anonymous.advertise(URL_, "git-upload-pack"),
    ).rejects.toBeInstanceOf(NeedsLogin);
  });

  it("refuses an answer that is not git's", async () => {
    const { fetchImpl } = host([
      new Response("<html>", { headers: { "Content-Type": "text/html" } }),
    ]);
    const git = relayTransport({ credentials: login(["t"]), fetch: fetchImpl });
    await expect(git.advertise(URL_, "git-upload-pack")).rejects.toBeInstanceOf(
      RemoteError,
    );
  });
});
