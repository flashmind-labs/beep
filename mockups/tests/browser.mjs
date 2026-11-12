// Check entry pages directly from disk, including every registered desktop variant.
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = new URL('../', import.meta.url);
const output = new URL('test-results/', root);
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ headless: true });
const errors = [];
let screens = 0;
let variants = 0;

function watch(page) {
  page.on('pageerror', error => errors.push(error.message));
  page.on('requestfailed', request => {
    if (request.url().startsWith('file:')) errors.push(`Missing asset: ${request.url()}`);
  });
}

try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1100 } });
  watch(page);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  const startupRequests = [];
  const recordStartup = request => { if (/^https?:/.test(request.url())) startupRequests.push(request.url()); };
  page.on('request', recordStartup);
  await page.addInitScript(() => {
    window.oldMenuSeen = false;
    new MutationObserver(() => {
      if (document.querySelector('#index > .group, #index > .pick')) window.oldMenuSeen = true;
    }).observe(document, { childList: true, subtree: true });
  });
  await page.goto(new URL('desktop/index.html', root).href);
  await page.getByRole('searchbox', { name: 'Find a screen' }).waitFor();
  assert.deepEqual(startupRequests, [], 'Desktop startup must work without external requests');
  assert.equal(await page.evaluate(() => window.oldMenuSeen), false, 'Only the hierarchical menu should render');
  page.off('request', recordStartup);
  await page.addStyleTag({ content: '*, *::before, *::after { animation: none !important; transition: none !important; }' });
  const ids = await page.evaluate(() => DESK_SCREENS.flatMap(group => group.items.map(item => item[0])));
  assert.equal(await page.locator('#index [data-screen]').count(), ids.length, 'Hierarchy must retain every screen');
  await page.getByRole('button', { name: 'Collapse all', exact: true }).click();
  assert.equal(await page.locator('#index details[open]').count(), 0);
  const search = page.getByRole('searchbox', { name: 'Find a screen' });
  await search.fill('HTML preview');
  assert.equal(await page.locator('#index [data-screen]').count(), 1);
  await search.press('Enter');
  assert.equal(await page.evaluate(() => S.screen), 'dHtmlPreview');
  await search.fill('no matching mockup screen');
  await page.getByRole('status').filter({ hasText: 'No screens match' }).waitFor();
  await search.press('Escape');
  assert.equal(await page.locator('#index [data-screen]').count(), ids.length);
  await page.locator('#index details[data-menu-branch="0/0"] > summary').click();
  await page.locator('#index [data-screen="dHome"]').click();
  assert.equal(await page.evaluate(() => S.screen), 'dHome');
  await search.fill('dWinProject');
  await page.locator('#index [data-screen="dWinProject"]').click();
  await search.press('Escape');
  const project = page.locator('#desk .project-current');
  assert.equal(await project.locator('.pj-strip > button').count(), 7);
  assert.deepEqual(await project.locator('.bd-lane > h4').evaluateAll(heads => heads.map(head => head.firstChild.textContent)),
    ['Needs you', 'Working', 'Ready to review', 'Done today'], 'A project opens on its board');
  assert.equal(await project.locator('.pj-li').count(), 0, 'The board stands alone on the front page');
  await project.locator('.bd-lane > h4 button').filter({ hasText: 'Working' }).click();
  assert.deepEqual(await project.locator('.pj-li.nav').evaluateAll(rows => rows.map(row => row.title)),
    ['Board', 'Running', 'Actions', 'Machines', 'Issues', 'Sessions', 'Git']);
  assert.equal(await page.locator('#desk .us-pill').count(), 0, 'Usage belongs on Home');
  assert.equal(await project.locator('.pj-ov button.lk').filter({ hasText: 'Actions' }).count(), 0);
  await project.locator('.pj-li.nav[title="Sessions"]').click();
  await project.getByRole('tab', { name: 'Agents' }).click();
  assert.equal(await project.getByRole('tab', { name: 'Agents' }).getAttribute('aria-selected'), 'true');
  await project.getByRole('tab', { name: 'Sessions' }).click();
  await project.locator('.pj-strip > button').filter({ hasText: 'main' }).click();
  await project.locator('.gh-nav').waitFor();
  assert.equal(await project.locator('.gh-legacy').count(), 0, 'Git uses the full split');
  await project.locator('.gh-back').click();
  assert.equal(await page.evaluate(() => S.screen), 'dWinProject', 'Git returns to the current Project');
  await project.locator('.bd-lanes').waitFor();
  await page.evaluate(() => { S.d.frontProject = 'list'; render(); });
  assert.equal(await project.locator('.pj-li.nav[title="Overview"]').getAttribute('aria-current'), 'true', 'The list layout keeps the Overview');
  await page.evaluate(() => { S.d.frontProject = ''; S.screen = 'dHome'; render(); });
  const home = page.locator('#desk .cc-view');
  assert.equal(await home.getByRole('tab', { selected: true }).textContent(), 'Board');
  assert.equal(await home.locator('.bd-lane').count(), 4);
  assert.equal(await page.locator('#desk .cc-side .cc-si').filter({ hasText: 'Running' }).count(), 0, 'Running is a lane on the board, not a page');
  await page.evaluate(() => { S.d.frontHome = 'list'; render(); });
  assert.equal(await home.getByRole('tab', { selected: true }).textContent(), 'Overview');
  assert.equal(await page.locator('#desk .cc-side .cc-si').filter({ hasText: 'Running' }).count(), 1);
  await page.evaluate(() => { S.d.frontHome = ''; render(); });
  await page.evaluate(() => { S.screen = 'dHome'; render(); });
  const baseline = await page.evaluate(() => JSON.stringify(S.d));
  for (const id of ids) {
    await page.evaluate(({ id, baseline }) => {
      S.d = JSON.parse(baseline);
      S.screen = id;
      render();
    }, { id, baseline });
    assert.ok(await page.locator('#desk').innerHTML(), `${id} rendered an empty screen`);
    const count = await page.evaluate(id => (VARIANTS[id] || []).length, id);
    screens++;
    for (let index = 0; index < count; index++) {
      try {
        await page.evaluate(({ id, index, baseline }) => {
          S.d = JSON.parse(baseline);
          S.screen = id;
          VARIANTS[id][index][1]();
          render();
        }, { id, index, baseline });
        variants++;
      } catch (error) {
        throw new Error(`${id}, variant ${index}: ${error.message}`);
      }
    }
  }
  for (const id of ['dHome', 'dAgentNew', 'dAgentSession', 'dWinProject', 'dMachines', 'dAppearance']) {
    await page.evaluate(({ id, baseline }) => { S.d = JSON.parse(baseline); S.screen = id; render(); }, { id, baseline });
    await page.waitForTimeout(100);
    await page.screenshot({ path: fileURLToPath(new URL(`desktop-${id}.png`, output)) });
  }
  await page.close();

  for (const platform of ['ios', 'android']) {
    const phone = await browser.newPage({ viewport: { width: 1280, height: 1000 } });
    watch(phone);
    await phone.goto(new URL(`${platform}/index.html`, root).href);
    const frames = phone.locator('iframe');
    const count = await frames.count();
    assert.ok(count > 0, `${platform} has no screen previews`);
    for (let index = 0; index < count; index++) {
      const frame = await frames.nth(index).elementHandle().then(element => element.contentFrame());
      assert.ok(frame, `${platform} preview ${index} did not load`);
      await frame.waitForSelector('.ph');
      for (const href of await frame.locator('.tabs a').evaluateAll(links => links.map(link => link.getAttribute('href')))) {
        const target = new URL(href, frame.url()).hash;
        assert.ok(target && await phone.locator(target).count(), `${platform} navigation target ${href} is missing`);
      }
      const dimensions = await frame.locator('.ph').first().evaluate(element => ({ width: element.clientWidth, scroll: element.scrollWidth }));
      assert.ok(dimensions.scroll <= dimensions.width + 1, `${platform} preview ${index} overflows horizontally`);
    }
    await phone.screenshot({ path: fileURLToPath(new URL(`${platform}.png`, output)), fullPage: true });
    await phone.close();
  }
  assert.deepEqual(errors, [], 'Browser errors or missing local assets');
  console.log(`Checked ${screens} desktop screens, ${variants} variants, and both phone galleries. Screenshots: mockups/test-results/`);
} finally {
  await browser.close();
}
