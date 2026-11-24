// What a batch spent, read from Claude Code's own transcripts. A T3 Code
// thread can't read its spend, but every session (the writer's and each
// sub-agent's) logs each API response's usage in
// ~/.claude/projects/<clone's path, slugged>/, so a batch is every response
// those dirs logged between its start and end. One product runs one batch at
// a time in its clone, so the window and the clone pick out the batch.
//
// Run alone to measure any window, Chancellor's batches included:
//   node src/usage.ts --clone /pool/dev/progress-engine-chancellor --from <iso> --to <iso>

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { parseArgs } from "node:util";

export type Usage = {
  responses: number;
  input: number;
  cacheWrite: number;
  cacheRead: number;
  output: number;
  models: string[];
};

/** Claude Code names a project's transcript dir by its path, non-alphanumerics as "-". */
export function slug(path: string): string {
  return path.replace(/[^a-zA-Z0-9]/g, "-");
}

/** The clone's dir and those of sessions started below it, never a sibling's. */
export function transcriptDirs(clone: string, root: string): string[] {
  if (!existsSync(root)) return [];
  const s = slug(clone);
  return readdirSync(root)
    .filter((d) => d === s || d.startsWith(`${s}-`))
    .map((d) => join(root, d));
}

function jsonlFiles(dir: string, since: number): string[] {
  const out: string[] = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) out.push(...jsonlFiles(p, since));
    // A file last written before the window holds nothing in it.
    else if (e.name.endsWith(".jsonl") && statSync(p).mtimeMs >= since)
      out.push(p);
  }
  return out;
}

export function usageBetween(dirs: string[], from: Date, to: Date): Usage {
  const t0 = from.getTime();
  const t1 = to.getTime();
  // A response with several content blocks is logged once per block, each
  // line repeating its usage.
  const seen = new Set<string>();
  const models = new Set<string>();
  const u: Usage = {
    responses: 0,
    input: 0,
    cacheWrite: 0,
    cacheRead: 0,
    output: 0,
    models: [],
  };
  for (const f of dirs.flatMap((d) => jsonlFiles(d, t0))) {
    for (const line of readFileSync(f, "utf8").split("\n")) {
      if (!line.includes('"usage"')) continue;
      let r: {
        type?: string;
        timestamp?: string;
        message?: {
          id?: string;
          model?: string;
          usage?: Record<string, number>;
        };
      };
      try {
        r = JSON.parse(line);
      } catch {
        continue;
      }
      const m = r.message;
      if (r.type !== "assistant" || !m?.id || !m.usage || !r.timestamp)
        continue;
      const t = Date.parse(r.timestamp);
      if (t < t0 || t > t1 || seen.has(m.id)) continue;
      seen.add(m.id);
      u.responses++;
      u.input += m.usage.input_tokens ?? 0;
      u.cacheWrite += m.usage.cache_creation_input_tokens ?? 0;
      u.cacheRead += m.usage.cache_read_input_tokens ?? 0;
      u.output += m.usage.output_tokens ?? 0;
      if (m.model && m.model !== "<synthetic>") models.add(m.model);
    }
  }
  u.models = [...models].sort();
  return u;
}

export const PROJECTS = join(homedir(), ".claude", "projects");

if (import.meta.main) {
  const { values } = parseArgs({
    options: {
      clone: { type: "string" },
      from: { type: "string" },
      to: { type: "string" },
    },
  });
  if (!values.clone || !values.from) {
    console.error("usage: usage.ts --clone <path> --from <iso> [--to <iso>]");
    process.exit(2);
  }
  const u = usageBetween(
    transcriptDirs(values.clone, PROJECTS),
    new Date(values.from),
    values.to ? new Date(values.to) : new Date(),
  );
  console.log(JSON.stringify(u));
}
