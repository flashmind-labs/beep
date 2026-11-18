import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

for (const width of [1280, 640]) {
  test(`Find overlays the conversation without moving messages at ${width}px`, { tag: width === 1280 ? "@windows" : [] }, async ({ page }, info) => {
    await page.setViewportSize({ width, height: 900 });
    await openDesktop(page, true);
    await page.evaluate(() => window.__navigation.open({ agent: {
      kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex",
      title: "First notification", cwd: "/srv/project", status: "working", doing: null,
      started_at: 1, last_activity: 1, external: false,
    } }));
    const body = page.locator("#agbody");
    await expect(page.locator("#agtitle")).toHaveText("First notification");
    await page.locator("#agtitle").click();
    await body.evaluate(node => {
      for (let i = 0; i < 80; i++) {
        const message = document.createElement("p");
        message.textContent = `Example message ${i}`;
        node.append(message);
      }

      node.scrollTop = 400;
    });
    const metrics = () => body.evaluate(node => ({
      top: node.getBoundingClientRect().top, height: node.clientHeight, scroll: node.scrollTop,
    }));
    const before = await metrics();
    await page.keyboard.press(info.project.name === "Windows" ? "Control+f" : "Meta+f");
    const bar = page.getByRole("search", { name: "Search conversation" });
    await expect(bar.getByRole("textbox")).toBeFocused();
    expect(await metrics()).toEqual(before);
    expect(await bar.evaluate(node => {
      const rect = node.getBoundingClientRect();
      return node.contains(document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2));
    })).toBe(true);
    const barBox = await bar.boundingBox();
    const bodyBox = await body.boundingBox();
    expect(barBox).not.toBeNull();
    expect(bodyBox).not.toBeNull();

    if (barBox && bodyBox) {
      expect(barBox.y).toBeGreaterThanOrEqual(bodyBox.y);
      expect(barBox.y + barBox.height).toBeLessThan(bodyBox.y + bodyBox.height);
      expect(barBox.x).toBeGreaterThanOrEqual(bodyBox.x);
      expect(barBox.x + barBox.width).toBeLessThanOrEqual(bodyBox.x + bodyBox.width);
    }

    await page.keyboard.press("Escape");
    await expect(bar).toHaveCount(0);
    await expect(page.locator(".chat-find-layer")).toHaveCount(0);
    expect(await metrics()).toEqual(before);
  });
}
