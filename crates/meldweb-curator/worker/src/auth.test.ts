import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import worker, { type Env } from "./index";

const SITE = "https://curator.example";
// The auth routes never wait on anything after answering.
const ctx = { waitUntil() {} };
const CLIENT_ID = "Iv1.testclient";
const SECRET = "test-secret";

const env: Env = {
  GITHUB_CLIENT_ID: CLIENT_ID,
  GITHUB_CLIENT_SECRET: SECRET,
  GITHUB_APP_SLUG: "meldweb-curator-test",
  ASSETS: {
    fetch: async (request) =>
      new Response(`asset ${new URL(request.url).pathname}`),
  },
};

// --- a GitHub that behaves like the real token endpoint -----------------------

async function s256(verifier: string): Promise<string> {
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(verifier),
  );
  return btoa(String.fromCharCode(...new Uint8Array(digest)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

class MockGitHub {
  codes = new Map<string, { challenge: string; redirectUri: string }>();
  refreshTokens = new Set<string>();
  calls: string[] = [];
  down = false;
  private n = 0;

  /** The user clicks "Authorize" on the page /api/auth/login sent them to. */
  approve(authorizeUrl: string): string {
    const url = new URL(authorizeUrl);
    expect(url.origin + url.pathname).toBe(
      "https://github.com/login/oauth/authorize",
    );
    expect(url.searchParams.get("client_id")).toBe(CLIENT_ID);
    expect(url.searchParams.get("code_challenge_method")).toBe("S256");
    const redirectUri = url.searchParams.get("redirect_uri") ?? "";
    const code = `code-${++this.n}`;
    this.codes.set(code, {
      challenge: url.searchParams.get("code_challenge") ?? "",
      redirectUri,
    });
    const back = new URL(redirectUri);
    back.searchParams.set("code", code);
    back.searchParams.set("state", url.searchParams.get("state") ?? "");
    return back.toString();
  }

  private issue(): Record<string, unknown> {
    const n = ++this.n;
    const refresh = `ghr_${n}`;
    this.refreshTokens.add(refresh);
    return {
      access_token: `ghu_${n}`,
      expires_in: 28800,
      refresh_token: refresh,
      refresh_token_expires_in: 15897600,
      token_type: "bearer",
      scope: "",
    };
  }

  fetch = async (
    input: RequestInfo | URL,
    init?: RequestInit,
  ): Promise<Response> => {
    const request = new Request(input, init);
    this.calls.push(`${request.method} ${request.url}`);
    if (this.down) throw new TypeError("network down");
    if (
      request.method !== "POST" ||
      request.url !== "https://github.com/login/oauth/access_token"
    ) {
      throw new Error(`the worker must not call ${request.url}`);
    }
    const form = new URLSearchParams(await request.text());
    const answer = (body: Record<string, unknown>) =>
      Response.json(body, { status: 200 });
    if (
      form.get("client_id") !== CLIENT_ID ||
      form.get("client_secret") !== SECRET
    ) {
      return answer({ error: "incorrect_client_credentials" });
    }
    if (form.get("grant_type") === "refresh_token") {
      const token = form.get("refresh_token") ?? "";
      // Single use: a refresh token is gone the moment it is spent.
      if (!this.refreshTokens.delete(token)) {
        return answer({ error: "bad_refresh_token" });
      }
      return answer(this.issue());
    }
    const grant = this.codes.get(form.get("code") ?? "");
    this.codes.delete(form.get("code") ?? "");
    if (
      !grant ||
      grant.redirectUri !== form.get("redirect_uri") ||
      grant.challenge !== (await s256(form.get("code_verifier") ?? ""))
    ) {
      return answer({ error: "bad_verification_code" });
    }
    return answer(this.issue());
  };
}

// --- a browser's cookie jar, enough of one ------------------------------------

type Stored = {
  value: string;
  path: string;
  attrs: string;
};

class Browser {
  jar = new Map<string, Stored>();

  async request(
    method: string,
    path: string,
    headers: Record<string, string> = {},
  ): Promise<Response> {
    const url = new URL(path, SITE);
    const cookies = [...this.jar]
      .filter(([, c]) => url.pathname.startsWith(c.path))
      .map(([name, c]) => `${name}=${c.value}`)
      .join("; ");
    const h = new Headers(headers);
    if (cookies) h.set("cookie", cookies);
    const response = await worker.fetch(
      new Request(url, { method, headers: h }),
      env,
      ctx,
    );
    for (const line of response.headers.getSetCookie()) {
      const [pair = "", ...attrs] = line.split("; ");
      const eq = pair.indexOf("=");
      const name = pair.slice(0, eq);
      const path = /(?:^|; )Path=([^;]+)/.exec(line)?.[1] ?? "/";
      if (/Max-Age=0(?:;|$)/.test(line)) this.jar.delete(name);
      else
        this.jar.set(name, {
          value: pair.slice(eq + 1),
          path,
          attrs: attrs.join("; "),
        });
    }
    return response;
  }

  get(path: string, headers?: Record<string, string>) {
    return this.request("GET", path, headers);
  }

  post(path: string, headers?: Record<string, string>) {
    return this.request("POST", path, headers);
  }

  /** Follow a redirect off-site or on-site, like a top-level navigation. */
  async navigate(location: string): Promise<Response> {
    const url = new URL(location);
    expect(url.origin).toBe(SITE);
    return this.get(url.pathname + url.search);
  }
}

let github: MockGitHub;
let browser: Browser;

beforeEach(() => {
  github = new MockGitHub();
  browser = new Browser();
  vi.stubGlobal("fetch", github.fetch);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

async function loggedIn(back = "/decks/lantern.deck.toml"): Promise<Response> {
  const login = await browser.get(
    `/api/auth/login?return=${encodeURIComponent(back)}`,
  );
  expect(login.status).toBe(302);
  return browser.navigate(github.approve(login.headers.get("location") ?? ""));
}

// --- the tests ------------------------------------------------------------------

describe("login → callback → refresh → logout", () => {
  it("walks the whole flow", async () => {
    const login = await browser.get(
      "/api/auth/login?return=/decks/lantern.deck.toml",
    );
    expect(login.status).toBe(302);
    const authorize = new URL(login.headers.get("location") ?? "");
    expect(authorize.searchParams.get("redirect_uri")).toBe(
      `${SITE}/api/auth/callback`,
    );
    const flow = browser.jar.get("meldweb_login");
    expect(flow?.path).toBe("/api/auth/callback");
    expect(flow?.attrs).toContain("HttpOnly");
    expect(flow?.attrs).toContain("Secure");
    expect(flow?.attrs).toContain("SameSite=Lax");

    const callback = await browser.navigate(github.approve(authorize.href));
    expect(callback.status).toBe(302);
    expect(callback.headers.get("location")).toBe("/decks/lantern.deck.toml");
    // No token in the URL: the page asks /refresh for one.
    expect(callback.headers.get("location")).not.toContain("ghu_");
    expect(browser.jar.has("meldweb_login")).toBe(false);
    const cookie = browser.jar.get("meldweb_refresh");
    expect(cookie?.value).toBe("ghr_2");
    expect(cookie?.path).toBe("/api/auth");
    expect(cookie?.attrs).toContain("HttpOnly");
    expect(cookie?.attrs).toContain("Secure");
    expect(cookie?.attrs).toContain("SameSite=Strict");
    expect(cookie?.attrs).toContain("Max-Age=15897600");

    const before = Date.now();
    const refreshed = await browser.post("/api/auth/refresh", {
      origin: SITE,
    });
    expect(refreshed.status).toBe(200);
    expect(refreshed.headers.get("cache-control")).toBe("no-store");
    const body = (await refreshed.json()) as {
      access_token: string;
      expires_at: number;
    };
    expect(body.access_token).toBe("ghu_3");
    expect(body.expires_at).toBeGreaterThanOrEqual(before + 28800 * 1000);
    expect(body.expires_at).toBeLessThanOrEqual(Date.now() + 28800 * 1000);
    // Rotated: the cookie now holds the new refresh token.
    expect(browser.jar.get("meldweb_refresh")?.value).toBe("ghr_3");

    const again = await browser.post("/api/auth/refresh");
    expect(again.status).toBe(200);
    expect(
      ((await again.json()) as { access_token: string }).access_token,
    ).toBe("ghu_4");

    const logout = await browser.post("/api/auth/logout");
    expect(logout.status).toBe(204);
    expect(browser.jar.has("meldweb_refresh")).toBe(false);

    const after = await browser.post("/api/auth/refresh");
    expect(after.status).toBe(401);

    // Only ever the token endpoint: the worker is not an API proxy.
    expect(new Set(github.calls)).toEqual(
      new Set(["POST https://github.com/login/oauth/access_token"]),
    );
  });

  it("the verifier is a real S256 PKCE pair GitHub can check", async () => {
    // The mock refuses unless sha256(code_verifier) matches the challenge.
    const callback = await loggedIn("/");
    expect(callback.status).toBe(302);
    expect(callback.headers.get("location")).toBe("/");
  });
});

describe("the callback refuses", () => {
  it("a state that is not the one login issued", async () => {
    const login = await browser.get("/api/auth/login");
    const back = new URL(github.approve(login.headers.get("location") ?? ""));
    back.searchParams.set("state", "forged");
    const callback = await browser.navigate(back.href);
    expect(callback.status).toBe(400);
    expect(await callback.json()).toEqual({ error: "state_mismatch" });
    expect(browser.jar.has("meldweb_refresh")).toBe(false);
    expect(browser.jar.has("meldweb_login")).toBe(false);
    expect(github.calls).toEqual([]);
  });

  it("a callback with no login in progress", async () => {
    const callback = await browser.get("/api/auth/callback?code=x&state=y");
    expect(callback.status).toBe(400);
    expect(await callback.json()).toEqual({ error: "no_login_in_progress" });
    expect(github.calls).toEqual([]);
  });

  it("a PKCE verifier that does not match the challenge", async () => {
    const login = await browser.get("/api/auth/login");
    const back = github.approve(login.headers.get("location") ?? "");
    const flow = browser.jar.get("meldweb_login");
    if (!flow) throw new Error("no login cookie");
    const [state, , path] = flow.value.split(".");
    flow.value = [state, "a-different-verifier", path].join(".");

    const callback = await browser.navigate(back);
    expect(callback.status).toBe(400);
    expect(await callback.json()).toEqual({ error: "bad_verification_code" });
    expect(browser.jar.has("meldweb_refresh")).toBe(false);
  });

  it("a login cookie with no PKCE verifier", async () => {
    const login = await browser.get("/api/auth/login");
    const back = github.approve(login.headers.get("location") ?? "");
    const flow = browser.jar.get("meldweb_login");
    if (!flow) throw new Error("no login cookie");
    flow.value = flow.value.split(".")[0] ?? "";

    const callback = await browser.navigate(back);
    expect(callback.status).toBe(400);
    expect(await callback.json()).toEqual({ error: "no_code_verifier" });
    expect(github.calls).toEqual([]);
    expect(browser.jar.has("meldweb_refresh")).toBe(false);
  });

  it("a code used twice", async () => {
    const login = await browser.get("/api/auth/login");
    const back = github.approve(login.headers.get("location") ?? "");
    const saved = browser.jar.get("meldweb_login");
    expect((await browser.navigate(back)).status).toBe(302);
    if (saved) browser.jar.set("meldweb_login", saved);
    const replay = await browser.navigate(back);
    expect(replay.status).toBe(400);
  });

  it("but sends a user who declined back to the app, logged out", async () => {
    const login = await browser.get("/api/auth/login?return=/decks/x");
    const state = new URL(login.headers.get("location") ?? "").searchParams.get(
      "state",
    );
    const callback = await browser.get(
      `/api/auth/callback?error=access_denied&state=${state}`,
    );
    expect(callback.status).toBe(302);
    expect(callback.headers.get("location")).toBe("/decks/x");
    expect(browser.jar.size).toBe(0);
    expect(github.calls).toEqual([]);
  });

  it("but turns GitHub's after-install redirect into a fresh login, never spending its code", async () => {
    const callback = await browser.get(
      "/api/auth/callback?code=x&installation_id=1&setup_action=install",
    );
    expect(callback.status).toBe(302);
    expect(callback.headers.get("location")).toBe("/api/auth/login?return=%2F");
    expect(github.calls).toEqual([]);
    expect(browser.jar.has("meldweb_refresh")).toBe(false);
  });
});

describe("refresh", () => {
  it("with a refresh token already spent: 401 and the cookie is cleared", async () => {
    await loggedIn();
    const spent = browser.jar.get("meldweb_refresh");
    if (!spent) throw new Error("no refresh cookie");

    // Another tab refreshes first and wins.
    const other = new Browser();
    other.jar.set("meldweb_refresh", { ...spent });
    expect((await other.post("/api/auth/refresh")).status).toBe(200);

    const loser = await browser.post("/api/auth/refresh");
    expect(loser.status).toBe(401);
    expect(await loser.json()).toEqual({ error: "bad_refresh_token" });
    expect(loser.headers.getSetCookie()).toEqual([
      "meldweb_refresh=; Path=/api/auth; Max-Age=0; HttpOnly; Secure; SameSite=Strict",
    ]);
    expect(browser.jar.has("meldweb_refresh")).toBe(false);
  });

  it("without a cookie: 401 and GitHub is not asked", async () => {
    const response = await browser.post("/api/auth/refresh");
    expect(response.status).toBe(401);
    expect(github.calls).toEqual([]);
  });

  it("when GitHub is unreachable: 502 and the cookie is kept", async () => {
    await loggedIn();
    github.down = true;
    const response = await browser.post("/api/auth/refresh");
    expect(response.status).toBe(502);
    expect(response.headers.getSetCookie()).toEqual([]);
    expect(browser.jar.get("meldweb_refresh")?.value).toBe("ghr_2");
  });

  it("refuses a GET and a cross-site POST", async () => {
    await loggedIn();
    expect((await browser.get("/api/auth/refresh")).status).toBe(405);
    const cross = await browser.post("/api/auth/refresh", {
      origin: "https://evil.example",
    });
    expect(cross.status).toBe(403);
    expect(github.calls).toHaveLength(1);
  });
});

describe("logout", () => {
  it("clears both cookies, even mid-login", async () => {
    await browser.get("/api/auth/login");
    browser.jar.set("meldweb_refresh", {
      value: "ghr_x",
      path: "/api/auth",
      attrs: "",
    });
    const response = await browser.post("/api/auth/logout");
    expect(response.status).toBe(204);
    expect(response.headers.getSetCookie().sort()).toEqual([
      "meldweb_login=; Path=/api/auth/callback; Max-Age=0; HttpOnly; Secure; SameSite=Lax",
      "meldweb_refresh=; Path=/api/auth; Max-Age=0; HttpOnly; Secure; SameSite=Strict",
    ]);
    expect(browser.jar.size).toBe(0);
  });
});

describe("login", () => {
  it.each([
    ["//evil.example/x", "/"],
    ["/\\evil.example", "/"],
    // Browsers strip tab, newline and CR from a URL, so these are `//evil`.
    ["/\t/evil.example", "/"],
    // Still encoded, it is a path segment on this site, not a stripped tab.
    ["/%09/evil.example", "/%09/evil.example"],
    ["/\n/evil.example", "/"],
    ["/\r/evil.example", "/"],
    ["/\r\n/evil.example", "/"],
    ["/\t\\evil.example", "/"],
    ["/api/../api/auth/logout", "/"],
    ["/decks/a%20b.deck.toml#top", "/decks/a%20b.deck.toml#top"],
    ["https://evil.example", "/"],
    ["/api/auth/logout", "/"],
    ["/decks/loam.deck.toml?tab=stats", "/decks/loam.deck.toml?tab=stats"],
  ])("returns to %s as %s", async (asked, landed) => {
    const callback = await loggedIn(asked);
    expect(callback.headers.get("location")).toBe(landed);
  });

  it("answers 503 until the app's client id is filled in", async () => {
    const response = await worker.fetch(
      new Request(`${SITE}/api/auth/login`),
      {
        ...env,
        GITHUB_CLIENT_ID: "REPLACE_WITH_GITHUB_APP_CLIENT_ID",
      },
      ctx,
    );
    expect(response.status).toBe(503);
  });
});

describe("the rest of the site", () => {
  it("names the app so the page can link its install page", async () => {
    const response = await browser.get("/api/auth/app");
    expect(await response.json()).toEqual({
      app_slug: "meldweb-curator-test",
      install_url:
        "https://github.com/apps/meldweb-curator-test/installations/new",
    });
  });

  it("is static assets", async () => {
    const response = await browser.get("/decks/lantern.deck.toml");
    expect(await response.text()).toBe("asset /decks/lantern.deck.toml");
  });

  it("has no other api routes", async () => {
    expect((await browser.get("/api/repos/x")).status).toBe(404);
  });
});
