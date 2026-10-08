# Gitaxian Probe in Meldweb Curator: scanning cards

Question: can Meldweb Curator scan a physical card into a deck, using Gitaxian
Probe's web host? And what stands between that and shipping it?

**Answer: it ships.** It began on 2026-09-28 as a dev-only spike: a Scan button
in the deck toolbar boots Delver X's engine on the page, reads a camera frame
or an image file, and adds the printing it names to the deck in one undoable
edit. The collection got a continuous scanner on 2026-09-30 (*Scanning into
the collection*). On 2026-10-01 it went into every site build. Card names come
back right; printings are a guess. Three things blocked shipping, and each was
settled as follows:

1. **Hosting Delver's engine and weights.** Their origin sends no CORS headers,
   and neither does the public archive of their builds on ghcr.io (checked
   2026-10-01: not the token, the manifests, any blob, or the storage a blob
   redirects to; a preflight is a 405 or a 403). So the site serves the files
   from its own origin: the worker pipes each file from the archive by the
   digest the page asks for (*How the site serves the engine*). The archive was made
   public on 2026-09-28, knowing it is redistribution, and the site serving
   it was decided on 2026-10-01 in the same way.
2. **The model was too big for the host.** `model-alpha.dat` is 34,050,684
   bytes (32.5 MiB), and Cloudflare's static assets take at most 25 MiB per
   file. The files no longer come from static assets at all, and the model is
   served as the archive holds it, the 23 MB `model-alpha.7z`, and unpacked in
   the page.
3. **The whole site has to be cross-origin isolated.** It is, with
   `require-corp`, so that phones can scan: see *Cross-origin isolation*.

