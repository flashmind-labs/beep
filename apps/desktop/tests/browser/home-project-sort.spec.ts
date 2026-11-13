import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

for (const width of [1280, 375]) {
  test(`Home project sorting and the shared menu at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.addInitScript(() => {
      localStorage.setItem("beep.projects.visited", JSON.stringify({ "example/beta": 400 }));
    });
    const projects = ["alpha", "beta"].map(name => ({ name: `example/${name}`, url: "", machine_id: "build-box", machine_name: "Build box", folder: `/srv/${name}`, cloned: true, imported_at: 1 }));
    const running = ["alpha", "beta"].map((name, index) => ({ kind: "agent", machine_id: "build-box", machine_name: "Build box", id: name, agent: "codex", title: `Task ${name}`, cwd: `/srv/${name}`, status: "working", doing: null, started_at: 1, last_activity: index === 0 ? 300 : 100, external: false, question: null }));
    await openDesktop(page, false, { center_home: { machines: [], projects, running, needs_you: [] }, projects_sessions: [] });
    const sort = page.getByRole("button", { name: "Sort projects" });
    const titles = width > 700 ? page.locator(".bd-rows > .pj") : page.locator(".bd-lane .bd-card > b");
    const alpha = width > 700 ? "example/alpha" : "Task alpha", beta = width > 700 ? "example/beta" : "Task beta";
    await expect(sort).toHaveText("Default");
    await expect(titles).toHaveText([alpha, beta]);
    await sort.click();
    await expect(page.getByRole("menuitemradio", { name: "Default", exact: true })).toHaveAttribute("aria-checked", "true");
    await page.keyboard.press("Escape");
    await expect(sort).toBeFocused();
    await expect(page.getByRole("menu")).toHaveCount(0);
    await sort.press("Enter");
    await page.keyboard.press("End");
    await page.keyboard.press("Enter");
    await expect(sort).toHaveText("Last opened");
    await expect(titles).toHaveText([beta, alpha]);
    await page.reload();
    await expect(sort).toHaveText("Last opened");
    await expect(titles).toHaveText([beta, alpha]);
    await sort.click();
    await page.getByRole("menuitemradio", { name: "Recent activity", exact: true }).click();
    await expect(titles).toHaveText([alpha, beta]);
    await sort.click();
    const menu = page.getByRole("menu");
    const bounds = await menu.boundingBox();
    expect(bounds).not.toBeNull();
    if (bounds) {
      expect(bounds.x).toBeGreaterThanOrEqual(0);
      expect(bounds.x + bounds.width).toBeLessThanOrEqual(width);
    }
    await page.screenshot({ path: `test-results/home-sort-${width}.png` });
    await page.mouse.click(10, 10);
    await expect(menu).toHaveCount(0);
    await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
    await expect(sort).toHaveText("Default");
    if (width > 700) {
      await page.evaluate(() => window.__navigation.open({ projects: { project: "example/alpha" } }));
      await expect(page.locator(".pj-strip")).toBeVisible();
      await page.evaluate(() => window.__navigation.open("home"));
      await sort.click();
      await page.getByRole("menuitemradio", { name: "Last opened", exact: true }).click();
      await expect(titles).toHaveText([alpha, beta]);
    }
  });
}
