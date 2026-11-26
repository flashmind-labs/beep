// PLAYWRIGHT_DIR=/path/with/node_modules/ node tests/perf/pane-zoom.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { buildSync } from "esbuild";
import { playwright, stubInit } from "./stub.mjs";

const code = buildSync({
  stdin: { contents: `export { setView } from "./src/bar/core.ts";
    export { paneFrame, startPane, leavePane } from "./src/agentpane/media.ts";
    export { showChanges } from "./src/agentpane/changes.ts";
    export { draw } from "./src/agentpane/media-draw.ts";`, resolveDir: process.cwd() },
  bundle: true, write: false, format: "iife", globalName: "zoomCheck",
}).outputFiles[0].text;
const files = Array.from({ length: 100 }, (_, i) => ({
  path: `src/file-${i}.ts`, old_path: null, status: "modified", additions: 250, deletions: 0, left_out: null,
  hunks: [{ old_start: 1, old_lines: 0, new_start: 1, new_lines: 250, section: "", lines: Array.from({ length: 250 }, (_, n) => ({ k: "+", t: `const value${n} = "${"example ".repeat(20)}";` })) }],
}));
const browser = await playwright().chromium.launch({ executablePath: process.env.CHROMIUM_BIN || undefined });

try {
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 }, reducedMotion: "reduce" });
  const errors = [];
  page.on("pageerror", e => { console.error(e.stack); errors.push(e.message); });
  await page.setContent('<div id="bar" class="dbar" data-mode="window"></div>');

  for (const name of ["style.css", "diff.css"]) {
    const css = readFileSync(new URL(`../../dist/${name}`, import.meta.url), "utf8");
    await page.addStyleTag({ content: css.replace('@import url("skins.css");', "") });
  }

  await page.addStyleTag({ content: '#bar { position: fixed; inset: 0; width: 1300px; height: 850px; max-height: none; max-width: none; display: flex; flex-direction: column } .agconv { overflow: hidden } #agbody { overflow: auto; flex: 1 }' });
  await page.evaluate(stubInit, JSON.stringify({
    agent_hosts: [], agent_sessions: [], agent_reviews: [], editors: [], agent_worktrees: [], agent_tasks: [], session_moves: [],
    agent_tree: { folders: [{ path: "", entries: [], more: 0 }], truncated: false },
    work_diff: { root: "/srv/project", base: "main", merge_base: "", working_tree: true, files, additions: 25000, deletions: 0, more: 0 },
  }));
  await page.evaluate(() => {
    crypto.randomUUID ??= () => String(Math.random());
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = (cmd, args) => cmd === "mesh_watch" ? new Promise(() => {}) : invoke(cmd, args);
  });
  await page.addScriptTag({ content: code });
  await page.evaluate(() => {
    zoomCheck.setView("agent");
    document.querySelector("#bar").innerHTML = `<div class="agshell" id="agshell">${zoomCheck.paneFrame('<div class="agconv"><div id="agbody" class="dbody"></div><textarea id="agmsg"></textarea></div>')}</div>`;
    document.querySelector("#agbody").innerHTML = Array.from({ length: 300 }, (_, i) => `<p>Paragraph ${i}: ${"Example text for the conversation. ".repeat(20)}</p>`).join("");
    zoomCheck.startPane({ machine_id: "machine", machine_name: "build-box", id: "session", agent: "claude", cwd: "/srv/project", external: false });
    zoomCheck.draw();
    zoomCheck.showChanges();
    zoomCheck.leavePane();
  });
  await page.waitForSelector(".dv-chunk .dv-l");
  await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
  await page.locator('.dv-splitbtn').click();
  await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
  const stats = await page.evaluate(async () => {
    const chat = document.querySelector(".agconv"), body = document.querySelector(".dv-files");
    const before = chat.getBoundingClientRect();
    let replacements = 0;
    const watch = new MutationObserver(records => { replacements += records.reduce((n, r) => n + r.removedNodes.length, 0); });
    watch.observe(body, { childList: true, subtree: true });
    const times = [];
    let resizedChat = false;

    for (let i = 0; i < 6; i++) {
      const start = performance.now();
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", shiftKey: true, bubbles: true }));
      const handler = performance.now() - start;
      const rect = chat.getBoundingClientRect();
      resizedChat ||= rect.width !== before.width || rect.height !== before.height;
      await new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done)));
      times.push({ handler: Math.round(handler * 10) / 10, painted: Math.round(performance.now() - start) });
    }

    watch.disconnect();
    return { times, replacements, resizedChat };
  });
  console.log(JSON.stringify(stats));
  assert.equal(stats.resizedChat, false, "zooming must preserve the conversation's layout");
  assert.ok(stats.replacements < 30, "zooming must not rebuild offscreen diffs");
  await page.keyboard.press("Shift+Escape");
  await page.locator('.dv-tr[data-path="src/file-70.ts"]').evaluate(e => e.click());
  await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
  await page.waitForSelector('.dv-file[data-path="src/file-70.ts"] .dv-pair');
  await page.keyboard.press("Shift+Escape");
  await page.waitForFunction(() => !document.querySelector('.dv-file[data-path="src/file-70.ts"] .dv-lines').classList.contains("split"));
  await page.locator('#agpane-right [data-pact="dock"]').click();
  const bottom = await page.locator('.agconv').boundingBox();
  await page.keyboard.press("Shift+Escape");
  assert.equal(await page.locator('#agpane-bottom').evaluate(e => e.classList.contains('zoomed')), true);
  assert.deepEqual(await page.locator('.agconv').boundingBox(), bottom);
  const overlay = await page.locator('#agpane-bottom .agpane-content').boundingBox();
  const shell = await page.locator('#agshell').boundingBox();
  assert.deepEqual(overlay, shell, "the bottom dock fills the session too");
  await page.keyboard.press("Shift+Escape");
  assert.deepEqual(await page.locator('.agconv').boundingBox(), bottom);
  assert.deepEqual(errors, []);
} finally { await browser.close(); }
