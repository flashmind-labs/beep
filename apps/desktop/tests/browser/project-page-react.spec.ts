import { expect, test } from "@playwright/test";
import { openDesktop, pageErrors, projectName } from "./harness.ts";

// A page error means React and a retained host fought over a node.
let errors: string[] = [];
test.beforeEach(({ page }) => {
  errors = pageErrors(page);
});
test.afterEach(() => { expect(errors).toEqual([]); });

test("sessions tab keeps its list host while tabs switch", async ({ page }) => {
  const past = [{ machine_id: "build-box", machine_name: "build-box", id: "past-one", agent: "codex", title: "Past session", cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 1, external: false, question: null }];
  await openDesktop(page, false, { project_sessions: past });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  const det = page.locator(".pj-split > .pj-det"), tabs = det.locator(":scope > .cc-tabs.pj-tabs");
  await expect(tabs.getByRole("tab", { name: /Sessions/ })).toHaveAttribute("aria-selected", "true");
  await expect(det.locator(":scope > .cc-panel .pv-row").first()).toBeVisible();
  const list = det.locator(":scope > .cc-panel").first();
  await list.evaluate(el => { el.dataset.identity = "list"; });
  await tabs.getByRole("tab", { name: /Agents/ }).click();
  await expect(det.locator(":scope > #sec-custom-agents")).toBeVisible();
  await expect(det.locator('[data-identity="list"]')).toHaveCount(0);
  await tabs.getByRole("tab", { name: /Sessions/ }).click();
  await expect(det.locator(':scope > [data-identity="list"]')).toBeVisible();
  await expect(det.locator(":scope > .cc-tabs.pj-tabs + .cc-panel")).toHaveAttribute("data-identity", "list");
});

test("projects page shows its empty state with an Add a project button", async ({ page }) => {
  await openDesktop(page, false, { found_machines: [], center_home: { machines: [], projects: [], running: [], needs_you: [] } });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  const empty = page.locator(`#main .cc-view[data-project=""] > .cc-panel.cc-empty`);
  await expect(empty.locator("b")).toHaveText("No projects yet");
  await expect(empty.locator("span")).toHaveText("A folder or a repository to clone.");
  await expect(empty.locator("button.pri svg")).toHaveCount(1);
  await expect(empty.getByRole("button", { name: "Add a project" })).toBeVisible();
  await expect(page.locator("#main .cc-view[data-project]")).toHaveCount(1);
});
