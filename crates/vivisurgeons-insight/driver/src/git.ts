import { log, must, run, tail } from "./sh.ts";

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

/**
 * The first of the batch's commits (those past `baseline` on HEAD) that
 * origin/main already holds, or null. Only that means the batch was pushed;
 * origin/main moving on its own is someone else's push, which the push's
 * rebase handles.
 */
export async function batchCommitOnOrigin(
  repo: string,
  baseline: string,
): Promise<string | null> {
  const onOrigin = new Set(
    (
      await must("git", ["rev-list", `${baseline}..origin/${BRANCH}`], {
        cwd: repo,
      })
    )
      .split("\n")
      .filter(Boolean),
  );
  const mine = await commitsSince(repo, baseline);
  return mine.find((c) => onOrigin.has(c.sha))?.sha ?? null;
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
  const target = await freeBranchName(repo, name);
  await must("git", ["branch", target, "HEAD"], { cwd: repo });
  await must("git", ["reset", "--hard", baseline], { cwd: repo });
  log(
    "git",
    `commits parked on ${target}; ${BRANCH} reset to ${baseline.slice(0, 7)}`,
  );
  return { kind: "parked", branch: target };
}

async function pushOnce(
  repo: string,
): Promise<
  { kind: "pushed"; head: string } | { kind: "rejected"; detail: string }
> {
  const r = await run("git", ["push", "origin", BRANCH], { cwd: repo });
  if (r.code !== 0)
    return { kind: "rejected", detail: tail(r.stderr || r.stdout, 8) };
  const h = await head(repo);
  const o = await originMain(repo);
  if (h !== o)
    return {
      kind: "rejected",
      detail: `pushed, but origin/${BRANCH} is ${o}, not ${h}`,
    };
  return { kind: "pushed", head: h };
}

/** Fetches and rebases main onto origin/main; a conflicted rebase is aborted. */
async function rebaseOntoOrigin(
  repo: string,
): Promise<
  | { kind: "rebased"; onto: string }
  | { kind: "conflict"; onto: string; detail: string }
> {
  await must("git", ["fetch", "origin", BRANCH], { cwd: repo });
  const onto = await originMain(repo);
  const r = await run("git", ["rebase", `origin/${BRANCH}`], { cwd: repo });
  if (r.code === 0) return { kind: "rebased", onto };
  await run("git", ["rebase", "--abort"], { cwd: repo });
  return {
    kind: "conflict",
    onto,
    detail: tail(`${r.stdout}\n${r.stderr}`, 12),
  };
}

export type Unpushed<F> =
  | { kind: "conflict"; onto: string; detail: string }
  | { kind: "gates"; onto: string; failure: F }
  | { kind: "rejected"; attempts: number; detail: string }
  | { kind: "error"; detail: string };

export type PushResult<P, F> =
  /** `regated` is the last re-gate's value, or null when main never moved. */
  | { kind: "pushed"; head: string; rebases: number; regated: P | null }
  | { kind: "unpushed"; branch: string; resetTo: string; cause: Unpushed<F> }
  | { kind: "refused"; reason: string; cause: Unpushed<F> };

/**
 * Pushes main. When someone else pushed first, rebases onto origin/main and
 * asks `regate` whether the rebased commits still pass against what is there
 * now, then tries again, at most `rebases` times. Anything that stops the push
 * (a conflict, a failed re-gate, a lost race every time, a git error) parks
 * main's commits on `parkAs` and resets main to origin/main; it never throws.
 */
export async function pushRebasing<P, F>(
  repo: string,
  o: {
    rebases: number;
    parkAs: string;
    regate: (
      onto: string,
      attempt: number,
    ) => Promise<{ ok: true; value: P } | { ok: false; failure: F }>;
  },
): Promise<PushResult<P, F>> {
  let regated: P | null = null;
  let cause: Unpushed<F>;
  try {
    for (let attempt = 0; ; attempt++) {
      const p = await pushOnce(repo);
      if (p.kind === "pushed")
        return { kind: "pushed", head: p.head, rebases: attempt, regated };
      if (attempt >= o.rebases) {
        cause = { kind: "rejected", attempts: attempt + 1, detail: p.detail };
        break;
      }
      log(
        "push",
        `rejected (${p.detail.split("\n")[0]}); rebasing onto origin/${BRANCH}`,
      );
      const rb = await rebaseOntoOrigin(repo);
      if (rb.kind === "conflict") {
        cause = { kind: "conflict", onto: rb.onto, detail: rb.detail };
        break;
      }
      log("push", `rebased onto ${rb.onto.slice(0, 7)}; gating again`);
      const g = await o.regate(rb.onto, attempt + 1);
      if (!g.ok) {
        cause = { kind: "gates", onto: rb.onto, failure: g.failure };
        break;
      }
      regated = g.value;
    }
  } catch (e) {
    cause = {
      kind: "error",
      detail: e instanceof Error ? e.message : String(e),
    };
  }
  try {
    const parked = await parkOnOrigin(repo, o.parkAs);
    return parked.kind === "parked"
      ? {
          kind: "unpushed",
          branch: parked.branch,
          resetTo: parked.resetTo,
          cause,
        }
      : { kind: "refused", reason: parked.reason, cause };
  } catch (e) {
    return {
      kind: "refused",
      reason: e instanceof Error ? e.message : String(e),
      cause,
    };
  }
}

/**
 * Parks main's commits on a branch and resets main to origin/main. Unlike
 * parkAndReset this drops nothing that isn't on the new branch, so it needs
 * only a clean main.
 */
async function parkOnOrigin(
  repo: string,
  name: string,
): Promise<
  | { kind: "parked"; branch: string; resetTo: string }
  | { kind: "refused"; reason: string }
> {
  // A re-gate that died mid-rebase can leave one open.
  await run("git", ["rebase", "--abort"], { cwd: repo });
  const b = await branch(repo);
  if (b !== BRANCH)
    return { kind: "refused", reason: `on ${b}, not ${BRANCH}` };
  const d = await dirty(repo);
  if (d) return { kind: "refused", reason: `working tree is dirty:\n${d}` };
  const target = await freeBranchName(repo, name);
  await must("git", ["branch", target, "HEAD"], { cwd: repo });
  // A rejected push doesn't move origin/main; without the network this resets
  // to the stale one, and the next preflight says so.
  await run("git", ["fetch", "origin", BRANCH], { cwd: repo });
  const o = await originMain(repo);
  await must("git", ["reset", "--hard", o], { cwd: repo });
  log(
    "git",
    `commits parked on ${target}; ${BRANCH} reset to origin/${BRANCH} ${o.slice(0, 7)}`,
  );
  return { kind: "parked", branch: target, resetTo: o };
}

async function freeBranchName(repo: string, name: string): Promise<string> {
  const exists = await run("git", ["rev-parse", "--verify", "--quiet", name], {
    cwd: repo,
  });
  return exists.code === 0 ? `${name}-${Date.now()}` : name;
}
