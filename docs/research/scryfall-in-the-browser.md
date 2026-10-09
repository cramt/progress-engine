# Scryfall in the browser: what Curator can ask it directly

> **Superseded by [ADR-0030](../adr/0030-curator-answers-card-facts-from-its-own-copy-of-scryfalls-bulk-data.md):** Curator now keeps its own
> copy of Scryfall's bulk data in each browser and answers card facts from it,
> falling back to the API below until the copy is ready. The measurements here
> still hold; the recommendation does not.

Question ([#107](https://github.com/cramt/progress-engine/issues/107)): quick
add, the search overlay, the printing picker and the all-printings grid need
card data. Can a static Meldweb Curator call Scryfall's API from the browser
for all of it, or must it ship `chip-scryfall` and a card index as wasm?

**Answer: call Scryfall directly.** Every endpoint the editor needs sends
`Access-Control-Allow-Origin: *`, the rate limits fit typing if searches are
debounced, and smart filters are plain query text. Shipping an index would cost
a 24.6 MB download per visitor for oracle data alone (78.6 MB with printings),
a daily republishing pipeline, and it would still miss images and most of the
syntax users paste from scryfall.com.

Measured on 2026-09-27 with curl from a Linux machine, against the live API.
Doc quotes come from the pages linked beside them, fetched the same day.

## CORS: every endpoint answers `*`

Each request below carried `Origin: https://example.pages.dev`. All returned
HTTP 200 with the same CORS block:

```
access-control-allow-origin: *
access-control-allow-methods: GET, POST, DELETE, OPTIONS
access-control-allow-headers: Accept, Accept-Charset, ..., Content-Type, ..., User-Agent, X-Requested-With
access-control-max-age: 300
```

| Request | Used for | Status | ACAO |
|---|---|---|---|
| `GET /cards/autocomplete?q=sol ri` | quick add | 200 | `*` |
| `GET /cards/search?q=lightning bolt&unique=prints` | search overlay, all printings | 200 | `*` |
| `GET /cards/named?fuzzy=sol ring` | resolve a typed name | 200 | `*` |
| `GET /cards/cmm/410` | one printing | 200 | `*` |
| `POST /cards/collection` | the deck view (already shipped) | 200 | `*` |
| `OPTIONS /cards/collection` (preflight, `content-type`) | | 200 | `*` |
| `OPTIONS /cards/search` (preflight, `accept`) | | 200 | `*` |
| `GET /catalog/card-names`, `/symbology`, `/bulk-data` | | 200 | `*` |
| `HEAD cards.scryfall.io/normal/front/...jpg` | card images | 200 | `*` |
| `HEAD data.scryfall.io/oracle-cards/...jsonl.gz` | bulk files | 200 | `*` |

This matches the docs. [CORS and CSP](https://scryfall.com/docs/api/http-concerns):
"api.scryfall.com, as well as all of the Scryfall image origins set CORS headers
for GET, HEAD, POST, OPTIONS requests." The same page gives the CSP to merge if
Curator ever sets one: `connect-src api.scryfall.com`, `img-src *.scryfall.io`.

Headers: [the API overview](https://scryfall.com/docs/api) requires
`User-Agent` and `Accept`, but "If you are accessing the API via on-page web
browser JavaScript, keep the browser's User-Agent intact", and `Accept: */*` is
explicitly acceptable. A plain `fetch` already complies.

## Rate limits against typing speed

From [Rate Limits](https://scryfall.com/docs/api/rate-limits):

| Endpoint | Limit |
|---|---|
| `/cards/search`, `/cards/named`, `/cards/random`, `/cards/collection` | 2/second (500 ms) |
| `/cards/manifest` | 10/minute |
| everything else, including `/cards/autocomplete` | 10/second (100 ms) |
| `*.scryfall.io` (images, bulk files) | no limit |

"Recieving an HTTP 429 response will result in your access being limited for
30 seconds", and "It is not acceptable to ignore HTTP 429 responses."

What that means per feature:

- **Quick add** uses autocomplete at 10/s. A fast typist makes 6-8 keystrokes a
  second, so a 150 ms debounce keeps one tab under the limit with room to
  spare. The docs describe the endpoint as "designed for creating assistive UI
  elements that allow users to free-type card names". It returns up to 20
  names, nearest first, and returns an empty catalog below 2 characters rather
  than erroring ([autocomplete](https://scryfall.com/docs/api/cards/autocomplete)).
- **Search overlay** uses `/cards/search` at 2/s. Search-as-you-type must be
  debounced to at least 500 ms, or the overlay searches on Enter. Paging (175
  cards per page) goes through the same 2/s budget.
- **Printing picker and all-printings grid** each cost one search
  (`unique=prints`), plus a page per 175 printings. Sol Ring has 145, so one
  request.
- The docs do not say what the limit is keyed on. Browser requests carry no
  app key, so it can only be the visitor's IP or browser, which gives each
  visitor their own budget. One client-side queue per endpoint class (500 ms and 100 ms) is
  enough to never send a 429-worthy burst.

**Existing bug:** `web/src/scryfall.ts` waits 100 ms between `/cards/collection`
batches with the comment "Scryfall asks for 50-100 ms between requests". The
current limit for that endpoint is 500 ms. A 100-card deck is two batches, so
it has not bitten yet, but it breaks the stated limit. Fix it with the queue.

## Latency

Measured with curl from a home connection, one request at a time:

| Request | Wall time | Notes |
|---|---|---|
| autocomplete, 20 prefixes | 150-280 ms | Cloudflare HIT/MISS/REVALIDATED all in this range |
| autocomplete, two outliers earlier | 1.6 s, 2.3 s | uncached, not reproduced on re-run |
| search, 6 typical deck queries | 0.33-1.76 s | `otag:ramp mv<=2 id<=g` was the slow one |
| search, 175-card page, body size | 0.9 MB raw, 106 KB gzipped | browsers send `Accept-Encoding` |

Autocomplete and search responses carry `cache-control: public,
max-age=57600` (16 h), so the browser's HTTP cache answers a repeated query
with no request at all.

A local wasm index would answer in milliseconds once loaded. That is the only
axis on which it wins, and it pays for it with the load (next section).

## Smart filters are query text, and they need parentheses

Archidekt's *Apply smart filters* adds the deck's format and colour identity
to every query. Scryfall's syntax has both: `id<=WG` and `f:commander`.
Appending works, but only if the user's query is wrapped first, because `or`
binds looser than implicit `and`:

| `q=` | total_cards |
|---|---|
| `(o:draw or o:scry) id<=WG f:commander` | 1034 |
| `o:draw or o:scry id<=WG f:commander` | 3881 |

The second applies the filters to `o:scry` only. The rule for Curator is
`q = "(" + userQuery + ") id<=" + identity + " f:" + format`. The identity is
the commander's, which `chip-decklist`'s `commander` already knows.
`(t:artifact mv<=2 o:"add {C}") id<=WG f:commander` narrows 52 cards to 36 as
expected. Autocomplete takes no filters: `q=sol ri id<=w` returns nothing, so
quick add stays unfiltered, as it is on Archidekt.

## Errors Curator has to show

From [Errors](https://scryfall.com/docs/api/errors) and measurement:

- **No match** is HTTP 404 with `code: "not_found"`, not an empty list. A
  malformed term such as `mv<<2` also comes back as a 404 no-match.
- **Unknown or unsupported terms are dropped, with a warning, on a 200.**
  `t:artifact foo:bar` returned 3863 cards and
  `warnings: ["Invalid expression “foo:bar” was ignored. Unknown keyword “foo”."]`.
  Only when *every* term is dropped is it a 400 (`is:slick cmc>cmc`: "All of
  your terms were ignored."). The overlay must render `warnings`, or a typo
  silently widens the search.
- The API search is stricter than the website: no automatic retry with
  `include:extras` or `lang:any`, no spelling help, no set-page redirect
  ([search](https://scryfall.com/docs/api/cards/search), "Missing Luxuries").

## The printing picker and the all-printings grid

Every card object carries `prints_search_uri`, for Sol Ring
`/cards/search?order=released&q=oracleid:6ad8011d-...&unique=prints`. It is
one request that returns every printing with its `set`, `set_name`,
`collector_number`, `released_at`, `image_uris` and prices, newest first. That
is exactly the grid Archidekt shows. Use the URI Scryfall gives rather than
rebuilding it: `!"Sol Ring"` with `unique=prints` returned 139 printings
against `oracleid:`'s 145, because the name form misses the reversible
`Sol Ring // Sol Ring` printings.

Images come from `cards.scryfall.io`, which has no rate limit, sends
`cache-control: max-age=31556952` (a year) and `*` CORS.

## The alternative: chip-scryfall and bulk data in wasm

Bulk file sizes, from `GET /bulk-data` (`compressed_size`) and checked with
`HEAD` on each `jsonl_download_uri`:

| File | Compressed | Needed for |
|---|---|---|
| Oracle Cards | 24.6 MB (203 MB of JSONL, 38,690 lines) | search by oracle facts |
| Default Cards | 78.6 MB | printings with images |
| All Cards | 393 MB | every language |
| Oracle Tags | 6.0 MB | `otag:` |
| Unique Artwork | 37.8 MB | |

The format changed since the index was first built: [bulk data](https://scryfall.com/docs/api/bulk-data)
now lists `jsonl_download_uri` and `compressed_size` only, and "Each bulk file
is a gzipped JSONL (JSON Lines) archive". The files are served as
`content-type: application/gzip` with no `Content-Encoding`, so a browser would
have to decompress them itself with `DecompressionStream("gzip")`.

Three ways to ship an index, and why none is better than the API:

1. **The browser downloads Scryfall's bulk files.** 24.6 MB before the first
   search, 103 MB with printings and tags, then 203 MB of JSON parsed into wasm
   memory. The data origin has no rate limit, but this is a cold start no deck
   editor should have.
2. **Curator ships `decks/index.jsonl`.** It is 28.5 MB raw, 5.5 MB with
   `gzip -9`, and already reduced to seventeen fields plus a printing-to-name
   map (35,004 cards, 112,071 printings). It has no image URLs, set names,
   release dates or prices, so the printing picker and grid still need the API.
   Keeping it fresh needs a scheduled job that runs `gauntlet sync` and
   republishes, and the API terms say "You may not simply repackage, republish,
   or proxy Scryfall data"
   ([overview](https://scryfall.com/docs/api)). Bundling a reduced index inside
   an editor is arguably "additional value", but it is a question the API route
   never has to ask.
3. **A worker proxies or caches Scryfall.** The map allows at most a small
   worker, for the token exchange. A card proxy would be the thing the terms
   forbid by name, and it moves the rate limit from each visitor's IP to one
   shared IP.

Freshness: the API reflects Scryfall's incremental updates. Bulk files are
regenerated "once every 12-24 hours", and an index is as old as its last sync.

Coverage: `chip-scryfall` is "Deliberately a subset" (its `lib.rs`). It knows
25 key families (`t o fo name kw otag cat mv c id produces pow tou pt loy def
r s f banned restricted m devotion layout is`) and errors on anything else.
Users will paste `a:`, `year:`, `usd<`, `game:paper`, `cn:`, `frame:`,
`is:foil`, `unique:prints` and `order:` from scryfall.com, and a subset parser
refuses all of them.

## Recommendation

**Curator calls Scryfall directly from the browser for every card fact, and
ships no card index.**

- Quick add: `/cards/autocomplete`, 150 ms debounce, no filters.
- Search overlay: `/cards/search` with `(user query) id<=<commander identity>
  f:<format>` when smart filters are on. Search on Enter or debounce at 500 ms
  or more. Show `warnings` and treat 404 as "no results".
- Printing picker and all-printings grid: the card's `prints_search_uri`.
- Deck view: `/cards/collection`, as now, with its delay raised to 500 ms.
- One request queue per rate class (500 ms, 100 ms) in `scryfall.ts`, backing
  off 30 s on a 429. Let the browser's HTTP cache do the caching; responses
  already say `max-age=57600`.

## What chip-scryfall's parser is still for

- **Gauntlet, unchanged.** A probability query must refuse what it cannot
  answer. Scryfall's own search drops unknown terms and answers anyway, which
  is the "silent no-match" `chip-scryfall` exists to prevent (`lib.rs`).
- **Checking queries that end up in the deck file**, if Curator ever edits a
  criteria file or a category query. Those are Gauntlet's queries, so they must
  parse with Gauntlet's parser, in wasm, with its error naming the bad term,
  before the save commits them.
- **Not for the search overlay.** Pre-validating an overlay query with the
  subset parser would reject valid Scryfall syntax. The overlay trusts Scryfall
  and shows Scryfall's warnings.

The index and `bulk.rs` stay where they are, for `gauntlet sync` and the
checker. Nothing in this recommendation touches them.
