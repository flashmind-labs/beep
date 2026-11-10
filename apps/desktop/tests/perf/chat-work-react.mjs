// PLAYWRIGHT_DIR=/path/to/package.json node tests/perf/chat-work-react.mjs
import assert from 'node:assert/strict';
import { buildSync } from 'esbuild';
import { playwright, stubInit } from './stub.mjs';

const code = buildSync({ entryPoints: [new URL('../../src/chat/transcript.ts', import.meta.url).pathname], bundle: true, write: false, format: 'iife', globalName: 'chatWork' }).outputFiles[0].text;
const browser = await playwright().chromium.launch({ executablePath: process.env.CHROMIUM_BIN || undefined });
try {
  const page = await browser.newPage();
  await page.setContent('<div id="bar"></div>');
  await page.evaluate(stubInit, JSON.stringify({ status: { server: false } }));
  await page.addScriptTag({ content: code });
  const result = await page.evaluate(async () => {
    const body = chatWork.createChatBody();
    document.querySelector('#bar').append(body);
    const heading = stats => ({ kind: 'heading', id: '1', state: 'working', start: null, ms: null, stats, turn: '1', opened: false });
    let done = 0;
    const work = (key, stats, thought) => ({ key, heading: heading(stats), thought: { key: `thought:${key}`, text: thought, ms: null, live: false, open: true, folding: false }, steps: [{ key: 'tool', kind: 'read', verb: 'Read', target: '', label: '', state: 'ok' }], live: null, toggleThought() {}, toggleSteps() {}, touched() { done++; } });
    const turn = chatWork.createTurn(body, { work: work('1', '1 tool', 'First thought') }), second = chatWork.createTurn(body, { work: work('2', 'Other', 'Other thought') });
    chatWork.appendTurn(turn); chatWork.appendTurn(second);
    const box = chatWork.turnElement(turn).querySelector('.tools'), other = chatWork.turnElement(second).querySelector('.tools');
    const button = box.querySelector('[data-turn]');
    button.focus();
    const selection = document.getSelection(), range = document.createRange();
    range.selectNodeContents(box.querySelector('.tx')); selection.removeAllRanges(); selection.addRange(range);
    chatWork.setView(turn, { work: work('1', '2 tools', 'Changed thought') });
    const held = box.querySelector('.tx').textContent;
    selection.removeAllRanges();
    await new Promise(resolve => setTimeout(resolve, 50));
    const value = { held, text: box.querySelector('.tx').textContent, buttonKept: box.querySelector('[data-turn]') === button, focused: document.activeElement === button, otherBefore: other.textContent };
    chatWork.placeTurns(body, [second]);
    value.firstGone = !box.isConnected;
    value.otherKept = other.isConnected && other.textContent === value.otherBefore;
    return value;
  });
  assert.deepEqual(result, { held: 'First thought', text: 'Changed thought', buttonKept: true, focused: true, otherBefore: 'OtherThoughtOther thoughtRead', firstGone: true, otherKept: true });
  console.log('chromium: chat work keeps focus, selected text and independent turns');
} finally { await browser.close(); }
