import { expect, test, type Page, type Locator } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

const opened = (page: Page) => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_open_window").map(call => call.args.alone));

async function gestures(page: Page, link: Locator) {
  const before = (await opened(page)).length;
  await link.click({ modifiers: ["Meta"] });
  await expect.poll(() => opened(page)).toHaveLength(before + 1);
  await expect(link).toBeVisible();
  await link.click({ button: "middle" });
  await expect.poll(() => opened(page)).toHaveLength(before + 2);
  await expect(link).toBeVisible();
  expect((await opened(page)).slice(before)).toEqual([{ kind: "project", project: projectName }, { kind: "project", project: projectName }]);
}

for (const active of [false, true]) {
  test(`machine ${active ? "active" : "folded"} project links open separate windows and keep Home`, async ({ page }) => {
    const machine = { id: "build-box", name: "Build box", online: true, can_host: true, seen_at: 0, version: "1", env: null, apps: [], gpus: [], capabilities: [], agents: 0, stats: null };
    const sync = { project: projectName, machine_id: machine.id, status: active ? "failed" : "in_sync", files: 10, pending: 0, error: null };
    await openDesktop(page, false, { project_sync: [sync] });
    await page.evaluate(machine => {
      // Keep the fixture's project and sessions while adding a connected machine.
      window.__navigation.respond("center_home", { machines: [machine], projects: [{ name: "example/widget", url: "", machine_id: machine.id, machine_name: machine.name, folder: "/srv/project", cloned: true, imported_at: 1 }], running: [], needs_you: [] });
      window.__navigation.notify("home");
    }, machine);
    await page.getByRole("tab", { name: "Machines", exact: true }).click();
    const card = page.locator("#m-build-box");
    if (!active) await card.getByRole("button", { name: /1 project here/ }).click();
    const link = card.getByRole("button", { name: /example\/widget/ });
    await gestures(page, link);
    await link.click({ button: "right" });
    await expect(page.getByRole("menuitem", { name: "Open in New Window" })).toBeVisible();
    await page.getByRole("menuitem", { name: "Open in New Window" }).click();
    await expect.poll(() => opened(page)).toHaveLength(3);
    await expect(link).toBeVisible();
    await link.click();
    await expect(page.locator(".pj-strip")).toBeVisible();
  });
}

test("session breadcrumbs keep their current page", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await gestures(page, page.locator("#agproj"));
});

test("palette project results open a separate window and close search", async ({ page }) => {
  const index = { actions: [], settings: [], code: [], projects: [{ kind: "project", id: projectName, title: "Widget", detail: "Project", keys: ["widget"] }], sessions: [], machines: [] };
  await openDesktop(page, false, { center_palette_index: index, center_find_prompts: [] });
  for (const middle of [false, true]) {
    await page.getByRole("button", { name: /^Search/ }).click();
    await page.getByRole("combobox").fill("Widget");
    await page.getByRole("option", { name: /Widget/ }).click(middle ? { button: "middle" } : { modifiers: ["Meta"] });
    await expect(page.getByRole("dialog", { name: "Search" })).toHaveCount(0);
  }
  expect(await opened(page)).toEqual([{ kind: "project", project: projectName }, { kind: "project", project: projectName }]);
  await expect(page.getByRole("tab", { name: "Board", exact: true })).toBeVisible();
});


test("draft project breadcrumbs support window gestures", { tag: "@windows" }, async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator(".pj-strip")).toBeVisible();
  await page.keyboard.press(test.info().project.name === "Windows" ? "Control+n" : "Meta+n");
  await gestures(page, page.locator("#agproj"));
});

test("Git Open project buttons keep the repository list in place", async ({ page }) => {
  const item = { host: "example.test", repo: projectName, number: 1, title: "Example change", kind: "issue", url: "https://example.test/example/widget/issues/1", updated_at: 1, authored: false, assigned: false, review_requested: false, draft: false, review: "none", checks_failing: false, labels: [] };
  await openDesktop(page, false, { git_items: { hosts: [{ error: null }], items: [item] } });
  await page.getByRole("tab", { name: "PRs and issues" }).click();
  await page.locator(".pl-row.repo").hover();
  await gestures(page, page.getByRole("button", { name: "Open project", exact: true }));
});

test("project notifications open windows without dismissing the row", async ({ page }) => {
  const note = { id: "build:1", at: 1, kind: "run", title: "Build passed", text: "", source: "build-box", read: false, done: false, target: { projects: { kind: "project", project: projectName } } };
  await openDesktop(page, false, { notes_list: [note] });
  await page.getByRole("button", { name: /Notifications/ }).click();
  await gestures(page, page.locator('.nt-row[data-id="build:1"]'));
  await page.locator(".nt-row").getByRole("button", { name: "Dismiss", exact: true }).click({ modifiers: ["Meta"] });
  await expect(page.locator(".nt-row")).toHaveCount(0);
  expect(await opened(page)).toHaveLength(2);
});

test("the project window back button reuses the project window", async ({ page }) => {
  // Too narrow for the project's sessions, which go back to the project with its name.
  await page.setViewportSize({ width: 640, height: 800 });
  await openDesktop(page, false, { center_window_alone: { kind: "project", project: projectName } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await gestures(page, page.locator("#pw-back"));
});


test("conflict cards and their Resolve buttons support window gestures", async ({ page }) => {
  const machine = { machine_id: "build-box", machine_name: "Build box", folder: "/srv/project", status: "in_sync", files: 18, bytes: 0, pending: 0, conflicts: ["notes (conflict 12345678).txt"], error: null, hint: null, paused: false, progress: null, waiting: [], can_resolve: true, can_start_over: false, git_only: false };
  await openDesktop(page, false, { project_sync: [{ project: projectName, machine_id: machine.machine_id, status: "in_sync", files: 18, pending: 0, error: null }], project_sync_detail: { project: projectName, pinned: false, paused: false, machines: [machine] } });
  // A row of Home's queue, then a card on the project's board.
  const card = page.locator(":is(#sec-needs .cc-row, .bd-card):visible").filter({ hasText: machine.conflicts[0] });
  await gestures(page, card);
  await gestures(page, card.getByRole("button", { name: "Resolve", exact: true }));
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await gestures(page, card);
  await gestures(page, card.getByRole("button", { name: "Resolve", exact: true }));
});

test("a restored project window returns to its conversation and divider size", async ({ page, context }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  const label = "project-sample";
  await openDesktop(page, false, { center_window_alone: { kind: "project", project: projectName } }, label);
  await expect(page.locator("body")).toHaveClass(/\bpw\b/);
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const divider = page.locator("#agdiv-right");
  await expect(divider).toBeVisible();
  await divider.press("ArrowLeft");
  const width = await page.locator("#agpane-right").evaluate(el => el.getBoundingClientRect().width);

  const other = await context.newPage();
  await other.setViewportSize({ width: 1600, height: 1000 });
  await openDesktop(other, false, { center_window_alone: { kind: "chat", machine_id: "build-box", session: "notice-one" } }, "chat-sample");
  await other.locator("#agdiv-right").press("ArrowRight");
  expect(await other.evaluate(() => localStorage.getItem("beep.mediaPane.window"))).toBeNull();
  await page.addInitScript(() => { window.__BEEP_RESTORING_WINDOW__ = true; });
  await page.reload();
  await expect(divider).toBeVisible();
  await expect.poll(() => page.locator("#agpane-right").evaluate(el => el.getBoundingClientRect().width)).toBe(width);
  await expect(page.locator("#rail .pj-li[aria-current=true]")).toContainText("First notification");
});
