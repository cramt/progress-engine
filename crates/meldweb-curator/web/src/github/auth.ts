/**
 * The page's GitHub access token, got from the worker and held in memory only
 * (github-login-static-site.md, "Where the token lives").
 *
 * The worker keeps the refresh token in an HttpOnly cookie, and a refresh
 * token works once: refreshing rotates the cookie and kills the old access
 * token. So every tab refreshes under one Web Lock, and the tab that did it
 * hands the new token to the others over a BroadcastChannel. A tab that waited
 * for the lock and meanwhile heard of a fresh token uses that instead of
 * refreshing again.
 *
 * If the broadcast arrives only after the waiting tab has taken the lock, that
 * tab refreshes once more with the already rotated cookie. That succeeds,
 * because the cookie jar is shared, and its broadcast replaces the first tab's
 * now-dead token; the cost is one extra refresh, never a logout.
 */

/** The worker's routes (#123). One place, so they are trivially moved. */
export const AUTH_ENDPOINTS = {
  login: "/api/auth/login",
  refresh: "/api/auth/refresh",
  logout: "/api/auth/logout",
  /** `{ app_slug, install_url }` for onboarding. */
  app: "/api/auth/app",
} as const;

/** Refresh when the token has less than this left, rather than at expiry. */
export const REFRESH_MARGIN_MS = 5 * 60 * 1000;

const LOCK_NAME = "meldweb-curator-auth-refresh";
const CHANNEL_NAME = "meldweb-curator-auth";

export interface Token {
  accessToken: string;
  /** Milliseconds since the epoch, as the worker answers it. */
  expiresAt: number;
}

/** The part of `navigator.locks` used here. */
export interface LockManagerLike {
  request<T>(name: string, callback: () => Promise<T>): Promise<T>;
}

/** The part of `BroadcastChannel` used here. */
export interface ChannelLike {
  postMessage(message: unknown): void;
  onmessage: ((event: MessageEvent) => void) | null;
  close(): void;
}

type Message = { type: "token"; token: Token } | { type: "logout" };

export interface Auth {
  /** A token good for at least the margin, refreshing if needed; `null` when logged out. */
  token(): Promise<string | null>;
  /**
   * After a 401 with `stale`: a different, fresh token, refreshing unless
   * another caller or tab already has; `null` when logged out.
   */
  refreshed(stale: string): Promise<string | null>;
  /** The token held right now, without asking anyone. */
  peek(): string | null;
  /** Leaves the page for GitHub's login, coming back to `returnPath`. */
  login(returnPath: string): void;
  logout(): Promise<void>;
  /** Stops listening to other tabs. */
  close(): void;
}

export interface AuthOptions {
  fetch?: typeof fetch;
  /** Defaults to `navigator.locks`, or a lock only this page sees without it. */
  locks?: LockManagerLike;
  /** Defaults to a `BroadcastChannel`; `null` for none. */
  channel?: ChannelLike | null;
  now?: () => number;
  navigate?: (url: string) => void;
}

export class AuthError extends Error {
  override name = "AuthError";
}

/** A lock per name that serialises within one page, for where Web Locks are missing. */
export function pageLocks(): LockManagerLike {
  const tails = new Map<string, Promise<unknown>>();
  return {
    request<T>(name: string, callback: () => Promise<T>): Promise<T> {
      const tail = tails.get(name) ?? Promise.resolve();
      const run = tail.then(callback, callback);
      tails.set(
        name,
        run.then(
          () => undefined,
          () => undefined,
        ),
      );
      return run;
    },
  };
}

function defaultLocks(): LockManagerLike {
  const locks = (globalThis.navigator as Navigator | undefined)?.locks;
  if (locks) {
    return {
      request: <T>(name: string, callback: () => Promise<T>) =>
        locks.request(name, callback) as Promise<T>,
    };
  }
  return pageLocks();
}

function defaultChannel(): ChannelLike | null {
  return typeof BroadcastChannel === "undefined"
    ? null
    : new BroadcastChannel(CHANNEL_NAME);
}

function isToken(body: unknown): body is {
  access_token: string;
  expires_at: number;
} {
  if (typeof body !== "object" || body === null) return false;
  const b = body as Record<string, unknown>;
  return typeof b.access_token === "string" && typeof b.expires_at === "number";
}

export function createAuth(options: AuthOptions = {}): Auth {
  const fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);
  const locks = options.locks ?? defaultLocks();
  const channel =
    options.channel === undefined ? defaultChannel() : options.channel;
  const now = options.now ?? Date.now;
  const navigate =
    options.navigate ?? ((url: string) => window.location.assign(url));

  let current: Token | null = null;
  let inflight: Promise<Token | null> | null = null;

  const fresh = (t: Token): boolean => t.expiresAt - REFRESH_MARGIN_MS > now();

  const adopt = (t: Token) => {
    if (!current || t.expiresAt >= current.expiresAt) current = t;
  };

  if (channel) {
    channel.onmessage = (event) => {
      const m = event.data as Message;
      if (m.type === "token") adopt(m.token);
      else if (m.type === "logout") current = null;
    };
  }

  const refresh = (stale: string | null): Promise<Token | null> => {
    inflight ??= locks
      .request(LOCK_NAME, async () => {
        // Another tab may have refreshed while this one waited. Its broadcast
        // was posted before it let go of the lock; one task lets it land.
        await new Promise((resolve) => setTimeout(resolve, 0));
        if (current && fresh(current) && current.accessToken !== stale)
          return current;
        const r = await fetchImpl(AUTH_ENDPOINTS.refresh, {
          method: "POST",
          credentials: "same-origin",
        });
        if (r.status === 401) {
          current = null;
          return null;
        }
        if (!r.ok) throw new AuthError(`refresh answered ${r.status}`);
        const body: unknown = await r.json();
        if (!isToken(body)) throw new AuthError("refresh answered no token");
        const token = {
          accessToken: body.access_token,
          expiresAt: body.expires_at,
        };
        current = token;
        channel?.postMessage({ type: "token", token } satisfies Message);
        return token;
      })
      .finally(() => {
        inflight = null;
      });
    return inflight;
  };

  return {
    async token() {
      if (current && fresh(current)) return current.accessToken;
      return (await refresh(current?.accessToken ?? null))?.accessToken ?? null;
    },
    async refreshed(stale) {
      if (current && fresh(current) && current.accessToken !== stale) {
        return current.accessToken;
      }
      return (await refresh(stale))?.accessToken ?? null;
    },
    peek: () => current?.accessToken ?? null,
    login(returnPath) {
      navigate(
        `${AUTH_ENDPOINTS.login}?return=${encodeURIComponent(returnPath)}`,
      );
    },
    async logout() {
      await fetchImpl(AUTH_ENDPOINTS.logout, {
        method: "POST",
        credentials: "same-origin",
      });
      current = null;
      channel?.postMessage({ type: "logout" } satisfies Message);
    },
    close() {
      channel?.close();
    },
  };
}
