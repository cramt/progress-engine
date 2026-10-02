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
// Each engine file by its digest: the worker pipes `<sha256>` from the archive
// on ghcr.io, and the dev server serves `<name>` from the probe's target dir.
// The scanner boots alpha, so the engine and alpha's model are all it asks for.
const PROBE_BASE = "/gitaxian-probe/";
const PROBE_FILES = Object.fromEntries(
  [...probePin.engine, ...probePin.tiers.alpha.model].map((f) => [
    f.name,
    `${PROBE_BASE}${f.sha256}/${f.name}`,
  ]),
);
const TYPES: Record<string, string> = {
  ".js": "text/javascript",
  ".wasm": "application/wasm",
  ".txt": "text/plain",
};

// The engine's thread pool needs SharedArrayBuffer, so every page is
// cross-origin isolated. `require-corp` rather than `credentialless`, which
// would spare every <img> its `crossOrigin`, because Firefox for Android and
// Safari never shipped `credentialless` and the scanner is for phones. So a
// cross-origin image must load in CORS mode, which Scryfall's CDN allows, and
// src/crossOrigin.test.ts fails on an <img> without it.
const ISOLATION = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "require-corp",
};

// Gitaxian Probe's card scanner. `virtual:gitaxian-probe` is the probe's
// JavaScript API, in every site build and in `MELDWEB_PROBE=1 pnpm dev`, and
// `null` in plain `pnpm dev` and in tests. The site never carries Delver X's
// engine files: the page asks for each by the digest the pin gives it, and the
// worker pipes it from the archive on ghcr.io (worker/src/probe.ts). The dev
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
        `export default { init, Scanner, base: ${JSON.stringify(PROBE_BASE)}, files: ${JSON.stringify(PROBE_FILES)} };`,
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
        const path = (req.url ?? "").split("?")[0] ?? "";
        const name = /^\/[0-9a-f]{64}\/([\w.-]+)$/.exec(path)?.[1];
        if (!name) return next();
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
