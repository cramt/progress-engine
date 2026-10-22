/**
 * A deck's branches (ADR-0041): `decks/<stem>/<name>`, the same physical deck
 * with a different list, saved by itself like `main`. Only `main` is the
 * built deck. Which branches there are, what merging one does and what counts
 * as unmerged are Rust's (`meldweb-wasm`'s `branches`); this names them for
 * the page and says a refusal in its words.
 */
import {
  type Branch,
  ConflictError,
  type MergeAsk,
  type MergeDone,
  type MergePlan,
  type Repo,
} from "../repo/api";
import { slugify } from "./decks";

export type { Branch, MergeDone, MergePlan };

/** The branch every deck has, and the only one that is the built deck. */
export const MAIN = "main";

/** A branch's own name from what the user typed: `Budget test` → `budget-test`. */
export const branchName = slugify;

export type MadeBranch =
  | { kind: "made"; branch: Branch }
  | { kind: "refused"; message: string };

/** Makes the deck's branch named from `label` at `at`, a commit of the deck. */
export async function makeBranch(
  repo: Repo,
  stem: string,
  label: string,
  at: string,
): Promise<MadeBranch> {
  const name = branchName(label);
  if (!name) {
    return {
      kind: "refused",
      message: "A branch name needs a letter or a digit.",
    };
  }
  if (name === MAIN) {
    return { kind: "refused", message: "Every deck has a main already." };
  }
  try {
    return { kind: "made", branch: await repo.createBranch(stem, name, at) };
  } catch (e) {
    if (e instanceof ConflictError) {
      return {
        kind: "refused",
        message: `This deck already has a branch ${name}.`,
      };
    }
    throw e;
  }
}

/**
 * What a merge plan says before anything moves, for the reader: whether it
 * is a fast-forward, a merge commit, or stopped.
 */
export function planSummary(plan: MergePlan, ask: MergeAsk): string {
  switch (plan.kind) {
    case "upToDate":
      return `${ask.into} already has everything on ${ask.from}.`;
    case "fastForward":
      return `${ask.into} has no saves ${ask.from} lacks, so it takes ${ask.from}'s saves as they are, with no merge commit.`;
    case "merge":
      return `Both have saves the other lacks, so this is a merge commit with both as parents.`;
    case "conflict":
      return `${ask.into} and ${ask.from} changed the same things differently, so pick whose to keep where they clash. Everything else from both is kept.`;
    case "unrelated":
      return `${ask.into} and ${ask.from} share no history, so they do not merge.`;
  }
}

/** Plans then merges in one go, as a caller that has already shown the plan does. */
export async function mergeAsPlanned(
  repo: Repo,
  stem: string,
  ask: MergeAsk,
  plan: MergePlan,
): Promise<MergeDone> {
  if (plan.kind !== "fastForward" && plan.kind !== "merge") {
    if (plan.kind === "upToDate") return { kind: "upToDate" };
    return { kind: "stopped", message: planSummary(plan, ask) };
  }
  return repo.merge(stem, { ...ask, intoTip: plan.into, fromTip: plan.from });
}
