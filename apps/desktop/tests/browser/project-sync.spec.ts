import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

/// Opens the project's Machines section, where its sync state and conflicts show.
async function openMachines(page: Page) {
  await page.evaluate(name => window.__navigation.open({ projects: { project: name, tile: "machines" } }), projectName);
  await expect(page.locator(".pm-machine")).toHaveCount(1);
}

test("sync refresh retains the section, focused action and scroll", async ({ page }) => {
  const machine = { machine_id: "build-box", machine_name: "Build box", folder: "/srv/project", status: "in_sync", files: 18, bytes: 1024, pending: 0, conflicts: [], error: null, hint: null, paused: false, progress: null, waiting: [], can_resolve: true, can_start_over: false, git_only: false };
  const detail = { project: projectName, pinned: false, paused: false, machines: [machine] };
  await openDesktop(page, false, { project_sync: [{ project: projectName, machine_id: "build-box", status: "in_sync", files: 18, pending: 0, error: null }], project_sync_detail: detail });
  await openMachines(page);
  const view = page.locator(".cc-view:visible"), pause = view.getByRole("button", { name: "Pause", exact: true });
  await expect(pause).toBeVisible();
  await pause.focus();
  await pause.evaluate(element => { element.dataset.identity = "kept"; });
  await view.evaluate(element => { element.style.height = "100px"; element.style.overflow = "auto"; element.scrollTop = 50; });
  const top = await view.evaluate(element => element.scrollTop);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_detail").length);
  await page.evaluate(value => window.__navigation.notify("sync", [
    { command: "project_sync", args: {}, value: [{ project: value.project, machine_id: "build-box", status: "in_sync", files: 19, pending: 0, error: null }] },
    { command: "project_sync_detail", args: { project: value.project }, value },
  ]), { ...detail, machines: [{ ...machine, files: 19 }] });
  await expect(view.getByText("19 files")).toBeVisible();
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_detail").length)).toBe(reads);
  await expect(pause).toHaveAttribute("data-identity", "kept");
  await expect(pause).toBeFocused();
  await expect(view).toHaveJSProperty("scrollTop", top);
});

test("sync conflict actions keep their identity across updates", async ({ page }) => {
  const machine = { machine_id: "build-box", machine_name: "Build box", folder: "/srv/project", status: "in_sync", files: 18, bytes: 0, pending: 0, conflicts: ["notes (conflict 12345678).txt"], error: null, hint: null, paused: false, progress: null, waiting: [], can_resolve: true, can_start_over: false, git_only: false };
  const detail = { project: projectName, pinned: false, paused: false, machines: [machine] };
  await openDesktop(page, false, { project_sync: [{ project: projectName, machine_id: "build-box", status: "in_sync", files: 18, pending: 0, error: null }], project_sync_detail: detail });
  await openMachines(page);
  const conflict = page.locator(".fs-conflict"), keep = conflict.getByRole("button", { name: "Keep the original" });
  await expect(keep).toBeVisible();
  await keep.focus();
  await keep.evaluate(element => { element.dataset.identity = "kept"; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_detail").length);
  await page.evaluate(value => window.__navigation.notify("sync", [
    { command: "project_sync", args: {}, value: [{ project: value.project, machine_id: "build-box", status: "in_sync", files: 19, pending: 0, error: null }] },
    { command: "project_sync_detail", args: { project: value.project }, value },
  ]), { ...detail, machines: [{ ...machine, files: 19 }] });
  await expect(page.getByText("19 files")).toBeVisible();
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_detail").length)).toBe(reads);
  await expect(keep).toHaveAttribute("data-identity", "kept");
  await expect(keep).toBeFocused();
  await page.evaluate(() => window.__navigation.hold("project_sync_resolve"));
  await keep.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_resolve").map(call => call.args.keepCopy))).toEqual([false]);
  await page.evaluate(() => window.__navigation.release("project_sync_resolve"));
});

test("a replaced folder says so and starts over on its machine", async ({ page }) => {
  const error = "not syncing: the folder changed or is not mounted: /srv/project is another folder now";
  const machine = { machine_id: "build-box", machine_name: "Build box", folder: "/srv/project", status: "failed", files: 18, bytes: 0, pending: 0, conflicts: [], error, hint: null, paused: false, progress: null, waiting: [], can_resolve: true, can_start_over: true, git_only: true };
  const detail = { project: projectName, pinned: false, paused: false, machines: [machine] };
  await openDesktop(page, false, { project_sync: [{ project: projectName, machine_id: "build-box", status: "failed", files: 18, pending: 0, error }], project_sync_detail: detail });
  await openMachines(page);
  await expect(page.getByText("This folder was replaced, so sync stopped.")).toBeVisible();
  await page.getByRole("button", { name: "Start over with this folder" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_start_over").map(call => call.args))).toEqual([{ project: projectName, machineId: "build-box" }]);
});

test("conflict cards, the notice and the sidebar lead to the project's machines", async ({ page }) => {
  const machine = { machine_id: "build-box", machine_name: "Build box", folder: "/srv/project", status: "in_sync", files: 18, bytes: 0, pending: 0, conflicts: ["notes (conflict 12345678).txt"], error: null, hint: null, paused: false, progress: null, waiting: [], can_resolve: true, can_start_over: false, git_only: false };
  const note = { id: `conflict:${projectName}:build-box`, kind: "conflict", title: `Sync conflict in ${projectName}`, text: machine.conflicts[0], source: "Build box", at: 1, read: false, done: false, target: null, here: [], held: [] };
  await openDesktop(page, false, { project_sync: [{ project: projectName, machine_id: "build-box", status: "in_sync", files: 18, pending: 0, error: null }], project_sync_detail: { project: projectName, pinned: false, paused: false, machines: [machine] }, notes_list: [note] });
  const side = page.locator("#side"), conflict = page.locator(".fs-conflict");
  await expect(side.getByRole("button", { name: "Sync", exact: true })).toHaveCount(0);
  await expect(side.locator(".cc-chat .n.hot")).toHaveText("1");

  // Home's card.
  await page.locator("#sec-needs .cc-row").filter({ hasText: machine.conflicts[0] }).getByRole("button", { name: "Resolve" }).click();
  await expect(conflict).toBeVisible();

  // The project board's card.
  await page.evaluate(name => window.__navigation.open({ projects: { project: name } }), projectName);
  await expect(conflict).toHaveCount(0);
  await page.locator(".pj-board .bd-card").filter({ hasText: machine.conflicts[0] }).getByRole("button", { name: "Resolve" }).click();
  await expect(conflict).toBeVisible();

  // The notice.
  await side.getByRole("button", { name: "Home", exact: true }).click();
  await page.locator(".nt-bell").click();
  await page.locator(".nt-row").filter({ hasText: "Sync conflict" }).click();
  await expect(conflict).toBeVisible();
});
