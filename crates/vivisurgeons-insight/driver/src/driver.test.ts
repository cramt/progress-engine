import assert from "node:assert/strict";
import { test } from "node:test";
import { PIN, PUSH } from "./agents.ts";
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
