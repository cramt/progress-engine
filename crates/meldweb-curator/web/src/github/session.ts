/**
 * Where a page load stands: in an open Magic repo, refused by the repo
 * version, or, with the dev mock's GitHub only, logged out or onboarding.
 * The browser's repo needs no login (ADR-0041). Every route asks this first,
 * so it is worked out once per page load and kept while it is `open`.
 */
import type { Repo } from "../repo/api";
import { author } from "../repo/author";
import { LoggedOutError, type PublicRepos } from "./api";
import { connect } from "./connect";
import { findMagicRepo, openMagicRepo } from "./repo";

export type Session =
  | { kind: "logged-out" }
  | { kind: "onboarding"; login: string; repoExists: boolean }
  | { kind: "refused"; message: string }
  | {
      kind: "open";
      repo: Repo;
      version: number;
      /**
       * Who the user is, to a friend they trade with: the GitHub login the
       * repo was opened with, or the author name set in this browser.
       */
      me: string;
      /** Other players' public repos, read to trade and source against. */
      others: PublicRepos;
    };

let open: Promise<Session> | null = null;

async function resolve(): Promise<Session> {
  const connection = await connect();
  if (connection.kind === "local") {
    const opened = await openMagicRepo(connection.repo);
    if (opened.kind === "refused") return opened;
    return {
      kind: "open",
      repo: connection.repo,
      version: opened.version,
      me: author().name,
      others: connection.others,
    };
  }
  const { auth, github } = connection;
  if (!(await auth.token())) return { kind: "logged-out" };
  try {
    const found = await findMagicRepo(github);
    if (found.kind === "onboarding") return found;
    const repo = github.repo(found.repo);
    const opened = await openMagicRepo(repo);
    if (opened.kind === "refused") return opened;
    return {
      kind: "open",
      repo,
      version: opened.version,
      me: found.login,
      others: connection.others,
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
