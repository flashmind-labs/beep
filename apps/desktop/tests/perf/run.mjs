// Manual perf harness for the desktop app's pages (issue #290). It is not
// part of `npm run check`.
//
// It serves dist/ on 127.0.0.1, loads center.html in Playwright with a stubbed
// `window.__TAURI__` that answers every command with a large synthetic data
// set (data.mjs), and measures per page: the page switch, one refresh
// redraw, keystroke to paint in the search box, and scrolling.
//
//   cd apps/desktop && npm run -s build
//   PLAYWRIGHT_DIR=/path/with/playwright/ node tests/perf/run.mjs [options]
//
// Options (or env):
//   --browser webkit|chromium|both   default both
//   --pages home,chats,...           default all (see PAGES below)
//   --runs N                         default 3, medians are printed
//   --dist DIR                       serve DIR instead of ../../dist, to compare builds
//   --json FILE                    also write the raw results
//   --profile                        Chromium only: print the functions that
//                                    take the most time during the page
//                                    switch and one refresh
//   WEBKIT_BIN / CHROMIUM_BIN        executablePath of the browser, when the
//                                    one Playwright finds does not launch
//
// Playwright is not a dependency: PLAYWRIGHT_DIR is a directory that holds
// node_modules/playwright (it is resolved from there).
import { createRequire } from "node:module";
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import { makeData } from "./data.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));

const args = process.argv.slice(2);
const opt = (name, fallback) => { const i = args.indexOf(`--${name}`); return i >= 0 ? args[i + 1] : fallback; };
const dist = normalize(opt("dist", join(here, "../../dist")));
const browsers = opt("browser", "both") === "both" ? ["webkit", "chromium"] : [opt("browser", "both")];
const runs = Number(opt("runs", "3"));
const jsonOut = opt("json", "");
const profile = args.includes("--profile");

if (!process.env.PLAYWRIGHT_DIR) { console.error("Set PLAYWRIGHT_DIR to a directory that holds node_modules/playwright."); process.exit(2); }
const { webkit, chromium } = createRequire(process.env.PLAYWRIGHT_DIR)("playwright");

// What each page is, how to open it and what to measure on it. `open` is a
// center_pending target (the window's own handoff) or a click on a DOM
// element; `ready` shows once the rows are drawn.
const PAGES = {
  home: { ready: ".cc-tabs", scroll: "#view" },
  "home-git": { click: ".cc-tabs button:nth-child(2)", ready: "#sec-git, .cc-tabs button:nth-child(2)[aria-selected=true]", scroll: "#view", from: "home" },
  "home-projects": { click: ".cc-tabs button:nth-child(3)", ready: "#sec-projects-home", scroll: "#view", from: "home" },
  "home-machines": { click: ".cc-tabs button:nth-child(4)", ready: "#sec-machines", scroll: "#view", from: "home" },
  chats: { open: "chats", ready: ".dchat", search: "#find", text: "chat 00", keys: "#find" },
  memories: { open: "memories", ready: ".dmem", search: "#mfind", text: "alpha widget", keys: "#mfind", scroll: "#mlist" },
  machines: { open: "machines", ready: ".dmach", scroll: "auto" },
  projects: { open: { projects: { project: null } }, ready: "#main .cc-view", scroll: "auto" },
  // A fold in the sidebar reloads its projects group; the page behind is cheap.
  sidebar: { from: "machines", click: ".cc-side-list .cc-org", ready: ".cc-side-list", scroll: ".cc-side-list" },
};
const pageNames = (opt("pages", "") || Object.keys(PAGES).join(",")).split(",").filter(Boolean);

// ---------- Server ----------

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".woff": "font/woff", ".png": "image/png" };
const server = createServer(async (req, res) => {
  const path = normalize(join(dist, decodeURIComponent(new URL(req.url, "http://x").pathname)));
  if (!path.startsWith(dist)) { res.writeHead(403).end(); return; }
  try {
    const body = await readFile(path);
    res.writeHead(200, { "content-type": TYPES[extname(path)] || "application/octet-stream" }).end(body);
  } catch { res.writeHead(404).end(); }
});
await new Promise(r => server.listen(0, "127.0.0.1", r));
const origin = `http://127.0.0.1:${server.address().port}`;

