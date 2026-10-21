---
status: accepted
---

# A deck is its own line of history, and the repo lives in the browser

A player should get what git gives code: a tag for the list taken to a tournament, a branch to try things on, the deck as it was two weeks ago, a fork of a friend's deck that can still pull their changes, and later blame on a card. Archidekt copies a deck and the copy forgets where it came from. That is the thing Meldweb Curator should do better.

ADR-0024 kept every deck on one `main` and refused branches, because a branch forks the whole tree and `collection.toml` with it, and a copy of a card is in one place ([ADR-0023](0023-the-collection-is-one-file-and-a-copy-is-in-one-place.md)). The way out is to give each deck its own line of history in the same repo, and leave the collection on a line that never branches.

## The refs

- **`refs/heads/main`** holds what is the player's and not one deck's: `VERSION`, `collection.toml`, `wanted.toml`, `meldweb.toml` and `cards/`. It never branches. A copy of a card is therefore still in exactly one place, and a trade still reads another player's collection from their default branch ([ADR-0035](0035-a-trade-reads-another-players-public-collection-against-the-wanted-list.md)).
- **`refs/heads/decks/<stem>/<branch>`** is a deck. Its tree holds `<stem>.deck.toml`, beside its Gauntlet criteria, `<stem>.criteria.toml`. A checkout of it is something Gauntlet runs as it is. The criteria always live with the deck. They branch and fork with it, so a fork can be measured against different questions, and a branch's numbers are its own. Every deck has a `main` branch, made when the deck is. The stem is the deck's identity: fixed when the deck is created, and never following a rename, as the path was ([ADR-0021](0021-curator-owns-one-fixed-name-repo-and-saves-by-itself.md)).
- **`refs/tags/decks/<stem>/<slug>`** is a snapshot, an annotated tag as before ([ADR-0024](0024-a-decks-past-is-gits-and-a-variant-is-a-deck.md)), on any commit of that deck's line.

## What the words mean now

- **A branch** is the same physical deck with a different list: "trying out new stuff". It saves by itself like `main`, and it merges back into `main` as a real merge commit with two parents. The merge is three-way from the merge base, using the same line merge that settles a conflicting save. Only a deck's `main` is ever the built deck: the collection's deck place, Build, Sourcing and the wanted list all read `decks/<stem>/main`, so an experiment never asks you to buy or move a card. An open deck has one picker for its branches and tags, as a repo page on GitHub does, and a revision from history is the same view at a commit.
- **A fork** is another deck whose line starts from a commit of the deck it was forked from, yours or someone else's. Its history is the original's up to the fork, as real ancestors, so "what changed since I forked" and "take their changes" are a merge base and a three-way merge, not a diff against a copy. Your own budget build is a fork of your own deck. This replaces `variant_of`: the parent is in the ancestry. `meldweb.toml` names the deck a fork follows (`upstream`), because the git config does not survive a push, and the deck list keeps grouping forks with their parent as it groups variants now ([ADR-0028](0028-the-deck-lists-order-is-meldweb-toml-and-a-deck-moves-with-its-variants.md)).
- **Rolling back** stays ADR-0024's restore: take every change from the revision, as one new commit. History is never rewritten by an edit.
- **Blame** is per card line, not per text line. A card's line is the commit that last changed that card's line in the deck's file, found by walking the deck's line. It is useful alone, and it is what collaboration will need.

## The repo is in the browser, and a remote is optional

The repo is a real bare git repo in the browser's OPFS. It holds loose objects and refs, with no working tree and no index, and the Rust in `meldweb-wasm` reads and writes it through `meldweb-git` (gitoxide's object crates). Every object is a real git object, so another player's commits keep their ids in your repo. **A remote is optional, as in any git repo: offline is a repo with none.** It moves between computers as a zip of that bare repo, which `git clone` also reads on a desktop. A remote is a URL to any git host, GitHub, GitLab, Codeberg or a server of the player's own, and it is how a fork reaches someone else's repo. Push and fetch speak git's smart HTTP protocol and never a host's REST API, so nothing a repo does depends on the host. What does depend on the host is how a player logs in, and creating the repo there. Each host is an adapter for those two things alone, and GitHub is only the first adapter. Git requests go through the Worker, which relays git's smart HTTP requests to the remote's host and nothing else, because a browser can't reach a host whose git endpoints send no CORS headers. github.com's don't.

## Consequences

- Logging in anywhere is not a gate. The first visit makes an empty repo in the browser and opens the deck list, and the player can build, branch, tag and export without an account. Connecting a host is adding a remote, in the settings, beside importing a zip. GitHub's onboarding, creating `mtg` and installing the app, moves there as GitHub's adapter, and it runs only for a player who wants a GitHub remote. What needs someone else's repo, a trade or a fork of a friend's deck, asks for that then.
- Repo version 2. The migration rewrites each deck's commits into its own line, keeping messages, authors and dates. It moves each snapshot tag to the rewritten commit, turns every `variant_of` into a fork whose first commit has its parent's commit as parent, and leaves one commit on `main` that removes `decks/`. Commit ids change once, here.
- GitHub's default view shows `main` with no decks on it. A player browses decks through the branch list, or through Curator.
- The commit's author offline is a name and email set once per browser. It is kept outside the repo, as git keeps it outside.
- Saves no longer count against GitHub's write limit, and history, compare and blame need no API calls.

## Considered Options

- **Branches of the whole repo**, with the collection read from `main` only. Rejected. Every branch would carry a collection nobody may read, and a fork would bring in the other player's whole repo, collection and all, as ancestors of your deck.
- **A repo per deck.** Rejected. The collection and the wanted list read every deck, and one repo is one zip and one install of the GitHub App.
- **Keep GitHub's REST API as the store, with the new refs** (the Contents API takes a branch). Rejected as the end state. Merge, blame and history would each be many round trips against a rate limit, and nothing would work offline. It is how an online repo can keep working while the local one is built.
- **isomorphic-git.** Rejected. It would be a second model of the repo in TypeScript, and what a repo holds belongs in Rust like what a deck is.
- **Keep `variant_of`, and add branches beside it.** Rejected. Two ways of saying "this deck came from that one" would disagree, and only ancestry can find a merge base.
