import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";
import type { FoundRow } from "../../src/stores/project/projects-state.ts";

const found: FoundRow[] = ["example/app", "example/library"].map(name => ({
  name, remote: `https://example.test/${name}`,
  places: [{ machine_id: "build-box", machine_name: "Build box", folder: `/srv/${name}` }],
  agents: [{ agent: "codex", sessions: 2 }], last_activity: 1,
}));

test("partial selection survives filtering and sync options stay accessible", async ({ page }) => {
  await openDesktop(page, false, { found_projects: found, found_machines: [], host_repos: [], syncthing_in_use: true });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await page.locator("#tools").getByRole("button", { name: "Add a project", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Add a project", exact: true });
  const all = sheet.getByRole("checkbox", { name: "Select all shown" });
  await sheet.getByRole("checkbox", { name: "Select example/app", exact: true }).check();
  await expect(all).toHaveJSProperty("indeterminate", true);
  await expect(sheet.getByRole("button", { name: /Import 1/ })).toBeEnabled();
  await sheet.getByRole("searchbox").fill("library");
  await expect(all).toHaveJSProperty("indeterminate", false);
  await all.check();
  await sheet.getByRole("searchbox").fill("");
  await expect(all).toBeChecked();
  await expect(all).toHaveJSProperty("indeterminate", false);
  const sync = sheet.locator("details.ap-sync");
  await expect(sync).not.toHaveAttribute("open");
  await sync.locator("summary").click();
  await expect(sync.getByRole("checkbox")).toBeVisible();
  await sync.getByRole("checkbox").uncheck();
  await sheet.getByRole("searchbox").fill("app");
  await expect(sync.getByRole("checkbox")).not.toBeChecked();
});

test("clicking a row ticks it, its Import button does not", async ({ page }) => {
  await openDesktop(page, false, { found_projects: found, found_machines: [], host_repos: [] });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await page.locator("#tools").getByRole("button", { name: "Add a project", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Add a project", exact: true });
  const box = sheet.getByRole("checkbox", { name: "Select example/app", exact: true });
  const row = sheet.locator('.ap-row[data-name="example/app"]');
  await row.locator(".n").click();
  await expect(box).toBeChecked();
  await box.click();
  await expect(box).not.toBeChecked();
  await row.locator("code").click();
  await expect(box).toBeChecked();
  await row.locator(".n").click();
  await expect(box).not.toBeChecked();
});

test("refresh preserves search, selected row focus and list scroll", async ({ page }) => {
  const rows = Array.from({ length: 35 }, (_, i) => ({ ...found[0], name: `example/repo-${i}` }));
  await openDesktop(page, false, { found_projects: rows, found_machines: [{ id: "build-box", name: "Build box" }], rescan_found: { rows, problem: null }, host_repos: [] });
  await page.evaluate(() => { window.__navigation.hold("rescan_found"); window.__navigation.open({ projects: { project: null } }); });
  await page.locator("#tools").getByRole("button", { name: "Add a project", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Add a project" });
  const row = sheet.locator('.ap-row[data-name="example/repo-12"]');
  await row.focus();
  await row.evaluate(el => { Reflect.set(window, "savedRow", el); const list = el.parentElement; if (list) { list.scrollTop = 300; Reflect.set(window, "savedScroll", list.scrollTop); } });
  await page.evaluate(() => window.__navigation.release("rescan_found"));
  await expect(row).toBeFocused();
  expect(await row.evaluate(el => el === Reflect.get(window, "savedRow") && el.parentElement?.scrollTop === Reflect.get(window, "savedScroll"))).toBe(true);
});

test("search regains focus when a refresh removes the focused row", async ({ page }) => {
  await openDesktop(page, false, { found_projects: found, found_machines: [{ id: "build-box", name: "Build box" }], rescan_found: { rows: [found[1]], problem: null }, host_repos: [] });
  await page.evaluate(() => { window.__navigation.hold("rescan_found"); window.__navigation.open({ projects: { project: null } }); });
  await page.locator("#tools").getByRole("button", { name: "Add a project", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Add a project" });
  await sheet.locator('.ap-row[data-name="example/app"]').focus();
  await page.evaluate(() => window.__navigation.release("rescan_found"));
  await expect(sheet.locator('.ap-row[data-name="example/app"]')).toHaveCount(0);
  await expect(sheet.getByRole("searchbox")).toBeFocused();
});
