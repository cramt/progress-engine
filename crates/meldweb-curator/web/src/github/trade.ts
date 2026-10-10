/**
 * Another player's Magic repo, read to trade against (ADR-0035). Only a
 * public one can be read: the app is installed on the user's own repo alone.
 */
import type { GitHubApi, PublicFile } from "./api";
import { COLLECTION_PATH } from "./collection";
import { MAGIC_REPO } from "./repo";

declare const login: unique symbol;
/** A name GitHub would take for an account. */
export type Login = string & { readonly [login]: true };

/**
 * `text` as a GitHub login, which is letters, digits and single hyphens, 39
 * at most, with no hyphen at either end; null for anything else, so a typo
 * never reaches GitHub as a path.
 */
export function parseLogin(text: string): Login | null {
  const t = text.trim().replace(/^@/, "");
  return /^[a-z\d](?:[a-z\d]|-(?=[a-z\d])){0,38}$/i.test(t)
    ? (t as Login)
    : null;
}

/** Their `collection.toml`, as their public Magic repo has it. */
export function loadTheirCollection(
  api: GitHubApi,
  who: Login,
): Promise<PublicFile> {
  return api.publicFile({ owner: who, name: MAGIC_REPO }, COLLECTION_PATH);
}
