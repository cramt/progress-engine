import { statSync } from "node:fs";
import { join } from "node:path";
import { parseCoverage, parseLint, parseRoundtrip } from "./parse.ts";
import { log, must, run, secs, tail } from "./sh.ts";
import type {
  BulkFingerprint,
  CoverageRow,
  Measurement,
  Milestone,
} from "./types.ts";

export type Paths = {
  repo: string;
  crate: string;
  /** The `insight` binary the measurements run. */
  insight: string;
};

export function pathsFor(repo: string): Paths {
  const target = process.env.CARGO_TARGET_DIR ?? join(repo, "target");
  return {
    repo,
    crate: join(repo, "crates/vivisurgeons-insight"),
    insight: join(target, "release/insight"),
  };
}

/** The insight CLI reads its bulk files from here (corpus::cache). */
function bulkFile(name: string): string {
  return join(process.env.HOME ?? "", ".cache/scryfall", name);
}

function fingerprint(path: string): string {
  const s = statSync(path);
  return `${s.size}@${s.mtimeMs}`;
}

export function bulkFingerprint(): BulkFingerprint {
  return {
    cards: fingerprint(bulkFile("oracle-cards.jsonl")),
    tags: fingerprint(bulkFile("oracle-tags.jsonl")),
  };
}

export async function buildRelease(
  repo: string,
  env?: NodeJS.ProcessEnv,
): Promise<void> {
  const r = await run(
    "cargo",
    ["build", "--release", "-p", "vivisurgeons-insight"],
    env ? { cwd: repo, env } : { cwd: repo },
  );
  if (r.code !== 0) {
    throw new Error(`release build failed:\n${tail(r.stderr, 40)}`);
  }
  log("build", `release build of insight in ${secs(r.ms)}`);
}

/** Coverage, round trip, lint and a snapshot, all from one binary. */
export async function measure(
  paths: Paths,
  snapshotPath: string,
): Promise<Measurement> {
  const opts = { cwd: paths.repo };
  const head = await must("git", ["rev-parse", "HEAD"], opts);
  const bulk = bulkFingerprint();
  const cov = await run(paths.insight, ["coverage", "--top", "0"], opts);
  if (cov.code !== 0)
    throw new Error(`insight coverage: ${tail(cov.stderr, 10)}`);
  const coverage = parseCoverage(cov.stdout);
  const all = row(coverage, "all cards");
  log(
    "measure",
    `coverage: abilities ${all.abilities.ok}/${all.abilities.total}, cards ${all.cards.ok}/${all.cards.total} (${coverage.length} rows, ${secs(cov.ms)})`,
  );
  const rt = await run(paths.insight, ["roundtrip", "--examples", "0"], opts);
  if (rt.code !== 0)
    throw new Error(`insight roundtrip: ${tail(rt.stderr, 10)}`);
  const roundtrip = parseRoundtrip(rt.stdout);
  log(
    "measure",
    `roundtrip: ${roundtrip.failures} failures of ${roundtrip.parsed} (${secs(rt.ms)})`,
  );
  const li = await run(paths.insight, ["lint", "--counts"], opts);
  if (li.code !== 0) throw new Error(`insight lint: ${tail(li.stderr, 10)}`);
  const lint = parseLint(li.stdout);
  log(
    "measure",
    `lint: ${Object.entries(lint)
      .filter(([, n]) => n > 0)
      .map(([k, n]) => `${k} ${n}`)
      .join(", ")} (${secs(li.ms)})`,
  );
  const sn = await run(paths.insight, ["snapshot", snapshotPath], opts);
  if (sn.code !== 0)
    throw new Error(`insight snapshot: ${tail(sn.stderr, 10)}`);
  log("measure", `snapshot: ${sn.stdout.trim()} (${secs(sn.ms)})`);
  if (JSON.stringify(bulk) !== JSON.stringify(bulkFingerprint())) {
    throw new Error("the Scryfall bulk files changed while measuring; rerun");
  }
  return {
    head,
    takenAt: new Date().toISOString(),
    coverage,
    roundtrip,
    lint,
    snapshotPath,
    bulk,
  };
}

export function row(coverage: CoverageRow[], label: string): CoverageRow {
  const r = coverage.find((c) => c.label === label);
  if (!r) throw new Error(`insight coverage printed no row '${label}'`);
  return r;
}

/** The failing abilities of a milestone, most played first. */
export async function failing(paths: Paths, m: Milestone): Promise<string> {
  return must(paths.insight, ["failing", m.target], { cwd: paths.repo });
}
