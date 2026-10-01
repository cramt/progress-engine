// Gitaxian Probe's engine files, proxied from the archive on ghcr.io. The
// page's scanner fetches them from its own origin, because neither Delver nor
// ghcr.io sends CORS headers (crates/gitaxian-probe/engine/README.md).
//
// Only the pinned build is served, and each file only by the digest its pin
// gives it, so the worker hands out nothing the build did not check. A blob
// on ghcr.io is addressed by its sha256, so what comes back is what was
// pinned. The pin is the site's own asset, written by its build, which keeps
// the files the worker serves and the scanner the site carries one build.

/** Where the site's build writes the pin: `{ tag, files: [{ name, sha256 }] }`. */
export const PIN_PATH = "/gitaxian-probe-pin.json";
export const PREFIX = "/gitaxian-probe/";

const REGISTRY = "https://ghcr.io";
const REPOSITORY = "cramt/delver-x";

export interface Pin {
  tag: string;
  files: { name: string; sha256: string }[];
}

export interface ProbeEnv {
  ASSETS: { fetch(request: Request): Promise<Response> };
}

/** What Cloudflare's `caches.default` is, as far as this file uses it. */
export interface EdgeCache {
  match(request: Request): Promise<Response | undefined>;
  put(request: Request, response: Response): Promise<void>;
}

export interface ProbeContext {
  waitUntil(promise: Promise<unknown>): void;
}

const TYPES: Record<string, string> = {
  js: "text/javascript",
  wasm: "application/wasm",
  txt: "text/plain",
  "7z": "application/x-7z-compressed",
};

// The scanner's page is cross-origin isolated, and its engine runs core.js as
// Web Workers, whose scripts must carry the same policy.
const ISOLATED = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "credentialless",
  "Cross-Origin-Resource-Policy": "same-origin",
};

function refuse(status: number, error: string): Response {
  return Response.json(
    { error },
    { status, headers: { ...ISOLATED, "Cache-Control": "no-store" } },
  );
}

/** ghcr.io's anonymous pull token, kept while ghcr.io keeps accepting it. */
let token: string | null = null;

async function pullToken(): Promise<string> {
  const response = await fetch(
    `${REGISTRY}/token?scope=repository:${REPOSITORY}:pull`,
  );
  if (!response.ok) throw new Error(`token: ${response.status}`);
  const body = (await response.json()) as { token?: unknown };
  if (typeof body.token !== "string") throw new Error("token: none given");
  return body.token;
}

/** The blob, following ghcr.io's redirect to its storage; once more on a 401. */
async function blob(sha256: string): Promise<Response> {
  const url = `${REGISTRY}/v2/${REPOSITORY}/blobs/sha256:${sha256}`;
  for (let attempt = 0; attempt < 2; attempt++) {
    token ??= await pullToken();
    const response = await fetch(url, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (response.status !== 401) return response;
    token = null;
  }
  throw new Error("ghcr.io refused a fresh token");
}

async function loadPin(request: Request, env: ProbeEnv): Promise<Pin | null> {
  const response = await env.ASSETS.fetch(
    new Request(new URL(PIN_PATH, request.url)),
  );
  if (!response.ok) return null;
  return (await response.json()) as Pin;
}

/**
 * `GET /gitaxian-probe/<tag>/<name>`: the pinned build's file, from the edge
 * cache or else from ghcr.io. A URL names one build, so its answer never
 * changes and is cached for good.
 */
export async function handleProbe(
  request: Request,
  env: ProbeEnv,
  ctx: ProbeContext,
  cache: EdgeCache | undefined = (
    globalThis as { caches?: { default?: EdgeCache } }
  ).caches?.default,
): Promise<Response> {
  if (request.method !== "GET" && request.method !== "HEAD") {
    return refuse(405, "method_not_allowed");
  }
  const url = new URL(request.url);
  const [tag, name, ...rest] = url.pathname.slice(PREFIX.length).split("/");
  const pin = await loadPin(request, env);
  if (!pin) return refuse(503, "no_pin");
  const file = pin.files.find((f) => f.name === name);
  if (rest.length > 0 || tag !== pin.tag || !file) {
    return refuse(404, "not_pinned");
  }

  // Keyed without the query string, so nothing can split the cache.
  const key = new Request(`${url.origin}${url.pathname}`);
  const hit = await cache?.match(key);
  if (hit) return hit;

  let upstream: Response;
  try {
    upstream = await blob(file.sha256);
  } catch {
    return refuse(502, "archive_unreachable");
  }
  if (!upstream.ok || !upstream.body) {
    return refuse(502, `archive_${upstream.status}`);
  }
  const headers = new Headers({
    ...ISOLATED,
    "Content-Type":
      TYPES[file.name.slice(file.name.lastIndexOf(".") + 1)] ??
      "application/octet-stream",
    "Cache-Control": "public, max-age=31536000, immutable",
    ETag: `"${file.sha256}"`,
  });
  const length = upstream.headers.get("Content-Length");
  if (length) headers.set("Content-Length", length);
  const response = new Response(upstream.body, { headers });
  if (cache) ctx.waitUntil(cache.put(key, response.clone()));
  return request.method === "HEAD"
    ? new Response(null, { headers: response.headers })
    : response;
}

/** For tests: forget the pull token, as a fresh isolate would. */
export function forgetToken(): void {
  token = null;
}
