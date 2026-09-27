// Open the check page in headless Chromium and report what it found.
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
// playwright is whatever node resolves - a local install, or NODE_PATH at a
// global one - since this directory carries no package.json of its own.
const { chromium } = require("playwright");

const url = process.argv[2];
const browser = await chromium.launch(
  process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {},
);
try {
  const page = await browser.newPage();
  page.on("pageerror", (e) => console.error("pageerror:", e.message));
  await page.goto(url);
  const handle = await page.waitForFunction(() => window.__result, null, { timeout: 300_000 });
  const { ok, report } = await handle.jsonValue();
  console.log(report);
  process.exitCode = ok ? 0 : 1;
} finally {
  await browser.close();
}
