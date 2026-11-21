import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { pinnedCards } from "./gates.ts";
import {
  batchCommitOnOrigin,
  preflight,
  pushRebasing,
  setBranch,
} from "./git.ts";
import {
  failingCount,
  parseCoverage,
  parseLint,
  parseRoundtrip,
  snapDiff,
} from "./parse.ts";
import { triage } from "./queue.ts";
import { sample } from "./review.ts";
import { parseReport, parseReview } from "./schemas.ts";
import type { Queued } from "./types.ts";

test("coverage rows, as insight prints them", () => {
  const rows = parseCoverage(
    [
      "excluded 62 cards tagged otag:unique-cr-reference",
      "",
      "all cards                    cards  24378/33437   72.9%   abilities  50399/60625   83.1%",
      "EDHREC top 2000              cards   1763/1986    88.8%   abilities   3590/3847    93.3%",
      "my-decks                     cards    886/1022    86.7%   abilities   1988/2134    93.2%",
      '  not matched in the bulk file: ["Clown Car"]',
    ].join("\n"),
  );
  assert.deepEqual(rows[1], {
    label: "EDHREC top 2000",
    cards: { ok: 1763, total: 1986 },
    abilities: { ok: 3590, total: 3847 },
  });
  assert.equal(rows.length, 3);
});

test("roundtrip and lint counts", () => {
  assert.deepEqual(
    parseRoundtrip(
      "50365/50399 parsed abilities round-trip (99.9%); 10226 of 60625 lines did not parse\n",
    ),
    { ok: 50365, parsed: 50399, failures: 34 },
  );
  assert.deepEqual(parseLint("dangling-antecedent 9\nself-comparison 0\n"), {
    "dangling-antecedent": 9,
    "self-comparison": 0,
  });
  assert.throws(() => parseRoundtrip("nothing"));
});

test("failing counts one per rank line, not the indented ones", () => {
  const out = [
    "   12 Sol Ring",
    "      {T}: Add {C}{C}.",
    '      BROKE at "," after: x',
    "    0 Unranked",
    "      2 damage",
    '      BROKE at "." after: y',
  ].join("\n");
  assert.equal(failingCount(out), 2);
});

test("snapshot diff: lost, new and changed by card and text", () => {
  const old = ["A | x | T1", "B | y | T2", "C | z | T3"].join("\n");
  const now = ["A | x | T1", "C | z | T3b", "D | w | T4"].join("\n");
  const d = snapDiff(old, now);
  assert.deepEqual(d.lost, ["B | y"]);
  assert.deepEqual(
    d.added.map((r) => r.key),
    ["D | w"],
  );
  assert.deepEqual(d.changed, [
    { key: "C | z", card: "C", text: "z", old: ["T3"], new: ["T3b"] },
  ]);
});

test("sampling is even and deterministic", () => {
  const xs = Array.from({ length: 10 }, (_, i) => i);
  assert.deepEqual(sample(xs, 5), [0, 2, 4, 6, 8]);
  assert.deepEqual(sample(xs, 20), xs);
});

const q = (
  card: string,
  severity: "game" | "minor",
  regression = false,
): Queued => ({
  card,
  severity,
  regression,
  node: "Quant::Another",
  should_be: "One",
  batch: 1,
  source: "new-1",
});

test("triage: urgent to the queue head, minor to the backlog, no duplicates", () => {
  const r = triage(
    [q("Old Game", "game")],
    [q("Was Minor", "minor")],
    [
      q("New Minor", "minor"),
      q("New Regression", "game", true),
      q("old game", "game"),
      q("Was Minor", "game"),
    ],
  );
  assert.deepEqual(
    r.queue.map((x) => x.card),
    ["New Regression", "Was Minor", "Old Game"],
  );
  assert.deepEqual(
    r.backlog.map((x) => x.card),
    ["New Minor"],
  );
});

// ---------- what the thread leaves in files ----------

test("report.json: resolutions and escalations, or a reason it is refused", () => {
  const r = parseReport({
    summary: "abilities +12",
    resolved: [{ card: "Sol Ring", node: "Add", verdict: "fixed" }],
    escalations: [{ kind: "exclusion", cards: ["Ante Up"], question: "ante?" }],
  });
  assert.ok(r.ok);
  assert.equal(r.value.resolved[0]?.verdict, "fixed");
  assert.ok(!parseReport({ summary: "x", resolved: [] }).ok);
  assert.ok(
    !parseReport({
      summary: "x",
      resolved: [{ card: "A", node: "B", verdict: "maybe" }],
      escalations: [],
    }).ok,
  );
});

test("findings files: the reviewer shape, nothing looser", () => {
  const ok = parseReview({
    findings: [
      {
        card: "A",
        severity: "game",
        regression: false,
        node: "N",
        should_be: "M",
        semantics_row: "",
      },
    ],
  });
  assert.ok(ok.ok);
  assert.equal(ok.value.findings[0]?.semantics_row, undefined);
  assert.ok(!parseReview({ findings: [{ card: "A", severity: "major" }] }).ok);
  assert.ok(!parseReview([]).ok);
});

