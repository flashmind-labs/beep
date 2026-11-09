import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("ignored sync validates, retries and blocks dismissal while saving", async ({ page }) => {
  await openDesktop(page, false, { project_ignored_sync: { enabled: true, folders: ["assets"] }, project_git_auto_recovery: false, project_syncthing_mode: null });
  await page.evaluate(() => window.__navigation.open({ projects: { project: "example/widget" } }));
  await page.getByRole("button", { name: "More", exact: true }).click();
  await page.getByRole("menuitem", { name: "Choose ignored folders…" }).click();
  const sheet = page.getByRole("dialog", { name: "Sync gitignored" });
  const paths = sheet.getByRole("textbox", { name: "Folders relative to the project, one per line" });
  await expect(paths).toBeFocused();
  await paths.fill("../bad");
  await sheet.getByRole("button", { name: "Save" }).click();
  await expect(sheet.getByRole("status")).toContainText("project-relative");
  await paths.fill("assets/models\ndata/captures");
  await page.evaluate(() => window.__navigation.rejectOnce("project_ignored_sync"));
  await sheet.getByRole("button", { name: "Save" }).click();
  await expect(sheet.getByRole("status")).toContainText("Requested test failure");
  await expect(paths).toHaveValue("assets/models\ndata/captures");
  await page.evaluate(() => window.__navigation.hold("project_ignored_sync"));
  await sheet.getByRole("button", { name: "Save" }).click();
  await expect(paths).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(sheet).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_ignored_sync" && "settings" in call.args).map(call => call.args))).toEqual([
    { project: projectName, settings: { enabled: true, folders: ["assets/models", "data/captures"] } },
    { project: projectName, settings: { enabled: true, folders: ["assets/models", "data/captures"] } },
  ]);
  await page.evaluate(() => window.__navigation.release("project_ignored_sync"));
  await expect(sheet).toHaveCount(0);
});
