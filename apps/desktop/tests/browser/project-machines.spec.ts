import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";
import type { MachineDetail, SyncDetail } from "../../src/project/filesync.ts";
import type { TerminalRow } from "../../src/terminals/terminals.tsx";

const machines = [
  { id: "build-box", name: "build-box", online: true, folder: "/srv/project", checking: false, checked_at: 1 },
  { id: "studio", name: "studio", online: true, folder: "/srv/project", checking: false, checked_at: 1 },
  { id: "offline-box", name: "offline-box", online: false, folder: "/srv/project", checking: false, checked_at: 1 },
  { id: "laptop", name: "laptop", online: true, folder: null, checking: false, checked_at: 1, mesh: true },
];
const syncMachine = (id: string, status: string, error: string | null): MachineDetail => ({
  machine_id: id, machine_name: id, folder: "/srv/project", status, error,
  files: 18, bytes: 1024, pending: 0, conflicts: [], hint: null, paused: false,
  progress: null, waiting: [], can_resolve: true, can_start_over: false, git_only: true,
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
  await page.evaluate(name => window.__navigation.open({ projects: { project: name, tile: "machines" } }), projectName);
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
  const terminalRow = build.locator(".pm-body > .cc-row").filter({ hasText: "cargo watch" });
  await expect(terminalRow).toHaveCSS("padding-left", "0px");
  await expect(terminalRow).toHaveCSS("padding-right", "0px");
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

test("Git controls retain their state and send one pull", async ({ page }) => {
  const git = { machine_id: "studio", branch: "main", upstream: "origin/main", ahead: 1, behind: 2, error: null };
  await openDesktop(page, false, { ...responses, project_git_state: [git] });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name, tile: "machines" } }), projectName);
  const studio = page.getByRole("article", { name: "studio", exact: true });
  const pull = studio.getByRole("button", { name: "Pull 2 commits" });
  await expect(pull).toBeEnabled();
  await page.evaluate(() => window.__navigation.hold("project_git_move"));
  await pull.click();
  await expect(pull).toBeDisabled();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_git_move").map(call => call.args.direction))).toEqual(["pull"]);
  await page.evaluate(() => window.__navigation.release("project_git_move"));
});

test("a git change in the background is drawn from its event without reading the state again", async ({ page }) => {
  const git = { machine_id: "studio", branch: "main", upstream: "origin/main", ahead: 0, behind: 2, error: null };
  await openDesktop(page, false, { ...responses, project_git_state: [git] });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name, tile: "machines" } }), projectName);
  const studio = page.getByRole("article", { name: "studio", exact: true });
  await expect(studio.getByRole("button", { name: "Pull 2 commits" })).toBeEnabled();
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_git_state").length);
  const before = await reads();
  await page.evaluate(([name, row]) => window.__navigation.emit("project-git-changed", { project: name, row }), [projectName, { ...git, behind: 3 }] as const);
  await expect(studio.getByRole("button", { name: "Pull 3 commits" })).toBeEnabled();
  expect(await reads()).toBe(before);
});

test("sync and terminal events refresh the combined machine sections", async ({ page }) => {
  await openMachines(page);
  const studio = page.getByRole("article", { name: "studio", exact: true });
  await expect(studio).toContainText("Local edits block Git sync");
  const folder = studio.getByRole("button", { name: "Change folder on studio" });
  await folder.focus();
  await folder.evaluate(element => { element.dataset.identity = "kept"; });
  const detailPane = page.locator(".pj-split > .pj-det");
  await detailPane.evaluate(element => { element.style.maxHeight = "180px"; element.scrollTop = 70; });
  const top = await detailPane.evaluate(element => element.scrollTop);
  await page.evaluate(({ sync, terminal }) => {
    // Sync detail reads are at most ten seconds apart.
    const now = Date.now(); Date.now = () => now + 61000;
    window.__navigation.respond("project_sync_detail", sync);
    window.__navigation.respond("project_sync", [{ project: sync.project, machine_id: "studio", status: "in_sync", files: 0, pending: 0, error: null }]);
    window.__navigation.respond("terminals_all", [terminal]);
    window.__navigation.notify("sync");
    window.__navigation.notify("terminals");
  }, { sync: { ...detail, machines: detail.machines.map(m => m.machine_id === "studio" ? { ...m, status: "in_sync", error: null } : m) }, terminal: { ...terminal, machine_id: "studio", machine_name: "studio" } });
  await expect(studio).toContainText("in sync");
  await expect(studio).not.toContainText("Local edits block Git sync");
  await expect(studio).toContainText("cargo watch");
  await expect(folder).toHaveAttribute("data-identity", "kept");
  await expect(folder).toBeFocused();
  await expect(detailPane).toHaveJSProperty("scrollTop", top);
  await expect(page.getByRole("article", { name: "build-box", exact: true })).not.toContainText("cargo watch");
});

