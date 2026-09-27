// The GitHub App web flow with PKCE, from docs/research/github-login-static-site.md.
//
// Two cookies, both HttpOnly and Secure, both on this worker's own origin:
// - `meldweb_login` carries state, the PKCE verifier and the return path from
//   /login to /callback. SameSite=Lax, because the callback arrives as a
//   top-level navigation from github.com. Ten minutes, callback path only.
// - `meldweb_refresh` carries GitHub's refresh token. SameSite=Strict, path
//   /api/auth, for as long as GitHub says the refresh token lives.
// Nothing else is kept anywhere: no KV, no database.

export interface AuthEnv {
  GITHUB_CLIENT_ID: string;
  GITHUB_CLIENT_SECRET: string;
  GITHUB_APP_SLUG: string;
}

const GITHUB = "https://github.com";
const LOGIN_COOKIE = "meldweb_login";
const REFRESH_COOKIE = "meldweb_refresh";
const LOGIN_PATH = "/api/auth/callback";
const REFRESH_PATH = "/api/auth";
const LOGIN_TTL_SECONDS = 600;
/** GitHub's documented refresh-token lifetime, used if it sends none. */
const REFRESH_TTL_SECONDS = 15_897_600;

export async function handleAuth(
  request: Request,
  env: AuthEnv,
): Promise<Response> {
  const url = new URL(request.url);
  const route = url.pathname.replace(/\/+$/, "");
  const method = request.method;

  switch (route) {
    case "/api/auth/login":
      return method === "GET" ? login(url, env) : notAllowed("GET");
    case "/api/auth/callback":
      return method === "GET" ? callback(request, url, env) : notAllowed("GET");
    case "/api/auth/refresh":
      if (method !== "POST") return notAllowed("POST");
      return crossOrigin(request, url) ?? refresh(request, env);
    case "/api/auth/logout":
      if (method !== "POST") return notAllowed("POST");
      return crossOrigin(request, url) ?? logout();
    case "/api/auth/app":
      return method === "GET" ? app(env) : notAllowed("GET");
    default:
      return json(404, { error: "not_found" });
  }
}

// --- routes -----------------------------------------------------------------

function app(env: AuthEnv): Response {
  const slug = env.GITHUB_APP_SLUG;
  if (!configured(slug)) return json(503, { error: "not_configured" });
  return json(200, {
    app_slug: slug,
    install_url: `${GITHUB}/apps/${encodeURIComponent(slug)}/installations/new`,
  });
}

async function login(url: URL, env: AuthEnv): Promise<Response> {
  if (!configured(env.GITHUB_CLIENT_ID) || !env.GITHUB_CLIENT_SECRET) {
    return json(503, { error: "not_configured" });
  }
  const back = returnPath(url.searchParams.get("return"));
  const state = randomToken();
  const verifier = randomToken();
  const challenge = base64url(
    new Uint8Array(
      await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)),
    ),
  );

  const authorize = new URL(`${GITHUB}/login/oauth/authorize`);
  authorize.searchParams.set("client_id", env.GITHUB_CLIENT_ID);
  authorize.searchParams.set("redirect_uri", `${url.origin}${LOGIN_PATH}`);
  authorize.searchParams.set("state", state);
  authorize.searchParams.set("code_challenge", challenge);
  authorize.searchParams.set("code_challenge_method", "S256");

  const flow = [state, verifier, base64url(new TextEncoder().encode(back))];
  return redirect(authorize.toString(), [
    cookie(LOGIN_COOKIE, flow.join("."), {
      path: LOGIN_PATH,
      sameSite: "Lax",
      maxAge: LOGIN_TTL_SECONDS,
    }),
  ]);
}

