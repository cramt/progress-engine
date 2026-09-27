import { afterEach, describe, expect, it } from "vitest";
import {
  AUTH_ENDPOINTS,
  type ChannelLike,
  createAuth,
  pageLocks,
} from "./auth";
import { createMockGitHub } from "./mock";

const HOUR = 60 * 60 * 1000;
const open: ChannelLike[] = [];
const channel = (name: string) => {
  const c = new BroadcastChannel(name) as unknown as ChannelLike;
  open.push(c);
  return c;
};
afterEach(() => {
  for (const c of open.splice(0)) c.close();
});

const refreshCalls = (mock: ReturnType<typeof createMockGitHub>) =>
  mock.requests().filter((r) => r.url === AUTH_ENDPOINTS.refresh).length;

describe("the access token", () => {
  it("is fetched once from the worker and reused until near expiry", async () => {
    let now = 0;
    const mock = createMockGitHub({ now: () => now });
    const auth = createAuth({
      fetch: mock.fetch,
      channel: null,
      now: () => now,
    });
    const first = await auth.token();
    expect(first).toMatch(/^ghu_mock_/);
    now = 7 * HOUR;
    expect(await auth.token()).toBe(first);
    expect(refreshCalls(mock)).toBe(1);
    // Within five minutes of the eight hours, it refreshes first.
    now = 8 * HOUR - 4 * 60 * 1000;
    const second = await auth.token();
    expect(second).not.toBe(first);
    expect(refreshCalls(mock)).toBe(2);
  });

  it("is null when the worker has no session, and login leaves for the worker", async () => {
    const mock = createMockGitHub({ loggedIn: false });
    const went: string[] = [];
    const auth = createAuth({
      fetch: mock.fetch,
      channel: null,
      navigate: (u) => went.push(u),
    });
    expect(await auth.token()).toBeNull();
    auth.login("/deck/decks/lantern.deck.toml");
    expect(went).toEqual([
      "/api/auth/login?return=%2Fdeck%2Fdecks%2Flantern.deck.toml",
    ]);
  });

  it("refreshes once for concurrent callers in one tab", async () => {
    const mock = createMockGitHub();
    const auth = createAuth({ fetch: mock.fetch, channel: null });
    const tokens = await Promise.all([
      auth.token(),
      auth.token(),
      auth.token(),
    ]);
    expect(new Set(tokens).size).toBe(1);
    expect(refreshCalls(mock)).toBe(1);
  });

  it("refreshes once across two tabs: the second takes the first's broadcast", async () => {
    const mock = createMockGitHub();
    // One lock manager stands for navigator.locks, which every tab shares.
    const locks = pageLocks();
    const a = createAuth({
      fetch: mock.fetch,
      locks,
      channel: channel("tabs"),
    });
    const b = createAuth({
      fetch: mock.fetch,
      locks,
      channel: channel("tabs"),
    });
    const [ta, tb] = await Promise.all([a.token(), b.token()]);
    expect(ta).toBe(tb);
    expect(refreshCalls(mock)).toBe(1);
  });

  it("after a 401, a tab uses the token another tab already got", async () => {
    const mock = createMockGitHub();
    const locks = pageLocks();
    const a = createAuth({
      fetch: mock.fetch,
      locks,
      channel: channel("tabs2"),
    });
    const b = createAuth({
      fetch: mock.fetch,
      locks,
      channel: channel("tabs2"),
    });
    const old = await a.token();
    await new Promise((r) => setTimeout(r, 10));
    expect(b.peek()).toBe(old);
    // Tab B's API call got a 401 and refreshes; tab A hears of it.
    const fresh = await b.refreshed(old ?? "");
    await new Promise((r) => setTimeout(r, 10));
    expect(a.peek()).toBe(fresh);
    expect(await a.refreshed(old ?? "")).toBe(fresh);
    expect(refreshCalls(mock)).toBe(2);
  });

  it("logout clears every tab's token", async () => {
    const mock = createMockGitHub();
    const locks = pageLocks();
    const a = createAuth({
      fetch: mock.fetch,
      locks,
      channel: channel("tabs3"),
    });
    const b = createAuth({
      fetch: mock.fetch,
      locks,
      channel: channel("tabs3"),
    });
    await a.token();
    await new Promise((r) => setTimeout(r, 10));
    await a.logout();
    await new Promise((r) => setTimeout(r, 10));
    expect(b.peek()).toBeNull();
    expect(await b.token()).toBeNull();
  });
});
