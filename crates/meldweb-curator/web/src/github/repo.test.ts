import { beforeAll, describe, expect, it } from "vitest";
import { decodeBase64, encodeBase64, LoggedOutError } from "./api";
import { deckFile } from "./decks";
import { installUrl, newRepoUrl, resolveInstallUrl } from "./onboarding";
import { findMagicRepo, type Migration, openMagicRepo } from "./repo";
import {
  fileOf,
  loadWasm,
  messages,
  mockConnection,
  seedDecks,
} from "./testkit";

beforeAll(loadWasm);

describe("finding the Magic repo", () => {
  it("finds octocat/mtg through the app's installation", async () => {
    const { github } = mockConnection();
    expect(await findMagicRepo(github)).toEqual({
      kind: "found",
      login: "octocat",
      repo: { owner: "octocat", name: "mtg" },
    });
  });

  it("onboards when the repo is missing, and when the app is not on it", async () => {
    const none = mockConnection({ start: "no-repo" });
    expect(await findMagicRepo(none.github)).toEqual({
      kind: "onboarding",
      login: "octocat",
      repoExists: false,
    });
    const bare = mockConnection({ start: "no-install" });
    expect(await findMagicRepo(bare.github)).toMatchObject({
      kind: "onboarding",
      repoExists: true,
    });
  });

  it("onboarding's two steps end at the repo", async () => {
    const { github, mock } = mockConnection({ start: "no-repo" });
    mock.createRepo();
    expect((await findMagicRepo(github)).kind).toBe("onboarding");
    mock.installApp();
    expect((await findMagicRepo(github)).kind).toBe("found");
  });

  it("a logged-out user gets LoggedOutError, not a 401", async () => {
    const { github } = mockConnection({ loggedIn: false });
    await expect(findMagicRepo(github)).rejects.toBeInstanceOf(LoggedOutError);
  });

  it("a stale token is refreshed once and the call retried", async () => {
    const { github, auth, mock } = mockConnection();
    await auth.token();
    // Another tab rotated the refresh token, killing this one's access token.
    await mock.fetch("/api/auth/refresh", { method: "POST" });
    expect((await github.user()).login).toBe("octocat");
  });
});

describe("onboarding URLs", () => {
  it("prefill github.com/new with documented parameters only", () => {
    const url = new URL(newRepoUrl());
    expect(url.origin + url.pathname).toBe("https://github.com/new");
    expect([...url.searchParams.keys()].sort()).toEqual([
      "description",
      "name",
      "owner",
    ]);
    expect(url.searchParams.get("name")).toBe("mtg");
    expect(url.searchParams.get("owner")).toBe("@me");
  });

  it("take the install URL from the worker", async () => {
    const { mock } = mockConnection();
    expect(await resolveInstallUrl(mock.fetch)).toBe(
      "https://github.com/apps/meldweb-curator-mock/installations/new",
    );
  });

  it("install the app by its slug", () => {
    expect(installUrl("meldweb-curator")).toBe(
      "https://github.com/apps/meldweb-curator/installations/new",
    );
  });
});

describe("the repo version", () => {
  it("writes the version it knows into an empty repo", async () => {
    const { repo } = mockConnection();
    expect(await openMagicRepo(repo)).toEqual({
      kind: "open",
      version: 2,
    });
    expect(await fileOf(repo, "VERSION")).toBe("2\n");
    expect(await messages(repo, "VERSION")).toEqual([
      "curator: this repo is at version 2",
    ]);
    // Opening again writes nothing.
    await openMagicRepo(repo);
    expect(await messages(repo, "VERSION")).toHaveLength(1);
  });

  it("refuses a repo newer than Progress Engine knows, and changes nothing", async () => {
    const { repo } = mockConnection({ files: { VERSION: "3\n" } });
    const opened = await openMagicRepo(repo);
    expect(opened.kind).toBe("refused");
    if (opened.kind === "refused")
      expect(opened.message).toMatch(/version 3.*up to version 2/);
    expect(await messages(repo, "VERSION")).toEqual(["add VERSION"]);
  });

  it("refuses a VERSION that is not a number", async () => {
    const { repo } = mockConnection({ files: { VERSION: "one" } });
    expect((await openMagicRepo(repo)).kind).toBe("refused");
  });

  it("runs the migrations from an older version, each in turn", async () => {
    const { repo } = mockConnection({ files: { VERSION: "2\n" } });
    const to3: Migration = {
      from: 2,
      async migrate(repo) {
        const v = await repo.getFile("VERSION");
        await repo.putFile("VERSION", {
          text: "3\n",
          message: "curator: repo version 3",
          sha: v?.sha ?? null,
        });
      },
    };
    expect(await openMagicRepo(repo, { known: 3, migrations: [to3] })).toEqual({
      kind: "open",
      version: 3,
    });
    expect(await fileOf(repo, "VERSION")).toBe("3\n");
    expect(
      (await openMagicRepo(repo, { known: 4, migrations: [to3] })).kind,
    ).toBe("refused");
  });

  it("migrates a version-1 repo to a line per deck as it opens (ADR-0041)", async () => {
    const lantern = seedDecks()["decks/lantern.deck.toml"] ?? "";
    const { repo } = mockConnection({
      version: 1,
      files: {
        VERSION: "1\n",
        "decks/lantern.deck.toml": lantern,
        "decks/lantern-budget.deck.toml": `name = "Budget"\nvariant_of = "decks/lantern.deck.toml"\n${lantern}`,
        "collection.toml":
          'cards = []\n\n[places]\nLantern = { deck = "decks/lantern.deck.toml" }\n',
      },
    });
    expect(await openMagicRepo(repo)).toEqual({ kind: "open", version: 2 });
    expect(await fileOf(repo, "VERSION")).toBe("2\n");
    expect(await repo.listDir("decks")).toEqual([]);
    expect(await fileOf(repo, "collection.toml")).toContain(
      'Lantern = { deck = "lantern" }',
    );
    const decks = await repo.decks();
    expect(decks.map((d) => [d.stem, d.parent])).toEqual([
      ["lantern", undefined],
      ["lantern-budget", "lantern"],
    ]);
    const budget = deckFile("lantern-budget");
    expect(await fileOf(repo, budget.path, budget.branch)).not.toContain(
      "variant_of",
    );
    expect(await messages(repo, budget.path, budget.branch)).toEqual([
      "add decks/lantern-budget.deck.toml",
    ]);
  });
});

describe("base64", () => {
  it("round-trips UTF-8, as deck names can carry accents", () => {
    const text = 'name = "Lórien Revealed // ✓"\n';
    expect(decodeBase64(encodeBase64(text))).toBe(text);
  });
});
