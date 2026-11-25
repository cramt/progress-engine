import { spawn } from "node:child_process";
import { appendFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

let logFile: string | null = null;
let batchTag = "-";

export function openLog(path: string): void {
  mkdirSync(dirname(path), { recursive: true });
  logFile = path;
}

export function setBatch(batch: number | null): void {
  batchTag = batch === null ? "-" : `b${batch}`;
}

/** One line per step, to stderr and driver.log; stdout is the result. */
export function log(step: string, message: string): void {
  const line = `${new Date().toISOString()} ${batchTag} ${step}: ${message.replaceAll("\n", " ")}`;
  console.error(line);
  if (logFile) appendFileSync(logFile, `${line}\n`);
}

export type Ran = { code: number; stdout: string; stderr: string; ms: number };

/**
 * A git fetch or push that failed because the network blinked, not because
 * origin said no. b45's finish crashed on one (DNS EAI_AGAIN, 2026-11-25) with
 * its gate passed, and the batch went unpushed until the overseer saw it.
 */
export const transientNetwork = (stderr: string): boolean =>
  /Could not resolve host|Temporary failure in name resolution|EAI_AGAIN|Connection (timed out|reset|refused|closed by remote host)|Network is unreachable|Operation timed out|unexpected disconnect|early EOF|The remote end hung up unexpectedly|kex_exchange_identification/i.test(
    stderr,
  );

/** Waits before each retry; about 8 minutes in all, a DNS outage's length. */
export const netRetry = { delaysMs: [15_000, 45_000, 120_000, 300_000] };

const isNetwork = (cmd: string, args: string[]): boolean =>
  cmd === "git" && (args[0] === "fetch" || args[0] === "push");

export async function run(
  cmd: string,
  args: string[],
  opts: { cwd: string; env?: NodeJS.ProcessEnv },
): Promise<Ran> {
  let r = await runOnce(cmd, args, opts);
  if (!isNetwork(cmd, args)) return r;
  for (const ms of netRetry.delaysMs) {
    if (r.code === 0 || !transientNetwork(r.stderr)) return r;
    log(
      "net",
      `${cmd} ${args[0]} failed on the network, retrying in ${secs(ms)}: ${tail(r.stderr, 1)}`,
    );
    await new Promise((res) => setTimeout(res, ms));
    r = await runOnce(cmd, args, opts);
  }
  return r;
}

function runOnce(
  cmd: string,
  args: string[],
  opts: { cwd: string; env?: NodeJS.ProcessEnv },
): Promise<Ran> {
  const started = Date.now();
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      cwd: opts.cwd,
      env: opts.env ?? process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const out: Buffer[] = [];
    const err: Buffer[] = [];
    child.stdout.on("data", (d: Buffer) => out.push(d));
    child.stderr.on("data", (d: Buffer) => err.push(d));
    child.on("error", reject);
    child.on("close", (code) =>
      resolve({
        code: code ?? -1,
        stdout: Buffer.concat(out).toString("utf8"),
        stderr: Buffer.concat(err).toString("utf8"),
        ms: Date.now() - started,
      }),
    );
  });
}

/** Runs a command that must succeed, returning trimmed stdout. */
export async function must(
  cmd: string,
  args: string[],
  opts: { cwd: string; env?: NodeJS.ProcessEnv },
): Promise<string> {
  const r = await run(cmd, args, opts);
  if (r.code !== 0) {
    throw new Error(
      `${cmd} ${args.join(" ")} exited ${r.code}: ${tail(r.stderr || r.stdout, 20)}`,
    );
  }
  return r.stdout.trim();
}

export function tail(text: string, lines: number): string {
  return text.trimEnd().split("\n").slice(-lines).join("\n");
}

export const secs = (ms: number): string => `${(ms / 1000).toFixed(1)}s`;
