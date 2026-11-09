import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";
import type { MachineDetail, SyncDetail } from "../../src/project/filesync.ts";
import type { TerminalRow } from "../../src/terminals/terminals.ts";

const machines = [
  { id: "build-box", name: "build-box", online: true, folder: "/srv/project", checking: false, checked_at: 1 },
  { id: "studio", name: "studio", online: true, folder: "/srv/project", checking: false, checked_at: 1 },
  { id: "offline-box", name: "offline-box", online: false, folder: "/srv/project", checking: false, checked_at: 1 },
  { id: "laptop", name: "laptop", online: true, folder: null, checking: false, checked_at: 1, mesh: true },
];
const syncMachine = (id: string, status: string, error: string | null): MachineDetail => ({
  machine_id: id, machine_name: id, folder: "/srv/project", status, error,
  files: 18, bytes: 1024, pending: 0, conflicts: [], hint: null, paused: false,
  progress: null, waiting: [], can_resolve: true, git_only: true,
});
const detail: SyncDetail = {
  project: projectName, pinned: false, paused: false,
  machines: [syncMachine("build-box", "in_sync", null), syncMachine("studio", "failed", "commit or stash local changes"), syncMachine("offline-box", "offline", null)],
};
const terminal: TerminalRow = {
  id: "term-build", project: projectName, machine_id: "build-box", machine_name: "build-box",
  name: "cargo watch", command: "cargo watch", cwd: "/srv/project", state: "running", code: null, started_at: 1,
};
const responses = {
  project_machines: machines, project_sync_detail: detail, terminals_list: [terminal],
};

async function openMachines(page: Page) {
  await openDesktop(page, false, responses);
  await page.evaluate(name => window.__navigation.open({ projects: { project: name } }), projectName);
  await page.locator('.pj-li[title="Machines"]').click();
  await expect(page.locator(".pm-machine")).toHaveCount(4);
}

test("machines contain their sync state and only their own terminals", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await openMachines(page);
  const build = page.getByRole("article", { name: "build-box", exact: true });
  const studio = page.getByRole("article", { name: "studio", exact: true });
  await expect(build).toContainText("in sync");
  await expect(build).toContainText("cargo watch");
  await expect(studio).toContainText("Local edits block Git sync");
  await expect(studio).not.toContainText("cargo watch");
  await expect(studio.getByRole("button", { name: "Fix with an agent" })).toBeVisible();
  await expect(page.locator('.pj-li[title="File sync"]')).toHaveCount(0);
  await expect(build.getByRole("button", { name: "Remove from build-box" })).toBeDisabled();
  await expect(studio.getByRole("button", { name: "Remove from studio" })).toBeEnabled();
  await expect(page.getByRole("article", { name: "laptop", exact: true }).getByRole("button", { name: "Add" })).toBeVisible();
  await page.screenshot({ path: "../../.artifacts/project-machines-app.png" });
  expect(errors).toEqual([]);
});

test("removal keeps the folder by default and sends deletion only when selected", async ({ page }) => {
  await openMachines(page);
  await page.getByRole("button", { name: "Remove from studio", exact: true }).click();
  const checkbox = page.getByRole("checkbox", { name: "Also delete the folder on studio" });
  await expect(checkbox).not.toBeChecked();
  await checkbox.check();
  await page.getByRole("button", { name: "Remove", exact: true }).last().click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "project_remove_from_machine"))).toEqual([
    { command: "project_remove_from_machine", args: { project: projectName, machineId: "studio", deleteFolder: true } },
  ]);
});

test("pause operates on the combined view and offline checkouts hide folder changes", async ({ page }) => {
  await openMachines(page);
  await page.locator(".pm-section").getByRole("button", { name: "Pause", exact: true }).click();
  await expect(page.locator(".pm-section").getByRole("button", { name: "Resume", exact: true })).toBeVisible();
  await expect(page.getByRole("article", { name: "offline-box", exact: true }).getByRole("button", { name: "Change folder on offline-box" })).toHaveCount(0);
});

test("sync and terminal events refresh the combined machine sections", async ({ page }) => {
  await openMachines(page);
  const studio = page.getByRole("article", { name: "studio", exact: true });
  await expect(studio).toContainText("Local edits block Git sync");
  await page.evaluate(({ sync, terminal }) => {
    window.__navigation.respond("project_sync_detail", sync);
    window.__navigation.respond("terminals_list", [terminal]);
    window.__navigation.notify("sync");
    window.__navigation.notify("terminals");
  }, { sync: { ...detail, machines: detail.machines.map(m => m.machine_id === "studio" ? { ...m, status: "in_sync", error: null } : m) }, terminal: { ...terminal, machine_id: "studio", machine_name: "studio" } });
  await expect(studio).toContainText("in sync");
  await expect(studio).not.toContainText("Local edits block Git sync");
  await expect(studio).toContainText("cargo watch");
  await expect(page.getByRole("article", { name: "build-box", exact: true })).not.toContainText("cargo watch");
});

test("machine actions and terminals remain available without detailed sync support", async ({ page }) => {
  await openDesktop(page, false, { ...responses, project_sync_detail: null });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name } }), projectName);
  await page.locator('.pj-li[title="Machines"]').click();
  await expect(page.locator(".pm-machine")).toHaveCount(4);
  await expect(page.getByRole("article", { name: "build-box", exact: true })).toContainText("cargo watch");
  await expect(page.getByRole("button", { name: "Remove from studio", exact: true })).toBeEnabled();
  await expect(page.locator(".pm-section")).toContainText("The server needs an update before it shows file sync in detail.");
});
