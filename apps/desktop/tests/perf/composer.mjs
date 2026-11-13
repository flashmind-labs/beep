// Manual check that a project page's start input sits centered in the main
// area (#450). Same environment as check.mjs; screenshots go to --out.
//
//   PLAYWRIGHT_DIR=/path/with/playwright/ node tests/perf/composer.mjs [--browser webkit|chromium] [--dist DIR] [--out DIR] [--widths 1280,1800]
import { join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import { makeData } from "./data.mjs";
import { playwright, serve, stubInit } from "./stub.mjs";

const args = process.argv.slice(2);
const opt = (name, fallback) => { const i = args.indexOf(`--${name}`); return i >= 0 ? args[i + 1] : fallback; };
const here = fileURLToPath(new URL(".", import.meta.url));
const { server, origin } = await serve(normalize(opt("dist", join(here, "../../dist"))));
const { webkit, chromium } = playwright();
const kind = opt("browser", "chromium");
const executablePath = (kind === "webkit" ? process.env.WEBKIT_BIN : process.env.CHROMIUM_BIN) || undefined;
const browser = await (kind === "webkit" ? webkit : chromium).launch({ executablePath });
const out = opt("out", join(here, "../../../../target"));
// Every imported project shows the split.
const data = makeData();
const json = JSON.stringify(data);
const sleep = ms => new Promise(r => setTimeout(r, ms));

let failed = 0;
const check = (name, ok, detail) => { console.log(`${ok ? "ok  " : "FAIL"} ${name}${detail ? ` (${detail})` : ""}`); if (!ok) failed++; };

for (const width of opt("widths", "1280,1800").split(",").map(Number)) {
  const page = await (await browser.newContext({ viewport: { width, height: 900 } })).newPage();
  await page.addInitScript(stubInit, json);
  await page.goto(`${origin}/center.html`);
  await page.waitForSelector(".cc-tabs");
  await sleep(500);
  await page.evaluate(() => { window.__pending = { projects: { project: "fake-org-00/fake-repo-000" } }; window.__emit("center-open"); });
  await page.waitForSelector(".cc-view.split ~ .cc-pjc .box");
  await sleep(800);
  const r = await page.evaluate(() => {
    const rect = el => { if (!el) return null; const b = el.getBoundingClientRect(); return { left: Math.round(b.left), right: Math.round(b.right), width: Math.round(b.width), top: Math.round(b.top), bottom: Math.round(b.bottom) }; };
    const form = document.querySelector(".cc-view.split ~ .cc-pjc");
    const det = document.querySelector(".pj-det");
    return {
      window: { width: innerWidth, height: innerHeight },
      main: rect(document.querySelector(".cc-main")),
      list: rect(document.querySelector(".pj-list")),
      det: rect(det),
      form: rect(form),
      box: rect(form.querySelector(".box")),
      containing: form.offsetParent && (form.offsetParent.id || form.offsetParent.className),
      containingRect: rect(form.offsetParent),
      position: getComputedStyle(form).position,
      margin: getComputedStyle(form).margin,
      detPad: det && getComputedStyle(det).paddingBottom,
      listPad: getComputedStyle(document.querySelector(".pj-list")).paddingBottom,
      formH: form.offsetHeight,
    };
  });
  console.log(width, JSON.stringify(r));
  const mid = (r.main.left + r.main.right) / 2, boxMid = (r.box.left + r.box.right) / 2;
  check(`${width}: box centered in the main area`, Math.abs(mid - boxMid) <= 2, `main center ${mid}, box center ${boxMid}`);
  check(`${width}: box at most 760px`, r.box.width <= 760, `${r.box.width}px`);
  check(`${width}: detail ends above the bar`, r.det.bottom <= r.form.top, `detail bottom ${r.det.bottom}, bar top ${r.form.top}`);
  check(`${width}: list ends above the bar`, r.list.bottom <= r.form.top, `list bottom ${r.list.bottom}, bar top ${r.form.top}`);
  await page.screenshot({ path: join(out, `composer-${width}.png`) });
  await page.close();
}

await browser.close();
server.close();
console.log(failed ? `${failed} failed` : "all ok");
process.exit(failed ? 1 : 0);