test("pinned cards count each card once, and absent is not zero", () => {
  assert.equal(pinnedCards("## A\nx\n## B\ny\n## A\n"), 2);
  assert.equal(pinnedCards(""), 0);
  assert.equal(pinnedCards(null), null);
});

// ---------- push races against a human committing to main ----------

function git(cwd: string, ...a: string[]): string {
  return execFileSync("git", a, { cwd, encoding: "utf8" }).trim();
}

/** A bare origin, the driver's clone and the human's, both on main. */
function repos(): { driver: string; human: string; origin: string } {
  const root = mkdtempSync(join(tmpdir(), "insight-push-"));
  const origin = join(root, "origin.git");
  execFileSync("git", ["init", "-q", "--bare", "-b", "main", origin]);
  const clone = (name: string) => {
    const d = join(root, name);
    execFileSync("git", ["clone", "-q", origin, d], { stdio: "ignore" });
    git(d, "config", "user.name", name);
    git(d, "config", "user.email", `${name}@example.com`);
    git(d, "config", "commit.gpgsign", "false");
    git(d, "checkout", "-q", "-B", "main");
    return d;
  };
  const human = clone("human");
  writeFileSync(join(human, "a.txt"), "one\n");
  git(human, "add", ".");
  git(human, "commit", "-qm", "base");
  git(human, "push", "-q", "origin", "main");
  const driver = clone("driver");
  return { driver, human, origin };
}

function commit(dir: string, f: string, text: string, msg: string): void {
  writeFileSync(join(dir, f), text);
  git(dir, "add", ".");
  git(dir, "commit", "-qm", msg);
}

function humanPushes(r: { human: string }, f: string, text: string): string {
  git(r.human, "pull", "-q", "--rebase", "origin", "main");
  commit(r.human, f, text, `human ${f}`);
  git(r.human, "push", "-q", "origin", "main");
  return git(r.human, "rev-parse", "HEAD");
}

const originHead = (r: { origin: string }) =>
  git(r.origin, "rev-parse", "main");

test("push: nothing moved, so no rebase and no re-gate", async () => {
  const r = repos();
  commit(r.driver, "b.txt", "writer\n", "insight: b");
  let regates = 0;
  const p = await pushRebasing(r.driver, {
    rebases: 3,
    parkAs: "insight/unpushed-1",
    regate: async () => {
      regates++;
      return { ok: true, value: null };
    },
  });
  assert.equal(p.kind, "pushed");
  assert.equal(regates, 0);
  assert.equal(originHead(r), git(r.driver, "rev-parse", "HEAD"));
});

test("push: main moved cleanly, so rebase, re-gate on origin's head, push", async () => {
  const r = repos();
  commit(r.driver, "b.txt", "writer\n", "insight: b");
  const human = humanPushes(r, "c.txt", "human\n");
  const ontos: string[] = [];
  const p = await pushRebasing(r.driver, {
    rebases: 3,
    parkAs: "insight/unpushed-2",
    regate: async (onto) => {
      ontos.push(onto);
      return { ok: true, value: `gated at ${onto}` };
    },
  });
  assert.ok(p.kind === "pushed");
  assert.equal(p.rebases, 1);
  assert.deepEqual(ontos, [human]);
  assert.equal(p.regated, `gated at ${human}`);
  assert.equal(originHead(r), p.head);
  assert.equal(git(r.driver, "rev-parse", "HEAD~1"), human);
});

test("push: a conflicting rebase is aborted, parked, and main reset to origin", async () => {
  const r = repos();
  commit(r.driver, "a.txt", "writer\n", "insight: a");
  const mine = git(r.driver, "rev-parse", "HEAD");
  const human = humanPushes(r, "a.txt", "human\n");
  const p = await pushRebasing(r.driver, {
    rebases: 3,
    parkAs: "insight/unpushed-3",
    regate: async () => assert.fail("a conflict is never re-gated"),
  });
  assert.ok(p.kind === "unpushed");
  assert.equal(p.cause.kind, "conflict");
  assert.equal(p.branch, "insight/unpushed-3");
  assert.equal(git(r.driver, "rev-parse", "insight/unpushed-3"), mine);
  assert.equal(git(r.driver, "rev-parse", "HEAD"), human);
  assert.equal(git(r.driver, "status", "--porcelain"), "");
  assert.equal(originHead(r), human);
});

test("push: a rebase that fails the re-gate is parked rebased; main reset", async () => {
  const r = repos();
  commit(r.driver, "b.txt", "writer\n", "insight: b");
  const human = humanPushes(r, "c.txt", "human\n");
  const p = await pushRebasing(r.driver, {
    rebases: 3,
    parkAs: "insight/unpushed-4",
    regate: async () => ({ ok: false, failure: "abilities 10 -> 9" }),
  });
  assert.ok(p.kind === "unpushed");
  assert.deepEqual(p.cause, {
    kind: "gates",
    onto: human,
    failure: "abilities 10 -> 9",
  });
  assert.equal(p.resetTo, human);
  assert.equal(git(r.driver, "rev-parse", "HEAD"), human);
  assert.equal(git(r.driver, "rev-parse", "insight/unpushed-4~1"), human);
  assert.equal(originHead(r), human);
});

