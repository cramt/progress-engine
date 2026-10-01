import { createReadStream, statSync } from "node:fs";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";
import { buildProbe, probeOut, probePin } from "./scripts/build-probe.mjs";
import { buildWasm, watched } from "./scripts/build-wasm.mjs";
import { devAuth } from "./scripts/dev-auth.ts";

// Rebuilds the parser whenever its Rust changes, so `pnpm dev` is the only
// command a session needs. A failed build keeps the last good package and
// prints cargo's error, rather than taking the dev server down with it.
function decklistWasm(): Plugin {
  return {
    name: "meldweb-decklist-wasm",
    apply: "serve",
    buildStart() {
      buildWasm();
    },
    configureServer(server) {
      server.watcher.add(watched);
      server.watcher.on("change", (file) => {
        if (!file.endsWith(".rs")) return;
        try {
          buildWasm();
          server.ws.send({ type: "full-reload" });
        } catch {
          // cargo already printed why
        }
      });
    },
  };
}

const PROBE = "virtual:gitaxian-probe";
// One URL per build, so the worker can cache its answers for good.
const PROBE_BASE = `/gitaxian-probe/${probePin.tag}/`;
const TYPES: Record<string, string> = {
  ".js": "text/javascript",
  ".wasm": "application/wasm",
  ".txt": "text/plain",
};

// The engine's thread pool needs SharedArrayBuffer, so every page is
// cross-origin isolated. `credentialless` rather than `require-corp`, so
// Scryfall's images, which send CORS headers but no CORP, load without a
// `crossorigin` on every <img>. Chromium and Firefox honour it; Safari does
// not, so Safari has no scanner.
const ISOLATION = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "credentialless",
};

// Gitaxian Probe's card scanner. `virtual:gitaxian-probe` is the probe's
// JavaScript API, in every site build and in `MELDWEB_PROBE=1 pnpm dev`, and
// `null` in plain `pnpm dev` and in tests. The site never carries Delver X's
// engine files: the build writes their pin beside it, and the worker proxies
// each pinned file from the archive on ghcr.io (worker/src/probe.ts). The dev
// server serves them from the probe's target dir instead.
function gitaxianProbe(): Plugin {
  let enabled = false;
  let building = false;
  return {
    name: "meldweb-gitaxian-probe",
    config(_, { command, mode }) {
      building = command === "build";
      enabled =
        mode !== "test" && (building || process.env.MELDWEB_PROBE === "1");
    },
    buildStart() {
      if (enabled) buildProbe({ assets: !building });
    },
    generateBundle() {
      if (!enabled) return;
      this.emitFile({
        type: "asset",
        fileName: "gitaxian-probe-pin.json",
        source: JSON.stringify({ tag: probePin.tag, files: probePin.files }),
      });
      // Cloudflare's static assets read their response headers from here.
      this.emitFile({
        type: "asset",
        fileName: "_headers",
        source: `/*\n${Object.entries(ISOLATION)
          .map(([k, v]) => `  ${k}: ${v}\n`)
          .join("")}`,
      });
    },
    resolveId(id) {
      return id === PROBE ? `\0${PROBE}` : undefined;
    },
    load(id) {
      if (id !== `\0${PROBE}`) return;
      if (!enabled) return "export default null;";
      return [
        `import init, { Scanner } from ${JSON.stringify(`/@fs${probeOut.glue}`)};`,
        `export default { init, Scanner, base: ${JSON.stringify(PROBE_BASE)} };`,
      ].join("\n");
    },
    configureServer(server) {
      // Isolated with the scanner or without it, as the site always is.
      server.middlewares.use((_req, res, next) => {
        for (const [k, v] of Object.entries(ISOLATION)) res.setHeader(k, v);
        next();
      });
      if (!enabled) return;
      server.middlewares.use(PROBE_BASE, (req, res, next) => {
        const name = (req.url ?? "").split("?")[0]?.replace(/^\//, "") ?? "";
        if (!/^[\w.-]+$/.test(name)) return next();
        const file = `${probeOut.assets}${name}`;
        let size: number;
        try {
          size = statSync(file).size;
        } catch {
          return next();
        }
        const ext = name.slice(name.lastIndexOf("."));
        res.setHeader("Content-Type", TYPES[ext] ?? "application/octet-stream");
        res.setHeader("Content-Length", size);
        createReadStream(file).pipe(res);
      });
    },
  };
}

export default defineConfig({
  plugins: [
    devAuth(),
    gitaxianProbe(),
    decklistWasm(),
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    react(),
  ],
});
