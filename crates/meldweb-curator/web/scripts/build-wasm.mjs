// Builds meldweb-wasm and its JS glue into src/wasm/pkg, and mockery-wasm, the
// custom cards page's renderer, into src/wasm/mockery; both are gitignored.
// The Vite dev server calls this too, on every change to the Rust it depends on.
//
// The Nix build sets MELDWEB_WASM_PREBUILT and copies in crane-built packages
// instead, because cargo cannot fetch crates inside the sandbox.
import { execFileSync } from "node:child_process";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const web = fileURLToPath(new URL("..", import.meta.url));
const repo = fileURLToPath(new URL("../../../..", import.meta.url));

export const watched = [
  `${repo}crates/meldweb-curator/wasm/src`,
  `${repo}crates/meldweb-curator/git/src`,
  `${repo}crates/reality-chip/decklist/src`,
  `${repo}crates/reality-chip/scryfall/src`,
  `${repo}crates/phyrexian-metamorph/sheet/src`,
  `${repo}crates/mirage-mockery/core/src`,
  `${repo}crates/mirage-mockery/wasm/src`,
];

/**
 * `release` uses Cargo.toml's `wasm` profile, which is what ships; the dev
 * server takes the debug build because it rebuilds on every save.
 */
export function buildWasm({ release = false } = {}) {
  if (!process.env.MELDWEB_WASM_PREBUILT) {
    const profile = release ? "wasm" : "dev";
    build("meldweb-wasm", profile, "pkg");
    // Always optimised: Typst at opt-level 0 is several times the size and
    // takes seconds to lay out one card, and its Rust rarely changes.
    build("mockery-wasm", "wasm", "mockery");
  }
  packMockery();
}

const MOCKERY = `${web}src/wasm/mockery`;

/**
 * mockery-wasm is 36 MB, Typst and its fonts, past the 25 MiB Cloudflare
 * serves as one static file. Gzipped it is 15 MB, and the page unpacks it
 * with DecompressionStream (src/mockery.ts). The raw module is removed, and
 * the glue's default URL for it with it, so no build ships it beside.
 */
function packMockery() {
  const wasm = `${MOCKERY}/mockery_wasm_bg.wasm`;
  let bytes;
  try {
    bytes = readFileSync(wasm);
  } catch {
    return; // packed already, and the Rust has not changed since
  }
  // Not `.gz`, which servers (Vite's among them) send as Content-Encoding:
  // gzip, so the browser would unpack it before the page could.
  writeFileSync(`${wasm}-gz`, gzipSync(bytes, { level: 9 }));
  rmSync(wasm);
  const glue = `${MOCKERY}/mockery_wasm.js`;
  const js = readFileSync(glue, "utf8");
  const url = "new URL('mockery_wasm_bg.wasm', import.meta.url)";
  if (!js.includes(url)) throw new Error(`${glue} no longer says ${url}`);
  writeFileSync(glue, js.replace(url, "undefined"));
}

function build(crate, profile, out) {
  execFileSync(
    "cargo",
    [
      "build",
      "-p",
      crate,
      "--target",
      "wasm32-unknown-unknown",
      "--profile",
      profile,
    ],
    { cwd: repo, stdio: "inherit" },
  );
  execFileSync(
    "wasm-bindgen",
    [
      "--target",
      "web",
      "--out-dir",
      `${web}src/wasm/${out}`,
      `${repo}target/wasm32-unknown-unknown/${profile === "dev" ? "debug" : profile}/${crate.replace("-", "_")}.wasm`,
    ],
    { stdio: "inherit" },
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  buildWasm({ release: true });
}
