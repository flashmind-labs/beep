// PLAYWRIGHT_DIR=/path/to/package.json node tests/perf/heading-react.mjs
import assert from "node:assert/strict";
import { buildSync } from "esbuild";
import { playwright } from "./stub.mjs";

const code = buildSync({ entryPoints: [new URL("../../src/parts/session/logwindow.tsx", import.meta.url).pathname], bundle: true, write: false, format: "iife", globalName: "logWindow" }).outputFiles[0].text;
const browser = await playwright().chromium.launch({ executablePath: process.env.CHROMIUM_BIN || undefined });
try {
  const page = await browser.newPage();
  await page.setContent('<style>.dbody{height:300px;overflow:auto}.dcoderlog,.agturn{display:flex;flex-direction:column;gap:12px}.agspace{flex:none}</style><div class="dbody"><div class="dcoderlog"></div></div>');
  await page.addScriptTag({ content: code });
  const result = await page.evaluate(() => {
    const body = document.querySelector('.dbody'), log = document.querySelector('.dcoderlog');
    const heading = (stats, opened = false) => ({ kind: 'heading', id: 'steps:7', html: `<div class="dcoderturn"><button class="hd${opened ? ' open' : ''}" data-turn="7" aria-expanded="${opened}"><span>${stats}</span></button></div>`, state: 'working', start: null, ms: null, stats, turn: 7, opened });
    const units = (first, second) => [{ key: 'one', rows: () => [heading(first)] }, { key: 'two', rows: () => [heading(second)] }];
    logWindow.setUnits(log, units('First', 'Second'), []);
    logWindow.fillLog(body, false);
    const buttons = [...log.querySelectorAll('[data-turn]')], firstText = buttons[0].querySelector('span').firstChild;
    buttons[0].focus();
    const blocked = logWindow.setUnits(log, units('Updated', 'Second'), [firstText]);
    const held = buttons[0].textContent;
    const updated = logWindow.setUnits(log, units('Updated', 'Second'), []);
    const now = [...log.querySelectorAll('[data-turn]')];
    const focused = document.activeElement === buttons[0];
    let delegated = false;
    log.addEventListener('click', e => { if (e.target.closest('[data-turn]')) delegated = true; });
    now[0].click();
    const search = logWindow.logSearchSources(log).map(item => item.root.textContent);
    logWindow.setUnits(log, [{ key: 'two', rows: () => [heading('Second')] }], []);
    return { blocked, held, updated, firstKept: now[0] === buttons[0], secondKept: now[1] === buttons[1], focused, text: now.map(b => b.textContent), delegated, search, remaining: [...log.querySelectorAll('[data-turn]')].map(b => b.textContent) };
  });
  assert.deepEqual(result, { blocked: false, held: 'First', updated: true, firstKept: true, secondKept: true, focused: true, text: ['Updated', 'Second'], delegated: true, search: ['Updated', 'Second'], remaining: ['Second'] });
  const transitions = await page.evaluate(() => {
    const body = document.querySelector('.dbody'), log = document.querySelector('.dcoderlog');
    const row = (id, stats) => ({ kind: 'heading', id, html: `<div class="dcoderturn"><button class="hd" data-turn="7" aria-expanded="false"><span>${stats}</span></button></div>`, state: 'working', start: null, ms: null, stats, turn: 7, opened: false });
    const units = rows => [{ key: 'one', rows: () => rows }];
    const first = row('duplicate', 'First'), second = row('duplicate', 'Second');
    logWindow.setUnits(log, units([first, second]), []);
    logWindow.fillLog(body, false);
    const before = [...log.querySelectorAll('[data-turn]')];
    logWindow.setUnits(log, units([row('duplicate', 'Updated first'), row('duplicate', 'Updated second')]), []);
    const after = [...log.querySelectorAll('[data-turn]')];
    const duplicateIdentity = after[0] === before[0] && after[1] === before[1];
    const text = after.map(item => item.textContent);
    const raw = row('duplicate', 'Updated first').html;
    logWindow.setUnits(log, units([raw]), []);
    const plain = log.querySelector('[data-turn]');
    const toPlain = plain !== after[0] && plain.parentElement.parentElement.className === 'agturn';
    logWindow.setUnits(log, units([row('duplicate', 'Updated first')]), []);
    const react = log.querySelector('[data-turn]');
    const toReact = react !== plain && react.parentElement.parentElement.style.display === 'contents';
    const search = logWindow.logSearchSources(log)[0].root.textContent;
    logWindow.setUnits(log, units([row('different', 'Updated first')]), []);
    const otherId = log.querySelector('[data-turn]');
    return { duplicateIdentity, text, toPlain, toReact, search, otherIdReplaced: react !== otherId };
  });
  assert.deepEqual(transitions, { duplicateIdentity: true, text: ['Updated first', 'Updated second'], toPlain: true, toReact: true, search: 'Updated first', otherIdReplaced: true });
  console.log('chromium: turn headings keep identity, selection, search and delegated controls');
} finally { await browser.close(); }
