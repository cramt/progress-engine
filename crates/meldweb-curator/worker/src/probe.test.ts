import { afterEach, beforeEach, expect, it, vi } from "vitest";
import worker, { type Env } from "./index";

const SITE = "https://curator.example";
const CORE = "fb8ce43fc99febf288f2123233602822184a3539d7b362d9576563f0c28bac50";

const env: Env = {
  GITHUB_CLIENT_ID: "Iv1.test",
  GITHUB_CLIENT_SECRET: "secret",
  GITHUB_APP_SLUG: "test",
  ASSETS: { fetch: async () => new Response("asset") },
};

let calls: { url: string; auth: string | null }[];

beforeEach(() => {
  calls = [];
  // ghcr.io: an anonymous token, then a blob by digest for that token.
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      const auth = new Headers(init?.headers).get("Authorization");
      calls.push({ url, auth });
      if (url.startsWith("https://ghcr.io/token?"))
        return Response.json({ token: "anon" });
      if (auth !== "Bearer anon") return new Response("", { status: 401 });
      return new Response(`blob ${url.split(":").at(-1)}`);
    }),
  );
});

afterEach(() => vi.unstubAllGlobals());

const get = (path: string) => worker.fetch(new Request(`${SITE}${path}`), env);

it("pipes the blob with that digest from ghcr.io, typed by its name", async () => {
  const response = await get(`/gitaxian-probe/${CORE}/core.wasm`);
  expect(response.status).toBe(200);
  expect(await response.text()).toBe(`blob ${CORE}`);
  expect(response.headers.get("Content-Type")).toBe("application/wasm");
  expect(calls).toEqual([
    {
      url: "https://ghcr.io/token?scope=repository:cramt/delver-x:pull",
      auth: null,
    },
    {
      url: `https://ghcr.io/v2/cramt/delver-x/blobs/sha256:${CORE}`,
      auth: "Bearer anon",
    },
  ]);
});

it("serves core.js as JavaScript, which a Web Worker needs", async () => {
  const response = await get(`/gitaxian-probe/${CORE}/core.js`);
  expect(response.headers.get("Content-Type")).toBe("text/javascript");
});

it("passes ghcr.io's status through", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL) =>
      String(input).includes("/token?")
        ? Response.json({ token: "anon" })
        : new Response("", { status: 404 }),
    ),
  );
  expect((await get(`/gitaxian-probe/${CORE}/core.wasm`)).status).toBe(404);
});

it("asks ghcr.io nothing for a path that is not a digest and a name", async () => {
  for (const path of [
    "/gitaxian-probe/core.wasm",
    `/gitaxian-probe/${CORE}`,
    `/gitaxian-probe/${CORE}/../x`,
    "/gitaxian-probe/latest/core.wasm",
  ]) {
    expect((await get(path)).status, path).toBe(404);
  }
  expect(calls).toEqual([]);
});
