"use strict";
// The entire browser surface core.js is allowed to see.
//
// deno_core gives us a bare V8 isolate: no fetch, no filesystem, no process, no
// module loader. Everything the Emscripten glue reaches for has to be handed to
// it explicitly here, which is the point - anything not on this list is not
// merely blocked, it does not exist.
//
// Deliberately absent, because core.js branches on them:
//   window            -> ENVIRONMENT_IS_WEB, and the EM_ASM file-written hook
//   process           -> ENVIRONMENT_IS_NODE
//   WorkerGlobalScope -> ENVIRONMENT_IS_WORKER (the pthread prelude adds it)
//   TextDecoder       -> emscripten falls back to its own UTF-8 decoder, which
//                        avoids decoding views backed by a SharedArrayBuffer
((globalThis) => {
  const core = Deno.core;
  const ops = core.ops;

  // Plain one-line-per-call logging, not a browser console: no format
  // specifiers, no inspection tree. It only has to survive the things you
  // reach for in here, which are enormous or cyclic or both.
  const MAX_ARG = 2000;
  const render = (a) => {
    if (typeof a === "string") return a;
    if (a instanceof Error) return a.stack || String(a);
    const seen = new WeakSet();
    let s;
    try {
      s = JSON.stringify(a, (_key, v) => {
        // Almost every typed array in here is a window onto a 400 MB heap.
        if (ArrayBuffer.isView(v)) return `[${v.constructor.name} ${v.length}]`;
        if (v instanceof ArrayBuffer) return `[ArrayBuffer ${v.byteLength}]`;
        if (typeof v === "function") return `[Function ${v.name || "anonymous"}]`;
        if (typeof v === "bigint") return `${v}n`;
        if (v && typeof v === "object") {
          if (seen.has(v)) return "[Circular]";
          seen.add(v);
        }
        return v;
      });
    } catch {
      s = null;
    }
    if (typeof s !== "string") s = String(a);
    return s.length > MAX_ARG ? `${s.slice(0, MAX_ARG)}... (${s.length} chars)` : s;
  };
  const format = (args) => args.map(render).join(" ");
  const at = (level) => (...args) => ops.op_probe_log(level, format(args));
  globalThis.console = {
    log: at(0), info: at(0), debug: at(0), dir: at(0),
    warn: at(1), error: at(1), trace: at(1),
  };

  // deno_core ships no timers. Emscripten needs them for the mailbox poke
  // (`setTimeout(checkMailbox)`) and for __setitimer_js. They fire from the
  // host pump rather than an event loop, which is enough: every one of them is
  // a zero-or-near-zero delay used to break a call stack, not to keep time.
  let nextTimer = 1;
  const timers = new Map();
  globalThis.setTimeout = (fn, delay, ...args) => {
    const id = nextTimer++;
    timers.set(id, { due: ops.op_probe_now() + (delay || 0), fn, args });
    return id;
  };
  globalThis.clearTimeout = (id) => timers.delete(id);
  globalThis.queueMicrotask = (fn) => Promise.resolve().then(fn);

  // Returns milliseconds until the next timer is due, or -1 when idle, so the
  // host loop knows how long it may sleep instead of spinning.
  const runTimers = () => {
    if (timers.size === 0) return -1;
    const now = ops.op_probe_now();
    for (const [id, timer] of [...timers]) {
      if (timer.due > now) continue;
      timers.delete(id);
      try {
        timer.fn(...timer.args);
      } catch (e) {
        console.error("[timer]", e);
      }
    }
    let soonest = -1;
    for (const timer of timers.values()) {
      const due = Math.max(0, timer.due - now);
      if (soonest < 0 || due < soonest) soonest = due;
    }
    return soonest;
  };

  globalThis.performance = {
    timeOrigin: ops.op_probe_time_origin(),
    now: () => ops.op_probe_now(),
  };
  globalThis.crypto = {
    getRandomValues(view) {
      ops.op_probe_random(view);
      return view;
    },
  };
  // No userAgent: core.js reads it only to decide whether Atomics.waitAsync
  // needs the pre-Chrome-91 polyfill, and V8 has the real thing.
  globalThis.navigator = {
    hardwareConcurrency: ops.op_probe_cores(),
    language: "en-US",
  };

  // `global` exists solely for the shipped EM_ASM file-written hook, which
  // reads `global.fileEvents.emit(...)` on its non-browser branch. That signal
  // is how every job after the first reports completion (FINDINGS.md S6).
  globalThis.global = globalThis;
  const fileListeners = [];
  globalThis.fileEvents = {
    on: (_event, fn) => fileListeners.push(fn),
    emit: (_event) => {
      for (const fn of fileListeners) fn();
    },
  };

  globalThis.__probeRunTimers = runTimers;
})(globalThis);
