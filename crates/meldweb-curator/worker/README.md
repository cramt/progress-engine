# meldweb-worker

Meldweb Curator's one Cloudflare Worker ([#123], designed in
[github-login-static-site.md](../../../docs/research/github-login-static-site.md)).
It serves the built site as static assets and trades a GitHub App login for
tokens. It stores nothing: the refresh token lives in a cookie, the access
token in the page's memory. It never proxies `api.github.com`; the page calls
GitHub itself, which allows any origin.

[#123]: https://github.com/cramt/progress-engine/issues/123

## The page's contract

Every route is on the site's own origin, so none needs CORS. Every answer
carries `Cache-Control: no-store`; errors are JSON `{ "error": "<code>" }`.

| Request | Answer |
|---|---|
| `GET /api/auth/login?return=/path` | `302` to GitHub's authorize page. `return` is a path on this site (default `/`); anything else, including `//host` and `/api/…`, becomes `/`. Navigate the whole tab here, don't `fetch` it. |
| `GET /api/auth/callback?code&state` | GitHub sends the browser here. `302` back to `return`, having set the refresh cookie. No token in the URL. A user who declines on GitHub also lands on `return`, logged out. `400` for a forged state, a missing or wrong PKCE verifier, or a spent code. |
| `POST /api/auth/refresh` | `200 { "access_token": "ghu_…", "expires_at": 1790000000000 }` (`expires_at` is epoch milliseconds, 8 hours out), or `401` when logged out or the refresh token is spent (the cookie is then cleared: send the user to login). `502` when GitHub could not be reached; the cookie is kept, so retry. |
| `POST /api/auth/logout` | `204`, both cookies cleared. |
| `GET /api/auth/app` | `200 { "app_slug": "…", "install_url": "https://github.com/apps/…/installations/new" }`, for onboarding's install button. |

What the page does with it:

- **On load**, `POST /api/auth/refresh`. `200` is logged in; `401` is logged out.
- **Keep `expires_at`** and refresh a few minutes before it, and once on a
  `401` from the API. Not on every load when a token is still good: GitHub
  allows 2,000 token requests an hour per app, across every user.
- **Refresh tokens are single-use**, so two tabs refreshing at once lose one of
  them to a `401`. Serialise refreshes with
  `navigator.locks.request("meldweb-refresh", …)`.
- **Before navigating to login**, persist unsaved edits: the page is left.

### Faking it in dev

`pnpm dev` runs without the worker; nothing in Vite serves `/api/auth`. A fake
only has to answer the table above: `refresh` → `200` with any token and an
`expires_at` in the future (or `401` to test logged out), `logout` → `204`,
`login` → `302` straight back to `return`. The access token then has to be
something the dev GitHub mock accepts.

## Cookies

| Cookie | Holds | Attributes |
|---|---|---|
| `meldweb_login` | state, PKCE verifier, return path | `HttpOnly; Secure; SameSite=Lax; Path=/api/auth/callback; Max-Age=600`. Lax because the callback is a top-level navigation from github.com. |
| `meldweb_refresh` | GitHub's refresh token (`ghr_…`) | `HttpOnly; Secure; SameSite=Strict; Path=/api/auth; Max-Age=` GitHub's `refresh_token_expires_in` (6 months). Rotated on every refresh. |

`POST` routes also refuse (`403`) a request whose `Origin` is another site.

## Configuration

`wrangler.toml` holds the public values from the GitHub App's settings
([#125], [meldweb-curator](https://github.com/apps/meldweb-curator)), and the
worker's name and Cloudflare account, which the tofu stack reads from it:

- `GITHUB_CLIENT_ID`: the app's client ID. While it is a `REPLACE_WITH_…`
  placeholder, `/api/auth/login` answers `503`.
- `GITHUB_APP_SLUG`: the app's URL name, for `/api/auth/app`.

The client secret is `GITHUB_CLIENT_SECRET` in the repo root's gitignored
`secrets.env` (copy `secrets.env.example`), never in a committed file. Every
deploy uploads it as a Worker secret.

The GitHub App's callback URL is `https://meldweb.cramt.dk/api/auth/callback`
(the worker builds `redirect_uri` from the request's own origin).

[#125]: https://github.com/cramt/progress-engine/issues/125

## Deploying

The Cloudflare side is an OpenTofu stack written in terranix,
[`../infra/`](../infra/): the worker and site, the client secret and the
`meldweb.cramt.dk` custom domain. From the repo root:

```
nix run .#infra -- plan
nix run .#infra -- apply
```

`apply` redeploys whenever the Nix build of the worker or site changes, or the
secret does. State is in the R2 bucket `cramt-tofu-state` on the cramt
account; `infra` derives its credentials from `CLOUDFLARE_API_TOKEN`.

One-time setup, before the first apply:

1. Enable R2 on the cramt account and create the bucket `cramt-tofu-state`
   (tofu can't create the bucket that holds its own state).
2. Create an account API token with the scopes `secrets.env.example` lists.
3. Fill in `secrets.env`.

`nix run .#deploy-curator` ships only the worker and site, uploading the
secret when `GITHUB_CLIENT_SECRET` is set in the environment. It copies both
into a temporary directory, so it touches nothing in the repo. `wrangler`
comes from nixpkgs (it is in `nix develop`), not npm.

`[assets]` serves the site with
`not_found_handling = "single-page-application"`, so a reload on a deck's URL
gets `index.html`, and `run_worker_first = ["/api/*"]` is the only traffic that
runs the worker.

To run the real worker locally instead, `pnpm build`, point the gitignored
`site` symlink at the web build
(`ln -sfn ../web/dist crates/meldweb-curator/worker/site`), put
`GITHUB_CLIENT_SECRET=…` in a gitignored `.dev.vars` beside `wrangler.toml`,
and `wrangler dev --config crates/meldweb-curator/worker/wrangler.toml`. The
GitHub App needs `http://localhost:8787/api/auth/callback` among its callback
URLs for that.

## Testing

`pnpm test` (vitest) runs the four routes end to end against a mocked GitHub
token endpoint that enforces single-use codes, S256 PKCE and single-use
refresh tokens, and fails any call to another GitHub URL. `pnpm check` is
biome and tsc. Both run from the repo root for every workspace package, and in
the flake's `meldweb-web` check.
