// What run.mjs and check.mjs share: a server for dist/ and the in-page stub of
// `window.__TAURI__` with its probes.
import { createRequire } from "node:module";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";

/// Playwright, resolved from the directory in PLAYWRIGHT_DIR (it is not a dependency).
export function playwright() {
  if (!process.env.PLAYWRIGHT_DIR) { console.error("Set PLAYWRIGHT_DIR to a directory that holds node_modules/playwright."); process.exit(2); }
  return createRequire(process.env.PLAYWRIGHT_DIR)("playwright");
}

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".woff": "font/woff", ".png": "image/png" };

/// Serves `dist` on 127.0.0.1; resolves to the server and its origin.
export async function serve(dist) {
  const root = normalize(dist);
  const server = createServer(async (req, res) => {
    const path = normalize(join(root, decodeURIComponent(new URL(req.url, "http://x").pathname)));
    if (!path.startsWith(root)) { res.writeHead(403).end(); return; }
    try {
      const body = await readFile(path);
      res.writeHead(200, { "content-type": TYPES[extname(path)] || "application/octet-stream" }).end(body);
    } catch { res.writeHead(404).end(); }
  });
  await new Promise(r => server.listen(0, "127.0.0.1", r));
  return { server, origin: `http://127.0.0.1:${server.address().port}` };
}

/// Runs in the page before the app: stubs `window.__TAURI__` with `serialized`
/// (a JSON object of command name to answer) and adds the probes.
export const stubInit = serialized => {
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