test("handoff retries errors and keeps its native request after closing", async ({ page }) => {
  const projects = machines.slice(0, 2).map(machine => ({ name: projectName, url: "https://example.test/example/widget", machine_id: machine.id, machine_name: machine.name, folder: machine.folder, cloned: true }));
  const git = machines.slice(0, 2).map(machine => ({ machine_id: machine.id, machine_name: machine.name, branch: "main", upstream: null, ahead: 0, behind: 0, error: null }));
  await openDesktop(page, false, { ...responses, center_home: { machines: [], projects, running: [], needs_you: [] }, project_git_state: git });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name, tile: "machines" } }), projectName);
  await page.getByRole("article", { name: "build-box", exact: true }).getByRole("button", { name: "Bring work here" }).click();
  const sheet = page.getByRole("dialog", { name: "Bring uncommitted work" });
  const bring = sheet.getByRole("button", { name: "Bring work from studio" });
  await page.evaluate(() => window.__navigation.rejectOnce("project_handoff_work"));
  await bring.click();
  await expect(sheet).toContainText("Requested test failure");
  await page.evaluate(() => window.__navigation.hold("project_handoff_work"));
  await bring.click();
  await expect(bring).toBeDisabled();
  await expect(sheet).toContainText("Bringing uncommitted work");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_handoff_work").map(call => call.args))).toEqual([
    { project: projectName, from: "studio", to: "build-box" }, { project: projectName, from: "studio", to: "build-box" },
  ]);
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
  await page.evaluate(() => window.__navigation.release("project_handoff_work"));
});

test("adding a machine picks its folder in a palette of the machine's folders", async ({ page }) => {
  const at = (path: string, over = {}) => ({ path, parent: path.slice(0, path.lastIndexOf("/")) || "/", exists: true, folders: [], more: 0, empty: false, origin: null, repos: [], ...over });
  await openMachines(page);
  const code = at("/home/dev/code", { folders: ["apps", "widget", "zeta"], repos: ["zeta"] });
  await page.evaluate(folders => { window.__navigation.respond("machine_search", "~/code"); window.__navigation.respond("machine_folders", folders); }, {
    "~": at("/home/dev", { folders: ["code"] }),
    "~/code": code,
    "/home/dev/code": code,
    "/home/dev/code/widget": at("/home/dev/code/widget", { folders: ["deploy"] }),
    "/home/dev/code/apps": at("/home/dev/code/apps", { empty: true }),
    "/srv/new": at("/srv/new", { exists: false, empty: true }),
  });
  await page.getByRole("article", { name: "laptop", exact: true }).getByRole("button", { name: "Add", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Add widget to laptop" });
  const field = sheet.getByRole("combobox");
  const selected = sheet.locator('[role="option"][aria-selected="true"]');
  const added = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_add_to_machine").map(call => call.args));

  // A folder named for the project is the place: the picker points at it and offers nothing around it.
  await expect(selected).toHaveText("widget");
  await expect(sheet.getByRole("option")).toHaveText(["apps", "widget", "zeta"]);
  await expect(sheet.locator(".fp-path")).toHaveText("Homecode");
  await expect(field).toBeFocused();

  // It has files and no `.git`: it takes the project as it is, not in `widget/widget`.
  await field.fill("wid");
  await expect(sheet.getByRole("option")).toHaveText(["widget"]);
  await page.keyboard.press("Enter");
  await expect(selected).toContainText("Add here, keeping its files");
  await expect(selected).toContainText("/home/dev/code/widget");
  await expect(sheet.getByRole("option", { name: "deploy" })).toBeVisible();

  // Backspace in the empty field goes up, a step of the path goes back, and a typed path goes there.
  await page.keyboard.press("Backspace");
  await expect(sheet.getByRole("option", { name: "zeta" })).toBeVisible();
  await sheet.getByRole("option", { name: "apps" }).click();
  await expect(selected).toHaveText(/^Add here.*\/home\/dev\/code\/apps$/);
  await sheet.locator(".fp-path").getByRole("button", { name: "code" }).click();
  await expect(sheet.getByRole("option", { name: "zeta" })).toBeVisible();
  await field.fill("/srv/new");
  await expect(selected).toHaveText("Go to /srv/new");
  await page.keyboard.press("Enter");
  await expect(selected).toContainText("/srv/new");

  // What the desk refuses shows in the sheet, which stays open; then the offer is taken.
  await page.evaluate(() => window.__navigation.rejectOnce("project_add_to_machine"));
  await page.keyboard.press("Enter");
  await expect(sheet).toContainText("Requested test failure");
  await page.evaluate(() => window.__navigation.hold("project_add_to_machine"));
  await page.keyboard.press("Enter");
  await expect.poll(added).toEqual(Array(2).fill({ project: projectName, machineId: "laptop", folder: "/srv/new" }));
  await page.keyboard.press("Escape");
  await page.evaluate(() => window.__navigation.release("project_add_to_machine"));
  await expect(sheet).toHaveCount(0);
});

test("adding a machine reads the sync once and follows it from the sync updates until it is copied", async ({ page }) => {
  const at = (path: string, over = {}) => ({ path, parent: path.slice(0, path.lastIndexOf("/")) || "/", exists: true, folders: [], more: 0, empty: false, origin: null, repos: [], ...over });
  const laptop = (files: number, pending: number, status: string): MachineDetail => ({ ...syncMachine("laptop", status, null), files, pending });
  const copying = (...rows: MachineDetail[]): SyncDetail => ({ ...detail, machines: [syncMachine("build-box", "in_sync", null), ...rows] });
  await openMachines(page);
  await page.evaluate(([folders, first]) => {
    window.__navigation.respond("machine_search", "~/code");
    window.__navigation.respond("machine_folders", folders);
    window.__navigation.respond("project_sync_detail", first);
  }, [{ "~": at("/home/dev", { folders: ["code"] }), "~/code": at("/home/dev/code", { folders: ["widget"] }), "/home/dev/code": at("/home/dev/code", { folders: ["widget"] }), "/home/dev/code/widget": at("/home/dev/code/widget", { empty: true }) }, copying(laptop(4, 14, "syncing"))] as const);
  await page.getByRole("article", { name: "laptop", exact: true }).getByRole("button", { name: "Add", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Add widget to laptop" });
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await expect(sheet).toContainText("Copying to laptop: 4 of 18 files");
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_sync_detail").length);
  const before = await reads();

  const push = (value: SyncDetail, at: number) => page.evaluate(([view, revision]) => window.__navigation.publish("sync.*", revision, { views: [view] }, "laptop"), [{ command: "project_sync_detail", args: { project: projectName }, value }, at] as const);
  await push(copying(laptop(11, 7, "syncing")), 5000);
  await expect(sheet).toContainText("Copying to laptop: 11 of 18 files");
  await push(copying(laptop(18, 0, "in_sync")), 5001);
  await expect(sheet).toHaveCount(0);
  expect(await reads()).toBe(before);
});

test("the project composer offers hosts that reconnect while it stays open", async ({ page }) => {
  const host = (id: string) => ({ machine_id: id, machine_name: id, agents: ["codex"], workspace: null, home: null, capabilities: [] });
  const projects = ["build-box", "studio"].map(id => ({ name: projectName, url: "https://example.test/example/widget", machine_id: id, machine_name: id, folder: "/srv/project", cloned: true }));
  await openDesktop(page, false, { ...responses, agent_hosts: [host("build-box")], center_home: { machines: [], projects, running: [], needs_you: [] } });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name } }), projectName);
  const picker = page.locator(".cc-pjc").getByRole("button", { name: /build-box/ });
  await expect(picker).toBeVisible();
  await page.evaluate(hosts => {
    window.__navigation.notify("machines", [{ command: "agent_hosts", value: hosts }]);
  }, [host("build-box"), host("studio")]);
  await picker.click();
  await expect(page.getByRole("menuitem", { name: "studio", exact: true })).toBeVisible();
});