// ---------- In-page stub and probes ----------

const json = JSON.stringify(makeData());
const stubInit = serialized => {
  const answers = JSON.parse(serialized);
  // One string per command, parsed on every call, so each read gives fresh
  // objects as Tauri's JSON does.
  const raw = Object.fromEntries(Object.entries(answers).map(([k, v]) => [k, JSON.stringify(v)]));
  const handlers = new Map();
  window.__pending = null;
  window.__parseMs = 0;
  window.__calls = {};
  window.__emit = (name, payload) => { for (const h of handlers.get(name) || []) h({ payload }); };
  window.__TAURI__ = {
    core: {
      invoke: async cmd => {
        window.__calls[cmd] = (window.__calls[cmd] || 0) + 1;
        if (cmd === "center_pending") { const p = window.__pending; window.__pending = null; return p; }
        if (!(cmd in raw)) return null;
        const t = performance.now();
        const v = JSON.parse(raw[cmd]);
        window.__parseMs += performance.now() - t;
        return v;
      },
      Channel: class { onmessage() {} },
    },
    event: { listen: async (name, h) => { if (!handlers.has(name)) handlers.set(name, []); handlers.get(name).push(h); return () => {}; } },
    window: { getCurrentWindow: () => ({ label: "center", onFocusChanged: async () => () => {}, setFocus: async () => {} }) },
  };
  window.__errors = [];
  window.addEventListener("error", e => window.__errors.push(String(e.message)));
  window.addEventListener("unhandledrejection", e => window.__errors.push("rejection: " + String(e.reason)));

  // Frame deltas for `ms` milliseconds.
  window.__frames = ms => new Promise(done => {
    const out = []; let last = performance.now(); const end = last + ms;
    const tick = now => { out.push(now - last); last = now; if (now < end) requestAnimationFrame(tick); else done(out); };
    requestAnimationFrame(tick);
  });
  // Gaps between event loop turns for `ms`: the longest task and the time
  // spent past 4 ms gaps, a measure of main thread blocking.
  window.__gaps = ms => new Promise(done => {
    const ch = new MessageChannel(); let last = performance.now(); const end = last + ms; let max = 0, sum = 0;
    ch.port1.onmessage = () => {
      const now = performance.now(), gap = now - last; last = now;
      if (gap > max) max = gap;
      if (gap > 4) sum += gap;
      if (now < end) ch.port2.postMessage(0); else done({ max, sum });
    };
    ch.port2.postMessage(0);
  });
  // Resolves one frame after `sel` first matches, from now.
  window.__shown = (sel, timeout = 8000) => new Promise((done, fail) => {
    const hit = () => document.querySelector(sel);
    const painted = () => requestAnimationFrame(() => requestAnimationFrame(() => done(performance.now())));
    if (hit()) return painted();
    const mo = new MutationObserver(() => { if (hit()) { mo.disconnect(); painted(); } });
    mo.observe(document, { childList: true, subtree: true });
    setTimeout(() => { mo.disconnect(); fail(new Error("timeout waiting for " + sel)); }, timeout);
  });
  window.__latency = [];
  document.addEventListener("keydown", () => {
    const t = performance.now();
    requestAnimationFrame(() => setTimeout(() => window.__latency.push(performance.now() - t)));
  }, true);
};

// ---------- Measuring ----------

const median = xs => { const s = [...xs].sort((a, b) => a - b); return s.length ? (s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2) : NaN; };
const pct = (xs, p) => { const s = [...xs].sort((a, b) => a - b); return s.length ? s[Math.min(s.length - 1, Math.floor(p * s.length))] : NaN; };
const sleep = ms => new Promise(r => setTimeout(r, ms));

