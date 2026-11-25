import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const machine = { id: "one", name: "Build box", online: true, can_host: true, seen_at: 0, version: "1", env: null, apps: [], gpus: [], capabilities: [], agents: 0, stats: null };
const home = { machines: [machine], projects: [], running: [], needs_you: [] };
const folder = { path: "/srv", parent: "/", exists: true, folders: ["projects", "public"], more: 0, empty: false, origin: null, repos: [] };
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
  // No machine shares the folder through Syncthing, so its choice stays out.
  await expect(sheet.getByRole("checkbox", { name: "Keep Syncthing file sync" })).toHaveCount(0);
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
  await page.evaluate(() => { window.__navigation.respond("import_project", { name: "example/first", url: "https://example.test/example/first", machine_id: "one", machine_name: "Build box", folder: "/srv/example/first", cloned: true }); window.__navigation.release("import_project"); });
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
  await page.getByRole("dialog", { name: "Choose a folder" }).getByRole("option", { name: /^Use this folder/ }).click();
  await expect(sheet.getByRole("combobox", { name: "Folder" })).toHaveValue("/srv");
  await page.evaluate(() => window.__navigation.hold("import_project"));
  await sheet.getByRole("button", { name: "Import", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "import_project").map(call => call.args))).toEqual([{ repo: "example/project", machineId: "one", folder: "/srv", worktrees: [{ branch: "feature", machine_id: null }], parent: null, syncthingFiles: null }]);
});

test("typing a Clone into path submits that folder rather than nesting org/repo, and Default resets", async ({ page }) => {
  const missing = { path: "/home/user/work/project", parent: "/home/user/work", exists: false, folders: [], more: 0, empty: true, origin: null, repos: [] };
  await openDesktop(page, false, { center_home: home, machine_search: "/srv/projects", list_machine_folder: missing, preview_import: null, find_import_fork: null, project_syncthing_files: { available: true, enabled: true, label: "Work" } });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "Clone repository", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Import a project" });
  await sheet.getByRole("textbox", { name: "Repository" }).fill("example/project");
  await sheet.getByRole("button", { name: "Build box", exact: true }).click();
  const path = sheet.getByRole("combobox", { name: "Clone into" });
  await expect(path).toHaveValue("/srv/projects");
  await expect(sheet.getByText("Goes in /srv/projects/example/project.")).toBeVisible();
  await path.fill("~/work/project");
  await expect(sheet.getByRole("button", { name: "Default", exact: true })).toBeVisible();
  await expect(sheet.getByText("Not there yet: the clone makes it.")).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_syncthing_files").map(call => call.args.folder))).toContain("~/work/project");
  await sheet.getByRole("button", { name: "Default", exact: true }).click();
  await expect(path).toHaveValue("/srv/projects");
  await expect(sheet.getByText("Goes in /srv/projects/example/project.")).toBeVisible();
  await path.fill("~/work/project");
  await page.evaluate(() => window.__navigation.hold("import_project"));
  await sheet.getByRole("button", { name: "Import", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "import_project").map(call => call.args))).toEqual([{ repo: "example/project", machineId: "one", folder: "~/work/project", worktrees: [], parent: null, syncthingFiles: null }]);
});

test("the folder picker and import progress restore keyboard focus", async ({ page }) => {
  const sheet = await openImport(page);
  await sheet.getByRole("textbox", { name: "Repository" }).fill("example/project");
  await sheet.getByRole("button", { name: "Build box", exact: true }).click();
  const choose = sheet.getByRole("button", { name: "Choose…" });
  await choose.click();
  const picker = page.getByRole("dialog", { name: "Choose a folder" });
  await expect(picker.getByRole("combobox")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(picker).toHaveCount(0);
  await expect(choose).toBeFocused();
  await choose.click();
  await picker.getByRole("option", { name: /^Use this folder/ }).click();
  await expect(choose).toBeFocused();
  await page.evaluate(() => window.__navigation.hold("import_project"));
  await sheet.getByRole("button", { name: "Import", exact: true }).click();
  await expect(sheet.getByRole("button", { name: "Close", exact: true })).toBeFocused();
  await page.evaluate(() => { window.__navigation.respond("import_project", { name: "example/project", url: "https://example.test/example/project", machine_id: "one", machine_name: "Build box", folder: "/srv/example/project", cloned: true }); window.__navigation.release("import_project"); });
  await expect(sheet.getByRole("button", { name: "Open project" })).toBeFocused();
});

test("Syncthing choice follows the folder and keeps an explicit choice", async ({ page }) => {
  const sheet = await openImport(page);
  await page.evaluate(() => window.__navigation.respond("project_syncthing_files", { available: true, enabled: false, label: "Projects" }));
  await sheet.getByRole("button", { name: "Build box", exact: true }).click();
  await sheet.getByRole("button", { name: "Choose…" }).click();
  await page.getByRole("dialog", { name: "Choose a folder" }).getByRole("option", { name: /^Use this folder/ }).click();
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

const checkout = { ...folder, origin: "https://example.test/example/project.git" };

/// Opens Add a folder from Add a project.
async function openAddFolder(page: Page, responses: Record<string, unknown>) {
  await openDesktop(page, false, { center_home: home, machine_search: "/srv", list_machine_folder: checkout, ...responses });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "Choose folder" }).click();
  return page.getByRole("dialog", { name: "Add a folder" });
}

test("Sync options show only when a machine runs Syncthing", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, list_machine_folder: folder });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Add a project" }).getByText("Choose folder")).toBeVisible();
  await expect(page.getByText("Sync options")).toHaveCount(0);
});

