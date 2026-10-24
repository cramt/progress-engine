import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  PushOutcome,
  RemoteFacts,
  SyncConflict,
  Synced,
  SyncSettle,
} from "../deck.gen";
import type { PushPlan, RemoteSteps } from "../repo/local";
import { createSyncEngine, PUSH_IDLE_MS } from "./sync";
import { type GitTransport, NeedsLogin } from "./transport";

const URL = "https://github.com/alex/mtg.git";
const bytes = (s: string) => new TextEncoder().encode(s);
const text = (b: Uint8Array | null) => (b ? new TextDecoder().decode(b) : null);

/**
 * The repo's side as a script: what each step answers, and every step
 * asked, in order. The bytes are labels; what they mean is Rust's, tested
 * in meldweb-git against real git.
 */
function fakeRepo(script: {
  integrate?: Synced[];
  push?: (PushPlan["kind"] | "rejected")[];
  facts?: Partial<RemoteFacts>;
}) {
  const log: string[] = [];
  /** What each integrate was asked to settle. */
  const settles: SyncSettle[] = [];
  let url: string | undefined = URL;
  const integrations = [...(script.integrate ?? [{ kind: "upToDate" }])];
  const pushes = [...(script.push ?? ["nothing"])];
  let facts: Omit<RemoteFacts, "url"> = {
    ahead: 0,
    behind: 0,
    branches: 0,
    tags: 0,
    ...script.facts,
  };
  let rejectNext = false;
  const steps: RemoteSteps = {
    async facts() {
      return url ? { url, ...facts } : { ...facts };
    },
    async setRemote(u) {
      log.push(`setRemote ${u}`);
      url = u;
    },
    async removeRemote() {
      log.push("removeRemote");
      url = undefined;
    },
    async fetchRequest(ad) {
      log.push(`fetchRequest ${text(ad)}`);
      return bytes("wants");
    },
    async fetchFinish(ad, response) {
      log.push(`fetchFinish ${text(ad)} ${text(response)}`);
    },
    async integrate(settle?: SyncSettle) {
      const next = integrations.shift() ?? { kind: "upToDate" };
      settles.push(settle ?? {});
      const prefer = settle?.prefer;
      log.push(`integrate${prefer ? ` ${prefer}` : ""} -> ${next.kind}`);
      return next;
    },
    async adoptRemote() {
      log.push("adoptRemote");
    },
    async pushRequest(ad): Promise<PushPlan> {
      const next = pushes.shift() ?? "nothing";
      log.push(`pushRequest ${text(ad)} -> ${next}`);
      if (next === "rejected") {
        rejectNext = true;
        return { kind: "send", body: bytes("push") };
      }
      if (next === "send") return { kind: "send", body: bytes("push") };
      return { kind: next };
    },
    async pushFinish(sent, response): Promise<PushOutcome> {
      log.push(`pushFinish ${text(sent)} ${text(response)}`);
      if (rejectNext) {
        rejectNext = false;
        return {
          updated: [],
          rejected: [{ ref: "refs/heads/main", reason: "stale info" }],
        };
      }
      facts = { ...facts, ahead: 0, tags: 0 };
      return { updated: ["refs/heads/main"], rejected: [] };
    },
  };
  return {
    steps,
    log,
    settles,
    setFacts: (f: Partial<RemoteFacts>) => (facts = { ...facts, ...f }),
  };
}

function fakeHost(log: string[], fail?: () => Error | null): GitTransport {
  return {
    async advertise(url, service) {
      const e = fail?.();
      if (e) throw e;
      log.push(`GET ${url} ${service}`);
      return bytes(`ad:${service}`);
    },
    async post(url, service, body) {
      log.push(`POST ${url} ${service} ${text(body)}`);
      return bytes(`answer:${service}`);
    },
  };
}

const unlocked = <T>(work: () => Promise<T>) => work();

function engine(repo: ReturnType<typeof fakeRepo>, fail?: () => Error | null) {
  const store = new Map<string, string>();
  return createSyncEngine({
    steps: repo.steps,
    transport: () => fakeHost(repo.log, fail),
    lock: unlocked,
    storage: {
      getItem: (k) => store.get(k) ?? null,
      setItem: (k, v) => void store.set(k, v),
      removeItem: (k) => void store.delete(k),
    },
    now: () => 1_000,
  });
}

afterEach(() => vi.useRealTimers());

