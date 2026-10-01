import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import worker, { type Env } from "./index";
import { type EdgeCache, forgetToken, handleProbe, type Pin } from "./probe";

const SITE = "https://curator.example";
const TAG = "delver-1.89.beta-eeb9c6a9c3ec";
const CORE = "fb8ce43fc99febf288f2123233602822184a3539d7b362d9576563f0c28bac50";
const pin: Pin = {
  tag: TAG,
  files: [
    { name: "core.wasm", sha256: CORE },
    { name: "version.txt", sha256: "a".repeat(64) },
  ],
};

function env(withPin = true): Env {
  return {
    GITHUB_CLIENT_ID: "Iv1.test",
    GITHUB_CLIENT_SECRET: "secret",
    GITHUB_APP_SLUG: "test",
    ASSETS: {
      fetch: async (request) =>
        new URL(request.url).pathname === "/gitaxian-probe-pin.json" && withPin
          ? Response.json(pin)
          : new Response("not found", { status: 404 }),
    },
  };
}

/** ghcr.io as the worker sees it: a token, then a blob by digest. */
class Ghcr {
  calls: string[] = [];
  tokens = 0;
  /** Tokens ghcr.io has stopped accepting. */
  expired = new Set<string>();
  fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    this.calls.push(url);
    if (url.startsWith("https://ghcr.io/token?")) {
      expect(url).toContain("scope=repository:cramt/delver-x:pull");
      return Response.json({ token: `t${++this.tokens}` });
    }
    const blob =
      /^https:\/\/ghcr\.io\/v2\/cramt\/delver-x\/blobs\/sha256:(\w+)$/.exec(
        url,
      );
    if (blob) {
      const auth = new Headers(init?.headers).get("Authorization") ?? "";
      const given = auth.replace("Bearer ", "");
      if (!given || this.expired.has(given)) {
        return new Response("", { status: 401 });
      }
      return new Response(`bytes of ${blob[1]}`, {
        headers: { "Content-Length": String(9 + 64) },
      });
    }
    throw new Error(`unexpected fetch ${url}`);
  };
}

/** caches.default, in memory. */
function memoryCache(): EdgeCache & { stored: Map<string, Response> } {
  const stored = new Map<string, Response>();
  return {
    stored,
    match: async (r) => stored.get(r.url)?.clone(),
    put: async (r, response) => void stored.set(r.url, response),
  };
}

let ghcr: Ghcr;
const waits: Promise<unknown>[] = [];
const ctx = { waitUntil: (p: Promise<unknown>) => void waits.push(p) };

beforeEach(() => {
  forgetToken();
  ghcr = new Ghcr();
  vi.stubGlobal("fetch", vi.fn(ghcr.fetch));
});

afterEach(() => {
  vi.unstubAllGlobals();
  waits.length = 0;
});

const get = (path: string, cache?: EdgeCache, method = "GET") =>
  handleProbe(new Request(`${SITE}${path}`, { method }), env(), ctx, cache);

describe("the scanner's engine files", () => {
  it("are the pinned blob, from ghcr.io, for the site's own page", async () => {
    const response = await get(`/gitaxian-probe/${TAG}/core.wasm`);
    expect(response.status).toBe(200);
    expect(await response.text()).toBe(`bytes of ${CORE}`);
    expect(response.headers.get("Content-Type")).toBe("application/wasm");
    expect(response.headers.get("Cross-Origin-Embedder-Policy")).toBe(
      "credentialless",
    );
    expect(response.headers.get("Cross-Origin-Resource-Policy")).toBe(
      "same-origin",
    );
    expect(response.headers.get("Cache-Control")).toContain("immutable");
    expect(ghcr.calls).toEqual([
      "https://ghcr.io/token?scope=repository:cramt/delver-x:pull",
      `https://ghcr.io/v2/cramt/delver-x/blobs/sha256:${CORE}`,
    ]);
  });

  it("go through the worker, not the static assets", async () => {
    const response = await worker.fetch(
      new Request(`${SITE}/gitaxian-probe/${TAG}/core.wasm`),
      env(),
      ctx,
    );
    expect(await response.text()).toBe(`bytes of ${CORE}`);
  });

  it("keep the token for the next file", async () => {
    await get(`/gitaxian-probe/${TAG}/core.wasm`);
    await get(`/gitaxian-probe/${TAG}/version.txt`);
    expect(ghcr.tokens).toBe(1);
  });

  it("take a fresh token when ghcr.io stops accepting the old one", async () => {
    await get(`/gitaxian-probe/${TAG}/core.wasm`);
    ghcr.expired.add("t1");
    const response = await get(`/gitaxian-probe/${TAG}/core.wasm`);
    expect(response.status).toBe(200);
    expect(ghcr.tokens).toBe(2);
  });

  it("are answered from the edge cache once fetched", async () => {
    const cache = memoryCache();
    await (await get(`/gitaxian-probe/${TAG}/core.wasm`, cache)).text();
    await Promise.all(waits);
    const calls = ghcr.calls.length;
    const again = await get(`/gitaxian-probe/${TAG}/core.wasm?x=1`, cache);
    expect(await again.text()).toBe(`bytes of ${CORE}`);
    expect(ghcr.calls.length).toBe(calls);
    expect([...cache.stored.keys()]).toEqual([
      `${SITE}/gitaxian-probe/${TAG}/core.wasm`,
    ]);
  });

  it("refuse a file the pin does not name", async () => {
    for (const path of [
      `/gitaxian-probe/${TAG}/model-gamma.7z`,
      `/gitaxian-probe/${TAG}/../core.wasm`,
      `/gitaxian-probe/${TAG}/core.wasm/x`,
      `/gitaxian-probe/${TAG}/`,
      "/gitaxian-probe/core.wasm",
    ]) {
      expect((await get(path)).status, path).toBe(404);
    }
    expect(ghcr.calls).toEqual([]);
  });

  it("refuse another build than the pinned one", async () => {
    const response = await get(
      "/gitaxian-probe/delver-1.83.beta-000000000000/core.wasm",
    );
    expect(response.status).toBe(404);
    expect(ghcr.calls).toEqual([]);
  });

  it("are a 503 on a site built without the scanner", async () => {
    const response = await handleProbe(
      new Request(`${SITE}/gitaxian-probe/${TAG}/core.wasm`),
      env(false),
      ctx,
    );
    expect(response.status).toBe(503);
  });

  it("are a 502 when ghcr.io is down, and not cached", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("", { status: 503 })),
    );
    const cache = memoryCache();
    const response = await get(`/gitaxian-probe/${TAG}/core.wasm`, cache);
    expect(response.status).toBe(502);
    expect(response.headers.get("Cache-Control")).toBe("no-store");
    expect(cache.stored.size).toBe(0);
  });

  it("answer HEAD with the headers alone", async () => {
    const response = await get(
      `/gitaxian-probe/${TAG}/core.wasm`,
      undefined,
      "HEAD",
    );
    expect(response.status).toBe(200);
    expect(response.headers.get("Content-Length")).toBe("73");
    expect(await response.text()).toBe("");
  });

  it("refuse anything but reading", async () => {
    const response = await get(
      `/gitaxian-probe/${TAG}/core.wasm`,
      undefined,
      "POST",
    );
    expect(response.status).toBe(405);
  });
});
