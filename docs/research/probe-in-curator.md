# Gitaxian Probe in Meldweb Curator: a scan button, as a spike

Question: can Meldweb Curator scan a physical card into a deck, using Gitaxian
Probe's web host? And what stands between that and shipping it?

**Answer: it works in `pnpm dev`, and three things block shipping it.** A Scan
button in the deck toolbar boots Delver X's engine on the page, reads a camera
frame or an image file, and adds the printing it names to the deck in one
undoable edit. Card names come back right; printings are a guess. What blocks
shipping, most serious first:

1. **Deploying it means hosting Delver's engine and weights.** Their origin
   sends no CORS headers, so no other site can fetch the files. The site would
   have to serve its own copy (engine README, *On the web*). That is
   redistributing a vendor's binaries and model, and it needs their permission
   before it needs any code. The spike keeps the files out of every build on
   purpose (*What was built*). The archive of Delver's builds on ghcr.io was
   made public on 2026-09-28 so that builds need no credentials, which already
   serves the files to anyone who asks; asking Delver Lab is still the step
   before the site ships them.
2. **The model is too big for the host.** `model-alpha.dat` is 34,050,684 bytes
   (32.5 MiB). Cloudflare Workers static assets take at most 25 MiB per file.
   The fixes are to serve it from R2, to split it and join it in the page, or to
   serve the 23 MB `.7z` and unpack it in the page, which puts an LZMA decoder in
   the page that `gitaxian-probe-assets` currently keeps out.
3. **The whole site has to be cross-origin isolated, and Safari needs more for
   that than Chromium does.** See *Cross-origin isolation*.

Measured on 2026-09-28 in headless Chromium (Playwright's build), against
`VITE_MOCK_GITHUB=1 MELDWEB_PROBE=1 pnpm dev`, with the lantern deck open.

## What was built

| | |
|---|---|
| `crates/gitaxian-probe/bindgen/` | `gitaxian-probe-bindgen`: the web `Engine` as a wasm-bindgen class, `Scanner.open(base, onProgress)` / `scan(rgba, w, h)`. `scan` resolves each detection and its runners-up to catalogue rows with `scryfall_id`, on the Rust side |
| `web/scripts/build-probe.mjs` | builds that crate from the probe's own workspace, runs *its* wasm-bindgen (0.2.129, while the editor's is 0.2.126), and copies the pinned engine files. Everything lands under `crates/gitaxian-probe/target/meldweb/` |
| `web/vite.config.ts`, `gitaxianProbe()` | with `MELDWEB_PROBE=1` in `vite` serve mode only, it sets COOP/COEP, serves the engine files at `/gitaxian-probe/`, and resolves `virtual:gitaxian-probe` to the API. Everywhere else, `pnpm build` and vitest included, the virtual module is `null` |
| `web/src/probe/` | `scanner.ts` (the page's one scanner, and framing an image), `printings.ts` (`scryfall_id` to `set/num`, and adding a printing), `ScanDialog.tsx` |

The Scan button appears only when `virtual:gitaxian-probe` is not `null`.
`MELDWEB_PROBE=1 vite build` was checked: `dist/` has no engine file and no
probe code. The flake, CI and the deployed site are therefore unchanged.

Run it:

```
cargo install wasm-bindgen-cli --version 0.2.129 --locked --root ~/.wbg129
WASM_BINDGEN_PROBE=~/.wbg129/bin/wasm-bindgen MELDWEB_PROBE=1 VITE_MOCK_GITHUB=1 pnpm dev
```

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
| Boot, Scan clicked to ready | 5.9–8.7 s, including ~52 MB of engine files from localhost |
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
blocked. The spike uses `credentialless`, where a plain `<img>` from Scryfall
loaded in the isolated page (checked with `crossOriginIsolated === true`).
Chromium and Firefox support `credentialless`. Safari does not, and iOS
Safari is where a phone camera is. Shipping to Safari means `require-corp`
plus `crossorigin="anonymous"` on every Scryfall `<img>`, which works because
Scryfall answers CORS.

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
1.89.beta value, on the web checks above. **The native accuracy test was not
run** (no ImageMagick, and V8 was not built). Nor was `web-check/run.sh`'s 4/6
printing assertion met or changed: it fails on 2/6 and 3/6 with the frames made
here, and it cannot be retried on the frames it was written against without
`magick`. That is a check to rerun, not a number to move.

Anything that ships this carries Delver's release schedule. A new build breaks
a fresh `cargo build` until the pin moves, unless the pinned build is in the
archive (the engine README, *The archive*).

## Not checked

- A real camera. The `getUserMedia` path is written but has not run (headless
  Chromium has no camera), and nothing has been checked on a phone.
- Firefox, Safari, and memory on mobile.
- Several cards in one frame. The dialog lists every detection, but every
  fixture holds one card.
- `lambda` and `gamma`, which the engine refuses (engine README, *Scope*).
