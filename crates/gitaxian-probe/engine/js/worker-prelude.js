"use strict";
// Pthread-isolate half of the sandbox, evaluated before core.js.
//
// core.js self-invokes when globalThis.name === "em-pthread", so this only has
// to make the isolate look like a web worker. Unlike the Node harness there is
// no message-loss window to paper over: inbound messages sit in a host-side
// queue and are pulled only once core.js has installed its handler.
((globalThis) => {
  const core = Deno.core;
  const ops = core.ops;

  globalThis.self = globalThis;
  globalThis.name = "em-pthread";
  globalThis.WorkerGlobalScope = function WorkerGlobalScope() {};
  globalThis.location = { href: "core.js" };

  const listeners = { message: [], error: [] };
  globalThis.addEventListener = (type, fn) => (listeners[type] ||= []).push(fn);
  globalThis.removeEventListener = (type, fn) => {
    const list = listeners[type];
    const i = list ? list.indexOf(fn) : -1;
    if (i !== -1) list.splice(i, 1);
  };
  globalThis.postMessage = (msg, transfer) => {
    ops.op_probe_self_post(
      core.serialize(
        msg,
        transfer && transfer.length ? { transferredArrayBuffers: transfer } : undefined,
      ),
    );
  };

  const drainSelf = () => {
    // Leave everything queued host-side until core.js has a handler, so no
    // message can be delivered into the void.
    if (typeof globalThis.onmessage !== "function" && listeners.message.length === 0) return;
    for (;;) {
      const buf = ops.op_probe_self_recv();
      if (buf.length === 0) break;
      const event = { data: core.deserialize(buf) };
      if (typeof globalThis.onmessage === "function") globalThis.onmessage(event);
      for (const fn of listeners.message) fn(event);
    }
  };

  globalThis.__probePump = () => {
    drainSelf();
    return globalThis.__probeRunTimers();
  };
})(globalThis);
