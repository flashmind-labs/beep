import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";

for (const width of [1280, 390]) {
  test(`nested message lists fit at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.setContent('<div class="dbody"><div class="dcoderlog"><div class="dcoderen user md"><b>You</b><div class="said"></div></div></div></div>');
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