async function open(page, cfg) {
  if (cfg.from) {
    await page.evaluate(target => { window.__pending = target; window.__emit("center-open"); }, PAGES[cfg.from].open ?? "home");
    await page.evaluate(sel => window.__shown(sel), PAGES[cfg.from].ready);
    await sleep(300);
  }
  if (cfg.click) {
    // Clicks run inside the page so the start time is its own clock.
    const ms = await page.evaluate(async ({ click, ready }) => {
      const t0 = performance.now();
      document.querySelector(click).click();
      const t1 = await window.__shown(ready);
      return t1 - t0;
    }, { click: cfg.click, ready: cfg.ready });
    return ms;
  }
  return page.evaluate(async ({ target, ready }) => {
    const t0 = performance.now();
    window.__pending = target;
    window.__emit("center-open");
    const t1 = await window.__shown(ready);
    return t1 - t0;
  }, { target: cfg.open, ready: cfg.ready });
}

async function scrollerBox(page, cfg) {
  return page.evaluate(sel => {
    let el = null;
    if (sel === "auto") {
      let best = 0;
      for (const e of document.querySelectorAll("#main *")) {
        const cs = getComputedStyle(e);
        if (!/(auto|scroll)/.test(cs.overflowY) || e.clientHeight === 0 || e.scrollHeight <= e.clientHeight + 100) continue;
        if (e.getBoundingClientRect().width < 100) continue;
        if (e.scrollHeight > best) { best = e.scrollHeight; el = e; }
      }
    } else el = document.querySelector(sel);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + Math.min(r.height / 2, 300), travel: el.scrollHeight - el.clientHeight, id: el.id || el.className };
  }, cfg.scroll);
}

/// Self time per function over `action`, with the Chromium profiler.
async function profiled(context, page, label, action) {
  const cdp = await context.newCDPSession(page);
  await cdp.send("Profiler.enable");
  await cdp.send("Profiler.setSamplingInterval", { interval: 200 });
  await cdp.send("Profiler.start");
  const result = await action();
  const { profile: p } = await cdp.send("Profiler.stop");
  const self = new Map();
  const byId = new Map(p.nodes.map(n => [n.id, n]));
  p.samples.forEach((id, i) => {
    const f = byId.get(id).callFrame;
    const key = `${f.functionName || "(anon)"} ${f.url.split("/").pop()}:${f.lineNumber + 1}`;
    self.set(key, (self.get(key) || 0) + (p.timeDeltas[i] || 0) / 1000);
  });
  const top = [...self].sort((a, b) => b[1] - a[1]).slice(0, 14);
  console.log(`  profile ${label}: ` + top.map(([k, ms]) => `${ms.toFixed(1)}ms ${k}`).join("\n    "));
  return result;
}