test("push: losing the race every time gives up after three rebases", async () => {
  const r = repos();
  commit(r.driver, "b.txt", "writer\n", "insight: b");
  humanPushes(r, "c0.txt", "0\n");
  let n = 0;
  const p = await pushRebasing(r.driver, {
    rebases: 3,
    parkAs: "insight/unpushed-5",
    regate: async () => {
      n++;
      humanPushes(r, `c${n}.txt`, `${n}\n`);
      return { ok: true, value: n };
    },
  });
  assert.ok(p.kind === "unpushed");
  assert.equal(p.cause.kind, "rejected");
  assert.equal(n, 3);
  assert.equal(git(r.driver, "rev-parse", "HEAD"), originHead(r));
});

test("push: a re-gate that throws parks instead of crashing", async () => {
  const r = repos();
  commit(r.driver, "b.txt", "writer\n", "insight: b");
  humanPushes(r, "c.txt", "human\n");
  const p = await pushRebasing(r.driver, {
    rebases: 3,
    parkAs: "insight/unpushed-6",
    regate: async () => {
      throw new Error("cargo build exploded");
    },
  });
  assert.ok(p.kind === "unpushed");
  assert.deepEqual(p.cause, { kind: "error", detail: "cargo build exploded" });
  assert.equal(git(r.driver, "rev-parse", "HEAD"), originHead(r));
});

test("gate: origin moving is the writer pushing only when it holds a batch commit", async () => {
  const r = repos();
  const baseline = git(r.driver, "rev-parse", "HEAD");
  commit(r.driver, "b.txt", "writer\n", "insight: b");
  humanPushes(r, "c.txt", "human\n");
  git(r.driver, "fetch", "-q", "origin", "main");
  assert.equal(await batchCommitOnOrigin(r.driver, baseline), null);

  const r2 = repos();
  const base2 = git(r2.driver, "rev-parse", "HEAD");
  commit(r2.driver, "b.txt", "writer\n", "insight: b");
  const mine = git(r2.driver, "rev-parse", "HEAD");
  commit(r2.driver, "d.txt", "writer\n", "insight: d");
  git(r2.driver, "push", "-q", "origin", `${mine}:main`);
  assert.equal(await batchCommitOnOrigin(r2.driver, base2), mine);
});

// ---------- a driver branch that takes in main ----------

/** `repos()` with the driver on its own `insight` branch, pushed. */
function branchRepos(): ReturnType<typeof repos> {
  const r = repos();
  git(r.driver, "checkout", "-q", "-b", "insight");
  git(r.driver, "push", "-q", "-u", "origin", "insight");
  return r;
}

async function onBranch<T>(f: () => Promise<T>): Promise<T> {
  setBranch("insight");
  try {
    return await f();
  } finally {
    setBranch("main");
  }
}

test("preflight: a branch takes in what the human pushed to main, and pushes it", async () => {
  const r = branchRepos();
  commit(r.driver, "b.txt", "writer\n", "insight: b");
  git(r.driver, "push", "-q", "origin", "insight");
  const human = humanPushes(r, "c.txt", "human\n");
  const pf = await onBranch(() => preflight(r.driver));
  assert.ok(pf.kind === "ready");
  assert.equal(git(r.origin, "rev-parse", "insight"), pf.head);
  assert.equal(
    git(r.driver, "merge-base", "--is-ancestor", human, pf.head),
    "",
  );
});

test("preflight: a branch main conflicts with is left alone and refused", async () => {
  const r = branchRepos();
  commit(r.driver, "a.txt", "writer\n", "insight: a");
  git(r.driver, "push", "-q", "origin", "insight");
  const mine = git(r.driver, "rev-parse", "HEAD");
  humanPushes(r, "a.txt", "human\n");
  const pf = await onBranch(() => preflight(r.driver));
  assert.equal(pf.kind, "refused");
  assert.equal(git(r.driver, "rev-parse", "HEAD"), mine);
  assert.equal(git(r.driver, "status", "--porcelain"), "");
  assert.equal(git(r.origin, "rev-parse", "insight"), mine);
});

test("a claim is held only by another top-level thread T3 calls live", async () => {
  const { liveOthers } = await import("./t3.ts");
  const row = (threadId: string, status: string, extra = {}) => ({
    threadId,
    status,
    parentThreadId: null,
    pendingBackgroundTasks: [],
    ...extra,
  });
  assert.deepEqual(
    liveOthers(
      [
        row("me", "running"),
        row("dead", "completed"),
        row("crashed", "failed"),
        row("reviewer", "running", { parentThreadId: "me" }),
      ],
      "me",
    ),
    [],
  );
  assert.deepEqual(liveOthers([row("other", "waiting")], "me"), ["other"]);
  assert.deepEqual(
    liveOthers(
      [row("bg", "completed", { pendingBackgroundTasks: [{}] })],
      undefined,
    ),
    ["bg"],
  );
});
