// Builds Gitaxian Probe's JavaScript API for the scanner, and for the dev
// server lays out Delver X's engine files too, all under the probe's own target
// dir. The site carries the API but never the engine's files: the worker
// proxies those from the archive (worker/src/probe.ts).
//
// The probe is its own cargo workspace, and laying out the files downloads them
// against a hash pin. Its Cargo.lock pins the editor's wasm-bindgen, so the
// devshell's CLI serves both; `WASM_BINDGEN_PROBE` names another CLI, should
// the two ever part. The Nix build sets MELDWEB_PROBE_PREBUILT to a built API
// instead, as it does MELDWEB_WASM_PREBUILT.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../../../..", import.meta.url));
const probe = `${repo}crates/gitaxian-probe/`;
const out = `${probe}target/meldweb/`;

const prebuilt = process.env.MELDWEB_PROBE_PREBUILT;

/** Where the API's JS glue is, and where the dev server finds the engine's files. */
export const probeOut = {
  glue: `${prebuilt ? `${prebuilt}/` : `${out}pkg/`}gitaxian_probe_bindgen.js`,
  assets: `${out}gitaxian-probe/`,
};

/** The pinned build: `{ version, tag, files: [{ name, sha256 }] }`. */
export const probePin = JSON.parse(
  readFileSync(`${probe}assets/pin.json`, "utf8"),
);

/** The API, and the engine's files as well when `assets`, for the dev server. */
export function buildProbe({ assets = true } = {}) {
  if (prebuilt) return;
  const lock = readFileSync(`${probe}Cargo.lock`, "utf8");
  const want = /name = "wasm-bindgen"\nversion = "([^"]+)"/.exec(lock)?.[1];
  const bindgen = process.env.WASM_BINDGEN_PROBE ?? "wasm-bindgen";
  const have = execFileSync(bindgen, ["--version"], { encoding: "utf8" })
    .trim()
    .split(" ")[1];
  if (have !== want) {
    throw new Error(
      `${bindgen} is ${have}, the probe's Cargo.lock has ${want}: ` +
        `cargo install wasm-bindgen-cli --version ${want} --locked --root <dir> ` +
        "and set WASM_BINDGEN_PROBE=<dir>/bin/wasm-bindgen",
    );
  }
  // From the probe's directory, so cargo resolves its workspace, not ours.
  const cargo = (...args) =>
    execFileSync("cargo", args, { cwd: probe, stdio: "inherit" });
  cargo(
    "build",
    "--release",
    "-p",
    "gitaxian-probe-bindgen",
    "--target",
    "wasm32-unknown-unknown",
  );
  execFileSync(
    bindgen,
    [
      "--target",
      "web",
      "--out-dir",
      `${out}pkg`,
      `${probe}target/wasm32-unknown-unknown/release/gitaxian_probe_bindgen.wasm`,
    ],
    { stdio: "inherit" },
  );
  if (!assets) return;
  cargo(
    "run",
    "--quiet",
    "-p",
    "gitaxian-probe-assets",
    "--example",
    "copy",
    "--",
    probeOut.assets,
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  buildProbe();
}
