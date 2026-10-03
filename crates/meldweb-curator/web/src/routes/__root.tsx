import { createRootRoute, Outlet } from "@tanstack/react-router";
import { loadDeck } from "../deck";
import { persistScryfallCache } from "../scryfallCache";

let booted: Promise<void> | null = null;

/**
 * The parser and the restored Scryfall cache, once per page. Here rather than
 * before the first render so the splash spins while they load and a failure
 * reaches the error component instead of leaving the page blank. A failed boot
 * is forgotten, so "Try again" retries it.
 */
function boot(): Promise<void> {
  if (!booted) {
    booted = Promise.all([loadDeck(), persistScryfallCache()]).then(
      () => undefined,
      (e) => {
        booted = null;
        throw e;
      },
    );
  }
  return booted;
}

export const Route = createRootRoute({
  // Before every child loader: they parse decks and read the cache.
  beforeLoad: boot,
  component: () => <Outlet />,
});
