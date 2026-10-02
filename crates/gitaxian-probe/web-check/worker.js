// The same check from inside a module worker: no document, no window, so the
// engine loads core.js without a <script> tag and its file-written hook takes
// the non-browser branch.
import init, { check } from "./pkg/gitaxian_probe_web_check.js";
import { frames } from "./frames.js";

const tier = new URLSearchParams(location.search).get("tier") || "alpha";

try {
  await init();
  postMessage({ ok: true, report: `in a worker\n${await check("gitaxian-probe/", tier, await frames())}` });
} catch (e) {
  postMessage({ ok: false, report: `in a worker\n${String(e?.message ?? e)}` });
}
