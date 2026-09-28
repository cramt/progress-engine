// Builds Gitaxian Probe's JavaScript API and lays out Delver X's engine files
// for the scan dialog, under the probe's own target dir: nothing here lands in
// src/ or public/, so `pnpm build` never carries Delver's files into the site.
//
// Opt-in and dev-only (`MELDWEB_PROBE=1 pnpm dev`): the probe is its own cargo
// workspace outside the flake, its build downloads Delver's engine against a
// hash pin, and its wasm-bindgen is not the editor's. `WASM_BINDGEN_PROBE`
// names a wasm-bindgen CLI matching the probe's Cargo.lock, when the one on
// PATH matches the editor's instead.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("../../../..", import.meta.url));
const probe = `${repo}crates/gitaxian-probe/`;
const out = `${probe}target/meldweb/`;

/** Where the dev server finds the API's JS glue, and the engine's files. */
export const probeOut = {
  glue: `${out}pkg/gitaxian_probe_bindgen.js`,
  assets: `${out}gitaxian-probe/`,
};

export function buildProbe() {
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
