// Test-only: the real auth and API clients wired to the mock, as `connect`
// wires them in dev, minus the browser, and the Magic repo as the browser
// holds it, in memory.
import { readFileSync } from "node:fs";
import { loadDeckSync } from "../deck";
import type { SaveAsk } from "../deck.gen";
import type { Repo } from "../repo/api";
import type { LocalRepo } from "../repo/local";
import { memoryRepo } from "../repo/memory";
import { createGitHubApi, createPublicRepos } from "./api";
import { createAuth, pageLocks } from "./auth";
import { createMockGitHub, type MockOptions } from "./mock";

const root = new URL("../../../../../", import.meta.url);

export const seedDecks = (): Record<string, string> => ({
  "decks/lantern.deck.toml": readFileSync(
    new URL("decks/lantern.deck.toml", root),
    "utf8",
  ),
  "decks/loam.deck.toml": readFileSync(
    new URL("decks/loam.deck.toml", root),
    "utf8",
  ),
});

export function loadWasm(): void {
  loadDeckSync(
    readFileSync(new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url)),
  );
}

const ALEX = { name: "Alex", email: "alex@example.com" };

/** `decks/<stem>.deck.toml` or `.criteria.toml`, as a test names a deck's file. */
const DECK_FILE = /^decks\/([^/]+)\.(deck|criteria)\.toml$/;

/**
 * The repo in memory, with `files` committed one by one as it is first used.
 * At version 2 (the default) a test's `decks/<stem>.deck.toml` goes onto
 * the deck's own line, `decks/<stem>/main` as `<stem>.deck.toml`, and the
 * rest onto `main`; at version 1 every file is on `main`, as a v1 repo held
 * it.
 */
export function seededRepo(
  files: Record<string, string> = {},
  version: 1 | 2 = 2,
): LocalRepo {
  const repo = memoryRepo(() => ALEX);
  const ready = (async () => {
    for (const [path, text] of Object.entries(files)) {
      const deck = version === 2 ? DECK_FILE.exec(path) : null;
      await repo.putFile(deck ? `${deck[1]}.${deck[2]}.toml` : path, {
        text,
        message: `add ${path}`,
        sha: null,
        ...(deck ? { branch: `decks/${deck[1]}/main` } : {}),
      });
    }
  })();
  return new Proxy(repo, {
    get(target, key, receiver) {
      const value = Reflect.get(target, key, receiver);
      if (typeof value !== "function") return value;
      return async (...args: unknown[]) => {
        await ready;
        return (value as (...a: unknown[]) => unknown).apply(target, args);
      };
    },
  });
}

/** A file as the repo has it, `branch` defaulting to `main`. */
export async function fileOf(
  repo: Repo,
  path: string,
  branch?: string,
): Promise<string | undefined> {
  return (await repo.getFile(path, branch))?.text;
}

export function mockConnection(
  options: MockOptions & {
    files?: Record<string, string>;
    version?: 1 | 2;
  } = {},
) {
  const mock = createMockGitHub(options);
  const auth = createAuth({
    fetch: mock.fetch,
    channel: null,
    locks: pageLocks(),
    navigate: () => mock.logIn(),
  });
  const github = createGitHubApi({ auth, fetch: mock.fetch });
  const repo = seededRepo(options.files, options.version);
  const others = createPublicRepos({ fetch: mock.fetch });
  return { mock, auth, github, repo, others };
}

/** The messages of the commits that touched `path` on `branch`, oldest first. */
export async function messages(
  repo: Repo,
  path: string,
  branch?: string,
): Promise<string[]> {
  const log = await repo.history(path, {
    perPage: 100,
    ...(branch ? { branch } : {}),
  });
  return log.map((r) => r.message).reverse();
}

/**
 * The repo in memory, as `seededRepo` has it, for tests of how the page
 * writes through one (saving, one-shot edits): every save it asks for,
 * another writer landing between two of its calls, and storage that fails.
 */
export function watchedRepo(files: Record<string, string> = {}) {
  const real = seededRepo(files);
  const saves: SaveAsk[] = [];
  let failing = false;
  const repo: Repo = new Proxy(real, {
    get(target, key, receiver) {
      if (key !== "save") return Reflect.get(target, key, receiver);
      return async (ask: SaveAsk) => {
        saves.push(ask);
        if (failing) throw new Error("the repo's storage failed");
        return target.save(ask);
      };
    },
  });
  return {
    repo,
    /** What the page asked to save, in order. */
    saves: (): readonly SaveAsk[] => saves,
    /** Every write fails, as storage that has gone away does, until `false`. */
    failWrites(on: boolean) {
      failing = on;
    },
    /**
     * A save by another tab, device or person over whatever the file is.
     * Asked for at once, so it lands before any call the page makes after.
     */
    async elsewhere(path: string, text: string, branch?: string) {
      const saved = await real.save({
        path,
        ...(branch === undefined ? {} : { branch }),
        text,
        made: { kind: "over" },
      });
      if (saved.kind !== "committed") throw new Error(JSON.stringify(saved));
    },
  };
}