describe("a sync", () => {
  it("fetches, integrates, then pushes, and remembers when", async () => {
    const repo = fakeRepo({ integrate: [{ kind: "ahead" }], push: ["send"] });
    const sync = engine(repo);
    await sync.sync();
    expect(repo.log).toEqual([
      `GET ${URL} git-upload-pack`,
      "fetchRequest ad:git-upload-pack",
      `POST ${URL} git-upload-pack wants`,
      "fetchFinish ad:git-upload-pack answer:git-upload-pack",
      "integrate -> ahead",
      `GET ${URL} git-receive-pack`,
      "pushRequest ad:git-receive-pack -> send",
      `POST ${URL} git-receive-pack push`,
      "pushFinish push answer:git-receive-pack",
    ]);
    const state = sync.getState();
    expect(state.kind === "on" && state.phase).toEqual({ kind: "idle" });
    expect(state.kind === "on" && state.lastSynced).toBe(1_000);
  });

  it("merges and pushes again when the remote refuses a push it moved under", async () => {
    const repo = fakeRepo({
      integrate: [
        { kind: "ahead" },
        { kind: "merged", commit: "m", taken: ["collection.toml"] },
      ],
      push: ["rejected", "send"],
    });
    const sync = engine(repo);
    const moved = vi.fn();
    sync.onMoved(moved);
    await sync.sync();
    expect(
      repo.log.filter((l) => !l.startsWith("GET") && !l.startsWith("fetch")),
    ).toEqual([
      `POST ${URL} git-upload-pack wants`,
      "integrate -> ahead",
      "pushRequest ad:git-receive-pack -> rejected",
      `POST ${URL} git-receive-pack push`,
      "pushFinish push answer:git-receive-pack",
      `POST ${URL} git-upload-pack wants`,
      "integrate -> merged",
      "pushRequest ad:git-receive-pack -> send",
      `POST ${URL} git-receive-pack push`,
      "pushFinish push answer:git-receive-pack",
    ]);
    expect(moved).toHaveBeenCalledTimes(1);
    const state = sync.getState();
    expect(state.kind === "on" && state.phase.kind).toBe("idle");
  });

  it("fetches and merges first when the remote is ahead of what it would push", async () => {
    const repo = fakeRepo({
      integrate: [
        { kind: "upToDate" },
        { kind: "merged", commit: "m", taken: [] },
      ],
      push: ["behind", "send"],
    });
    await engine(repo).sync();
    expect(
      repo.log.filter((l) => l.startsWith("integrate") || l.startsWith("push")),
    ).toEqual([
      "integrate -> upToDate",
      "pushRequest ad:git-receive-pack -> behind",
      "integrate -> merged",
      "pushRequest ad:git-receive-pack -> send",
      "pushFinish push answer:git-receive-pack",
    ]);
  });

  it("stops at a conflict, pushes nothing, and goes on once a side is chosen", async () => {
    const lines: SyncConflict = {
      branch: "main",
      conflict: {
        guard: "tips main a1 b1",
        files: [
          {
            needs: "wholeFile",
            path: "collection.toml",
            why: {
              kind: "lines",
              hunks: [{ base: ["x"], ours: ["y"], theirs: ["z"] }],
            },
          },
        ],
      },
    };
    const repo = fakeRepo({
      integrate: [
        { kind: "conflict", conflicts: [lines] },
        { kind: "merged", commit: "m", taken: ["decks/lantern.deck.toml"] },
      ],
      push: ["send"],
      facts: { ahead: 1 },
    });
    const sync = engine(repo);
    await sync.sync();
    let state = sync.getState();
    expect(state.kind === "on" && state.phase).toEqual({
      kind: "conflict",
      conflicts: [lines],
    });
    expect(repo.log.some((l) => l.startsWith("pushRequest"))).toBe(false);

    // Coming back into view does not run into it again.
    sync.focused();
    await Promise.resolve();
    expect(repo.log.filter((l) => l.startsWith("integrate"))).toHaveLength(1);

    await sync.resolve("theirs");
    expect(repo.log).toContain("integrate theirs -> merged");
    expect(repo.log).toContain("pushFinish push answer:git-receive-pack");
    state = sync.getState();
    expect(state.kind === "on" && state.phase.kind).toBe("idle");
  });

  it("settles each branch's merge by its own picks, under its conflict's guard", async () => {
    const clash = (branch: string, ours: string, theirs: string) => ({
      branch,
      conflict: {
        guard: `tips ${branch} here there`,
        files: [
          {
            needs: "eachClash" as const,
            path: "lantern.deck.toml",
            clashes: [
              {
                index: 0,
                subject: "Sol Ring",
                attribute: "quantity",
                ours,
                theirs,
              },
            ],
          },
        ],
      },
    });
    const conflicts = [
      clash("decks/lantern/budget", "5", "4"),
      clash("decks/lantern/main", "3", "2"),
    ];
    const repo = fakeRepo({
      integrate: [
        { kind: "conflict", conflicts },
        { kind: "merged", commit: "m", taken: [] },
      ],
      push: ["send"],
      facts: { ahead: 2 },
    });
    const sync = engine(repo);
    await sync.sync();
    let state = sync.getState();
    expect(state.kind === "on" && state.phase).toEqual({
      kind: "conflict",
      conflicts,
    });
    const sides = ["ours", "theirs"] as const;
    const picks = conflicts.map(({ conflict }, i) => ({
      guard: conflict.guard,
      files: conflict.files.map((f) => ({
        needs: "eachClash" as const,
        path: f.path,
        picks: [sides[i] ?? "ours"],
      })),
    }));
    await sync.settle(picks);
    expect(repo.settles.at(-1)).toEqual({ settles: picks });
    expect(repo.log).toContain("pushFinish push answer:git-receive-pack");
    state = sync.getState();
    expect(state.kind === "on" && state.phase.kind).toBe("idle");
  });

  it("stops at unrelated history until this repo takes the remote's", async () => {
    const repo = fakeRepo({
      integrate: [{ kind: "unrelated" }, { kind: "fastForwarded", to: "x" }],
    });
    const sync = engine(repo);
    await sync.connect(URL);
    let state = sync.getState();
    expect(state.kind === "on" && state.phase.kind).toBe("unrelated");
    await sync.replaceWithRemote();
    expect(repo.log.indexOf("adoptRemote")).toBeLessThan(
      repo.log.lastIndexOf("integrate -> fastForwarded"),
    );
    state = sync.getState();
    expect(state.kind === "on" && state.phase.kind).toBe("idle");
  });

  it("says a login is wanted rather than failing", async () => {
    const repo = fakeRepo({});
    const sync = engine(repo, () => new NeedsLogin("github.com wants a login"));
    await sync.sync();
    const state = sync.getState();
    expect(state.kind === "on" && state.phase).toEqual({ kind: "logged-out" });
  });

  it("says the repo could not be read, and reads it again on the next sync", async () => {
    const repo = fakeRepo({});
    const facts = repo.steps.facts;
    let broken = true;
    repo.steps.facts = () =>
      broken ? Promise.reject(new Error("object ffc45b1 is missing")) : facts();
    const sync = engine(repo);
    await sync.refresh();
    await sync.sync();
    expect(sync.getState()).toEqual({
      kind: "unreadable",
      message: "object ffc45b1 is missing",
    });
    expect(repo.log).toEqual([]);
    broken = false;
    await sync.sync();
    const state = sync.getState();
    expect(state.kind === "on" && state.phase).toEqual({ kind: "idle" });
  });

  it("is off without a remote, and removing one turns it off", async () => {
    const repo = fakeRepo({});
    const sync = engine(repo);
    await sync.disconnect();
    expect(sync.getState()).toEqual({ kind: "off" });
    await sync.sync();
    expect(repo.log).toEqual(["removeRemote"]);
  });
});

describe("pushing after a commit", () => {
  it("waits for the repo to be idle, and only when something waits to go", async () => {
    vi.useFakeTimers();
    const repo = fakeRepo({ push: ["send"] });
    const sync = engine(repo);
    await sync.refresh();

    sync.committed();
    await vi.advanceTimersByTimeAsync(PUSH_IDLE_MS * 2);
    expect(repo.log).toEqual([]);

    repo.setFacts({ ahead: 1 });
    sync.committed();
    await vi.advanceTimersByTimeAsync(PUSH_IDLE_MS / 2);
    sync.committed();
    await vi.advanceTimersByTimeAsync(PUSH_IDLE_MS / 2);
    expect(repo.log).toEqual([]);
    await vi.advanceTimersByTimeAsync(PUSH_IDLE_MS);
    expect(repo.log.filter((l) => l.startsWith("pushFinish"))).toHaveLength(1);
  });
});
