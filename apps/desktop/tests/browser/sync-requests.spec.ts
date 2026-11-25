import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const row = (project: string, files = 10) => ({ project, machine_id: "build-box", status: "in_sync", files, pending: 0, error: null });
const detail = (project: string) => ({ project, pinned: false, paused: false, machines: [] });
const projects = ["acme/one", "acme/two", "acme/three"];

const count = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).map(call => String(call.args.project ?? "")), command);

test("a project's sync is read once, and its changes come with the sync update", async ({ page }) => {
  await openDesktop(page, false, { project_sync: projects.map(p => row(p)), project_sync_detail: detail("acme/one"), machine_clocks: [] });
  await expect.poll(async () => (await count(page, "project_sync_detail")).length).toBeGreaterThanOrEqual(projects.length);
  const details = (await count(page, "project_sync_detail")).length;
  const clocks = (await count(page, "machine_clocks")).length;

  // The same summary again and again, and a change of one project's rows, read nothing.
  for (let i = 0; i < 10; i++) await page.evaluate(() => window.__navigation.notify("sync"));
  await page.evaluate(rows => { window.__navigation.respond("project_sync", rows); window.__navigation.notify("sync"); }, [row("acme/one"), row("acme/two", 11), row("acme/three")]);
  await page.evaluate(value => window.__navigation.notify("sync", [{ command: "project_sync_detail", args: { project: "acme/two" }, value }]), { ...detail("acme/two"), paused: true });
  expect((await count(page, "project_sync_detail")).length).toBe(details);
  expect((await count(page, "machine_clocks")).length).toBe(clocks);
});
