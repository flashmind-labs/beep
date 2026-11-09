// PLAYWRIGHT_DIR=/path/to/package.json node tests/perf/reply-react.mjs
import assert from "node:assert/strict";
import { buildSync } from "esbuild";
import { playwright } from "./stub.mjs";

const code = buildSync({
  entryPoints: [new URL("../../src/agents/logwindow.ts", import.meta.url).pathname],
  bundle: true, write: false, format: "iife", globalName: "logWindow",
}).outputFiles[0].text;
const browser = await playwright().chromium.launch({ executablePath: process.env.CHROMIUM_BIN || undefined });
try {
  const page = await browser.newPage();
  await page.setContent('<style>.dbody{height:300px;overflow:auto}.dcoderlog,.agturn{display:flex;flex-direction:column;gap:12px}.agspace{flex:none}</style><div class="dbody"><div class="dcoderlog"></div></div>');
  await page.addScriptTag({ content: code });
  const result = await page.evaluate(() => {
    const body = document.querySelector(".dbody"), log = document.querySelector(".dcoderlog");
    const row = text => `<div class="dcoderen agent md" data-row="5"><p>${text}</p><pre class="dcode"><button data-copy>Copy</button><code>example</code></pre></div>`;
    const units = text => [{ key: "turn", rows: () => [row(text)] }];
    logWindow.setUnits(log, units("First"), []);
    logWindow.fillLog(body, false);
    const reply = log.querySelector('.dcoderen.agent[data-row="5"]');
    const text = reply.querySelector("p").firstChild;
    const blocked = logWindow.setUnits(log, units("Updated"), [text]);
    const heldText = reply.querySelector("p").textContent;
    const updated = logWindow.setUnits(log, units("Updated"), []);
    const same = log.querySelector('.dcoderen.agent[data-row="5"]') === reply;
    const updatedText = reply.querySelector("p").textContent;
    const copy = reply.querySelector("[data-copy]");
    let delegated = false;
    log.addEventListener("click", event => { if (event.target.closest("[data-copy]")) delegated = true; });
    copy.click();
    const search = logWindow.logSearchSources(log).map(item => item.root.textContent).join("");
    logWindow.setUnits(log, [], []);
    return { blocked, heldText, updated, same, updatedText, delegated, search, gone: !log.contains(reply) };
  });
  assert.deepEqual(result, { blocked: false, heldText: "First", updated: true, same: true, updatedText: "Updated", delegated: true, search: "UpdatedCopyexample", gone: true });
  const duplicates = await page.evaluate(() => {
    const body = document.querySelector(".dbody"), log = document.querySelector(".dcoderlog");
    const row = text => `<div class="dcoderen agent md" data-row="5"><p>${text}</p></div>`;
    const units = (one, two) => [{ key: "one", rows: () => [row(one)] }, { key: "two", rows: () => [row(two)] }];
    logWindow.setUnits(log, units("First", "Second"), []);
    logWindow.fillLog(body, false);
    const old = [...log.querySelectorAll('.dcoderen.agent[data-row="5"]')];
    logWindow.setUnits(log, units("First updated", "Second"), []);
    const now = [...log.querySelectorAll('.dcoderen.agent[data-row="5"]')];
    const value = { oldCount: old.length, count: now.length, firstKept: now[0] === old[0], secondKept: now[1] === old[1], text: now.map(node => node.textContent) };
    logWindow.setUnits(log, [{ key: "two", rows: () => [row("Second")] }], []);
    value.afterRemoval = [...log.querySelectorAll('.dcoderen.agent[data-row="5"]')].map(node => node.textContent);
    return value;
  });
  assert.deepEqual(duplicates, { oldCount: 2, count: 2, firstKept: true, secondKept: true, text: ["First updated", "Second"], afterRemoval: ["Second"] });
  const longLog = await page.evaluate(() => {
    const body = document.querySelector(".dbody"), log = document.querySelector(".dcoderlog");
    const units = Array.from({ length: 1000 }, (_, index) => ({ key: `turn-${index}`, rows: () => [`<div class="dcoderen agent md" data-row="${index}"><p>Reply ${index}</p></div>`] }));
    const start = performance.now();
    logWindow.setUnits(log, units, []);
    logWindow.fillLog(body, false);
    body.scrollTop = body.scrollHeight;
    logWindow.fillLog(body, false);
    const count = log.querySelectorAll(".dcoderen.agent").length;
    const hasLast = !!log.querySelector('.dcoderen.agent[data-row="999"]');
    logWindow.setUnits(log, units, []);
    return { count, hasLast, ms: Math.round(performance.now() - start) };
  });
  assert.ok(longLog.hasLast && longLog.count < 100, `long log drew ${longLog.count} rows; last reply: ${longLog.hasLast}`);
  console.log(`chromium: 1000 replies keep ${longLog.count} rows mounted (${longLog.ms} ms locally)`);
  console.log("chromium: replies preserve selection, search, copy and independent turns with matching row numbers");
} finally { await browser.close(); }
