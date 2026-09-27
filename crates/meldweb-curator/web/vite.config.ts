import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vite";
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

export default defineConfig({
  plugins: [
    decklistWasm(),
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    react(),
  ],
});