async function measure(browser, name) {
  const cfg = PAGES[name];
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", e => errors.push(e.message));
  await page.addInitScript(stubInit, json);
  await page.goto(`${origin}/center.html`);
  await page.waitForSelector(".cc-tabs", { timeout: 15000 });
  await sleep(600);
  const out = { page: name };
  const doOpen = () => open(page, cfg);
  out.switchMs = profile ? await profiled(context, page, `${name} switch`, doOpen) : await doOpen();
  await sleep(800);
  out.rows = await page.evaluate(sel => document.querySelectorAll(sel).length, cfg.ready.split(",")[0]);
  out.domNodes = await page.evaluate(() => document.getElementsByTagName("*").length);

  // One refresh redraw: the window's focus event reads Home again.
  const refreshes = [];
  if (profile) await profiled(context, page, `${name} refresh`, () => page.evaluate(async () => { window.dispatchEvent(new Event("focus")); await window.__gaps(500); }));
  for (let i = 0; i < 3; i++) {
    const r = await page.evaluate(async () => {
      window.__parseMs = 0;
      const gaps = window.__gaps(500);
      window.dispatchEvent(new Event("focus"));
      const g = await gaps;
      return { max: g.max, sum: g.sum, parse: window.__parseMs };
    });
    refreshes.push(r);
    await sleep(300);
  }
  out.refreshMaxMs = median(refreshes.map(r => r.max));
  out.refreshBusyMs = median(refreshes.map(r => r.sum));
  out.refreshParseMs = median(refreshes.map(r => r.parse));

  // Keystroke to paint, then the same with the arrow keys.
  if (cfg.keys) {
    await page.focus(cfg.keys);
    await page.evaluate(() => { window.__latency.length = 0; });
    const frames = page.evaluate(() => window.__frames(3500));
    for (const ch of cfg.text) { await page.keyboard.type(ch); await sleep(230); }
    const f = await frames;
    const lat = await page.evaluate(() => window.__latency.slice());
    out.keyMedianMs = median(lat);
    out.keyP95Ms = pct(lat, 0.95);
    out.typingWorstFrameMs = Math.max(...f);
    await page.evaluate(() => { window.__latency.length = 0; });
    for (let i = 0; i < 15; i++) { await page.keyboard.press("ArrowDown"); await sleep(90); }
    const nav = await page.evaluate(() => window.__latency.slice());
    out.arrowMedianMs = median(nav);
    out.arrowP95Ms = pct(nav, 0.95);
    await page.fill(cfg.keys, "");
    await sleep(500);
  }

  // Scrolling: wheel ticks every frame for 1.5 s.
  const box = await scrollerBox(page, cfg);
  if (box && box.travel > 200) {
    await page.mouse.move(box.x, box.y);
    const frames = page.evaluate(() => window.__frames(1500));
    const t0 = Date.now();
    while (Date.now() - t0 < 1500) { await page.mouse.wheel(0, 120); await sleep(14); }
    const f = (await frames).slice(1);
    out.scrollFrames = f.length;
    out.scrollLong = f.filter(x => x > 16.7 * 1.25).length;
    out.scrollWorstMs = Math.max(...f);
    out.scrollMedianMs = median(f);
    out.scroller = box.id;
  } else {
    out.scrollNote = box ? "short" : "no scroller";
  }
  out.errors = [...new Set([...errors, ...(await page.evaluate(() => window.__errors))])].slice(0, 5);
  await context.close();
  return out;
}

// ---------- Main ----------

const results = {};
for (const b of browsers) {
  const type = b === "webkit" ? webkit : chromium;
  const executablePath = (b === "webkit" ? process.env.WEBKIT_BIN : process.env.CHROMIUM_BIN) || undefined;
  let browser;
  try { browser = await type.launch({ executablePath }); } catch (e) { console.log(`${b}: launch failed: ${String(e).split("\n")[0]}`); continue; }
  console.log(`\n== ${b} ${browser.version()} (${runs} runs, medians) ==`);
  results[b] = {};
  for (const name of pageNames) {
    const samples = [];
    for (let i = 0; i < runs; i++) {
      try { samples.push(await measure(browser, name)); } catch (e) { console.log(`${name}: run failed: ${String(e).split("\n")[0]}`); }
    }
    if (!samples.length) continue;
    const row = { page: name, rows: samples[0].rows, domNodes: samples[0].domNodes, scroller: samples[0].scroller, errors: samples[0].errors };
    for (const k of ["switchMs", "refreshMaxMs", "refreshBusyMs", "refreshParseMs", "keyMedianMs", "keyP95Ms", "typingWorstFrameMs", "arrowMedianMs", "arrowP95Ms", "scrollMedianMs", "scrollLong", "scrollWorstMs"]) {
      const xs = samples.map(s => s[k]).filter(x => typeof x === "number" && !Number.isNaN(x));
      if (xs.length) row[k] = Math.round(median(xs) * 10) / 10;
    }
    results[b][name] = row;
    console.log(JSON.stringify(row));
  }
  await browser.close();
}
if (jsonOut) await writeFile(jsonOut, JSON.stringify(results, null, 2));
server.close();
