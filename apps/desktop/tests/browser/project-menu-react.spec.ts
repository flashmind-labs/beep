import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

const openMenu = async (page: import("@playwright/test").Page) => {
  await page.evaluate(() => window.__navigation.open({ projects: { project: "example/widget" } }));
  await page.getByRole("button", { name: "More", exact: true }).click();
  return page.getByRole("menu", { name: projectName });
};

test("the project menu groups sync toggles under a label with Remove last", async ({ page }) => {
  await openDesktop(page, false, { project_ignored_sync: { enabled: true, folders: ["assets"] }, project_git_auto_recovery: false, project_syncthing_mode: null });
  const menu = await openMenu(page);
  const sync = menu.getByRole("group", { name: "Sync on all machines" }).first();
  await expect(sync).toBeVisible();
  await expect(menu.getByRole("menuitemcheckbox", { name: "Sync ignored folders" })).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Use Beep sync instead of Syncthing" })).toBeVisible();

  const lines = await menu.locator('[role="menuitem"], [role="menuitemcheckbox"], [role="separator"], header').evaluateAll(nodes => nodes.map(node => node.getAttribute("role") === "separator" ? "-" : node.tagName === "HEADER" ? `# ${node.textContent}` : node.textContent));
  expect(lines).toEqual(["Pin", "Open in New Window", "Rename…", "-", "# Sync on all machines", "Preserve edits before Git sync", "Sync ignored folders", "Choose ignored folders…", "Use Beep sync instead of Syncthing", "-", "Remove project…"]);
  await expect(menu.getByRole("menuitemcheckbox", { name: "Preserve edits before Git sync" })).toHaveAttribute("aria-checked", "false");
  await expect(menu.getByRole("menuitemcheckbox", { name: "Sync ignored folders" })).toHaveAttribute("aria-checked", "true");
  await expect(menu.getByRole("menuitem", { name: "Remove project…" })).toHaveClass(/warn/);
  await expect(menu.getByRole("menuitem", { name: "All projects" })).toHaveCount(0);

  await menu.getByRole("menuitemcheckbox", { name: "Sync ignored folders" }).click();
  await expect(menu).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_ignored_sync" && "settings" in call.args).map(call => call.args))).toEqual([
    { project: projectName, settings: { enabled: false, folders: ["assets"] } },
  ]);
});

test("the project menu hides the folder choice while ignored sync is off and offers Syncthing back", async ({ page }) => {
  await openDesktop(page, false, { project_ignored_sync: { enabled: false, folders: [] }, project_git_auto_recovery: true, project_syncthing_mode: false });
  const menu = await openMenu(page);
  await expect(menu.getByRole("menuitem", { name: "Use Syncthing instead of Beep sync" })).toBeVisible();
  await expect(menu.getByRole("menuitemcheckbox", { name: "Preserve edits before Git sync" })).toHaveAttribute("aria-checked", "true");
  await expect(menu.getByRole("menuitemcheckbox", { name: "Sync ignored folders" })).toHaveAttribute("aria-checked", "false");
  await expect(menu.getByRole("menuitem", { name: "Choose ignored folders…" })).toHaveCount(0);
  await expect(menu.getByRole("menuitem", { name: "Use Beep sync instead of Syncthing" })).toHaveCount(0);

  await menu.getByRole("menuitem", { name: "Use Syncthing instead of Beep sync" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_syncthing_mode" && "enabled" in call.args).map(call => call.args))).toEqual([
    { project: projectName, enabled: true },
  ]);
});

test("in the phone layout the project menu stays open after More scrolls the header", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDesktop(page, false, { project_ignored_sync: { enabled: false, folders: [] }, project_git_auto_recovery: false, project_syncthing_mode: null });
  const menu = await openMenu(page);
  await expect(menu.getByRole("menuitemcheckbox", { name: "Sync ignored folders" })).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Remove project…" })).toBeVisible();
});

test("the project menu moves by keyboard across its sections and a toggle asks before turning on", async ({ page }) => {
  await openDesktop(page, false, { project_ignored_sync: { enabled: true, folders: ["assets"] }, project_git_auto_recovery: false, project_syncthing_mode: null });
  const menu = await openMenu(page);
  const remove = menu.getByRole("menuitem", { name: "Remove project…" });
  await expect(remove).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Pin", exact: true })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(menu.getByRole("menuitem", { name: "Open in New Window" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(menu.getByRole("menuitem", { name: "Rename…", exact: true })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  const preserve = menu.getByRole("menuitemcheckbox", { name: "Preserve edits before Git sync" });
  await expect(preserve).toBeFocused();
  await page.keyboard.press("End");
  await expect(remove).toBeFocused();
  await page.keyboard.press("ArrowUp");
  await expect(menu.getByRole("menuitem", { name: "Use Beep sync instead of Syncthing" })).toBeFocused();
  await page.keyboard.press("Home");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(preserve).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(menu).toHaveCount(0);
  await expect(page.getByRole("dialog", { name: "Preserve edits before Git sync?" })).toBeVisible();
});

test("project rename keeps its identity and only renames the repository when selected", async ({ page }) => {
  await openDesktop(page, false);
  const menu = await openMenu(page);
  await menu.getByRole("menuitem", { name: "Rename…", exact: true }).click();
  await expect(page.getByLabel("Project name", { exact: true })).toHaveValue(projectName);
  await expect(page.getByLabel("Repository name", { exact: true })).toBeDisabled();
  await page.getByLabel("Project name", { exact: true }).fill("New name");
  await page.evaluate(project => window.__navigation.respond("center_home", {
    machines: [], running: [], needs_you: [], projects: [{ name: project, display_name: "New name", url: "https://example.test/example/widget", machine_id: "build-box", machine_name: "build-box", folder: "/srv/project", cloned: true, imported_at: 1 }],
  }), projectName);
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "rename_project").map(call => call.args))).toEqual([{ project: projectName, name: "New name", repository: null }]);
  await expect(page.getByRole("heading", { name: "New name", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "New name", exact: true })).toBeVisible();
});

test("repository rename sends its own name and shows errors without discarding the draft", async ({ page }) => {
  await openDesktop(page, false);
  await page.evaluate(() => window.__navigation.rejectOnce("rename_project"));
  const menu = await openMenu(page);
  await menu.getByRole("menuitem", { name: "Rename…", exact: true }).click();
  await page.getByLabel("Project name", { exact: true }).fill("New name");
  await page.getByLabel("Also rename the hosted repository").check();
  await page.getByLabel("Repository name", { exact: true }).fill("new-repo");
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "rename_project").map(call => call.args))).toEqual([{ project: projectName, name: "New name", repository: "new-repo" }]);
  await expect(page.getByRole("alert")).toHaveText("Requested test failure");
  await expect(page.getByLabel("Project name", { exact: true })).toHaveValue("New name");
});

test("the rename sheet fits a phone and hides window-only actions", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDesktop(page, false);
  await page.evaluate(() => Object.defineProperty(window, "__BEEP_PHONE__", { value: true }));
  const menu = await openMenu(page);
  await expect(menu.getByRole("menuitem", { name: "Open in New Window" })).toHaveCount(0);
  await menu.getByRole("menuitem", { name: "Rename…", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Rename project", exact: true });
  await expect(dialog.getByLabel("Project name", { exact: true })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Save", exact: true })).toBeVisible();
  const bounds = await dialog.boundingBox();
  expect(bounds).not.toBeNull();
  expect(bounds?.x).toBeGreaterThanOrEqual(0);
  expect((bounds?.x ?? 0) + (bounds?.width ?? 0)).toBeLessThanOrEqual(390);
});
