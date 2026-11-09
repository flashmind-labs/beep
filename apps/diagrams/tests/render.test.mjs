import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { chromium, webkit } from "playwright";
const engine = process.env.DIAGRAM_ENGINE === "webkit" ? webkit : chromium;
let browser;
before(async () => { browser = await engine.launch({ executablePath: process.env.DIAGRAM_BROWSER }); });
after(async () => { await browser?.close(); });
async function pageFor(t, source, expanded = false) {
  const page = await browser.newPage();
  t.after(() => page.close());
  await page.addInitScript(() => { window.messages = []; window.beepDiagram = { postMessage: text => window.messages.push(JSON.parse(text)) }; });
  await page.goto(new URL("../assets/index.html", import.meta.url).href);
  await page.evaluate(input => window.renderDiagram(input), { source, expanded });
  return page;
}
for (const [type, source] of Object.entries({
  flowchart: 'flowchart LR\n A[Write change] --> B[Run checks]',
  sequence: 'sequenceDiagram\n Alice->>Bob: Hello',
  class: 'classDiagram\n Animal <|-- Duck',
  state: 'stateDiagram-v2\n [*] --> Ready',
  pie: 'pie\n "Done" : 12\n "Pending" : 3',
  mindmap: 'mindmap\n root((Project))\n  Build\n  Review',
})) test(`renders ${type} offline and exports SVG`, async t => {
  const page = await pageFor(t, source);
  await assert.doesNotReject(() => page.locator('#picture').waitFor({state:'visible', timeout:10000}));
  await page.locator('#svg').click();
  const saved = await page.evaluate(() => window.messages.find(m => m.action === 'save'));
  assert.equal(saved.mime, 'image/svg+xml');
  const svg = Buffer.from(saved.data, 'base64').toString();
  assert.match(svg, /<svg/);
  assert.doesNotMatch(svg, /<foreignObject|<script|<image/);
});
test('PNG export, expansion, source and copying', async t => {
  const source = 'flowchart LR\n A[Unicode café] --> B[Done]';
  const page = await pageFor(t, source, true);
  await page.locator('#png').click();
  await page.waitForFunction(() => window.messages.some(m => m.action === 'save'));
  const saved = await page.evaluate(() => window.messages.find(m => m.action === 'save'));
  const png = Buffer.from(saved.data, 'base64');
  assert.equal(png.subarray(1,4).toString(), 'PNG');
  assert.ok(png.readUInt32BE(16) <= 4096 && png.readUInt32BE(20) <= 4096);
  const before = await page.locator('#picture').evaluate(img => img.clientWidth);
  await page.getByRole('button', {name:'Zoom in'}).click();
  assert.ok(await page.locator('#picture').evaluate(img => img.clientWidth) > before);
  await page.locator('#source').click();
  assert.equal(await page.locator('#code').textContent(), source);
  await page.locator('#copy').click();
  assert.equal(await page.evaluate(() => window.messages.find(m => m.action === 'copy').text), source);
  await page.locator('#expand').click();
  assert.ok(await page.evaluate(() => window.messages.some(m => m.action === 'close')));
});

for (const source of ['not a diagram', '%%{init: {"securityLevel":"loose"}}%%\nflowchart LR\n A-->B', '---\nconfig: {}\n---\nflowchart LR\n A-->B', 'x'.repeat(50001)]) {
  test('invalid or configurable diagram falls back to source', async t => {
    const page = await pageFor(t, source);
    assert.equal(await page.locator('#code').textContent(), source);
    assert.equal(await page.locator('#code').isVisible(), true);
    assert.equal(await page.locator('#svg').isDisabled(), true);
  });
}

test('diagram text cannot fetch resources or execute script', async t => {
  const page = await pageFor(t, `flowchart LR\n A["<img src='https://example.invalid/pixel' onerror='alert(1)'>"] --> B[Done]`);
  await page.locator('#svg').click();
  const svg = await page.evaluate(() => atob(window.messages.find(m => m.action === 'save').data));
  assert.doesNotMatch(svg, /<image|<a\b|\sonerror=/);
  assert.equal(await page.locator('#picture').isVisible(), true);
});

test('copies the image with a button, shortcuts and the context menu', async t => {
  const page = await pageFor(t, 'flowchart LR\n A[Copy me] --> B[Done]');
  await page.locator('#copy-image').click();
  await page.keyboard.press('Meta+c');
  await page.keyboard.press('Control+c');
  await page.locator('#picture').click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Copy image', exact: true }).click();
  const copies = await page.evaluate(() => messages.filter(m => m.action === 'copy-image'));
  assert.equal(copies.length, 4);

  for (const copy of copies) {
    assert.equal(copy.mime, 'image/png');
    assert.equal(Buffer.from(copy.data, 'base64').subarray(1, 4).toString(), 'PNG');
  }

  await page.locator('#picture').click({ button: 'right' });
  await page.keyboard.press('ArrowDown');
  assert.equal(await page.getByRole('menuitem', { name: 'Save image', exact: true }).evaluate(el => el === document.activeElement), true);
  await page.keyboard.press('Enter');
  assert.ok(await page.evaluate(() => messages.some(m => m.action === 'save' && m.mime === 'image/png')));
  await page.locator('#picture').click({ button: 'right' });
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('#menu').isHidden(), true);
  await page.locator('#source').click();
  await page.evaluate(() => {
    const range = document.createRange(); range.selectNodeContents(document.getElementById('code'));
    getSelection().removeAllRanges(); getSelection().addRange(range);
  });
  await page.keyboard.press('Meta+c');
  assert.equal(await page.evaluate(() => messages.filter(m => m.action === 'copy-image').length), 4);
});
