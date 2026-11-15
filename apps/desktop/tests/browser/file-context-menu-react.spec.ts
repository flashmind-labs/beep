import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("file link menu supports keyboard navigation and late local actions", async ({ page }) => {
  await openDesktop(page, true, { local_file: { apps: [] } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const body = page.locator("#agbody"); await expect(body).toBeVisible();
  await page.evaluate(() => {
    const link = document.createElement("a"); link.href = "notes.txt"; link.textContent = "Notes"; link.id = "test-file-link";
    document.querySelector("#agbody")?.append(link);
    window.__navigation.hold("local_file");
  });
  const link = page.locator("#test-file-link"); await link.focus(); await link.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "notes.txt" });
  await expect(menu).toBeVisible();
  const copy = menu.getByRole("menuitem", { name: "Copy path" });
  await menu.getByRole("menuitem", { name: "Open in Beep" }).press("c");
  await expect(copy).toBeFocused();
  await copy.press("ArrowUp");
  await expect(menu.getByRole("menuitem", { name: "Open in Beep" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(copy).toBeFocused();
  await page.evaluate(() => window.__navigation.release("local_file"));
  await expect(menu.getByRole("menuitem", { name: "Reveal in Finder" })).toBeVisible();
  await expect(copy).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0); await expect(link).toBeFocused();
});

test("a menu stays open while the page scrolls itself and closes on a wheel", async ({ page }) => {
  await openDesktop(page, true, { local_file: { apps: [] } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await expect(page.locator("#agbody")).toBeVisible();
  await page.evaluate(() => {
    const box = document.createElement("div"); box.id = "test-scroller"; box.style.cssText = "height:40px;overflow:auto";
    box.innerHTML = '<a href="notes.txt" id="test-file-link">Notes</a><div style="height:400px"></div>';
    document.querySelector("#agbody")?.append(box);
  });
  await page.locator("#test-file-link").click({ button: "right" });
  const menu = page.getByRole("menu", { name: "notes.txt" });
  await expect(menu).toBeVisible();

  await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
  await page.locator("#test-scroller").evaluate(box => { box.scrollTop = 100; });
  await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
  await expect(menu).toBeVisible();

  await page.locator("#test-scroller").hover();
  await page.mouse.wheel(0, -50);
  await expect(menu).toHaveCount(0);
});
