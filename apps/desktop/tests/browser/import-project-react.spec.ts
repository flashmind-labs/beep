import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const machine = { id: "one", name: "Build box", online: true, can_host: true, seen_at: 0, version: "1", env: null, joined: true, apps: [], gpus: [], capabilities: [], agents: 0, stats: null };
const home = { machines: [machine], projects: [], running: [], needs_you: [] };
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

test("Syncthing choice follows the folder and keeps an explicit choice", async ({ page }) => {
  const sheet = await openImport(page);
  await page.evaluate(() => window.__navigation.respond("project_syncthing_files", { available: true, enabled: false, label: "Projects" }));
  await sheet.getByRole("button", { name: "Build box", exact: true }).click();
  await sheet.getByRole("button", { name: "Choose…" }).click();
  await sheet.getByRole("button", { name: "Use this folder" }).click();
  const checkbox = sheet.getByRole("checkbox", { name: "Keep Syncthing file sync" });
  await expect(checkbox).not.toBeChecked();
  await checkbox.check();
  await expect(checkbox).toBeFocused();
  await sheet.getByRole("textbox", { name: "Repository" }).fill("example/project");
  await expect(checkbox).toBeChecked();
  await page.evaluate(() => window.__navigation.hold("import_project"));
  await sheet.getByRole("button", { name: "Import", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "import_project").map(call => call.args.syncthingFiles))).toEqual([true]);
});

test("shared Syncthing controls mount and clean up in the other project sheets", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, list_machine_folder: folder });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  const add = page.getByRole("dialog", { name: "Add a project" });
  await add.getByText("Sync options").click();
  const choice = add.getByRole("checkbox", { name: "Keep Syncthing file sync" });
  await expect(choice).toBeVisible();
  await choice.uncheck();
  await expect(choice).not.toBeChecked();
  await add.getByRole("button", { name: "Choose folder" }).click();
  const folderSheet = page.getByRole("dialog", { name: "Add a folder" });
  await expect(folderSheet.getByRole("checkbox", { name: "Keep Syncthing file sync" })).toBeVisible();
  await folderSheet.getByRole("button", { name: "Back" }).click();
  await add.getByText("Sync options").click();
  await expect(add.getByRole("checkbox", { name: "Keep Syncthing file sync" })).toBeChecked();
});

test("add folder keeps the selected checkout and submits its sync preference", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, list_machine_folder: { ...folder, origin: "https://example.test/example/project.git" }, folder_branch: "main", project_syncthing_files: { available: true, enabled: true, label: "Projects" } });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "Choose folder" }).click();
  const sheet = page.getByRole("dialog", { name: "Add a folder" });
  await sheet.getByRole("button", { name: "Choose…" }).click();
  await sheet.getByRole("button", { name: "Use this folder" }).click();
  await expect(sheet.getByText(/Branch main/)).toBeVisible();
  await sheet.getByRole("checkbox", { name: "Keep Syncthing file sync" }).uncheck();
  await page.evaluate(() => window.__navigation.hold("import_project"));
  await sheet.getByRole("button", { name: "Add project" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "import_project").map(call => call.args))).toEqual([{ repo: "https://example.test/example/project.git", machineId: "one", folder: "/srv", worktrees: [], parent: null, syncthingFiles: false }]);
});


test("with project sync on, a shared folder starts off Syncthing and an untouched add follows the setting", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, list_machine_folder: { ...folder, origin: "https://example.test/example/project.git" }, folder_branch: "main", project_sync_get: true, project_syncthing_files: { available: true, enabled: true, label: "Projects" } });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "Choose folder" }).click();
  const sheet = page.getByRole("dialog", { name: "Add a folder" });
  await sheet.getByRole("button", { name: "Choose…" }).click();
  await sheet.getByRole("button", { name: "Use this folder" }).click();
  await expect(sheet.getByText(/Branch main/)).toBeVisible();
  await expect(sheet.getByRole("checkbox", { name: "Keep Syncthing file sync" })).not.toBeChecked();
  await page.evaluate(() => window.__navigation.hold("import_project"));
  await sheet.getByRole("button", { name: "Add project" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "import_project").map(call => call.args.syncthingFiles))).toEqual([null]);
});

test("add folder restores focus and ignores an older branch read", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, list_machine_folder: { ...folder, origin: "https://example.test/example/project.git" }, folder_branch: "old" });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "Choose folder" }).click();
  const sheet = page.getByRole("dialog", { name: "Add a folder" });
  const choose = sheet.getByRole("button", { name: "Choose…" });
  await choose.click();
  await sheet.locator(".ip-browse").getByRole("button", { name: "Cancel" }).click();
  await expect(choose).toBeFocused();
  await page.evaluate(() => window.__navigation.holdNext("folder_branch"));
  await choose.click();
  await sheet.getByRole("button", { name: "Use this folder" }).click();
  await expect(choose).toBeFocused();
  await page.evaluate(() => window.__navigation.respond("folder_branch", "new"));
  await choose.click();
  await sheet.getByRole("button", { name: "Use this folder" }).click();
  await expect(sheet.getByText(/Branch new/)).toBeVisible();
  await page.evaluate(() => { window.__navigation.respond("folder_branch", "old"); window.__navigation.release("folder_branch"); });
  await expect(sheet.getByText(/Branch new/)).toBeVisible();
});

test("add folder only opens the local picker when requested", async ({ page }) => {
  const local = { ...machine, id: "local", name: "Local machine" };
  await openDesktop(page, false, { center_home: { ...home, machines: [local, machine] }, mac_control: { id: "local" }, list_machine_folder: folder, pick_folder: null });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "Choose folder" }).click();
  const sheet = page.getByRole("dialog", { name: "Add a folder" });
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "pick_folder").length)).toBe(0);
  await sheet.getByRole("button", { name: "Choose…" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "pick_folder").length)).toBe(1);
  await sheet.getByRole("button", { name: "Build box", exact: true }).click();
  await sheet.getByRole("button", { name: "Choose…" }).click();
  await expect(sheet.locator(".ip-browse")).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "list_machine_folder").map(call => call.args))).toEqual([{ machineId: "one", path: null }]);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "pick_folder").length)).toBe(1);
});