async function callback(
  request: Request,
  url: URL,
  env: AuthEnv,
): Promise<Response> {
  const clearLogin = expire(LOGIN_COOKIE, LOGIN_PATH, "Lax");
  const flow = readLoginCookie(request);
  if (!flow) {
    return json(400, { error: "no_login_in_progress" }, [clearLogin]);
  }
  const state = url.searchParams.get("state");
  if (!state || state !== flow.state) {
    return json(400, { error: "state_mismatch" }, [clearLogin]);
  }
  // The user said no on GitHub's page: back to the app, still logged out.
  if (url.searchParams.has("error")) return redirect(flow.back, [clearLogin]);
  const code = url.searchParams.get("code");
  if (!code) return json(400, { error: "no_code" }, [clearLogin]);
  if (!flow.verifier) {
    return json(400, { error: "no_code_verifier" }, [clearLogin]);
  }

  const answer = await tokenRequest(env, {
    code,
    redirect_uri: `${url.origin}${LOGIN_PATH}`,
    code_verifier: flow.verifier,
  });
  if (answer.kind === "unreachable") {
    return json(502, { error: "github_unreachable" }, [clearLogin]);
  }
  if (answer.kind === "refused") {
    return json(400, { error: answer.error }, [clearLogin]);
  }
  if (!answer.refreshToken) {
    // The app has "expire user authorization tokens" off, so there is nothing
    // to refresh with and the page could never get the token back.
    return json(500, { error: "no_refresh_token" }, [clearLogin]);
  }
  return redirect(flow.back, [clearLogin, refreshCookie(answer)]);
}

async function refresh(request: Request, env: AuthEnv): Promise<Response> {
  const token = readCookie(request, REFRESH_COOKIE);
  if (!token) return json(401, { error: "logged_out" });

  const answer = await tokenRequest(env, {
    grant_type: "refresh_token",
    refresh_token: token,
  });
  if (answer.kind === "unreachable") {
    // The cookie may well still be good: keep it, and let the page retry.
    return json(502, { error: "github_unreachable" });
  }
  if (answer.kind === "refused") {
    if (answer.error === "bad_refresh_token") {
      // Used already (another tab won the race), expired, or revoked. Refresh
      // tokens are single-use, so this cookie is dead: drop it.
      return json(401, { error: answer.error }, [expireRefresh()]);
    }
    return json(502, { error: answer.error });
  }
  if (!answer.refreshToken) {
    return json(500, { error: "no_refresh_token" }, [expireRefresh()]);
  }
  return json(
    200,
    { access_token: answer.accessToken, expires_at: answer.expiresAt },
    [refreshCookie(answer)],
  );
}

function logout(): Response {
  return new Response(null, {
    status: 204,
    headers: headers([
      expireRefresh(),
      expire(LOGIN_COOKIE, LOGIN_PATH, "Lax"),
    ]),
  });
}

// --- GitHub -----------------------------------------------------------------

type TokenAnswer =
  | {
      kind: "tokens";
      accessToken: string;
      /** Milliseconds since the epoch. */
      expiresAt: number;
      refreshToken: string | undefined;
      refreshTtl: number;
    }
  | { kind: "refused"; error: string }
  | { kind: "unreachable" };

async function tokenRequest(
  env: AuthEnv,
  params: Record<string, string>,
): Promise<TokenAnswer> {
  const body = new URLSearchParams({
    client_id: env.GITHUB_CLIENT_ID,
    client_secret: env.GITHUB_CLIENT_SECRET,
    ...params,
  });
  let response: Response;
  let data: Record<string, unknown>;
  try {
    response = await fetch(`${GITHUB}/login/oauth/access_token`, {
      method: "POST",
      headers: {
        accept: "application/json",
        "content-type": "application/x-www-form-urlencoded",
      },
      body,
    });
    data = (await response.json()) as Record<string, unknown>;
  } catch {
    return { kind: "unreachable" };
  }
  // GitHub answers an OAuth error with 200 and an `error` field.
  if (typeof data.error === "string") {
    return { kind: "refused", error: data.error };
  }
  if (!response.ok || typeof data.access_token !== "string") {
    return { kind: "unreachable" };
  }
  const expiresIn = typeof data.expires_in === "number" ? data.expires_in : 0;
  return {
    kind: "tokens",
    accessToken: data.access_token,
    expiresAt: Date.now() + expiresIn * 1000,
    refreshToken:
      typeof data.refresh_token === "string" ? data.refresh_token : undefined,
    refreshTtl:
      typeof data.refresh_token_expires_in === "number"
        ? data.refresh_token_expires_in
        : REFRESH_TTL_SECONDS,
  };
}

