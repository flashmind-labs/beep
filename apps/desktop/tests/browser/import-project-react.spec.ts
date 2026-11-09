import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const machine = { id: "one", name: "Build box", online: true, can_host: true, seen_at: 0, version: "1", env: null, joined: true, apps: [], gpus: [], capabilities: [], agents: 0, stats: null };
const home = { machines: [machine], projects: [], running: [], needs_you: [], coming_up: null, updates: null };
const folder = { path: "/srv", parent: "/", exists: true, folders: ["projects", "public"], more: 0, empty: false, origin: null };
async function openImport(page: Page) {
  await openDesktop(page, false, { center_home: home, list_machine_folder: folder, preview_import: null, find_import_fork: null });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "Clone repository", exact: true }).click();
  return page.getByRole("dialog", { name: "Import a project" });
}
test("repository and folder controls retain focus across preview and completion", async ({ page }) => {
  const sheet = await openImport(page), repo = sheet.getByRole("textbox", { name: "Repository" });
  await repo.fill("example/project");
  await repo.evaluate(el => { if (el instanceof HTMLInputElement) { el.setSelectionRange(2, 5); Reflect.set(window, "importRepo", el); } });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "preview_import"))).toBe(true);
  expect(await repo.evaluate(el => el === Reflect.get(window, "importRepo") && document.activeElement === el && el instanceof HTMLInputElement && el.selectionStart === 2 && el.selectionEnd === 5)).toBe(true);
  await sheet.getByRole("button", { name: "Build box", exact: true }).click();
  const path = sheet.getByRole("combobox", { name: "Clone into" });
  await path.fill("/srv/p");
  await expect(sheet.getByRole("option", { name: "projects", exact: true })).toBeVisible();
  await path.press("ArrowDown");
  await path.press("Enter");
  await expect(path).toHaveValue("/srv/public/");
  await expect(path).toBeFocused();
  await expect(sheet.getByRole("checkbox", { name: "Keep Syncthing file sync" })).toBeVisible();
});

test("closed import result does not replace a later import sheet", async ({ page }) => {
  const sheet = await openImport(page);
  await sheet.getByRole("textbox", { name: "Repository" }).fill("example/first");
  await page.evaluate(() => window.__navigation.hold("import_project"));
  await sheet.getByRole("button", { name: "Import", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "import_project").length)).toBe(1);
  await sheet.getByRole("button", { name: "Close", exact: true }).click();
  await expect(sheet).toHaveCount(0);
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "Clone repository", exact: true }).click();
  const next = page.getByRole("dialog", { name: "Import a project" });
  await next.getByRole("textbox", { name: "Repository" }).fill("example/second");
  await page.evaluate(() => { window.__navigation.respond("import_project", { name: "example/first", url: "https://example.test/example/first", machine_id: "one", machine_name: "Build box", folder: "/srv/example/first", cloned: true, imported_at: 1 }); window.__navigation.release("import_project"); });
  await expect(next.getByRole("textbox", { name: "Repository" })).toHaveValue("example/second");
});

test("branch choices and selected folder reach the import command", async ({ page }) => {
  const preview = { project: "example/project", checkouts: [], default_branch: "main", branches: [{ name: "feature", pr: 1, pr_title: "Change", ahead: 1, worktree: null, in_checkout: false, committed_at: null }], note: "" };
  const sheet = await openImport(page);
  await page.evaluate(value => window.__navigation.respond("preview_import", value), preview);
  await sheet.getByRole("textbox", { name: "Repository" }).fill("example/project");
  await expect(sheet.locator(".ip-branch").filter({ hasText: "feature" })).toBeVisible();
  await sheet.getByRole("button", { name: "Build box", exact: true }).click();
  await expect(sheet.getByRole("button", { name: "Choose…" })).toBeEnabled();
  await sheet.getByRole("button", { name: "Choose…" }).click();
  await expect(sheet.locator(".ip-browse")).toBeVisible();
  await sheet.getByRole("button", { name: "Use this folder" }).click();
  await expect(sheet.getByRole("combobox", { name: "Folder" })).toHaveValue("/srv");
  await page.evaluate(() => window.__navigation.hold("import_project"));
  await sheet.getByRole("button", { name: "Import", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "import_project").map(call => call.args))).toEqual([{ repo: "example/project", machineId: "one", folder: "/srv", worktrees: [{ branch: "feature", machine_id: null }], parent: null, syncthingFiles: null }]);
});

test("folder browser and import progress restore keyboard focus", async ({ page }) => {
  const sheet = await openImport(page);
  const repo = sheet.getByRole("textbox", { name: "Repository" });
  await repo.fill("example/project");
  await sheet.getByRole("button", { name: "Build box", exact: true }).click();
  await sheet.getByRole("button", { name: "Choose…" }).click();
  await sheet.locator(".ip-browse").getByRole("button", { name: "Cancel" }).click();
  await expect(repo).toBeFocused();
  await sheet.getByRole("button", { name: "Choose…" }).click();
  await sheet.getByRole("button", { name: "Use this folder" }).click();
  await expect(repo).toBeFocused();
  await page.evaluate(() => window.__navigation.hold("import_project"));
  await sheet.getByRole("button", { name: "Import", exact: true }).click();
  await expect(sheet.getByRole("button", { name: "Close", exact: true })).toBeFocused();
  await page.evaluate(() => { window.__navigation.respond("import_project", { name: "example/project", url: "https://example.test/example/project", machine_id: "one", machine_name: "Build box", folder: "/srv/example/project", cloned: true, imported_at: 1 }); window.__navigation.release("import_project"); });
  await expect(sheet.getByRole("button", { name: "Open project" })).toBeFocused();
});
