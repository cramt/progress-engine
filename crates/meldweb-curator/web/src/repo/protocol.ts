import type { Reply, Request } from "./local";

/** The Web Lock every tab's worker holds around the repo. */
export const LOCK = "meldweb-repo";
/** Where a worker says it moved the repo, so the others re-read its refs. */
export const CHANNEL = "meldweb-repo";
/** The repo's directory in the origin's private file system. */
export const DIRECTORY = "meldweb-repo";

/** What the page sends the worker. */
export interface Asked {
  id: number;
  request: Request;
}

/** What the worker sends back. */
export interface Told {
  id: number;
  reply: Reply;
}
