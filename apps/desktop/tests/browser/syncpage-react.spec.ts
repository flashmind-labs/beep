import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("sync refresh retains the page, focused action and scroll", async ({ page }) => {
  const machine = { machine_id: "build-box", machine_name: "Build box", folder: "/srv/project", status: "in_sync", files: 18, bytes: 1024, pending: 0, conflicts: [], error: null, hint: null, paused: false, progress: null, waiting: [], can_resolve: true, git_only: false };
  const detail = { project: projectName, pinned: false, paused: false, machines: [machine] };
  await openDesktop(page, false, { project_sync: [{ project: projectName, machine_id: "build-box", status: "in_sync", files: 18, pending: 0, error: null }], project_sync_detail: detail });
  await page.getByRole("button", { name: "Sync", exact: true }).click();
  const view = page.locator(".cc-view:visible"), pause = view.getByRole("button", { name: "Pause", exact: true });
  await expect(pause).toBeVisible();
  await pause.focus();
  await pause.evaluate(element => { element.dataset.identity = "kept"; });
  await view.evaluate(element => { element.style.height = "100px"; element.style.overflow = "auto"; element.scrollTop = 50; });
  const top = await view.evaluate(element => element.scrollTop);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_detail").length);
  await page.evaluate(value => { window.__navigation.respond("project_sync_detail", value); window.__navigation.respond("project_sync", [{ project: value.project, machine_id: "build-box", status: "in_sync", files: 19, pending: 0, error: null }]); window.__navigation.notify("sync"); }, { ...detail, machines: [{ ...machine, files: 19 }] });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_detail").length)).toBeGreaterThan(reads);
  await expect(view.getByText("19 files")).toBeVisible();
  await expect(pause).toHaveAttribute("data-identity", "kept");
  await expect(pause).toBeFocused();
  await expect(view).toHaveJSProperty("scrollTop", top);
});

test("sync conflict actions keep their identity across updates", async ({ page }) => {
  const machine = { machine_id: "build-box", machine_name: "Build box", folder: "/srv/project", status: "in_sync", files: 18, bytes: 0, pending: 0, conflicts: ["notes (conflict 12345678).txt"], error: null, hint: null, paused: false, progress: null, waiting: [], can_resolve: true, git_only: false };
  const detail = { project: projectName, pinned: false, paused: false, machines: [machine] };
  await openDesktop(page, false, { project_sync: [{ project: projectName, machine_id: "build-box", status: "in_sync", files: 18, pending: 0, error: null }], project_sync_detail: detail });
  await page.getByRole("button", { name: "Sync", exact: true }).click();
  const conflict = page.locator(".fs-conflict"), keep = conflict.getByRole("button", { name: "Keep the original" });
  await expect(keep).toBeVisible();
  await keep.focus();
  await keep.evaluate(element => { element.dataset.identity = "kept"; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_detail").length);
  await page.evaluate(value => { window.__navigation.respond("project_sync_detail", value); window.__navigation.respond("project_sync", [{ project: value.project, machine_id: "build-box", status: "in_sync", files: 19, pending: 0, error: null }]); window.__navigation.notify("sync"); }, { ...detail, machines: [{ ...machine, files: 19 }] });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_detail").length)).toBeGreaterThan(reads);
  await expect(page.getByText("19 files")).toBeVisible();
  await expect(keep).toHaveAttribute("data-identity", "kept");
  await expect(keep).toBeFocused();
  await page.evaluate(() => window.__navigation.hold("project_sync_resolve"));
  await keep.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_resolve").map(call => call.args.keepCopy))).toEqual([false]);
  await page.evaluate(() => window.__navigation.release("project_sync_resolve"));
});
