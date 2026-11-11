import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("project composer keeps draft, menu choices and issue branch across redraws", async ({ page }) => {
  const now = Date.now();
  const issue = { number: 7, title: "Example issue", url: "https://example.test/issues/7", author: "Example", opened_at: now, comments: 0, labels: [] };
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    center_project_lists: { issues: [issue], prs: [], issues_error: null, prs_error: null, synced_at: now }, center_project_threads: [], center_thread_chats: [],
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const input = form.getByRole("textbox", { name: "Task for a coding agent" });
  await input.fill("Keep this draft");
  const machine = form.locator('[data-composer="machine"]');
  const folder = form.locator('[data-composer="folder"]');
  await expect(machine).toBeEnabled();
  await expect(folder).toBeEnabled();
  await machine.evaluate(el => { el.dataset.identity = "kept"; });
  await folder.click();
  await page.getByRole("menu", { name: "Folder" }).getByRole("menuitem", { name: "New worktree" }).click();
  await expect(folder).toContainText("New worktree");
  await expect(input).toHaveValue("Keep this draft");
  await expect(machine).toHaveAttribute("data-identity", "kept");
  await page.locator('.pj-list .pj-li[title="Issues"]').click();
  await page.locator('.pj-det .pj-li[title="Example issue"]').click();
  await page.getByRole("button", { name: "Work on it", exact: true }).click();
  const branch = form.getByRole("textbox", { name: "Branch name" });
  await expect(branch).toHaveValue("issue-7-example-issue");
  await branch.fill("custom-branch");
  await branch.focus();
  await branch.evaluate(el => { if (el instanceof HTMLInputElement) el.setSelectionRange(3, 3); });
  await page.locator(".pj-strip button").filter({ hasText: "Machines" }).evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  await expect(branch).toBeFocused();
  await expect(branch).toHaveValue("custom-branch");
  await expect(branch).toHaveJSProperty("selectionStart", 3);
  await form.getByRole("button", { name: "Clear", exact: true }).click();
  await expect(branch).toBeHidden();
  await expect(input).toHaveValue("");
});

test("project composer submits keyboard draft and attached image with the chosen checkout", async ({ page }) => {
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    agent_start: { session: { machine_id: "build-box", machine_name: "build-box", id: "started", agent: "codex", cwd: "/srv/project", status: "working", external: false, question: null } },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const folder = form.locator('[data-composer="folder"]');
  await expect(folder).toBeEnabled();
  await folder.click();
  await page.getByRole("menu", { name: "Folder" }).getByRole("menuitem", { name: "New worktree" }).click();
  await form.locator('input[type="file"]').setInputFiles({ name: "example.png", mimeType: "image/png", buffer: Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=", "base64") });
  await expect(form.locator(".ag-att")).toContainText("example.png");
  const input = form.getByRole("textbox", { name: "Task for a coding agent" });
  await input.fill("Inspect the attachment");
  await input.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_start").length)).toBe(1);
  const args = await page.evaluate(() => window.__navigation.calls().find(call => call.command === "agent_start")?.args);
  expect(args?.machineId).toBe("build-box");
  expect(args?.prompt).toBe("Inspect the attachment");
  expect(args?.checkout).toEqual({ new_worktree: { base: null } });
  expect(args?.images).toEqual([expect.objectContaining({ name: "example.png", mime: "image/png" })]);
});

test("project composer closes and reopens its model picker", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }] });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const harness = form.locator('button[title="Harness, model and thinking"]');
  await expect(harness).toBeEnabled();
  await harness.click();
  await expect(harness).toHaveAttribute("aria-expanded", "true");
  await expect(form.locator(".pick .agc-search")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(harness).toHaveAttribute("aria-expanded", "false");
  await expect(form.locator(".pick .agc-search")).toHaveCount(0);
  await expect(harness).toBeFocused();
  await harness.click();
  await expect(form.locator(".pick .agc-search")).toBeVisible();
});

test("Shift+Enter keeps the focused draft without submitting", async ({ page }) => {
  const hosts = ["build-box", "studio"].map(name => ({ machine_id: name, machine_name: name, agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }));
  const machines = ["build-box", "studio"].map(name => ({ id: name, name, online: true, folder: "/srv/project", checking: false, checked_at: 1 }));
  await openDesktop(page, false, { agent_hosts: hosts, project_machines: machines });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const input = form.getByRole("textbox", { name: "Task for a coding agent" });
  const machine = form.locator('[data-composer="machine"]');
  await expect(machine).toHaveText("build-box");
  await input.fill("First line");
  await input.press("Shift+Enter");
  await input.pressSequentially("Second line");
  await expect(machine).toHaveText("build-box");
  await expect(input).toBeFocused();
  await expect(input).toHaveValue("First line\nSecond line");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_start").length)).toBe(0);
});


test("newer host availability wins over a delayed initial read", async ({ page }) => {
  const host = { machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] };
  const machine = { id: "build-box", name: "build-box", online: true, folder: "/srv/project", checking: false, checked_at: 1 };
  await openDesktop(page, false, { agent_hosts: [host], project_machines: [machine] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    // The first read is the one opening the project starts; WebKit names minified functions by
    // their short names, so the stack cannot tell.
    let first = true;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "agent_hosts" && first) {
        first = false;
        await new Promise<void>(resolve => Reflect.set(window, "releaseOldHosts", resolve));
        Reflect.set(window, "oldHostsDelivered", true);
        return [] as T;
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, "releaseOldHosts"))).toBe("function");
  const form = page.locator(".cc-pjc:visible"), input = form.getByRole("textbox", { name: "Task for a coding agent" });
  await input.fill("Keep the draft");
  await input.evaluate(el => { if (el instanceof HTMLTextAreaElement) el.setSelectionRange(2, 5); });
  await page.evaluate(() => window.__navigation.notify("machines"));
  await expect(form.locator('[data-composer="machine"]')).toHaveText("build-box");
  await page.evaluate(() => Reflect.get(window, "releaseOldHosts")());
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "oldHostsDelivered"))).toBe(true);
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => resolve())));
  await expect(form.locator('[data-composer="machine"]')).toHaveText("build-box");
  await expect(input).toHaveValue("Keep the draft");
  await expect(input).toBeFocused();
  await expect(input).toHaveJSProperty("selectionStart", 2);
  await expect(input).toHaveJSProperty("selectionEnd", 5);
});

test("⌘M in the project task field opens and closes the model picker", async ({ page }, info) => {
  await openDesktop(page, false, { agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }] });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const harness = form.locator('button[title="Harness, model and thinking"]');
  await expect(harness).toBeEnabled();
  await form.getByRole("textbox", { name: "Task for a coding agent" }).click();
  const key = info.project.name === "Windows" ? "Control+m" : "Meta+m";
  await page.keyboard.press(key);
  await expect(harness).toHaveAttribute("aria-expanded", "true");
  await expect(form.locator(".pick .agc-search")).toBeVisible();
  await page.keyboard.press(key);
  await expect(harness).toHaveAttribute("aria-expanded", "false");
});
