// Manual check for the lists that draw a screenful at a time (src/ui/lazyrows.ts):
// each one must start small, reach every row by scrolling, and keep the
// keyboard selection on screen. Same environment as run.mjs.
//
//   PLAYWRIGHT_DIR=/path/with/playwright/ node tests/perf/check.mjs [--browser webkit|chromium] [--dist DIR]
import { join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import { makeData, SIZES } from "./data.mjs";
import { playwright, serve, stubInit } from "./stub.mjs";

const args = process.argv.slice(2);
const opt = (name, fallback) => { const i = args.indexOf(`--${name}`); return i >= 0 ? args[i + 1] : fallback; };
const here = fileURLToPath(new URL(".", import.meta.url));
const { server, origin } = await serve(normalize(opt("dist", join(here, "../../dist"))));
const { webkit, chromium } = playwright();
const kind = opt("browser", "chromium");
const executablePath = (kind === "webkit" ? process.env.WEBKIT_BIN : process.env.CHROMIUM_BIN) || undefined;
const browser = await (kind === "webkit" ? webkit : chromium).launch({ executablePath });
const json = JSON.stringify(makeData());
const sleep = ms => new Promise(r => setTimeout(r, ms));

let failed = 0;
const check = (name, ok, detail) => { console.log(`${ok ? "ok  " : "FAIL"} ${name}${detail ? ` (${detail})` : ""}`); if (!ok) failed++; };

async function fresh() {
  const page = await (await browser.newContext({ viewport: { width: 1280, height: 800 } })).newPage();
  await page.addInitScript(stubInit, json);
  const errors = [];
  page.on("pageerror", e => errors.push(e.message));
  await page.goto(`${origin}/center.html`);
  await page.waitForSelector(".cc-tabs");
  await sleep(500);
  return { page, errors };
}
const open = async (page, target, ready) => {
  await page.evaluate(t => { window.__pending = t; window.__emit("center-open"); }, target);
  await page.waitForSelector(ready);
  await sleep(500);
};
const count = (page, sel) => page.evaluate(s => document.querySelectorAll(s).length, sel);

/// Scrolls `scroller` over its whole length, again and again, until `sel`
/// stops growing; returns the count. Columns side by side need the sweep: a
/// shorter one only draws when the view passes it.
async function drain(page, scroller, sel) {
  let last = -1, now = await count(page, sel);
  for (let i = 0; i < 60 && now !== last; i++) {
    last = now;
    for (const at of [1, 0.75, 0.5, 0.25, 0, 0.5, 1]) {
      await page.evaluate(([s, f]) => { const el = document.querySelector(s); el.scrollTop = (el.scrollHeight - el.clientHeight) * f; }, [scroller, at]);
      await sleep(150);
    }
    now = await count(page, sel);
  }
  return now;
}

{
  const { page, errors } = await fresh();
  await open(page, "memories", ".dmem");
  const first = await count(page, ".dmem");
  check("memories start small", first > 0 && first <= 30, `${first} rows`);
  check("memories scroll to every row", await drain(page, "#mlist", ".dmem") === SIZES.memories);
  check("memories end with Forget all", await page.evaluate(() => !!document.querySelector("#mlist > .dpad button")));
  await open(page, "memories", ".dmem");
  check("memories start small again", await count(page, ".dmem") <= 30);
  await page.focus("#mfind");
  for (let i = 0; i < 45; i++) await page.keyboard.press("ArrowDown");
  await sleep(200);
  const sel = await page.evaluate(() => {
    const row = document.querySelector('.dmem[aria-selected="true"]');
    const box = row?.getBoundingClientRect(), list = document.querySelector("#mlist").getBoundingClientRect();
    return { i: row?.dataset.i, inView: !!box && box.bottom > list.top && box.top < list.bottom, acts: !!row?.querySelector(".acts"), rows: document.querySelectorAll('.dmem[aria-selected="true"]').length };
  });
  check("memories arrow keys select row 45, in view, one selected", sel.i === "45" && sel.inView && sel.acts && sel.rows === 1, JSON.stringify(sel));
  await page.fill("#mfind", "alpha");
  await sleep(300);
  const shown = await page.evaluate(() => document.querySelector("#mcount").textContent);
  check("memories search counts every match", /^\d+ of 2000$/.test(shown) && Number(shown.split(" ")[0]) === SIZES.memories / 10, shown);
  check("memories no page errors", errors.length === 0, errors.join("; "));
  await page.close();
}

{
  const { page, errors } = await fresh();
  await page.click(".cc-tabs button:nth-child(2)");
  await page.waitForSelector("#sec-git");
  await sleep(500);
  const itemRows = () => count(page, "#sec-git .cc-row.it");
  const a = await itemRows();
  check("git tab starts small", a > 0 && a < SIZES.items / 4, `${a} rows`);
  const now = await drain(page, "#view", "#sec-git .cc-row.it");
  check("git tab scrolls to every item", now === SIZES.items, `${now} rows`);
  check("git tab no page errors", errors.length === 0, errors.join("; "));
  await page.close();
}

{
  const { page, errors } = await fresh();
  await page.click(".cc-tabs button:nth-child(3)");
  await page.waitForSelector("#sec-projects-home");
  await sleep(500);
  const first = await count(page, "#sec-projects-home .cc-row");
  check("home projects start small", first > 0 && first < SIZES.projects, `${first} rows`);
  check("home projects scroll to every row", await drain(page, "#view", "#sec-projects-home .cc-row") === SIZES.projects);
  check("home projects no page errors", errors.length === 0, errors.join("; "));
  await page.close();
}

{
  const { page, errors } = await fresh();
  await open(page, { projects: { project: null } }, ".pj-card");
  const first = await count(page, ".pj-card");
  check("projects start small", first > 0 && first < SIZES.projects, `${first} cards`);
  const cards = await drain(page, "#main .cc-box:not([hidden]) .cc-view", ".pj-card");
  check("projects scroll to every card", cards === SIZES.projects, `${cards} cards`);
  check("projects no page errors", errors.length === 0, errors.join("; "));
  await page.close();
}

{
  const { page, errors } = await fresh();
  const rows = ".cc-side-list .cc-grp.projects > *";
  const first = await count(page, rows);
  check("sidebar projects start small", first > 0 && first < SIZES.projects / 2, `${first} rows`);
  // The heading, 20 orgs, 200 projects and the spacer.
  const now = await drain(page, ".cc-side-list", rows);
  check("sidebar projects scroll to every row", now >= SIZES.projects + 20, `${now} rows`);
  check("sidebar text shows", (await page.evaluate(() => document.querySelector("#side").textContent)).includes("Beep"));
  check("sidebar no page errors", errors.length === 0, errors.join("; "));
  await page.close();
}

await browser.close();
server.close();
console.log(failed ? `${failed} failed` : "all ok");
process.exit(failed ? 1 : 0);
