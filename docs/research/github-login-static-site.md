# Logging in with GitHub and committing to one repo from a static site

Research for [#106](https://github.com/cramt/progress-engine/issues/106), under
the map [#105](https://github.com/cramt/progress-engine/issues/105). The
question: Meldweb Curator is a static Vite app plus at most a small Cloudflare
Worker. What is the smallest sound path from "log in with GitHub" to "saving
commits a `.deck.toml` to one repo"?

Sources are GitHub's and Cloudflare's own docs, read on 2026-09-27, plus
`curl` against the live endpoints where the docs are silent (marked
**observed**). Anything not checked either way is marked **unverified**.

## Recommendation

- **A GitHub App, not an OAuth App, with the web application flow + PKCE.**
  The user installs it on *only the deck repo*; the app asks for
  `Contents: read & write` and nothing else (Metadata read is implied).
  User access tokens expire after 8 hours and refresh for 6 months.
- **One Cloudflare Worker with static assets** serves the built site and three
  routes under `/api/auth/`. It holds the client secret, does the code
  exchange and the refresh, and keeps the refresh token in an `HttpOnly`
  cookie on its own origin. It never proxies API calls.
- **The browser calls `api.github.com` directly** with the access token, held
  in memory only. The API sends `Access-Control-Allow-Origin: *`.
- **Saving a single deck is one Contents API `PUT` carrying the blob `sha`
  the editor loaded.** A mismatch is the "the file moved underneath us"
  signal. Saves that touch several files in one commit (rename, delete + add)
  use GraphQL `createCommitOnBranch` with `expectedHeadOid`.
- **Deploy with `wrangler deploy`** from `nix build .#meldweb-web` output, the
  Worker in TypeScript beside it. Cloudflare says to start new projects on
  Workers, not Pages.

The sections below are the evidence for each point.

## Which kind of app

Three options were on the table: an OAuth App, a GitHub App, or the device
flow (which is a way to get a token, available to both).

**GitHub App.** It is the only one that can be scoped to one repo:

- At install, a user picks "All repositories" or "Only select repositories"
  and then chooses the repos
  ([installing a GitHub App from a third party](https://docs.github.com/en/apps/using-github-apps/installing-a-github-app-from-a-third-party)).
- A user access token "can only access resources that both the user and app
  can access": if the app is installed on repos A and B and the user can reach
  B and C, the token reaches only B
  ([generating a user access token](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app)).
- The code exchange also takes an optional `repository_id`, which narrows the
  token to that one repository (same page). Curator does not need it if the
  installation is already one repo, but it is there if a user installs on
  more.
- Permissions are fine-grained, not scopes: the token has "only permissions
  that both the user and the app have" (same page).

**OAuth App.** An authorized OAuth App "has access to all of the user's ...
accessible resources", and writing to a repo needs the `repo` scope, which is
every repo the user can write to
([differences between GitHub Apps and OAuth apps](https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/differences-between-github-apps-and-oauth-apps)).
There is no way to confine it to the deck repo. GitHub's own guidance is that
GitHub Apps are preferred in general (same page). Rejected.

**Device flow.** GitHub says not to enable it "unless you are using the app in
a constrained environment (CLIs, IoT devices, or headless systems)", because
it needs no redirect URI and so can be used to impersonate the app in
phishing
([best practices for creating a GitHub App](https://docs.github.com/en/apps/creating-github-apps/about-creating-github-apps/best-practices-for-creating-a-github-app)).
It would not remove the worker either: `github.com/login/device/code` and
`github.com/login/oauth/access_token` send no CORS headers (**observed**,
below), so a browser cannot call them. Rejected.

### App registration settings

From [registering a GitHub App](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/registering-a-github-app):

| Setting | Value | Why |
|---|---|---|
| Repository permissions | Contents: read & write | Read decks, commit decks. Metadata: read comes with it. |
| Account / org permissions | none | Nothing else is needed. Requesting none also lets repo admins install in an org without the owner ([installing](https://docs.github.com/en/apps/using-github-apps/installing-a-github-app-from-a-third-party)). |
| Callback URL | `https://<site>/api/auth/callback`, plus `http://localhost:5173/api/auth/callback` for dev | Up to 10 are allowed. |
| Expire user authorization tokens | on (the default) | GitHub "strongly recommends" it. |
| Request user authorization (OAuth) during installation | on | Install and log in become one trip for a new user. |
| Enable Device Flow | off | See above. |
| Where can this app be installed | Any account | Curator is for "the owner and people like them" (#105), not one account. |
| Webhook | off | Curator does not react to pushes. |

The app's private key is never generated or used. It mints installation
tokens, which act as the app's bot and not as the user; the best-practices page
says a client-side app "should not generate installation access tokens" and
should use user access tokens instead.

## What the worker does

The code exchange needs `client_secret`; the docs list it as **Required**
next to `code_verifier` for PKCE
([generating a user access token](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-user-access-token-for-a-github-app)).
Refreshing needs it too, "unless the user access token was generated using the
device flow"
([refreshing user access tokens](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/refreshing-user-access-tokens)).
GitHub's alternative for a pure SPA is to "ship the client secret in the
application's code" and rely on PKCE (best practices). That would let anyone
refresh any Curator user's stolen refresh token. With a worker available,
the secret stays in a Worker secret
([Workers secrets](https://developers.cloudflare.com/workers/configuration/secrets/)).

Three routes, all on the site's own origin, so none of them need CORS:

1. **`GET /api/auth/login`** generates `state` and a PKCE `code_verifier`,
   stores both in a short-lived `HttpOnly; Secure; SameSite=Lax` cookie, and
   redirects to `https://github.com/login/oauth/authorize?client_id=…&redirect_uri=…&state=…&code_challenge=…&code_challenge_method=S256`.
   `Lax` rather than `Strict` because the callback arrives as a top-level
   navigation from github.com.
2. **`GET /api/auth/callback?code&state`** checks `state` against the cookie,
   `POST`s `client_id`, `client_secret`, `code`, `redirect_uri` and
   `code_verifier` to `https://github.com/login/oauth/access_token` with
   `Accept: application/json`, and gets back `access_token` (`ghu_…`,
   `expires_in: 28800`) and `refresh_token` (`ghr_…`,
   `refresh_token_expires_in: 15897600`). It sets the refresh token as an
   `HttpOnly; Secure; SameSite=Strict; Path=/api/auth` cookie and redirects to
   the app. The access token is not put in a URL; the app fetches one from
   the refresh route on load, which costs one extra refresh per login and
   keeps the handoff to one mechanism.
3. **`POST /api/auth/refresh`** reads the cookie, `POST`s
   `grant_type=refresh_token` with the secret, rotates the cookie to the new
   refresh token, and returns `{ access_token, expires_at }` in the body. A
   cross-site page cannot trigger it with the cookie (`Strict`) or read the
   response (no CORS headers on it).

A fourth, `POST /api/auth/logout`, clears the cookie. Revoking the grant
itself (`DELETE /applications/{client_id}/grant`, basic auth with the secret)
is optional.

The worker never sees deck contents and never proxies `api.github.com`.
It is stateless: no KV, no database.

On the setup URL: GitHub warns that the `installation_id` it appends can be
spoofed and must be checked against a user token
([about the setup URL](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/about-the-setup-url)).
With "request user authorization during installation" on, the user lands on
the callback URL instead and the setup URL is not used. After login the app
calls `GET /user/installations` and
`GET /user/installations/{installation_id}/repositories` with the user token
(named in the token doc as the way to see what a token reaches) and takes the
repo from there.

## Where the token lives and how long it lasts

| Token | Lives in | Lifetime | Source |
|---|---|---|---|
| Access (`ghu_`) | JS memory in the page, never storage | 8 h, always `28800` s | token doc |
| Refresh (`ghr_`) | `HttpOnly` cookie on the worker's origin | 6 months, always `15897600` s | token doc |

- **Expiry.** The page refreshes a few minutes before `expires_at`, and on
  any `401` from the API retries once after a refresh. A reload loses the
  in-memory token and the page gets a fresh one from `/api/auth/refresh`,
  which is invisible to the user.
- **Refresh is single-use.** "Once you use a refresh token, that refresh token
  and the old user access token will no longer work" (refresh doc). Two tabs
  refreshing at once will race, and the loser's cookie is dead. Serialize
  refreshes across tabs with the Web Locks API (`navigator.locks.request`),
  or accept that the losing tab bounces through login.
- **Refresh expired (6 months idle) or grant revoked.** Refresh fails; the
  page sends the user through `/api/auth/login` again. Since the app is
  already installed and authorized this is a redirect with no prompt
  (**unverified**: GitHub normally skips the consent screen for an existing
  grant). A revoked grant makes API calls return `401 Bad Credentials` (token
  doc).
- **Unsaved edits across a forced re-login** are #105's "offline and
  multi-tab" item, not this ticket's. The page must not drop the edit buffer
  when it navigates to login; persist it first.

Why not `localStorage` for the access token: GitHub's advice for web apps is
to keep tokens server-side and to store refresh tokens separately from access
tokens (best practices). An `HttpOnly` cookie is the one place the page's own
JavaScript, and so an XSS, cannot read. The access token in memory is still
readable by an XSS for up to 8 hours, which is the accepted cost of a static
site calling GitHub directly.

## Committing: Contents API vs Git Data API

### Contents API: one request per save

`PUT /repos/{owner}/{repo}/contents/{path}` with `message`, base64
`content`, optional `branch`, and `sha`, "Required if you are updating a
file. The blob SHA of the file being replaced"
([repository contents](https://docs.github.com/en/rest/repos/contents#create-or-update-file-contents)).
Documented responses include `409 Conflict` and `422 Validation failed`.

- **Conflict detection is per file.** The editor keeps the blob `sha` it got
  from `GET …/contents/{path}` and sends it back. If someone pushed a change
  to *that file* since, the sha no longer matches and GitHub refuses. A push
  that touched other files does not block the save, which is what a deck
  editor wants. The docs list 409 without saying which of 409/422 a stale sha
  produces (**unverified**; the first build ticket should trigger it on a
  scratch repo and pin the code).
- **Creating a file** is the same `PUT` without `sha`. Sending no `sha` for a
  path that exists fails (**unverified** which status), so "new deck" can
  never silently overwrite.
- GitHub warns that concurrent `PUT`s and `DELETE`s on this endpoint
  conflict and must be serialized (same page). Curator saves one file at a
  time, so queue saves per repo.
- The committer defaults to the authenticated user, so a save shows up as the
  user's commit (attributed to the app as well, per the token doc).

### Git Data API: four writes per save

Blob, tree, commit, then
`PATCH /repos/{owner}/{repo}/git/refs/heads/{branch}` with `force: false`,
which "make[s] sure the update is a fast-forward"
([Git references](https://docs.github.com/en/rest/git/refs#update-a-reference)).
Conflict detection is **per branch**: any push since the base commit makes
the update non-fast-forward, even one to an unrelated file, and the client
must rebase its tree and retry. It costs four `POST`/`PATCH` requests (the
blob can be inlined in the tree for three) where the Contents API costs one.
Not worth it for Curator.

### GraphQL `createCommitOnBranch` for multi-file saves

When a save must be one commit over several paths (a rename is a delete plus
an add), the GraphQL mutation `createCommitOnBranch` takes `branch`,
`fileChanges` (additions and deletions), `message` and `expectedHeadOid`,
"the git commit oid expected at the head of the branch prior to the commit"
(read from the live schema with `gh api graphql` introspection on
`CreateCommitOnBranchInput`). That is the branch-level check in one request.
The GraphQL endpoint takes the same user token, and `api.github.com/graphql`
answers a preflight with `access-control-allow-origin: *` (**observed**).

### Listing decks

`GET /repos/{owner}/{repo}/git/trees/{branch}?recursive=1` returns every path
and blob sha in one request. The Contents API directory listing caps at 1,000
entries (contents doc); a deck repo will not hit that, but the tree call is
one request for any layout.

## CORS

| Endpoint | Browser can call it? | Evidence |
|---|---|---|
| `api.github.com` (REST) | Yes | "The REST API supports CORS for AJAX requests from any origin" ([CORS doc](https://docs.github.com/en/rest/using-the-rest-api/using-cors-and-jsonp-to-make-cross-origin-requests)). **Observed:** preflight on `/repos/…/git/refs/heads/main` returns `204`, `access-control-allow-origin: *`, methods `GET, POST, PATCH, PUT, DELETE`, headers including `Authorization`, `Content-Type`, `If-Match`, `X-GitHub-Api-Version`; `ETag` and the `X-RateLimit-*` headers are exposed. |
| `github.com/login/oauth/access_token` | No | **Observed:** `OPTIONS` returns `404` with no `Access-Control-*` headers; a `POST` with an `Origin` header returns no `Access-Control-Allow-Origin`. The docs do not mention CORS for it. |
| `api.github.com/graphql` | Yes | **Observed:** preflight `204`, `access-control-allow-origin: *`. |
| `github.com/login/device/code` | No | **Observed:** `404`, no CORS headers. |

So: API calls go browser → `api.github.com` directly; only the token
exchange and refresh need the worker.

## Rate limits

From [rate limits for the REST API](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api):

- **Primary:** a GitHub App user access token uses the user's own limit of
  5,000 requests/hour, shared with every other app and PAT acting for that
  user (15,000 if the app is owned by an Enterprise Cloud org, which Curator's
  won't be).
- **Secondary, the one that matters for saves:** "no more than 80
  content-generating requests per minute and no more than 500
  content-generating requests per hour". `POST`/`PATCH`/`PUT`/`DELETE` cost 5
  points against a 900 points/minute per-endpoint budget; at most 100
  concurrent requests.
- **Token requests:** at most 2,000 OAuth access token requests per hour per
  app. That counts every login and refresh across all Curator users. At one
  refresh per 8 hours per active tab it is far away, but the worker must not
  refresh on every page load when an unexpired token would do; the page keeps
  `expires_at` and asks only when needed.
- Exceeding a limit gives `403` or `429`; honour `retry-after` or
  `x-ratelimit-reset`.

Consequence for the save model: **a save is a commit, and commits are the
scarce resource (500/hour).** Saving on every keystroke or every drag would
hit the secondary limit in an editing session. Saving must be an explicit
action or a debounce of tens of seconds. That feeds the *What is a save?*
ticket. Reads are cheap: use `If-None-Match` with the `ETag` (exposed via
CORS above) so an unchanged tree or file answers `304`, which "does not count
against your primary rate limit" when the request carries `Authorization`
([best practices for the REST API](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)).

## Deploy target

**One Worker with static assets, not Pages plus a separate Worker.**
Cloudflare's Pages landing page now opens with "Are you sure you want to use
Pages? ... Start new projects with Workers"
([Cloudflare Pages](https://developers.cloudflare.com/pages/)), and static
asset requests on Workers are free as on Pages
([migrate from Pages](https://developers.cloudflare.com/workers/static-assets/migration-guides/migrate-from-pages/)).

From [Workers static assets](https://developers.cloudflare.com/workers/static-assets/):

```toml
name = "meldweb-curator"
main = "worker/index.ts"
compatibility_date = "2026-09-27"

[assets]
directory = "./result"                      # nix build .#meldweb-web
not_found_handling = "single-page-application"
run_worker_first = ["/api/*"]               # everything else is served without invoking the worker
```

- A request that matches a file is served without running the worker;
  `not_found_handling = "single-page-application"` returns `index.html` for
  TanStack Router's client routes; `run_worker_first` sends `/api/*` to the
  worker (same page).
- Serving the site and the auth routes from **one origin** is what makes the
  `SameSite=Strict` refresh cookie work and removes CORS from the worker
  entirely. Pages + a separate `*.workers.dev` worker would make the cookie
  third-party.
- **Secrets:** `GITHUB_CLIENT_SECRET` via `wrangler secret put`;
  `GITHUB_CLIENT_ID` is public and can be a `var`. Declare the secret under
  `secrets.required` so `wrangler deploy` fails if it is missing
  ([Workers secrets](https://developers.cloudflare.com/workers/configuration/secrets/)).
  Local dev reads `.dev.vars`, which must be gitignored.
- **From the flake:** `packages.meldweb-web` already builds the site to a
  store path (`flake.nix`). The worker is TypeScript (house rule: TypeScript
  for HTTP backends) and small enough to type-check inside the existing
  `meldweb-web` check. `wrangler` is in nixpkgs (4.132.0 at the time of
  writing, from `nix search`); add it to the devshell and deploy with
  `nix build .#meldweb-web && wrangler deploy`. CI deploys need a
  `CLOUDFLARE_API_TOKEN` secret, which is a manual step for the owner.
- **Dev:** `pnpm dev` alone has no `/api/auth`. Either run the worker with
  `wrangler dev` and proxy `/api` to it from Vite, or use the Cloudflare Vite
  plugin, which runs the worker inside Vite's dev server (named on the static
  assets page). The Vite plugin is the smaller change to `pnpm dev`.

## What this does not settle

- *Which repo holds a user's decks?* This doc assumes the user installs the
  app on one existing repo. Whether Curator can create that repo for a new
  user, and with what extra permission, is the other ticket's question and
  was not researched here.
- The exact status code for a stale `sha` and for a create-over-existing
  `PUT`: check them on a scratch repo in the first build
  ticket before writing the error handling.
