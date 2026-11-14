import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { openDesktop } from "./harness.ts";

for (const width of [1280, 390]) {
  test(`nested message lists fit at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.route("https://app.test/skins.css", async route => {
      await route.fulfill({ contentType: "text/css", body: await readFile(new URL("../../dist/skins.css", import.meta.url), "utf8") });
    });
    await page.setContent('<base href="https://app.test/"><div class="dbody"><div class="dcoderlog"><div class="dcoderen user md"><b>You</b><div class="said"></div></div></div></div>');
    await page.addStyleTag({ content: await readFile(new URL("../../dist/style.css", import.meta.url), "utf8") });
    await page.locator('.said').evaluate(node => {
      const list = document.createElement('ol');
      const item = document.createElement('li');
      item.textContent = 'Conversation summary';
      const nested = document.createElement('ul');

      for (let i = 0; i < 4; i++) {
        const row = document.createElement('li');
        row.textContent = 'A longer description of the requested changes and the remaining work. '.repeat(12);
        nested.append(row);
      }

      item.append(nested);
      list.append(item);
      node.append(list);
    });
    expect(await page.locator('.dbody').evaluate(node => node.scrollWidth - node.clientWidth)).toBeLessThanOrEqual(1);

    for (const item of await page.locator('li').all()) {
      expect(await item.evaluate(node => node.scrollWidth - node.clientWidth)).toBeLessThanOrEqual(1);
    }
  });
}

for (const width of [1280, 390]) {
  test(`code copy stays at the right edge while scrolling at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.setContent('<div class="dbody"><div class="dcoderlog"><div class="dcoderen agent md"><pre class="dcode"><button class="dcopy">Copy</button><code></code></pre></div></div></div>');
    await page.addStyleTag({ content: await readFile(new URL("../../dist/style.css", import.meta.url), "utf8") });
    const code = page.locator('pre code');
    const button = page.getByRole('button', { name: 'Copy', exact: true });
    await code.evaluate(node => { node.textContent = 'sample command '.repeat(100) + '\nsecond line'; });
    await button.focus();
    const initial = await button.boundingBox();
    expect(initial).not.toBeNull();

    for (const fraction of [0.5, 1, 0]) {
      const offset = await code.evaluate((node, fraction) => {
        node.scrollLeft = (node.scrollWidth - node.clientWidth) * fraction;
        return node.scrollLeft;
      }, fraction);
      if (fraction > 0) expect(offset).toBeGreaterThan(100);
      else expect(offset).toBe(0);

      const position = await button.boundingBox();
      const bounds = await page.locator('pre').boundingBox();
      expect(position).not.toBeNull();
      expect(bounds).not.toBeNull();
      if (initial && position && bounds) {
        expect(Math.abs(position.x - initial.x)).toBeLessThanOrEqual(1);
        const rightGap = bounds.x + bounds.width - position.x - position.width;
        expect(rightGap).toBeGreaterThanOrEqual(0);
        expect(rightGap).toBeLessThanOrEqual(10);
      }
    }

    expect(await page.locator('.dbody').evaluate(node => node.scrollWidth - node.clientWidth)).toBeLessThanOrEqual(1);
  });
}

test("the start screen does not scroll sideways under its painting", async ({ page }, info) => {
  await openDesktop(page, false, {});
  await page.keyboard.press(info.project.name === "Windows" ? "Control+n" : "Meta+n");
  const wrap = page.locator("#agcomposer");
  await expect(wrap.locator(".agc-intro h3")).toBeVisible();
  await wrap.hover();
  const scrolled = wrap.evaluate(node => new Promise<number>(done => {
    node.addEventListener("wheel", () => requestAnimationFrame(() => requestAnimationFrame(() => done(node.scrollLeft))), { once: true });
  }));
  await page.mouse.wheel(400, 0);
  expect(await scrolled).toBe(0);
});
