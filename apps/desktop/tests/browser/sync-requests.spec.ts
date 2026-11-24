import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const row = (project: string, files = 10) => ({ project, machine_id: "build-box", status: "in_sync", files, pending: 0, error: null });
const detail = (project: string) => ({ project, pinned: false, paused: false, machines: [] });
const projects = ["acme/one", "acme/two", "acme/three"];

const count = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).map(call => String(call.args.project ?? "")), command);

test("sync changes read only the projects they touch, and clocks once", async ({ page }) => {
  await openDesktop(page, false, { project_sync: projects.map(p => row(p)), project_sync_detail: detail("acme/one"), machine_clocks: [] });
  await expect.poll(async () => (await count(page, "project_sync_detail")).length).toBeGreaterThanOrEqual(projects.length);
  const details = (await count(page, "project_sync_detail")).length;
  const clocks = (await count(page, "machine_clocks")).length;

  // The same summary again and again reads nothing: the reads counted below would show it.
  for (let i = 0; i < 10; i++) await page.evaluate(() => window.__navigation.notify("sync"));

  // One project's change reads that project.
  await page.evaluate(rows => { window.__navigation.respond("project_sync", rows); window.__navigation.notify("sync"); }, [row("acme/one"), row("acme/two", 11), row("acme/three")]);
  // A detail read waits out the sync page's ten seconds between reads of one project.
  await expect.poll(async () => (await count(page, "project_sync_detail")).slice(details), { timeout: 15_000 }).toEqual(["acme/two"]);
  expect((await count(page, "machine_clocks")).length).toBe(clocks);
});
