// Meldweb Curator's one worker: the built site as static assets, the four
// GitHub login routes, and the card scanner's engine files, proxied from the
// archive on ghcr.io. It stores nothing and never proxies api.github.com;
// the page calls GitHub itself with the access token these routes hand it.

import { type AuthEnv, handleAuth } from "./auth";
import { handleProbe, PREFIX, type ProbeContext } from "./probe";

export interface Env extends AuthEnv {
  /** The static assets binding from wrangler.toml's `[assets]`. */
  ASSETS: { fetch(request: Request): Promise<Response> };
}

export default {
  async fetch(
    request: Request,
    env: Env,
    ctx: ProbeContext = { waitUntil: () => {} },
  ): Promise<Response> {
    const url = new URL(request.url);
    if (url.pathname === "/api" || url.pathname.startsWith("/api/")) {
      return handleAuth(request, env);
    }
    if (url.pathname.startsWith(PREFIX)) {
      return handleProbe(request, env, ctx);
    }
    // `run_worker_first` sends only those two here, so this is a safety net for
    // a config without it: everything else is a file or an SPA route.
    return env.ASSETS.fetch(request);
  },
};
