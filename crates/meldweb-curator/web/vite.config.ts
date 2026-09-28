import { createReadStream, statSync } from "node:fs";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";
import { buildProbe, probeOut } from "./scripts/build-probe.mjs";
import { buildWasm, watched } from "./scripts/build-wasm.mjs";

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
const PROBE_BASE = "/gitaxian-probe/";
const TYPES: Record<string, string> = {
  ".js": "text/javascript",
  ".wasm": "application/wasm",
  ".txt": "text/plain",
};

// Gitaxian Probe's card scanner, in `MELDWEB_PROBE=1 pnpm dev` only. It builds
// the probe's JavaScript API, serves Delver X's engine files from the probe's
// target dir, and makes the page cross-origin isolated, which the engine's
// thread pool cannot start without. `virtual:gitaxian-probe` is that API, or
// `null` everywhere else - `pnpm build` included, so the site never carries
// Delver's files (docs/research/probe-in-curator.md says why).
function gitaxianProbe(): Plugin {
  let enabled = false;
  return {
    name: "meldweb-gitaxian-probe",
    config(_, { command, mode }) {
      enabled =
        command === "serve" &&
        mode !== "test" &&
        process.env.MELDWEB_PROBE === "1";
    },
    buildStart() {
      if (enabled) buildProbe();
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
      if (!enabled) return;
      // `credentialless` rather than `require-corp`, so Scryfall's images,
      // which send CORS headers but no CORP, load without a `crossorigin` on
      // every <img>. Chromium and Firefox honour it; Safari does not.
      server.middlewares.use((_req, res, next) => {
        res.setHeader("Cross-Origin-Opener-Policy", "same-origin");
        res.setHeader("Cross-Origin-Embedder-Policy", "credentialless");
        next();
      });
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
    gitaxianProbe(),
    decklistWasm(),
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    react(),
  ],
});
