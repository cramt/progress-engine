// Test-only: the real auth and API clients wired to the mock, as `connect`
// wires them in dev, minus the browser.
import { readFileSync } from "node:fs";
import { loadDeckSync } from "../deck";
import { createGitHubApi } from "./api";
import { createAuth, pageLocks } from "./auth";
import { createMockGitHub, type MockOptions } from "./mock";

const repo = new URL("../../../../../", import.meta.url);

export const seedDecks = (): Record<string, string> => ({
  "decks/lantern.deck.toml": readFileSync(
    new URL("decks/lantern.deck.toml", repo),
    "utf8",
  ),
  "decks/loam.deck.toml": readFileSync(
    new URL("decks/loam.deck.toml", repo),
    "utf8",
  ),
});

export function loadWasm(): void {
  loadDeckSync(
    readFileSync(new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url)),
  );
}

export function mockConnection(options: MockOptions = {}) {
  const mock = createMockGitHub(options);
  const auth = createAuth({
    fetch: mock.fetch,
    channel: null,
    locks: pageLocks(),
    navigate: () => mock.logIn(),
  });
  const api = createGitHubApi({ auth, fetch: mock.fetch });
  return { mock, auth, api, repo: { owner: "octocat", name: "mtg" } };
}
