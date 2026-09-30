// The worker's GitHub login routes inside `pnpm dev`, so the dev server logs
// in through the real meldweb-curator GitHub App and edits a real repo. It is
// the worker's own `handleAuth`, not a copy, with its public values read from
// wrangler.toml and its client secret from wherever this machine keeps it.
//
// The app only redirects to callback URLs registered on its settings page, so
// DEV_ORIGIN's callback has to be one of them beside the production one.

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import { fileURLToPath } from "node:url";
import type { Plugin } from "vite";
import { type AuthEnv, handleAuth } from "../../worker/src/auth.ts";

export const DEV_PORT = 5173;
export const DEV_ORIGIN = `http://localhost:${DEV_PORT}`;

const worker = (file: string) =>
  fileURLToPath(new URL(`../../worker/${file}`, import.meta.url));

// The same reference `nix run .#infra` uploads the worker's secret from.
const SECRET_REF = "op://Homelab/MeldwebCurator/githubClientSecret";
const OPNIX_TOKEN = "/etc/opnix-token";

function wranglerVar(toml: string, name: string): string | undefined {
  return toml.match(new RegExp(`^${name}\\s*=\\s*"([^"]*)"`, "m"))?.[1];
}

/** The secret, and where it came from, or why there is none. */
function clientSecret(): { secret: string; from: string } | { why: string } {
  const env = process.env.GITHUB_CLIENT_SECRET;
  if (env) return { secret: env, from: "$GITHUB_CLIENT_SECRET" };
  const devVars = worker(".dev.vars");
  if (existsSync(devVars)) {
    const secret = readFileSync(devVars, "utf8").match(
      /^GITHUB_CLIENT_SECRET\s*=\s*"?([^"\n]*)"?/m,
    )?.[1];
    if (secret) return { secret, from: "worker/.dev.vars" };
  }
  try {
    const token = readFileSync(OPNIX_TOKEN, "utf8").trim();
    const secret = execFileSync("op", ["read", SECRET_REF], {
      env: { ...process.env, OP_SERVICE_ACCOUNT_TOKEN: token },
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
    if (secret) return { secret, from: "1Password" };
  } catch (e) {
    return {
      why: `no $GITHUB_CLIENT_SECRET, no worker/.dev.vars, and 1Password could not be read (${e instanceof Error ? e.message.split("\n")[0] : e})`,
    };
  }
  return { why: `${SECRET_REF} is empty` };
}

async function toRequest(req: IncomingMessage): Promise<Request> {
  const headers = new Headers();
  for (const [name, value] of Object.entries(req.headers)) {
    if (value === undefined) continue;
    for (const v of Array.isArray(value) ? value : [value])
      headers.append(name, v);
  }
  const chunks: Buffer[] = [];
  if (req.method !== "GET" && req.method !== "HEAD") {
    for await (const chunk of req) chunks.push(chunk as Buffer);
  }
  return new Request(new URL(req.url ?? "/", DEV_ORIGIN), {
    method: req.method ?? "GET",
    headers,
    ...(chunks.length > 0 ? { body: Buffer.concat(chunks) } : {}),
  });
}

async function send(res: ServerResponse, response: Response): Promise<void> {
  res.statusCode = response.status;
  response.headers.forEach((value, name) => {
    if (name !== "set-cookie") res.setHeader(name, value);
  });
  // Folded into one header, the login and refresh cookies would be one
  // unreadable cookie.
  const cookies = response.headers.getSetCookie();
  if (cookies.length > 0) res.setHeader("set-cookie", cookies);
  res.end(Buffer.from(await response.arrayBuffer()));
}

/**
 * Serves `/api/auth/*` from the worker's code in `pnpm dev`, unless
 * `VITE_MOCK_GITHUB` fakes GitHub in the page instead.
 */
export function devAuth(): Plugin {
  let enabled = false;
  return {
    name: "meldweb-dev-auth",
    config(_, { command, mode }) {
      enabled =
        command === "serve" && mode !== "test" && !process.env.VITE_MOCK_GITHUB;
      // The callback URL registered on the app names this port.
      if (enabled) return { server: { port: DEV_PORT, strictPort: true } };
    },
    configureServer(server) {
      if (!enabled) return;
      const toml = readFileSync(worker("wrangler.toml"), "utf8");
      const found = clientSecret();
      if ("why" in found) {
        server.config.logger.warn(
          `[meldweb] login is off: ${found.why}. Use VITE_MOCK_GITHUB=1 for the in-page fake.`,
        );
        return;
      }
      const env: AuthEnv = {
        GITHUB_CLIENT_ID: wranglerVar(toml, "GITHUB_CLIENT_ID") ?? "",
        GITHUB_APP_SLUG: wranglerVar(toml, "GITHUB_APP_SLUG") ?? "",
        GITHUB_CLIENT_SECRET: found.secret,
      };
      server.config.logger.info(
        `[meldweb] logging in through github.com/apps/${env.GITHUB_APP_SLUG}, secret from ${found.from}; its callback URLs need ${DEV_ORIGIN}/api/auth/callback`,
      );
      server.middlewares.use((req, res, next) => {
        if (!req.url?.startsWith("/api/")) return next();
        toRequest(req)
          .then((request) => handleAuth(request, env))
          .then((response) => send(res, response))
          .catch(next);
      });
    },
  };
}
