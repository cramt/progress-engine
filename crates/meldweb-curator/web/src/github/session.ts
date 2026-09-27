/**
 * Where a page load stands: logged out, onboarding, refused by the repo
 * version, or in an open Magic repo. Every route asks this first, so it is
 * worked out once per page load and kept while it is `open`.
 */
import type { RepoRef } from "./api";
import { LoggedOutError } from "./api";
import { connect } from "./connect";
import { findMagicRepo, openMagicRepo } from "./repo";

export type Session =
  | { kind: "logged-out" }
  | { kind: "onboarding"; login: string; repoExists: boolean }
  | { kind: "refused"; message: string }
  | { kind: "open"; login: string; repo: RepoRef; version: number };

let open: Promise<Session> | null = null;

async function resolve(): Promise<Session> {
  const { auth, api } = await connect();
  if (!(await auth.token())) return { kind: "logged-out" };
  try {
    const found = await findMagicRepo(api);
    if (found.kind === "onboarding") return found;
    const opened = await openMagicRepo(api, found.repo);
    if (opened.kind === "refused") return opened;
    return {
      kind: "open",
      login: found.login,
      repo: found.repo,
      version: opened.version,
    };
  } catch (e) {
    if (e instanceof LoggedOutError) return { kind: "logged-out" };
    throw e;
  }
}

export function openSession(): Promise<Session> {
  if (open) return open;
  const next = resolve();
  open = next;
  // Only an open repo is worth keeping; anything else is asked again.
  next.then(
    (s) => {
      if (s.kind !== "open" && open === next) open = null;
    },
    () => {
      if (open === next) open = null;
    },
  );
  return next;
}

/** Forget the session, e.g. after onboarding or logging out. */
export function forgetSession(): void {
  open = null;
}
