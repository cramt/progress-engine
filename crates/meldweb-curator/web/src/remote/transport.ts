/**
 * Git's smart HTTP requests to the repo's remote, through the worker's relay
 * on this site's own origin (`worker/src/git.ts`), since no git host answers
 * a browser's CORS preflight. This moves bytes; what they say is Rust's.
 */

export type Service = "git-upload-pack" | "git-receive-pack";

/** The login a host takes, as git's HTTP Authorization header. */
export interface Credentials {
  /** The header for the next request, or null when there is no login. */
  authorization(): Promise<string | null>;
  /** After the host refused `stale` with a 401: a fresh one, or null. */
  refreshed(stale: string): Promise<string | null>;
}

/** No login at all: enough to fetch a public repo. */
export const ANONYMOUS: Credentials = {
  authorization: async () => null,
  refreshed: async () => null,
};

/** The host wants a login this page does not have. */
export class NeedsLogin extends Error {
  override name = "NeedsLogin";
}

/** The host, or the relay, answered with something other than git. */
export class RemoteError extends Error {
  override name = "RemoteError";
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
  }
}

export interface GitTransport {
  /** `GET <url>/info/refs?service=…`: the refs and capabilities. */
  advertise(url: string, service: Service): Promise<Uint8Array>;
  /** `POST <url>/<service>` with `body`: the host's answer. */
  post(url: string, service: Service, body: Uint8Array): Promise<Uint8Array>;
}

/** `https://github.com/o/mtg.git` as the relay's path for it. */
export function relayPath(url: string): string {
  const u = new URL(url);
  if (u.protocol !== "https:" || u.search || u.hash || u.username)
    throw new Error(`${url} is not a remote Progress Engine can reach`);
  return `/api/git/${u.host}${u.pathname.replace(/\/+$/, "")}`;
}

/**
 * GitHub, GitLab and Codeberg all take a token as the password of HTTP basic
 * auth. GitHub documents `x-access-token` as the user for a GitHub App's
 * tokens ("Authenticating as a GitHub App installation", docs.github.com).
 */
export function basicToken(token: string, user = "x-access-token"): string {
  return `Basic ${btoa(`${user}:${token}`)}`;
}

export function relayTransport(options: {
  credentials: Credentials;
  fetch?: typeof fetch;
}): GitTransport {
  const { credentials } = options;
  const fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);

  async function send(
    url: string,
    service: Service,
    init: { method: "GET" | "POST"; body?: Uint8Array },
  ): Promise<Uint8Array> {
    const path =
      init.method === "GET"
        ? `${relayPath(url)}/info/refs?service=${service}`
        : `${relayPath(url)}/${service}`;
    const once = (auth: string | null) => {
      const headers = new Headers();
      if (auth) headers.set("Authorization", auth);
      if (init.method === "POST") {
        headers.set("Content-Type", `application/x-${service}-request`);
        headers.set("Accept", `application/x-${service}-result`);
      }
      return fetchImpl(path, {
        method: init.method,
        headers,
        credentials: "omit",
        cache: "no-store",
        ...(init.body ? { body: init.body as Uint8Array<ArrayBuffer> } : {}),
      });
    };
    let auth = await credentials.authorization();
    let r = await once(auth);
    if (r.status === 401 && auth) {
      auth = await credentials.refreshed(auth);
      if (auth) r = await once(auth);
    }
    if (r.status === 401 || (r.status === 404 && !auth)) {
      // A private repo answers 404 to a request with no login on GitHub.
      throw new NeedsLogin(`${new URL(url).host} wants a login for ${url}`);
    }
    if (!r.ok) {
      const why = (await r.text()).trim().slice(0, 300);
      throw new RemoteError(
        `${new URL(url).host} answered ${r.status}${why ? `: ${why}` : ""}`,
        r.status,
      );
    }
    const kind =
      init.method === "GET"
        ? `application/x-${service}-advertisement`
        : `application/x-${service}-result`;
    if (!r.headers.get("Content-Type")?.startsWith(kind)) {
      throw new RemoteError(
        `${url} did not answer as a git host: no ${kind}`,
        r.status,
      );
    }
    return new Uint8Array(await r.arrayBuffer());
  }

  return {
    advertise: (url, service) => send(url, service, { method: "GET" }),
    post: (url, service, body) => send(url, service, { method: "POST", body }),
  };
}
