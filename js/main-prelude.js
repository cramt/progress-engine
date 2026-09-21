"use strict";
// Main-isolate half of the sandbox: the Worker shim.
//
// Emscripten preallocates a 32-thread pool at startup and waits on it before
// run() completes, so the pool cannot be stubbed out - these are real OS
// threads running real V8 isolates. What crosses between them is a structured
// clone carrying the shared WebAssembly.Memory and the compiled module, which
// deno_core's serializer transfers through the cross-isolate stores the host
// wired up.
((globalThis) => {
  const core = Deno.core;
  const ops = core.ops;
  const workers = new Map();

  const encode = (msg, transfer) =>
    core.serialize(
      msg,
      transfer && transfer.length ? { transferredArrayBuffers: transfer } : undefined,
    );

  class Worker {
    #id;
    #listeners = { message: [], error: [] };

    constructor(_script, opts = {}) {
      this.#id = ops.op_delver_worker_spawn(opts.name || "");
      this.onmessage = null;
      this.onerror = null;
      workers.set(this.#id, this);
    }

    postMessage(msg, transfer) {
      ops.op_delver_worker_post(this.#id, encode(msg, transfer));
    }

    terminate() {
      workers.delete(this.#id);
      ops.op_delver_worker_terminate(this.#id);
    }

    addEventListener(type, fn) {
      (this.#listeners[type] ||= []).push(fn);
    }

    removeEventListener(type, fn) {
      const list = this.#listeners[type];
      const i = list ? list.indexOf(fn) : -1;
      if (i !== -1) list.splice(i, 1);
    }

    _deliver(data) {
      const event = { data };
      if (this.onmessage) this.onmessage(event);
      for (const fn of this.#listeners.message) fn(event);
    }

    get _id() {
      return this.#id;
    }
  }

  const drainWorkers = () => {
    for (const worker of [...workers.values()]) {
      for (;;) {
        // An empty buffer means "nothing pending"; a real message always
        // carries at least a serializer header.
        const buf = ops.op_delver_worker_recv(worker._id);
        if (buf.length === 0) break;
        worker._deliver(core.deserialize(buf));
      }
    }
  };

  globalThis.Worker = Worker;
  globalThis.__delverPump = () => {
    drainWorkers();
    return globalThis.__delverRunTimers();
  };
})(globalThis);
