import { log, must, run } from "./sh.ts";

const BRANCH = "main";

export async function head(repo: string): Promise<string> {
  return must("git", ["rev-parse", "HEAD"], { cwd: repo });
}

export async function branch(repo: string): Promise<string> {
  return must("git", ["rev-parse", "--abbrev-ref", "HEAD"], { cwd: repo });
}

/** origin/main as this clone last saw it, without fetching. */
export async function originMain(repo: string): Promise<string> {
  return must("git", ["rev-parse", `origin/${BRANCH}`], { cwd: repo });
}

export async function dirty(repo: string): Promise<string> {
  return must("git", ["status", "--porcelain"], { cwd: repo });
}

export async function isAncestor(
  repo: string,
  ancestor: string,
  descendant: string,
): Promise<boolean> {
  const r = await run(
    "git",
    ["merge-base", "--is-ancestor", ancestor, descendant],
    { cwd: repo },
  );
  return r.code === 0;
}

export type Commit = { sha: string; subject: string; body: string };

export async function commitsSince(
  repo: string,
  base: string,
): Promise<Commit[]> {
  const out = await must(
    "git",
    ["log", "--reverse", "--format=%H%x1f%s%x1f%b%x1e", `${base}..HEAD`],
    { cwd: repo },
  );
  return out
    .split("\x1e")
    .map((c) => c.trim())
    .filter(Boolean)
    .map((c) => {
      const [sha = "", subject = "", body = ""] = c.split("\x1f");
      return { sha, subject, body };
    });
}

export type Preflight =
  | { kind: "ready"; head: string }
  | { kind: "refused"; reason: string };

/**
 * A batch may only start on a clean main that matches origin/main, so every
 * commit past the baseline is the writer's and resetting can lose nothing else.
 */
export async function preflight(repo: string): Promise<Preflight> {
  const b = await branch(repo);
  if (b !== BRANCH)
    return { kind: "refused", reason: `on ${b}, not ${BRANCH}` };
  const d = await dirty(repo);
  if (d) return { kind: "refused", reason: `working tree is dirty:\n${d}` };
  await must("git", ["fetch", "origin", BRANCH], { cwd: repo });
  const h = await head(repo);
  const o = await originMain(repo);
  if (h !== o) {
    return {
      kind: "refused",
      reason: `${BRANCH} ${h.slice(0, 7)} is not origin/${BRANCH} ${o.slice(0, 7)}`,
    };
  }
  return { kind: "ready", head: h };
}

/** Why the batch's commits may not be reset, or null when they may. */
export async function resetHazard(
  repo: string,
  baseline: string,
): Promise<string | null> {
  const b = await branch(repo);
  if (b !== BRANCH) return `on ${b}, not ${BRANCH}`;
  const h = await head(repo);
  if (!(await isAncestor(repo, baseline, h))) {
    return `baseline ${baseline.slice(0, 7)} is not an ancestor of HEAD ${h.slice(0, 7)}`;
  }
  const o = await originMain(repo);
  if (o !== baseline) {
    return `origin/${BRANCH} moved from the baseline ${baseline.slice(0, 7)} to ${o.slice(0, 7)}`;
  }
  return null;
}

/**
 * Parks the batch's commits on a branch and puts main back on the baseline.
 * Refuses unless every commit it drops is one this batch made.
 */
export async function parkAndReset(
  repo: string,
  baseline: string,
  name: string,
): Promise<
  { kind: "parked"; branch: string } | { kind: "refused"; reason: string }
> {
  const hazard = await resetHazard(repo, baseline);
  if (hazard) return { kind: "refused", reason: hazard };
  const exists = await run("git", ["rev-parse", "--verify", "--quiet", name], {
    cwd: repo,
  });
  const target = exists.code === 0 ? `${name}-${Date.now()}` : name;
  await must("git", ["branch", target, "HEAD"], { cwd: repo });
  await must("git", ["reset", "--hard", baseline], { cwd: repo });
  log(
    "git",
    `commits parked on ${target}; ${BRANCH} reset to ${baseline.slice(0, 7)}`,
  );
  return { kind: "parked", branch: target };
}

export async function push(repo: string): Promise<string> {
  await must("git", ["push", "origin", BRANCH], { cwd: repo });
  const h = await head(repo);
  const o = await originMain(repo);
  if (h !== o)
    throw new Error(`pushed, but origin/${BRANCH} is ${o}, not ${h}`);
  return h;
}
