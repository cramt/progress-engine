import { describe, expect, it } from "vitest";
import { decodeBase64, encodeBase64, LoggedOutError } from "./api";
import { installUrl, newRepoUrl, resolveInstallUrl } from "./onboarding";
import { findMagicRepo, type Migration, openMagicRepo } from "./repo";
import { mockConnection } from "./testkit";

describe("finding the Magic repo", () => {
  it("finds octocat/mtg through the app's installation", async () => {
    const { api } = mockConnection();
    expect(await findMagicRepo(api)).toEqual({
      kind: "found",
      login: "octocat",
      repo: { owner: "octocat", name: "mtg" },
    });
  });

  it("onboards when the repo is missing, and when the app is not on it", async () => {
    const none = mockConnection({ start: "no-repo" });
    expect(await findMagicRepo(none.api)).toEqual({
      kind: "onboarding",
      login: "octocat",
      repoExists: false,
    });
    const bare = mockConnection({ start: "no-install" });
    expect(await findMagicRepo(bare.api)).toMatchObject({
      kind: "onboarding",
      repoExists: true,
    });
  });

  it("onboarding's two steps end at the repo", async () => {
    const { api, mock } = mockConnection({ start: "no-repo" });
    mock.createRepo();
    expect((await findMagicRepo(api)).kind).toBe("onboarding");
    mock.installApp();
    expect((await findMagicRepo(api)).kind).toBe("found");
  });

  it("a logged-out user gets LoggedOutError, not a 401", async () => {
    const { api } = mockConnection({ loggedIn: false });
    await expect(findMagicRepo(api)).rejects.toBeInstanceOf(LoggedOutError);
  });

  it("a stale token is refreshed once and the call retried", async () => {
    const { api, auth, mock } = mockConnection();
    await auth.token();
    // Another tab rotated the refresh token, killing this one's access token.
    await mock.fetch("/api/auth/refresh", { method: "POST" });
    expect((await api.user()).login).toBe("octocat");
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
  it("writes VERSION 1 into an empty repo", async () => {
    const { api, mock, repo } = mockConnection({ start: "no-install" });
    mock.installApp();
    expect(await openMagicRepo(api, repo)).toEqual({
      kind: "open",
      version: 1,
    });
    expect(mock.file("VERSION")?.text).toBe("1\n");
    expect(mock.commits().map((c) => c.message)).toEqual([
      "curator: this repo is at version 1",
    ]);
    // Opening again writes nothing.
    await openMagicRepo(api, repo);
    expect(mock.commits()).toHaveLength(1);
  });

  it("refuses a repo newer than Curator knows, and changes nothing", async () => {
    const { api, mock, repo } = mockConnection({ files: { VERSION: "2\n" } });
    const opened = await openMagicRepo(api, repo);
    expect(opened.kind).toBe("refused");
    if (opened.kind === "refused")
      expect(opened.message).toMatch(/version 2.*up to version 1/);
    expect(mock.commits()).toHaveLength(0);
  });

  it("refuses a VERSION that is not a number", async () => {
    const { api, repo } = mockConnection({ files: { VERSION: "one" } });
    expect((await openMagicRepo(api, repo)).kind).toBe("refused");
  });

  it("runs the migrations from an older version, one commit each", async () => {
    const { api, mock, repo } = mockConnection({ files: { VERSION: "1\n" } });
    const to2: Migration = {
      from: 1,
      async migrate(api, repo) {
        const v = await api.getFile(repo, "VERSION");
        await api.putFile(repo, "VERSION", {
          text: "2\n",
          message: "curator: repo version 2",
          sha: v?.sha ?? null,
        });
      },
    };
    expect(
      await openMagicRepo(api, repo, { known: 2, migrations: [to2] }),
    ).toEqual({
      kind: "open",
      version: 2,
    });
    expect(mock.file("VERSION")?.text).toBe("2\n");
    expect(
      (await openMagicRepo(api, repo, { known: 3, migrations: [to2] })).kind,
    ).toBe("refused");
  });
});

describe("base64", () => {
  it("round-trips UTF-8, as deck names can carry accents", () => {
    const text = 'name = "Lórien Revealed // ✓"\n';
    expect(decodeBase64(encodeBase64(text))).toBe(text);
  });
});
