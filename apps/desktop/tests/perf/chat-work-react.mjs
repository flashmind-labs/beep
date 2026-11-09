// PLAYWRIGHT_DIR=/path/to/package.json node tests/perf/chat-work-react.mjs
import assert from 'node:assert/strict';
import { buildSync } from 'esbuild';
import { playwright } from './stub.mjs';

const code = buildSync({ entryPoints: [new URL('../../src/chat/work-view.tsx', import.meta.url).pathname], bundle: true, write: false, format: 'iife', globalName: 'chatWork' }).outputFiles[0].text;
const browser = await playwright().chromium.launch({ executablePath: process.env.CHROMIUM_BIN || undefined });
try {
  const page = await browser.newPage();
  await page.setContent('<div class="dbody"><div class="turn"><div class="tools"></div></div><div class="turn"><div class="tools"></div></div></div>');
  await page.addScriptTag({ content: code });
  const result = await page.evaluate(() => {
    const body = document.querySelector('.dbody'), [turn, second] = body.querySelectorAll('.turn'), box = turn.firstElementChild, other = second.firstElementChild;
    const heading = stats => ({ kind: 'heading', id: '1', html: '', state: 'working', start: null, ms: null, stats, turn: '1', opened: false });
    const model = (stats, thought) => ({ heading: heading(stats), thought: `<div class="dthink"><div class="tx">${thought}</div></div>`, steps: [{ key: 'tool', html: '<div class="tool">Read</div>' }], live: '' });
    let done = 0;
    chatWork.renderChatWork(body, turn, box, model('1 tool', 'First thought'), () => done++);
    chatWork.renderChatWork(body, second, other, model('Other', 'Other thought'), () => done++);
    const button = box.querySelector('[data-turn]'), text = box.querySelector('.tx').firstChild;
    button.focus();
    const selection = document.getSelection(), range = document.createRange();
    range.selectNodeContents(box.querySelector('.tx')); selection.removeAllRanges(); selection.addRange(range);
    chatWork.renderChatWork(body, turn, box, model('2 tools', 'Changed thought'), () => done++);
    const held = box.querySelector('.tx').textContent;
    selection.removeAllRanges(); document.dispatchEvent(new Event('selectionchange'));
    const value = { held, text: box.querySelector('.tx').textContent, buttonKept: box.querySelector('[data-turn]') === button, focused: document.activeElement === button, done, otherBefore: other.textContent };
    chatWork.discardChatWork(turn);
    value.firstGone = !box.firstChild;
    value.otherKept = other.textContent === value.otherBefore;
    return value;
  });
  assert.deepEqual(result, { held: 'First thought', text: 'Changed thought', buttonKept: true, focused: true, done: 3, otherBefore: 'OtherOther thoughtRead', firstGone: true, otherKept: true });
  console.log('chromium: chat work keeps focus, selected text and independent turn ownership');
} finally { await browser.close(); }
