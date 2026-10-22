import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { GIT_HOSTS, handleGit } from "./git";
import worker, { type Env } from "./index";

const SITE = "https://curator.example";
const REPO = "/api/git/github.com/cramt/mtg.git";

const env: Env = {
  GITHUB_CLIENT_ID: "Iv1.test",
  GITHUB_CLIENT_SECRET: "secret",
  GITHUB_APP_SLUG: "test",
  ASSETS: { fetch: async () => new Response("asset") },
};
const ctx = { waitUntil: () => {} };

interface Seen {
  url: string;
  method: string;
  headers: Headers;
  body: string | null;
  redirect: RequestRedirect | undefined;
}
let seen: Seen[];
let answer: () => Response;

beforeEach(() => {
  seen = [];
  answer = () =>
    new Response("0000", {
      headers: {
        "Content-Type": "application/x-git-upload-pack-advertisement",
        "Set-Cookie": "host=1",
      },
    });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const body = init?.body
        ? await new Response(init.body as BodyInit).text()
        : null;
      seen.push({
        url: String(input),
        method: init?.method ?? "GET",
        headers: new Headers(init?.headers),
        body,
        redirect: init?.redirect,
      });
      return answer();
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

const ask = (path: string, init?: RequestInit) =>
  worker.fetch(new Request(`${SITE}${path}`, init), env, ctx);

it("relays a ref advertisement with the login and git's headers, and nothing else", async () => {
  const r = await ask(`${REPO}/info/refs?service=git-upload-pack`, {
    headers: {
      Authorization: "Basic eC1hY2Nlc3MtdG9rZW46dA==",
      "Git-Protocol": "version=0",
      Cookie: "meldweb_refresh=secret",
      "X-Other": "no",
    },
  });
  expect(r.status).toBe(200);
  expect(await r.text()).toBe("0000");
  expect(r.headers.get("Content-Type")).toBe(
    "application/x-git-upload-pack-advertisement",
  );
  expect(r.headers.get("Set-Cookie")).toBeNull();
  expect(r.headers.get("Cache-Control")).toBe("no-store");
  expect(seen).toHaveLength(1);
  const [s] = seen;
  expect(s?.url).toBe(
    "https://github.com/cramt/mtg.git/info/refs?service=git-upload-pack",
  );
  expect(s?.redirect).toBe("manual");
  expect(s?.headers.get("Authorization")).toBe(
    "Basic eC1hY2Nlc3MtdG9rZW46dA==",
  );
  expect(s?.headers.get("Git-Protocol")).toBe("version=0");
  expect(s?.headers.get("Cookie")).toBeNull();
  expect(s?.headers.get("X-Other")).toBeNull();
});

it("relays a push's body to receive-pack", async () => {
  answer = () =>
    new Response("000eunpack ok\n0000", {
      headers: { "Content-Type": "application/x-git-receive-pack-result" },
    });
  const r = await ask(`${REPO}/git-receive-pack`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-git-receive-pack-request",
      Accept: "application/x-git-receive-pack-result",
    },
    body: "0000PACK",
  });
  expect(r.status).toBe(200);
  expect(await r.text()).toBe("000eunpack ok\n0000");
  expect(seen[0]?.url).toBe(
    "https://github.com/cramt/mtg.git/git-receive-pack",
  );
  expect(seen[0]?.method).toBe("POST");
  expect(seen[0]?.body).toBe("0000PACK");
  expect(seen[0]?.headers.get("Content-Type")).toBe(
    "application/x-git-receive-pack-request",
  );
});

it("takes GitLab's subgroups and every listed host", async () => {
  for (const host of GIT_HOSTS) {
    const r = await ask(
      `/api/git/${host}/group/sub/mtg.git/info/refs?service=git-receive-pack`,
    );
    expect(r.status).toBe(200);
  }
  expect(seen.map((s) => new URL(s.url).host)).toEqual([...GIT_HOSTS]);
});

it("refuses anything that is not one of git's three requests to a known host", async () => {
  const refused: [string, RequestInit?][] = [
    ["/api/git/evil.example/a/b/info/refs?service=git-upload-pack"],
    ["/api/git/github.com.evil.example/a/b/info/refs?service=git-upload-pack"],
    ["/api/git/github.com/a/b/info/refs?service=git-upload-archive"],
    ["/api/git/github.com/a/b/info/refs"],
    ["/api/git/github.com/a/b/info/refs?service=git-upload-pack&x=1"],
    ["/api/git/github.com/a/../b/info/refs?service=git-upload-pack"],
    ["/api/git/github.com/a/%2e%2e/info/refs?service=git-upload-pack"],
    ["/api/git/github.com/only-one/info/refs?service=git-upload-pack"],
    ["/api/git/github.com/a/b/HEAD"],
    ["/api/git/github.com/a/b/objects/info/packs"],
    ["/api/git/github.com/a/b.git/git-upload-pack?x=1", { method: "POST" }],
    ["/api/git/github.com/api/v3/user"],
  ];
  for (const [path, init] of refused) {
    const r = await ask(path, init);
    expect([404, 405, 415], path).toContain(r.status);
  }
  // Wrong method, and a POST that is not git's.
  expect(
    (await ask(`${REPO}/info/refs?service=git-upload-pack`, { method: "POST" }))
      .status,
  ).toBe(405);
  expect((await ask(`${REPO}/git-upload-pack`)).status).toBe(405);
  expect(
    (
      await ask(`${REPO}/git-upload-pack`, {
        method: "POST",
        headers: { "Content-Type": "text/plain" },
        body: "x",
      })
    ).status,
  ).toBe(415);
  expect(seen).toEqual([]);
});

it("does not follow a host's redirect, and hides its basic-auth challenge from the browser", async () => {
  answer = () =>
    new Response(null, {
      status: 302,
      headers: { Location: "https://elsewhere.example/" },
    });
  expect((await ask(`${REPO}/info/refs?service=git-upload-pack`)).status).toBe(
    502,
  );
  answer = () =>
    new Response("", {
      status: 401,
      headers: { "WWW-Authenticate": 'Basic realm="GitHub"' },
    });
  const r = await ask(`${REPO}/info/refs?service=git-upload-pack`);
  expect(r.status).toBe(401);
  expect(r.headers.get("WWW-Authenticate")).toBeNull();
  expect(r.headers.get("X-Git-Authenticate")).toBe('Basic realm="GitHub"');
});

it("is reached through the worker before the auth routes", async () => {
  const direct = await handleGit(
    new Request(`${SITE}${REPO}/info/refs?service=git-upload-pack`),
  );
  expect(direct.status).toBe(200);
  expect((await ask("/api/auth/nope")).status).toBe(404);
  expect(seen).toHaveLength(1);
});