// --- cookies and responses ----------------------------------------------------

function refreshCookie(answer: {
  refreshToken: string | undefined;
  refreshTtl: number;
}): string {
  return cookie(REFRESH_COOKIE, answer.refreshToken ?? "", {
    path: REFRESH_PATH,
    sameSite: "Strict",
    maxAge: answer.refreshTtl,
  });
}

function expireRefresh(): string {
  return expire(REFRESH_COOKIE, REFRESH_PATH, "Strict");
}

function expire(name: string, path: string, sameSite: SameSite): string {
  return cookie(name, "", { path, sameSite, maxAge: 0 });
}

type SameSite = "Strict" | "Lax";

function cookie(
  name: string,
  value: string,
  opts: { path: string; sameSite: SameSite; maxAge: number },
): string {
  return `${name}=${value}; Path=${opts.path}; Max-Age=${opts.maxAge}; HttpOnly; Secure; SameSite=${opts.sameSite}`;
}

function readCookie(request: Request, name: string): string | undefined {
  const header = request.headers.get("cookie");
  if (!header) return undefined;
  for (const part of header.split(";")) {
    const eq = part.indexOf("=");
    if (eq < 0) continue;
    if (part.slice(0, eq).trim() === name) {
      const value = part.slice(eq + 1).trim();
      return value === "" ? undefined : value;
    }
  }
  return undefined;
}

function readLoginCookie(
  request: Request,
): { state: string; verifier: string; back: string } | undefined {
  const value = readCookie(request, LOGIN_COOKIE);
  if (!value) return undefined;
  const [state, verifier, back] = value.split(".");
  if (!state) return undefined;
  let path = "/";
  try {
    path = returnPath(new TextDecoder().decode(unbase64url(back ?? "")));
  } catch {
    // A mangled return path is not worth failing a login over.
  }
  return { state, verifier: verifier ?? "", back: path };
}

/** A path on this site to come back to; anything else is `/`. */
function returnPath(raw: string | null): string {
  if (!raw?.startsWith("/")) return "/";
  // `//host` and `/\host` are other origins to a browser.
  if (raw.startsWith("//") || raw.startsWith("/\\")) return "/";
  if (raw === "/api" || raw.startsWith("/api/")) return "/";
  return raw;
}

/** A POST whose Origin is another site's: refused before touching the cookie. */
function crossOrigin(request: Request, url: URL): Response | undefined {
  const origin = request.headers.get("origin");
  if (origin && origin !== url.origin) {
    return json(403, { error: "cross_origin" });
  }
  return undefined;
}

function configured(value: string | undefined): value is string {
  return !!value && !value.startsWith("REPLACE_WITH_");
}

function headers(cookies: string[]): Headers {
  const h = new Headers({ "cache-control": "no-store" });
  for (const c of cookies) h.append("set-cookie", c);
  return h;
}

function redirect(location: string, cookies: string[]): Response {
  const h = headers(cookies);
  h.set("location", location);
  return new Response(null, { status: 302, headers: h });
}

function json(status: number, body: unknown, cookies: string[] = []): Response {
  const h = headers(cookies);
  h.set("content-type", "application/json");
  return new Response(JSON.stringify(body), { status, headers: h });
}

function notAllowed(allow: string): Response {
  const h = headers([]);
  h.set("allow", allow);
  return new Response(null, { status: 405, headers: h });
}

// --- encoding -----------------------------------------------------------------

function randomToken(): string {
  return base64url(crypto.getRandomValues(new Uint8Array(32)));
}

function base64url(bytes: Uint8Array): string {
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return btoa(binary)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function unbase64url(text: string): Uint8Array {
  const b64 = text.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(b64 + "=".repeat((4 - (b64.length % 4)) % 4));
  return Uint8Array.from(binary, (c) => c.charCodeAt(0));
}
