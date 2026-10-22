/**
 * The page's side of the browser's repo: the worker holding it, and the
 * `Repo` every page reads and writes through.
 */
import { author } from "./author";
import { type Call, createLocalRepo, type LocalRepo } from "./local";
import type { Asked, Told } from "./protocol";

export function openLocalRepo(): LocalRepo {
  const worker = new Worker(new URL("./worker.ts", import.meta.url), {
    type: "module",
  });
  const waiting = new Map<number, (reply: Told["reply"]) => void>();
  let nextId = 0;
  worker.onmessage = ({ data }: MessageEvent<Told>) => {
    waiting.get(data.id)?.(data.reply);
    waiting.delete(data.id);
  };
  worker.onerror = (e) => {
    const message = e.message || "the repo's worker failed";
    for (const answer of waiting.values()) answer({ kind: "failed", message });
    waiting.clear();
  };
  const call = ((request) =>
    new Promise((resolve) => {
      const id = nextId++;
      waiting.set(id, resolve as (reply: Told["reply"]) => void);
      worker.postMessage({ id, request } satisfies Asked);
    })) as Call;
  return createLocalRepo(call, () => author());
}