test("Fix with an agent on a blocked Git sync starts the agent on that machine at once", async ({ page }) => {
  await openDesktop(page, false, {
    ...responses,
    agent_hosts: [{ machine_id: "studio", machine_name: "studio", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    agent_models: { pick: "listed", current: "alpha", models: [{ id: "alpha", name: "Alpha", description: null }] },
    agent_begin: { session: { machine_id: "studio", machine_name: "studio", id: "start:repair", agent: "codex", cwd: "/srv/project", status: "starting", external: false, question: null }, why: "" },
    project_sessions: [],
    center_home: { machines: [], running: [], needs_you: [], projects: ["build-box", "studio"].map(id => ({ display_name: null, name: projectName, url: "https://example.test/example/widget", machine_id: id, machine_name: id, folder: "/srv/project", cloned: true })) },
  });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name, tile: "machines" } }), projectName);
  const studio = page.getByRole("article", { name: "studio", exact: true });
  await studio.getByRole("button", { name: "Fix with an agent" }).click();
  await page.getByRole("dialog", { name: "Machine, harness, model and thinking" }).locator('[data-agh]:not(:disabled)').first().click();
  await page.getByRole("dialog", { name: "Machine, harness, model and thinking" }).locator('[data-agmo="alpha"]').click();
  await page.getByRole("dialog", { name: "Machine, harness, model and thinking" }).locator('[data-age=""]').click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin").length)).toBe(1);
  const args = await page.evaluate(() => window.__navigation.calls().find(call => call.command === "agent_begin")?.args);
  expect(args).toMatchObject({ machineId: "studio", agent: "codex", checkout: null });
  expect(String(args?.prompt)).toContain("commit or stash local changes");
});
