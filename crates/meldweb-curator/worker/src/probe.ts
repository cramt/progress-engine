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

export async function handleProbe(request: Request): Promise<Response> {
  const path = new URL(request.url).pathname;
  const [, sha256, name] =
    /^\/gitaxian-probe\/([0-9a-f]{64})\/([\w.-]+)$/.exec(path) ?? [];
  if (!sha256 || !name) return new Response("not found", { status: 404 });

  // ghcr.io wants a token even for a public blob, and gives one to anybody.
  const { token } = (await (
    await fetch(`https://ghcr.io/token?scope=repository:${ARCHIVE}:pull`)
  ).json()) as { token: string };
  const blob = await fetch(
    `https://ghcr.io/v2/${ARCHIVE}/blobs/sha256:${sha256}`,
    { headers: { Authorization: `Bearer ${token}` } },
  );
  return new Response(blob.body, {
    status: blob.status,
    headers: {
      // A Web Worker's script must be JavaScript, and core.js runs as 32.
      "Content-Type":
        TYPES[name.slice(name.lastIndexOf(".") + 1)] ??
        "application/octet-stream",
      // The page is cross-origin isolated, and so must its workers' scripts be.
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  });
}
