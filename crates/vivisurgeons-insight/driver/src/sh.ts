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

/** One line per step, to stdout and driver.log alike. */
export function log(step: string, message: string): void {
  const line = `${new Date().toISOString()} ${batchTag} ${step}: ${message.replaceAll("\n", " ")}`;
  console.log(line);
  if (logFile) appendFileSync(logFile, `${line}\n`);
}

export type Ran = { code: number; stdout: string; stderr: string; ms: number };

export function run(
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
