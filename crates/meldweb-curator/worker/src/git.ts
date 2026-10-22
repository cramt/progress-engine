// A relay for git's smart HTTP protocol, and nothing else (ADR-0041). The
// browser's repo pushes to and fetches from its remote over git, and a git
// host's endpoints send no CORS headers, so the page asks this worker, on its
// own origin, and the worker asks the host.
//
// `/api/git/<host>/<repo path>/info/refs?service=git-upload-pack` and
// `…=git-receive-pack` (GET), and `/api/git/<host>/<repo path>/git-upload-pack`
// and `…/git-receive-pack` (POST), become the same request to
// `https://<host>/<repo path>/…`. Anything else is refused, so this is not an
// open proxy: one of three request shapes, to a host on the list below, with
// only git's own headers passed through. Bodies stream both ways, and nothing
// is kept: the Authorization header reaches the host and goes no further.

export const GIT_PREFIX = "/api/git/";

/**
 * The git hosts a remote may be on. To add one, put its host name here (as
 * it appears in its HTTPS clone URLs, with no scheme or path) and add a
 * case to git.test.ts. It must speak smart HTTP at
 * `https://<host>/<path>/info/refs`; one that redirects git elsewhere is
 * refused rather than followed.
 */
export const GIT_HOSTS: readonly string[] = [
  "github.com",
  "gitlab.com",
  "codeberg.org",
];

const SERVICES = ["git-upload-pack", "git-receive-pack"] as const;
type Service = (typeof SERVICES)[number];

/** Headers a git client sends that the host needs, and none else. */
const REQUEST_HEADERS = [
  "Authorization",
  "Git-Protocol",
  "Content-Type",
  "Accept",
  "Content-Encoding",
] as const;
const RESPONSE_HEADERS = ["Content-Type", "Content-Encoding"] as const;

// One path segment of a repo, as GitHub, GitLab (with subgroups) and
// Codeberg name them: no dots alone, no empty segments, nothing to escape.
const SEGMENT = /^(?!\.{1,2}$)[A-Za-z0-9._~-]+$/;

type Route =
  | { kind: "advertise"; host: string; repo: string; service: Service }
  | { kind: "rpc"; host: string; repo: string; service: Service };

function route(url: URL): Route | null {
  const parts = url.pathname.slice(GIT_PREFIX.length).split("/");
  const host = parts.shift();
  if (!host || !GIT_HOSTS.includes(host)) return null;
  const last = parts.at(-1);
  if (last === "refs" && parts.at(-2) === "info") {
    const repo = parts.slice(0, -2);
    const service = url.searchParams.get("service");
    if (!SERVICES.includes(service as Service)) return null;
    if ([...url.searchParams.keys()].length !== 1) return null;
    return valid(repo)
      ? {
          kind: "advertise",
          host,
          repo: repo.join("/"),
          service: service as Service,
        }
      : null;
  }
  if (SERVICES.includes(last as Service)) {
    const repo = parts.slice(0, -1);
    if (url.search !== "") return null;
    return valid(repo)
      ? { kind: "rpc", host, repo: repo.join("/"), service: last as Service }
      : null;
  }
  return null;
}

function valid(repo: string[]): boolean {
  return repo.length >= 2 && repo.every((s) => SEGMENT.test(s));
}

const refuse = (status: number, message: string) =>
  new Response(`${message}\n`, {
    status,
    headers: {
      "Content-Type": "text/plain; charset=utf-8",
      "Cache-Control": "no-store",
    },
  });

export async function handleGit(
  request: Request,
  fetchImpl: typeof fetch = fetch,
): Promise<Response> {
  const url = new URL(request.url);
  const r = route(url);
  if (!r) return refuse(404, "not a git smart HTTP request to a known host");
  const method = r.kind === "advertise" ? "GET" : "POST";
  if (request.method !== method) {
    return new Response(`${method} only\n`, {
      status: 405,
      headers: { Allow: method },
    });
  }
  if (r.kind === "rpc") {
    const type = request.headers.get("Content-Type");
    if (type !== `application/x-${r.service}-request`)
      return refuse(
        415,
        `a ${r.service} request is application/x-${r.service}-request`,
      );
  }
  const upstream =
    r.kind === "advertise"
      ? `https://${r.host}/${r.repo}/info/refs?service=${r.service}`
      : `https://${r.host}/${r.repo}/${r.service}`;
  const headers = new Headers({
    "User-Agent": "git/2 (progress-engine relay)",
  });
  for (const name of REQUEST_HEADERS) {
    const value = request.headers.get(name);
    if (value !== null) headers.set(name, value);
  }
  let answer: Response;
  try {
    answer = await fetchImpl(upstream, {
      method,
      headers,
      redirect: "manual",
      ...(method === "POST" ? { body: request.body, duplex: "half" } : {}),
    } as RequestInit);
  } catch (e) {
    return refuse(
      502,
      `${r.host} could not be reached: ${e instanceof Error ? e.message : e}`,
    );
  }
  if (answer.status >= 300 && answer.status < 400) {
    return refuse(
      502,
      `${r.host} redirected the request, and the relay does not follow`,
    );
  }
  const out = new Headers({ "Cache-Control": "no-store" });
  for (const name of RESPONSE_HEADERS) {
    const value = answer.headers.get(name);
    if (value !== null) out.set(name, value);
  }
  // A 401's challenge, so the page can tell "log in again" from a refusal.
  const challenge = answer.headers.get("WWW-Authenticate");
  if (answer.status === 401 && challenge !== null)
    out.set("X-Git-Authenticate", challenge);
  return new Response(answer.body, { status: answer.status, headers: out });
}
