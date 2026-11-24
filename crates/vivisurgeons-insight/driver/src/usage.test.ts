import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { slug, transcriptDirs, usageBetween } from "./usage.ts";

const line = (id: string, at: string, output: number) =>
  JSON.stringify({
    type: "assistant",
    timestamp: at,
    message: {
      id,
      model: "claude-opus-5-5",
      usage: {
        input_tokens: 1,
        cache_creation_input_tokens: 10,
        cache_read_input_tokens: 100,
        output_tokens: output,
      },
    },
  });

test("a batch is the clone's responses in its window, each counted once", () => {
  const root = mkdtempSync(join(tmpdir(), "usage-"));
  const clone = "/pool/dev/progress-engine-insight";
  const mine = join(root, slug(clone));
  mkdirSync(join(mine, "s1", "subagents"), { recursive: true });
  writeFileSync(
    join(mine, "s1.jsonl"),
    [
      line("a", "2026-11-24T06:00:00Z", 1000), // before the batch
      line("b", "2026-11-24T07:00:00Z", 5),
      line("b", "2026-11-24T07:00:00Z", 5), // one response, two blocks
      '{"type":"user","message":{"content":"hi"}}',
    ].join("\n"),
  );
  writeFileSync(
    join(mine, "s1", "subagents", "agent-x.jsonl"),
    line("c", "2026-11-24T07:30:00Z", 7),
  );
  // The overseer's checkout is a prefix of the clone's path, not below it.
  const parent = join(root, slug("/pool/dev/progress-engine"));
  mkdirSync(parent);
  writeFileSync(join(parent, "o.jsonl"), line("d", "2026-11-24T07:10:00Z", 9));

  const dirs = transcriptDirs(clone, root);
  assert.deepEqual(dirs, [mine]);
  const u = usageBetween(
    dirs,
    new Date("2026-11-24T06:30:00Z"),
    new Date("2026-11-24T08:00:00Z"),
  );
  assert.equal(u.responses, 2);
  assert.equal(u.output, 12);
  assert.equal(u.cacheRead, 200);
  assert.deepEqual(u.models, ["claude-opus-5-5"]);
});