test("shared Syncthing controls mount and clean up in the other project sheets", async ({ page }) => {
  const sheet = await openAddFolder(page, { syncthing_in_use: true, project_syncthing_files: { available: true, enabled: true, label: "Projects" } });
  await expect(sheet.getByRole("checkbox", { name: "Keep Syncthing file sync" })).toBeChecked();
  await sheet.getByRole("checkbox", { name: "Keep Syncthing file sync" }).uncheck();
  await sheet.getByRole("combobox").press("Escape");
  const add = page.getByRole("dialog", { name: "Add a project" });
  await add.getByText("Sync options").click();
  await expect(add.getByRole("checkbox", { name: "Keep Syncthing file sync" })).toBeChecked();
});

test("add folder takes the checkout shown and submits its sync preference", async ({ page }) => {
  const sheet = await openAddFolder(page, { project_syncthing_files: { available: true, enabled: true, label: "Projects" } });
  await expect(sheet.getByRole("option", { name: /^Add / })).toContainText("/srv");
  await sheet.getByRole("checkbox", { name: "Keep Syncthing file sync" }).uncheck();
  await page.evaluate(() => window.__navigation.hold("import_project"));
  await sheet.getByRole("option", { name: /^Add / }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "import_project").map(call => call.args))).toEqual([{ repo: "https://example.test/example/project.git", machineId: "one", folder: "/srv", worktrees: [], parent: null, syncthingFiles: false }]);
});

test("with project sync on, a shared folder starts off Syncthing and an untouched add follows the setting", { tag: "@windows" }, async ({ page }, info) => {
  const sheet = await openAddFolder(page, { project_sync_get: true, project_syncthing_files: { available: true, enabled: true, label: "Projects" } });
  await expect(sheet.getByRole("checkbox", { name: "Keep Syncthing file sync" })).not.toBeChecked();
  await page.evaluate(() => window.__navigation.hold("import_project"));
  await sheet.getByRole("combobox").press(info.project.name === "Windows" ? "Control+Enter" : "Meta+Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "import_project").map(call => call.args.syncthingFiles))).toEqual([null]);
});

test("add folder takes a typed path, offers nothing for a plain folder, and says nothing of Syncthing without it", async ({ page }) => {
  const plain = { ...folder, path: "/srv/notes", parent: "/srv" };
  const sheet = await openAddFolder(page, { machine_folders: { "/srv/notes": plain } });
  const field = sheet.getByRole("combobox");
  await expect(field).toBeFocused();
  await field.fill("/srv/notes");
  await page.keyboard.press("Enter");
  await expect(sheet.getByRole("option", { name: "projects" })).toBeVisible();
  await expect(sheet.getByRole("option", { name: /^Add / })).toHaveCount(0);
  await expect(sheet.getByText(/Syncthing/)).toHaveCount(0);
});

test("add folder starts on this computer and follows the machine picked", async ({ page }) => {
  const local = { ...machine, id: "local", name: "Local machine" };
  await openDesktop(page, false, { center_home: { ...home, machines: [local, machine] }, mac_control: { id: "local" }, machine_search: "/srv", list_machine_folder: folder });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "Choose folder" }).click();
  const sheet = page.getByRole("dialog", { name: "Add a folder" });
  const listed = () => page.evaluate(() => [...new Set(window.__navigation.calls().filter(call => call.command === "list_machine_tree").map(call => call.args.machineId))]);
  await expect.poll(listed).toEqual(["local"]);
  await sheet.getByRole("button", { name: "Build box", exact: true }).click();
  await expect.poll(listed).toEqual(["local", "one"]);
});
