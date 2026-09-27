// Builds meldweb-wasm and its JS glue into src/wasm/pkg, which is gitignored.
// The Vite dev server calls this too, on every change to the Rust it depends on.
//
// The Nix build sets MELDWEB_WASM_PREBUILT and copies in a crane-built package
// instead, because cargo cannot fetch crates inside the sandbox.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const web = fileURLToPath(new URL("..", import.meta.url));
const repo = fileURLToPath(new URL("../../../..", import.meta.url));

export const watched = [
  `${repo}crates/meldweb-curator/wasm/src`,
  `${repo}crates/reality-chip/decklist/src`,
];

/**
 * `release` uses Cargo.toml's `wasm` profile, which is what ships; the dev
 * server takes the debug build because it rebuilds on every save.
 */
export function buildWasm({ release = false } = {}) {
  if (process.env.MELDWEB_WASM_PREBUILT) return;
  const profile = release ? "wasm" : "dev";
  execFileSync(
    "cargo",
    [
      "build",
      "-p",
      "meldweb-wasm",
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
      `${web}src/wasm/pkg`,
      `${repo}target/wasm32-unknown-unknown/${release ? "wasm" : "debug"}/meldweb_wasm.wasm`,
    ],
    { stdio: "inherit" },
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  buildWasm({ release: true });
}
