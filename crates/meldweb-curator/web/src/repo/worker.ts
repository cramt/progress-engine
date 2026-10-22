/**
 * The browser's repo, in a worker of its own (ADR-0041): OPFS's sync access
 * handles exist only in a worker, and the repo's git runs off the main
 * thread. `local.ts` is all of what it does; this carries its messages.
 */
import init from "../wasm/pkg/meldweb_wasm.js";
import { opfsDisk } from "./disk";
import { createLocalCore, type Request } from "./local";
import { type Asked, CHANNEL, DIRECTORY, LOCK, type Told } from "./protocol";

const scope = globalThis as unknown as {
  postMessage(message: Told): void;
  onmessage: ((e: MessageEvent<Asked>) => void) | null;
};

const channel = new BroadcastChannel(CHANNEL);

const core = (async () => {
  await init();
  const root = await (
    await navigator.storage.getDirectory()
  ).getDirectoryHandle(DIRECTORY, { create: true });
  const made = createLocalCore({
    disk: opfsDisk(root),
    lock: (mode, work) => navigator.locks.request(LOCK, { mode }, work),
    announce: () => channel.postMessage("moved"),
  });
  channel.onmessage = () => made.moved();
  return made;
})();

scope.onmessage = ({ data: { id, request } }) => {
  core
    .then((c) =>
      c.handle(request as Extract<Request, { kind: typeof request.kind }>),
    )
    .catch((e: unknown) => ({
      kind: "failed" as const,
      message: e instanceof Error ? e.message : String(e),
    }))
    .then((reply) => scope.postMessage({ id, reply }));
};
