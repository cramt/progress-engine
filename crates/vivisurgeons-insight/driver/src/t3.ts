import { run } from "./sh.ts";

/**
 * Whether a batch's thread is alive is T3 Code's to say, not a clock's: a
 * claim in state.json outlives a thread that died, and a slow batch outlives
 * any fixed timeout. Statuses from `t3 thread list`.
 */
const LIVE = new Set(["preparing", "queued", "starting", "running", "waiting"]);

export type ThreadRow = {
  threadId: string;
  status: string;
  parentThreadId: string | null;
  pendingBackgroundTasks: unknown[];
};

/**
 * The top-level threads still working, other than `self`. A sub-agent
 * (a reviewer) has a parent, and its parent is what counts.
 */
export function liveOthers(
  rows: ThreadRow[],
  self: string | undefined,
): string[] {
  return rows
    .filter((r) => r.parentThreadId === null && r.threadId !== self)
    .filter((r) => LIVE.has(r.status) || r.pendingBackgroundTasks.length > 0)
    .map((r) => r.threadId);
}

/** `self` is the thread running this, from T3CODE_THREAD_ID. */
export async function otherLiveThreads(project: string): Promise<string[]> {
  const r = await run(
    "t3",
    ["thread", "list", "--project", project, "--json"],
    {
      cwd: project,
    },
  );
  if (r.code !== 0) {
    throw new Error(`t3 thread list exited ${r.code}: ${r.stderr.trim()}`);
  }
  return liveOthers(
    JSON.parse(r.stdout) as ThreadRow[],
    process.env.T3CODE_THREAD_ID,
  );
}
