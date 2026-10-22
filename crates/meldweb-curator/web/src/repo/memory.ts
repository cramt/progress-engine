/**
 * The browser's repo held in memory, in the page itself: one "tab" over a
 * `memoryDisk`. Tests use it, and so does the dev mock (`VITE_MOCK_GITHUB=1`),
 * whose repo starts from a seed and is gone on reload.
 */
import type { Author } from "./author";
import { memoryDisk } from "./disk";
import {
  createLocalCore,
  createLocalRepo,
  type LocalCoreOptions,
  type LocalRepo,
} from "./local";

/** One lock for every "tab", as navigator.locks is one per origin. */
export function mutex(): LocalCoreOptions["lock"] {
  let held: Promise<unknown> = Promise.resolve();
  return (_mode, work) => {
    const run = held.then(work);
    held = run.catch(() => undefined);
    return run;
  };
}

export function memoryRepo(
  author: () => Author,
  clock: () => number = Date.now,
): LocalRepo {
  const core = createLocalCore({
    disk: memoryDisk(),
    lock: mutex(),
    announce: () => {},
  });
  return createLocalRepo((request) => core.handle(request), author, clock);
}
