/**
 * Where the browser's repo keeps its files: the bare git dir's paths
 * (`HEAD`, `objects/ab/cdef…`, `refs/heads/main`) under a slot directory, in
 * the origin's private file system. This moves bytes and knows nothing of
 * git; `local.ts` and the Rust decide what to read and write.
 */

export interface Disk {
  /** The file's bytes, or null when there is none. */
  read(path: string): Promise<Uint8Array | null>;
  /** Replaces the file, making the directories it needs. */
  write(path: string, bytes: Uint8Array): Promise<void>;
  /** Removes the file; a missing one is already removed. */
  remove(path: string): Promise<void>;
  /** Makes the directory, so an empty one is still there. */
  makeDir(path: string): Promise<void>;
  /** Removes the directory and everything in it. */
  removeDir(path: string): Promise<void>;
  /** Every file and directory under `dir` at any depth, relative to it. */
  list(dir: string): Promise<{ files: string[]; dirs: string[] }>;
}

const parts = (path: string) => path.split("/").filter((p) => p !== "");

/** A disk held in memory, which tests share between two "tabs". */
export function memoryDisk(): Disk & { files: Map<string, Uint8Array> } {
  const files = new Map<string, Uint8Array>();
  const dirs = new Set<string>();
  const under = (dir: string, path: string) =>
    dir === "" || path.startsWith(`${dir.replace(/\/$/, "")}/`);
  const relative = (dir: string, path: string) =>
    dir === "" ? path : path.slice(dir.replace(/\/$/, "").length + 1);
  return {
    files,
    async read(path) {
      const bytes = files.get(path);
      return bytes ? bytes.slice() : null;
    },
    async write(path, bytes) {
      files.set(path, bytes.slice());
    },
    async remove(path) {
      files.delete(path);
    },
    async makeDir(path) {
      dirs.add(path);
    },
    async removeDir(path) {
      for (const p of [...files.keys()]) if (under(path, p)) files.delete(p);
      for (const d of [...dirs])
        if (d === path || under(path, d)) dirs.delete(d);
    },
    async list(dir) {
      return {
        files: [...files.keys()]
          .filter((p) => under(dir, p))
          .map((p) => relative(dir, p)),
        dirs: [...dirs]
          .filter((d) => under(dir, d))
          .map((d) => relative(dir, d)),
      };
    },
  };
}

// The handle a worker may write a file through. TypeScript's DOM library
// leaves it out, since a window cannot have one.
interface SyncAccess {
  write(data: Uint8Array, at: { at: number }): number;
  truncate(size: number): void;
  flush(): void;
  close(): void;
}
type WritableFile = FileSystemFileHandle & {
  createSyncAccessHandle(): Promise<SyncAccess>;
};
type Listable = FileSystemDirectoryHandle & {
  entries(): AsyncIterable<[string, FileSystemHandle]>;
};

/** Writes not yet moved into place, outside every slot. */
const TMP = "tmp";

// `move` is in every browser with OPFS, but not in TypeScript's DOM library.
type MovableFile = FileSystemFileHandle & {
  move(parent: FileSystemDirectoryHandle, name: string): Promise<void>;
};

/** Replaces what `handle` holds with `bytes`. */
async function fill(handle: FileSystemFileHandle, bytes: Uint8Array) {
  // A writable stream swaps the file in whole on close, so a tab closed
  // mid-write never leaves a ref half written. Where there is none, a sync
  // access handle, with the lock around every write keeping readers out
  // meanwhile.
  if ("createWritable" in handle) {
    const out = await handle.createWritable();
    await out.write(bytes as Uint8Array<ArrayBuffer>);
    await out.close();
    return;
  }
  const access = await (handle as WritableFile).createSyncAccessHandle();
  try {
    access.truncate(0);
    let at = 0;
    while (at < bytes.length) at += access.write(bytes.subarray(at), { at });
    access.flush();
  } finally {
    access.close();
  }
}

const isMissing = (e: unknown) =>
  e instanceof DOMException &&
  (e.name === "NotFoundError" || e.name === "TypeMismatchError");

/**
 * The origin-private file system under `root`. Only a worker can write
 * through it: sync access handles exist nowhere else.
 */
export function opfsDisk(root: FileSystemDirectoryHandle): Disk {
  async function dir(path: string[], create: boolean) {
    let at = root;
    for (const name of path) {
      at = await at.getDirectoryHandle(name, { create });
    }
    return at;
  }
  async function parentOf(path: string, create: boolean) {
    const p = parts(path);
    const name = p.pop();
    if (name === undefined) throw new Error(`${path} names no file`);
    return { parent: await dir(p, create), name };
  }

  async function walk(
    at: FileSystemDirectoryHandle,
    prefix: string,
    out: { files: string[]; dirs: string[] },
  ) {
    for await (const [name, handle] of (at as Listable).entries()) {
      const path = `${prefix}${name}`;
      if (handle.kind === "directory") {
        out.dirs.push(path);
        await walk(handle as FileSystemDirectoryHandle, `${path}/`, out);
      } else {
        out.files.push(path);
      }
    }
  }

  return {
    async read(path) {
      try {
        const { parent, name } = await parentOf(path, false);
        const file = await (await parent.getFileHandle(name)).getFile();
        return new Uint8Array(await file.arrayBuffer());
      } catch (e) {
        if (isMissing(e)) return null;
        throw e;
      }
    },
    async write(path, bytes) {
      const { parent, name } = await parentOf(path, true);
      let handle: FileSystemFileHandle | null;
      try {
        handle = await parent.getFileHandle(name);
      } catch (e) {
        if (!isMissing(e)) throw e;
        // Deleting the branch `a/b` leaves `refs/heads/a/` behind, which a
        // later branch `a` has to replace, as git's own ref pruning would.
        if (e instanceof DOMException && e.name === "TypeMismatchError")
          await parent.removeEntry(name, { recursive: true });
        handle = null;
      }
      if (handle === null) {
        // Made whole under a name of its own and moved into place, as git
        // does: making the file first and filling it after left an empty
        // object behind a tab closed in between, which every later sync
        // failed on.
        const tmp = await dir([TMP], true);
        const tmpName = crypto.randomUUID();
        const fresh = await tmp.getFileHandle(tmpName, { create: true });
        await fill(fresh, bytes);
        await (fresh as MovableFile).move(parent, name);
        return;
      }
      await fill(handle, bytes);
    },
    async remove(path) {
      try {
        const { parent, name } = await parentOf(path, false);
        await parent.removeEntry(name);
      } catch (e) {
        if (!isMissing(e)) throw e;
      }
    },
    async makeDir(path) {
      await dir(parts(path), true);
    },
    async removeDir(path) {
      try {
        const { parent, name } = await parentOf(path, false);
        await parent.removeEntry(name, { recursive: true });
      } catch (e) {
        if (!isMissing(e)) throw e;
      }
    },
    async list(path) {
      const out = { files: [] as string[], dirs: [] as string[] };
      try {
        await walk(await dir(parts(path), false), "", out);
      } catch (e) {
        if (!isMissing(e)) throw e;
      }
      return out;
    },
  };
}
