import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";
import type { FoundRow } from "../../src/project/projects-state.ts";

const found: FoundRow[] = ["example/app", "example/library"].map(name => ({
  name, remote: `https://example.test/${name}`,
  places: [{ machine_id: "build-box", machine_name: "Build box", folder: `/srv/${name}` }],
  agents: [{ agent: "codex", sessions: 2 }], last_activity: 1,
}));

test("partial selection survives filtering and sync options stay accessible", async ({ page }) => {
  await openDesktop(page, false, { found_projects: found, found_machines: [], host_repos: [] });
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
