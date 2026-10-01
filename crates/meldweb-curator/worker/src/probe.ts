// Gitaxian Probe's engine files, proxied from the archive on ghcr.io, which
// sends no CORS headers (crates/gitaxian-probe/engine/README.md).
//
// `GET /gitaxian-probe/<sha256>/<name>` is the blob with that digest, typed by
// the name. The page knows which digest is which file; this is only a pipe.

export const PREFIX = "/gitaxian-probe/";

const ARCHIVE = "cramt/delver-x";

const TYPES: Record<string, string> = {
  js: "text/javascript",
  wasm: "application/wasm",
};

// A path names its blob by digest, so what it serves never changes: the
// browser keeps it for a year without asking again, and the edge keeps it so
// ghcr.io is asked once per blob per data centre rather than once per visit.
const IMMUTABLE = "public, max-age=31536000, immutable";

export async function handleProbe(
  request: Request,
  waitUntil: (promise: Promise<unknown>) => void,
): Promise<Response> {
  const path = new URL(request.url).pathname;
  const [, sha256, name] =
    /^\/gitaxian-probe\/([0-9a-f]{64})\/([\w.-]+)$/.exec(path) ?? [];
  if (!sha256 || !name) return new Response("not found", { status: 404 });

  const edge = await caches.open("gitaxian-probe");
  const cached = await edge.match(request);
  if (cached) return cached;

  // ghcr.io wants a token even for a public blob, and gives one to anybody.
  const { token } = (await (
    await fetch(`https://ghcr.io/token?scope=repository:${ARCHIVE}:pull`)
  ).json()) as { token: string };
  const blob = await fetch(
    `https://ghcr.io/v2/${ARCHIVE}/blobs/sha256:${sha256}`,
    { headers: { Authorization: `Bearer ${token}` } },
  );
  const response = new Response(blob.body, {
    status: blob.status,
    headers: {
      // A failure is ghcr.io's for now, not this digest's forever.
      "Cache-Control": blob.ok ? IMMUTABLE : "no-store",
      // A Web Worker's script must be JavaScript, and core.js runs as 32.
      "Content-Type":
        TYPES[name.slice(name.lastIndexOf(".") + 1)] ??
        "application/octet-stream",
      // The page is cross-origin isolated, and so must its workers' scripts be.
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  });
  if (blob.ok) waitUntil(edge.put(request, response.clone()));
  return response;
}
