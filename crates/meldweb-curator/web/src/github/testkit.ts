// Test-only: the real auth and API clients wired to the mock, as `connect`
// wires them in dev, minus the browser, and the Magic repo as the browser
// holds it, in memory.
import { readFileSync } from "node:fs";
import { loadDeckSync } from "../deck";
import { ConflictError, type Repo } from "../repo/api";
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

export interface FakeCommit {
  path: string;
  message: string;
  sha: string;
  /** Made by `editElsewhere`, not through the repo's calls. */
  outOfBand: boolean;
}

export interface FakeCall {
  method: "GET" | "PUT" | "DELETE";
  path: string;
  branch?: string;
  keepalive: boolean;
}

/**
 * A `Repo` of plain files in memory that records every call, for tests of
 * how the page writes through one (saving, one-shot edits): what a write is
 * guarded by, what each commit says, and another writer landing between two
 * calls, synchronously. Anything else a `Repo` does is the real repo's, in
 * `seededRepo`.
 */
export function fakeRepo(files: Record<string, string> = {}) {
  let seq = 0;
  const sha = () => (++seq).toString(16).padStart(40, "0");
  const key = (path: string, branch?: string) =>
    branch === undefined || branch === "main" ? path : `${branch}:${path}`;
  const held = new Map<string, { text: string; sha: string }>();
  const commits: FakeCommit[] = [];
  const calls: FakeCall[] = [];
  let failing = false;
  for (const [path, text] of Object.entries(files)) {
    held.set(path, { text, sha: sha() });
  }
  const refuse = (path: string) => {
    throw new ConflictError(`${path} changed in the repo since it was read`);
  };
  const repo: Repo = {
    where: "a test",
    async getFile(path, ref) {
      calls.push({
        method: "GET",
        path,
        ...(ref ? { branch: ref } : {}),
        keepalive: false,
      });
      const file = held.get(key(path, ref));
      return file ? { ...file } : null;
    },
    async getBytes() {
      return null;
    },
    async history(path, page = {}) {
      return commits
        .filter((c) => c.path === key(path, page.branch))
        .map((c) => ({
          commit: c.sha,
          message: c.message,
          date: "",
          author: null,
        }))
        .reverse();
    },
    async revision() {
      return null;
    },
    async snapshots() {
      return [];
    },
    async takeSnapshot() {
      throw new Error("no snapshots in fakeRepo");
    },
    async dropSnapshot() {},
    async putFile(path, put) {
      calls.push({
        method: "PUT",
        path,
        ...(put.branch ? { branch: put.branch } : {}),
        keepalive: put.keepalive ?? false,
      });
      if (failing) throw new Error("the repo's storage failed");
      const k = key(path, put.branch);
      const now = held.get(k);
      if ((put.sha ?? null) !== (now?.sha ?? null)) refuse(path);
      const file = { text: put.text, sha: sha() };
      held.set(k, file);
      const commit = sha();
      commits.push({
        path: k,
        message: put.message,
        sha: commit,
        outOfBand: false,
      });
      return { sha: file.sha, commit };
    },
    async deleteFile(path, del) {
      calls.push({ method: "DELETE", path, keepalive: false });
      if (held.get(path)?.sha !== del.sha) refuse(path);
      held.delete(path);
      commits.push({
        path,
        message: del.message,
        sha: sha(),
        outOfBand: false,
      });
    },
    async listDir(dir) {
      const prefix = dir === "" ? "" : `${dir}/`;
      return [...held.entries()]
        .filter(([p]) => !p.includes(":") && p.startsWith(prefix))
        .filter(([p]) => !p.slice(prefix.length).includes("/"))
        .map(([p, f]) => ({
          name: p.slice(prefix.length),
          path: p,
          sha: f.sha,
          type: "file" as const,
        }));
    },
    async decks() {
      return [];
    },
    async deleteDeck() {},
    async migrate() {},
    async branches() {
      return [];
    },
    async blame() {
      return null;
    },
    async createBranch() {
      throw new Error("no branches in fakeRepo");
    },
    async deleteBranch() {},
    async planMerge() {
      return { kind: "upToDate" as const };
    },
    async merge() {
      return { kind: "upToDate" as const };
    },
    async followRequest() {
      throw new Error("no other players' repos in fakeRepo");
    },
    async followFinish() {
      throw new Error("no other players' repos in fakeRepo");
    },
    async fork() {
      throw new Error("no other players' repos in fakeRepo");
    },
    async following() {
      return null;
    },
    async unfollow() {},
    async planTake() {
      return { kind: "upToDate" as const };
    },
    async takeUpstream() {
      return { kind: "upToDate" as const };
    },
  };
  return {
    repo,
    file: (path: string, branch?: string) => held.get(key(path, branch)),
    commits: (): readonly FakeCommit[] => commits,
    calls: (): readonly FakeCall[] => calls,
    /** Every write fails, as storage that has gone away does, until `false`. */
    failWrites(on: boolean) {
      failing = on;
    },
    /** A commit by another tab, device or person, between two of the page's calls. */
    editElsewhere(path: string, text: string, branch?: string) {
      const k = key(path, branch);
      held.set(k, { text, sha: sha() });
      commits.push({
        path: k,
        message: `edit ${path}`,
        sha: sha(),
        outOfBand: true,
      });
    },
  };
}
