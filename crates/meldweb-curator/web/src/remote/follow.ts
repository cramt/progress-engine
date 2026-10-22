/**
 * Another player's deck, over git (ADR-0041): what their repo offers to
 * fork, fetching one deck's line of it, forking it, and fetching it again to
 * see what is new. Their repo is read with no login, through the same relay
 * as the user's own remote, so only a public one answers. What the bytes say
 * is Rust's (`meldweb-wasm`'s `upstream`).
 */
import type { Fetched, Forked, TheirDecks } from "../deck.gen";
import type { Repo, TheirDeck } from "../repo/api";
import { their_decks } from "../wasm/pkg/meldweb_wasm.js";
import { FOCUS_MS } from "./sync";
import {
  ANONYMOUS,
  type GitTransport,
  NeedsLogin,
  relayTransport,
} from "./transport";

export type { TheirDeck, TheirDecks };

/** The Magic repo's name on GitHub, as every player's is (ADR-0021). */
const MAGIC_REPO = "mtg";

/**
 * Their repo's URL from what the user typed: a GitHub login (`alice`,
 * `@alice`) is their `mtg` repo there, and an `https://` git URL is itself.
 * Null for anything else.
 */
export function theirRepoUrl(text: string): string | null {
  const t = text.trim();
  if (/^@?[a-z\d](?:[a-z\d]|-(?=[a-z\d])){0,38}$/i.test(t))
    return `https://github.com/${t.replace(/^@/, "")}/${MAGIC_REPO}.git`;
  try {
    const u = new URL(t);
    if (u.protocol !== "https:" || u.search || u.hash || u.username)
      return null;
    const path = u.pathname.replace(/\/+$/, "");
    if (path.split("/").filter(Boolean).length < 2) return null;
    return `https://${u.host}${path}`;
  } catch {
    return null;
  }
}

/** Who keeps the repo at `url`, as the page names them: its path's first part. */
export function ownerOf(url: string): string {
  try {
    return new URL(url).pathname.split("/").filter(Boolean)[0] ?? url;
  } catch {
    return url;
  }
}

const anonymous = () => relayTransport({ credentials: ANONYMOUS });

function unreachable(url: string, e: unknown): Error {
  if (e instanceof NeedsLogin)
    return new Error(
      `${url} did not answer without a login: it has to be a public repo.`,
    );
  return e instanceof Error ? e : new Error(String(e));
}

/** The decks their repo offers, from its refs alone. */
export async function theirDecks(
  url: string,
  git: GitTransport = anonymous(),
): Promise<TheirDecks> {
  try {
    const advertised = await git.advertise(url, "git-upload-pack");
    return JSON.parse(their_decks(advertised)) as TheirDecks;
  } catch (e) {
    throw unreachable(url, e);
  }
}

/** Fetches their deck's line alone into the repo, and tracks it. */
export async function fetchTheirs(
  repo: Repo,
  theirs: TheirDeck,
  git: GitTransport = anonymous(),
): Promise<Fetched> {
  try {
    const advertised = await git.advertise(theirs.url, "git-upload-pack");
    const want = await repo.followRequest(theirs, advertised);
    const response = want
      ? await git.post(theirs.url, "git-upload-pack", want)
      : null;
    return await repo.followFinish(theirs, advertised, response);
  } catch (e) {
    throw unreachable(theirs.url, e);
  }
}

/** Fetches their deck and forks it as `stem`. */
export async function forkTheirs(
  repo: Repo,
  theirs: TheirDeck,
  stem: string,
  git: GitTransport = anonymous(),
): Promise<Forked> {
  await fetchTheirs(repo, theirs, git);
  return repo.fork(theirs, stem);
}

const lastFetched = new Map<string, number>();

/**
 * Fetches their deck unless it was fetched in the last `FOCUS_MS`, as a
 * page coming back into view syncs: `true` when it fetched.
 */
export async function refreshTheirs(
  repo: Repo,
  theirs: TheirDeck,
  options: { force?: boolean; now?: () => number; git?: GitTransport } = {},
): Promise<boolean> {
  const now = options.now ?? Date.now;
  const key = `${theirs.url} ${theirs.deck} ${theirs.branch}`;
  const was = lastFetched.get(key);
  if (!options.force && was !== undefined && now() - was < FOCUS_MS)
    return false;
  lastFetched.set(key, now());
  await fetchTheirs(repo, theirs, options.git);
  return true;
}

/** A local stem for their deck: theirs, unless one of `taken` has it. */
export function freeStem(
  theirs: string,
  owner: string,
  taken: ReadonlySet<string>,
): string {
  if (!taken.has(theirs)) return theirs;
  const base = `${theirs}-${owner.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
  if (!taken.has(base)) return base;
  for (let n = 2; ; n++) if (!taken.has(`${base}-${n}`)) return `${base}-${n}`;
}
