import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import type { HookCallback } from "@anthropic-ai/claude-agent-sdk";
import { PIN, PUSH, touches, writerGuard } from "./agents.ts";
import { batchCommitOnOrigin, pushRebasing } from "./git.ts";
import {
  failingCount,
  parseCoverage,
  parseLint,
  parseRoundtrip,
  snapDiff,
} from "./parse.ts";
import { triage } from "./queue.ts";
import { sample } from "./review.ts";
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

test("the writer guard denies push and pin, not commits that mention them", () => {
  for (const c of [
    "git push",
    "git push origin main",
    "git -C /pool/dev/progress-engine push",
    "cargo fmt --all && git push --force",
  ]) {
    assert.ok(PUSH.test(c), c);
  }
  for (const c of [
    'git commit -m "insight: a push effect parses"',
    "git log --oneline",
  ]) {
    assert.ok(!PUSH.test(c), c);
  }
  assert.ok(PIN.test("./target/release/insight pin my-decks"));
  assert.ok(
    PIN.test("cargo run --release -p vivisurgeons-insight -- pin 2000"),
  );
  assert.ok(!PIN.test('git commit -m "insight: pin counters on it"'));
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

// ---------- decisions.md is read-only to writers ----------

const DECISIONS = "/tmp/insight/driver/decisions.md";

test("a writer may read decisions.md but nothing may change it", () => {
  assert.ok(touches("Edit", { file_path: DECISIONS }, DECISIONS));
  assert.ok(touches("Write", { file_path: DECISIONS }, DECISIONS));
  assert.ok(touches("MultiEdit", { file_path: DECISIONS }, DECISIONS));
  assert.ok(
    touches(
      "Edit",
      { file_path: "/tmp/insight/driver/../driver/decisions.md" },
      DECISIONS,
    ),
  );
  for (const c of [
    `echo done >> ${DECISIONS}`,
    "cd /tmp/insight/driver && sed -i 's/x/y/' decisions.md",
    `cp /tmp/x ${DECISIONS}`,
  ]) {
    assert.ok(touches("Bash", { command: c }, DECISIONS), c);
  }
  assert.ok(!touches("Read", { file_path: DECISIONS }, DECISIONS));
  assert.ok(
    !touches(
      "Edit",
      { file_path: "/tmp/insight/driver/handoff.md" },
      DECISIONS,
    ),
  );
  assert.ok(!touches("Bash", { command: "cargo test" }, DECISIONS));
});

test("the writer guard denies writing decisions.md before anything else", async () => {
  const guard = writerGuard({
    repo: "/nonexistent",
    baseline: "HEAD",
    commitLimit: 15,
    softDeadline: Date.now() + 60_000,
    handoffPath: "/tmp/insight/driver/handoff.md",
    decisionsPath: DECISIONS,
  });
  const call = (tool_name: string, tool_input: unknown) =>
    guard(
      {
        hook_event_name: "PreToolUse",
        tool_name,
        tool_input,
      } as unknown as Parameters<HookCallback>[0],
      undefined,
      { signal: new AbortController().signal },
    );
  const out = await call("Write", { file_path: DECISIONS, content: "" });
  assert.equal(
    (out as { hookSpecificOutput?: { permissionDecision?: string } })
      .hookSpecificOutput?.permissionDecision,
    "deny",
  );
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