The spike's measurements were taken on 2026-09-28 in headless Chromium
(Playwright's build), against `VITE_MOCK_GITHUB=1 MELDWEB_PROBE=1 pnpm dev`,
with the lantern deck open.

## What was built

| | |
|---|---|
| `crates/gitaxian-probe/bindgen/` | `gitaxian-probe-bindgen`: the web `Engine` as a wasm-bindgen class, `Scanner.open(base, onProgress)` / `scan(rgba, w, h)`. `scan` resolves each detection and its runners-up to catalogue rows with `scryfall_id`, on the Rust side |
| `web/scripts/build-probe.mjs` | builds that crate from the probe's own workspace with its wasm-bindgen (the probe's Cargo.lock now pins the editor's, 0.2.126), and for the dev server copies the pinned engine files. Everything lands under `crates/gitaxian-probe/target/meldweb/`. The Nix build takes the API from the flake's `gitaxian-probe-bindgen` instead (`MELDWEB_PROBE_PREBUILT`) |
| `web/vite.config.ts`, `gitaxianProbe()` | makes every page cross-origin isolated, in dev and, through a `_headers` file, on the site. Resolves `virtual:gitaxian-probe` to the API and each engine file's URL by its digest, in every site build and in `MELDWEB_PROBE=1 pnpm dev`, and to `null` in plain `pnpm dev` and vitest. The dev server serves the engine files itself |
| `worker/src/probe.ts` | `GET /gitaxian-probe/<sha256>/<name>`: that blob, piped from ghcr.io |
| `web/src/probe/` | `scanner.ts` (the page's scanner for each model, and framing an image), `models.ts` and `ModelPicker.tsx` (which model, picked per browser), `printings.ts` (`scryfall_id` to `set/num`, and adding a printing), `ScanDialog.tsx`, and the collection scanner's `tracker.ts` and `beep.ts` |

The Scan buttons appear only when `virtual:gitaxian-probe` is not `null`. The
site carries the probe's API, a 1.9 MB wasm, and none of Delver's files.

Run it in dev:

```
MELDWEB_PROBE=1 VITE_MOCK_GITHUB=1 pnpm dev
```

## How the site serves the engine

The worker is a pipe. `GET /gitaxian-probe/<sha256>/<name>` asks ghcr.io for an
anonymous pull token, fetches the blob with that digest from
`cramt/delver-x`, and returns its body with ghcr.io's status. It knows no pin
and checks nothing. It adds three headers:

- `Content-Type` from the name, because a Web Worker's script must be
  JavaScript, and core.js runs as 32 of them.
- `Cross-Origin-Embedder-Policy: require-corp`, the page's own. Without a COEP Chromium refuses
  those workers' scripts in the isolated page, and the engine never boots
  (checked: the boot timed out after 120 s).
- `Cache-Control: public, max-age=31536000, immutable` on a 2xx, since a URL
  names one digest and so can never change; `no-store` on anything else.
  Without it the browser fetched core.js once per pool worker, about 33 times
  a boot (see the numbers below). A 2xx also goes into the edge cache, so
  ghcr.io is asked once per blob per data centre rather than once per visit.

The page knows which digest is which file. The build turns the pin into a map
from each name to `/gitaxian-probe/<sha256>/<name>`, and the engine's web host
takes it (`EngineConfig::files`), so it fetches each file there rather than at
`base` + name. The dev server answers the same URLs from the probe's target
dir.

The map carries every tier's model, and the scan dialog and the collection's
scan panel each have a Model picker: alpha (the default, 23 MB), lambda (28 MB)
or gamma (40 MB). The pick is kept per browser and shared by both. A page
fetches and boots a model the first time it is picked and keeps it: the
engine's 32 workers outlive `close()`, so a page holds at most three engines,
and going back to one already booted is instant. Alpha is the default because
it names one more exact printing on the fixtures than the other two
(the engine README, *Accuracy and cost*). The collection's picker is locked
while scanning, since a new model is a new tracker and would count the card
in view again.

The files are byte for byte as upstream shipped them, the model packed, so the
engine's web host unpacks `model-<tier>.7z` in the page with the same LZMA2
reader the native host uses (`sevenz-rust2` without its encryption, which does
not build for wasm32), and checks it against `model-<tier>.size`.

Measured on 2026-10-01, with the built site served by the worker's own code
against the real ghcr.io (Node, in this sandbox: `wrangler dev` was not run):

| | |
|---|---|
| Each file through the worker | `model-alpha.7z` 23,247,473 bytes in 1.4 s; `core.wasm` 8,895,139 in 1.1 s; `data.7z` 9,627,184 in 0.6 s; the small files 0.2-0.9 s, each a token and a blob from ghcr.io |
| Boot, Scan clicked to ready | 12.3-12.6 s without caching, since each of the pool's workers fetches core.js through ghcr.io again: about 33 fetches of it per boot. With `Cache-Control: immutable` on the worker's answers, which is correct since a URL names one digest, it was 6.5-6.9 s and core.js was fetched once. The worker now sends it |
| `web-check/run.sh`, same machine | 4.0 s with the model served unpacked, 4.8-5.1 s unpacking it in the page |
| The scan itself | the fake-camera run of *Scanning into the collection*, unchanged: three beeps, Black Lotus, Black Lotus, Counterspell |

On the deployed site, meldweb.cramt.dk, checked on 2026-10-01 in headless
Chromium (Playwright's build, no login): the page is cross-origin isolated, the
worker answers each `/gitaxian-probe/<sha256>/<name>` with 200, the bytes
hash to the digest in the URL, and the answer carries
`Cross-Origin-Embedder-Policy: credentialless`. The shipped chunk's
`openScanner` booted the engine in 15.9 s, and `scan` read the six
`engine/.fixtures` frames in 0.2-0.4 s each: 6/6 names and 5/6 printings,
with the same miss as the web check (Swords to Plowshares placed in Foreign
Black Border).

## Scanning into the collection

The collection page has its own Scan, for putting a stack of cards away one
after another rather than one card into a deck. Press Start, and the camera is
read continuously: a frame is scanned, and the next is taken 150 ms after that
scan ends, so the rate is set by the engine. The frames are a live feed to the
engine (`Scanner.watch`, FINDINGS §12 *A live feed*): each is pushed once, with
the engine's tracking carried over from the frame before, and without the
dark border a still gets, since a camera frame of a card on the table already
shows its edge. A card the engine still tracks but did not see in the frame
does not count as in it. Each card that goes in beeps, and
lands in a log with a Take back, since its printing is a guess. The panel
chooses the place the cards go to (Unsorted by default, as quick add does),
the finish, which the engine cannot see, whether to keep the printing it
guesses or add by name, and the speed. These are kept in the browser's
`localStorage` for the next session, and a place since removed falls back to
Unsorted.

**The same card is not added twice for lying there.** `probe/tracker.ts`
decides, by card name, since the printing can change between two frames of the
same card:

- a card goes in once it has been in **2 frames in a row**, so a card read
  wrongly while it slides into place is not added. **Fast** takes it on the
  first frame that reads it, about a frame sooner, misreads included;
- it goes in **once**, and counts again only after it has been out of **2
  frames in a row**, so one missed detection with the card still there does
  not add a second copy. Taking it away, or covering it with the hand that puts
  the next copy down, is what rearms it;
- a different card counts straight away, with no gap needed.

The cost of the rule: two copies of one card in the same frame are one copy,
and a second copy put down faster than two frames is missed. Both are in
`tracker.test.ts`. **Same again** (a button, or Space) is how to scan a stack of
one card: it adds one more of the last card counted, with its printing, place
and finish, without the card leaving the frame.

The beep sounds when the card is counted, before Scryfall is asked for its
printing, and the log shows it as "Naming the printing…" until it goes in. If
Scryfall cannot be reached, it goes in by name, and the log says why.

Above the log, a tally counts each card's copies (Counterspell ×4, Shock ×2),
so a session can be checked against the stack at a glance. The settings fold
to one line once Start is pressed, so on a phone the camera and the log fill
the screen. A guessed printing that turns out wrong is fixed afterwards on the
collection page: a line's name or printing opens every printing as a picture,
to match against the card in hand, and Apply changes as many of the line's
copies as asked, to that printing and a finish it comes in. Those copies join
a line already holding them alike (`collection::reprint`), as a move's do.
Sorting what a scan left in Unsorted is ticking lines and moving them together,
one edit (`collection::move_lines`).

Measured on 2026-09-30 in headless Chromium, with a fake camera playing a video
of the fixture frames: 5 s of empty table, Black Lotus for 8 s, 5 s empty,
Black Lotus for 8 s, then Counterspell for 8 s with no gap. It beeped three
times and added Black Lotus, Black Lotus and Counterspell. The frames of the
card lying still, around eight each time, added nothing more. Boot took
3.5–3.6 s. A frame took 0.73–0.91 s once the first had been scanned (the first
took about 1.1 s), and a card was counted 0.5–1.1 s after the first frame that
read it. The two Lotuses came back as `lea/232` and `o90p/2`, the misread named
in *Accuracy* below. The same video on Fast, into Bulk, with one Space after
the first Lotus, gave three Lotuses and a Counterspell, as `3ed/54`. Fast
counted each card on the first frame that read it. A real camera and a phone
remain unchecked (*Not checked*).

That was each frame scanned as a still (`Scanner.scan`), which pushes it
through the engine until a card is reported: twice at least with a card in
view, and twelve times on an empty table. Measured again on 2026-10-08 against
the live feed, with the same video, in the same headless Chromium, one run
each way:

| | still per frame (main) | live feed |
|---|---|---|
| A frame of empty table | 3.0–3.3 s | 0.12–0.25 s, median 0.15 s |
| Card placed to card counted, Careful | 3.4–3.6 s | 0.7–1.0 s |
| Counted | Black Lotus, Black Lotus, Counterspell | the same |

Boot was ~24 s on this machine both ways, against 3.5 s on 09-30. That is the
machine and not the change: the engine boots the same way on either path. The empty frame
matters as much as the card frame: a card put down while an empty frame is
being scanned waited for that frame to finish before it could be read.

## Joining a scan to the deck

Delver's catalogue names editions ("Limited Edition Alpha"), not set codes, and
the deck names a printing by `set/num`. The join goes through `scryfall_id`,
which CONTEXT-MAP predicted: one `POST /cards/collection` with `{ "id": … }`
identifiers for the pick and its runners-up, through the same rate gate as
every other collection lookup. This gives `lea/232`, which is added with
`addPrinting`: one more copy on a line that already names that printing, and
otherwise a new line with no category. Adding the same Black Lotus twice gave
`{ printing = "lea/232", qty = 2 }`. `editions.tl_abb` might hold the set code
and save the request. That was not checked.

If Scryfall cannot be reached, the dialog still shows what the engine found,
with Add disabled. One early version kept the previous frame's results on
screen when a lookup failed, and Add then put the wrong card in the deck. The
dialog now clears its results when a scan starts.

## Accuracy: names hold, printings are noise

The six fixture cards from `engine/.fixtures/fetch-cards.sh`, alpha tier,
Delver X 1.89.beta. ImageMagick was not available, so each set of frames below
was made another way:

| Frames | Card name | Exact printing | Misses |
|---|---|---|---|
| the web check's frames, resampled by Chromium's canvas | 6/6 | 2/6 | Black Lotus → Oversized 90's Promos, Counterspell → Foreign Black Border, Llanowar Elves → Dominaria, Swords → Foreign Black Border |
| the same, resampled by Pillow's Lanczos | 6/6 | 3/6 | Black Lotus → Oversized 90's Promos, Llanowar → Dominaria, Swords → FBB |
| the dialog's own framing (`frameOf`: at most 1280 px, 15% dark border) | 6/6 | 4/6 | Counterspell → Revised, Llanowar → Dominaria |

The resampler alone moved Counterspell's printing, and each framing misses a
different set of cards. So the printing depends on the image pipeline, and a
phone camera will be a worse pipeline than any of these. The dialog treats the
pick as a suggestion, and the editor's existing printing picker (`P`) is how a
wrong one gets fixed.

The runners-up do not help with printings. `Detection.similar` never held
another printing of the picked card in any of the six scans. It held nearest
*other* artworks: for Black Lotus these were Masterwork of Ingenuity, Crusade,
Giant Mantis and so on. That fits FINDINGS §3's reading that the index holds
one embedding per artwork: same-art reprints share it, which is why the engine
cannot tell them apart. The dialog shows them as "Not Black Lotus? It also
looked like …", which only helps when the name is wrong.

The engine's confidences do not predict correctness. `rec_conf` was 19–63 on
six correct names, and `set_conf` was 100 on every scan, the wrong printings
included. The dialog shows them but does not act on them.

A tight crop works once framed. `lotus.jpg`, Scryfall's edge-to-edge image,
was recognised through the dialog because `frameOf` puts a border around every
image. On its own the detector finds nothing in it (engine README).

## Cost

| | |
|---|---|
| Boot, Scan clicked to ready | 5.9–8.7 s in the spike, including ~52 MB of engine files from localhost. Shipped, it downloads 42 MB and unpacks the model in the page (*How the site serves the engine*) |
| One scan | 0.6–1.8 s, including the catalogue lookups for the runners-up; the first scan after boot is the slowest |
| Resident | a 1 GiB shared `WebAssembly.Memory` (FINDINGS §5) and 32 Web Workers, for the life of the page, since the pool cannot be stopped (FINDINGS §6). Not measured on a phone |

The scanner boots on the first Scan and is then shared. Closing the dialog
does not free it.

## Cross-origin isolation

The engine's pool needs `SharedArrayBuffer`, so the page needs
`Cross-Origin-Opener-Policy: same-origin` and a `Cross-Origin-Embedder-Policy`.
COOP costs the editor nothing: GitHub login is a whole-tab navigation, not a
popup.

COEP is where it costs. The editor shows Scryfall images with plain `<img>`,
and `cards.scryfall.io` sends `access-control-allow-origin: *` but no
`Cross-Origin-Resource-Policy`, so under `require-corp` every card picture is
blocked. `credentialless` lets a plain `<img>` load, and the site shipped with
it on 2026-10-01, but only Chromium and desktop Firefox support it: Firefox for
Android and Safari, and so every browser on an iPhone, ignore it, and there the
scanner refused with "the page is not cross-origin isolated". Since the
scanner is for phones, the site moved the same day to `require-corp`, which
Firefox has supported since 79 and iOS Safari since 15.2, and every `<img>`
carries `crossOrigin="anonymous"`, which works because Scryfall answers CORS.
`web/src/crossOrigin.test.ts` fails on an `<img>` without it, since a missing
one shows nothing until a browser blocks the picture.

Checked on 2026-10-01 at 390x844 against `MELDWEB_PROBE=1 VITE_MOCK_GITHUB=1
pnpm dev`, in Playwright's Chromium, Firefox and WebKit builds: each page is
`crossOriginIsolated`, every visible card image loads, and the collection's
scanner boots in 3.0-3.6 s.

Isolating only a scan page avoids that, but it would have to be a
separately loaded document, since a TanStack route change does not re-send
headers. It would hand its cards back to the editor over a same-origin
`BroadcastChannel`.

## Upstream moved a week after the pin

`assets/src/pin.rs` pinned 1.83.beta, but on 2026-09-28 upstream served
1.89.beta, and only that. So no build could get the pinned files, and the pin
had to move before anything ran. It is now 1.89.beta. Every file changed,
`core.wasm` included, and so did the import fingerprint:

| | 1.83.beta (FINDINGS §5) | 1.89.beta |
|---|---|---|
| imported functions | 60 | 59: one `() -> i32` fewer (`sig15` ×3, was ×4) |
| memory import | first (`a.a`) | last, still named `a` |
| singleton signatures | 1, 21, 33, 38, 62, 72, 97, 98 | the same signatures, at type indices 1, 21, 33, 39, 62, 73, 99, 72 |
| fingerprint | `3411ecc782a61347` | `e7615396c5ece313` |

The fingerprint hashes type *indices*, so renumbering the type section moves it
even when no signature changed. On the web host this cannot misroute anything,
because `core.js` supplies its own imports. `KNOWN_FINGERPRINT` is now the
1.89.beta value, on the web checks above. The native accuracy test and
`web-check/run.sh` could not be run on 2026-09-28 (no ImageMagick, and V8 was
not built), and the web check failed its 4/6 printing assertion on 2/6 and 3/6
with frames made another way. Both were rerun on 2026-10-01 with ImageMagick
frames: 6/6 names and 5/6 printings, natively and on the web, with the same
picks. 1.89.beta places Llanowar Elves right, where 1.83.beta did not, so both
tests then pinned 5/6. 1.90.beta moved Llanowar Elves to a same-art reprint in
Game Night: Free-for-All, and they pin 4/6 again (engine README, *Accuracy and
cost, measured*).

Anything that ships this carries Delver's release schedule. A new build breaks
a fresh `cargo build` until the pin moves, unless the pinned build is in the
archive (the engine README, *The archive*).

## Not checked

- A real camera. The `getUserMedia` path is written but has not run (headless
  Chromium has no camera), and nothing has been checked on a phone.
- Firefox for Android and Safari on a real phone, and memory on mobile.
- Several cards in one frame. The dialog lists every detection, but every
  fixture holds one card.
- `lambda` and `gamma`, which the engine refuses (engine README, *Scope*).
