import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

/// A start this device keeps, the session its machine names for it, and their pages.
const kept = { machine_id: "build-box", machine_name: "build-box", agent: "codex", cwd: "/srv/project", external: false, question: null, started_at: 1, last_activity: 1, assistant: false };
const start = { ...kept, id: "start:one", title: "Inspect the project", status: "starting", doing: "Waiting for build-box" };
const named = { ...kept, id: "started", title: null, status: "starting", doing: null };
const pageOf = (session: Omit<typeof start, "title" | "doing"> & { title: string | null; doing: string | null }, text: string | null, error: string | null) => ({ session, plan: [], entries: text === null ? [] : [{ kind: "user", text }], earlier: 0, error, queued: 0, queued_messages: [], turns: 0 });
const calls = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).map(call => call.args), command);

test("opening and revisiting a project focuses its composer without stealing focus on refresh", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }] });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const input = form.getByRole("textbox", { name: "Task for a coding agent" });
  await expect(input).toBeFocused();

  const folder = form.locator('[data-composer="folder"]');
  await expect(folder).toBeEnabled();
  await folder.focus();
  await page.evaluate(host => window.__navigation.notify("machines", [{ command: "agent_hosts", value: [host] }]),
    { machine_id: "build-box", machine_name: "build-box", agents: ["codex", "claude"], workspace: null, home: "/srv", capabilities: [] });
  await expect(form.locator('[data-composer="machine"]')).toHaveText("build-box");
  await expect(folder).toBeFocused();
  expect(await calls(page, "agent_hosts")).toHaveLength(1);

  await page.evaluate(() => window.__navigation.open("home"));
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(input).toBeFocused();
});

test("project composer keeps draft, menu choices and issue branch across redraws", async ({ page }) => {
  const now = Date.now();
  const issue = { number: 7, title: "Example issue", url: "https://example.test/issues/7", author: "Example", opened_at: now, comments: 0, labels: [] };
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    center_project_lists: { issues: [issue], prs: [], issues_error: null, prs_error: null },
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
  await page.locator(".pj-strip button").filter({ hasText: "issue" }).click();
  await page.locator('.pj-det .pj-li[title="Example issue"]').click({ button: "right" });
  await page.getByRole("menu", { name: "#7 Example issue" }).getByRole("menuitem", { name: "Work on it" }).click();
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

for (const typed of ["Inspect the attachment", ""]) test(`project composer submits ${typed ? "typed" : "default"} image prompt with the chosen checkout`, async ({ page }) => {
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    agent_tree_upload: "/cache/attachments/example.png",
    agent_begin: { session: start, why: "" },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const folder = form.locator('[data-composer="folder"]');
  await expect(folder).toBeEnabled();
  await folder.click();
  await page.getByRole("menu", { name: "Folder" }).getByRole("menuitem", { name: "New worktree" }).click();
  await form.locator('input[type="file"]').setInputFiles({ name: "example.png", mimeType: "image/png", buffer: Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=", "base64") });
  await expect(form.locator(".attachment-count")).toHaveText("1 file");
  const input = form.getByRole("textbox", { name: "Task for a coding agent" });
  await input.fill(typed);
  await input.press("Enter");
  await expect.poll(async () => (await calls(page, "agent_begin")).length).toBe(1);
  const [args] = await calls(page, "agent_begin");
  expect(args?.machineId).toBe("build-box");
  expect(args?.prompt).toBe(`${typed || "Check this image."}\n\n@/cache/attachments/example.png`);
  expect(await calls(page, "agent_tree_upload")).toMatchObject([
    { root: "/srv/project", path: "example.png", attachment: { agent: "draft", session: expect.any(String) } },
  ]);
  expect(args?.checkout).toEqual({ new_worktree: { base: null } });
  expect(args?.images).toEqual([expect.objectContaining({ name: "example.png", mime: "image/png" })]);
});

test("a draft's start opens as a session at once, and the session its machine names takes it over", { tag: "@windows" }, async ({ page }, info) => {
  await openDesktop(page, false, {
    agent_sessions: [],
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: "/srv/project", home: "/srv", capabilities: [] }],
    agent_begin: { session: start, why: "" },
    agent_view: pageOf(start, "Inspect the project", null),
  });
  // The machine names the session before it sends the prompt: its first page has no entries.
  await page.evaluate(first => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const channel = args?.onEvent;
      if (command !== "agent_follow" || typeof channel !== "object" || channel === null || !("onmessage" in channel) || typeof channel.onmessage !== "function") return invoke<T>(command, args);
      channel.onmessage({ kind: "snapshot", resumed: false, cursor: { start: 0, next: 0, fingerprint: "empty" }, page: first, modes: null, tasks: [] });
      Reflect.set(window, "followed", args?.session);
      return new Promise<never>(() => {});
    };
  }, pageOf(named, null, null));
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator(".pj-strip")).toBeVisible();
  const modifier = info.project.name === "Windows" ? "Control" : "Meta";
  await page.keyboard.press(`${modifier}+n`);
  const draft = page.locator("#agdraft textarea"), sent = page.locator("#agbody .dcoderen.user"), live = page.locator("#agbody .dlive .lb"), banner = page.locator("#agconv .dbanner");
  await expect(draft).toBeFocused();

  // This device refuses the start: the input keeps the task and says why.
  await page.evaluate(() => window.__navigation.rejectOnce("agent_begin"));
  await draft.fill("Inspect the project");
  await draft.press("Enter");
  await expect(page.locator("#agdraft .cc-err")).not.toBeEmpty();
  await expect(draft).toHaveValue("Inspect the project");

  // Kept: the session view opens with the message and what the start waits for, and only this device is read.
  await draft.press("Enter");
  await expect(sent).toContainText("Inspect the project");
  await expect(live).toHaveText("Waiting for build-box");
  await expect(banner).toContainText("Waiting for build-box");
  await expect(banner.getByRole("button", { name: "Retry" })).toHaveCount(0);
  await expect(page.locator("#agdraft, #agmsg")).toHaveCount(0);
  expect((await calls(page, "agent_begin")).at(-1)).toEqual({ machineId: "build-box", agent: "codex", cwd: "/srv/project", prompt: "Inspect the project", model: null, effort: null, fast: null, checkout: null, images: [], account: null });
  await expect.poll(async () => (await calls(page, "agent_view")).map(args => args.session)).toContain("start:one");

  for (const command of ["agent_start", "agent_follow", "agent_modes", "agent_tasks", "transcript_kept", "outbox_list"]) {
    expect(await calls(page, command), command).toEqual([]);
  }

  // The machine came online: the start says so, and the view reads it again.
  await page.evaluate(next => { window.__navigation.respond("agent_view", next); window.__navigation.emit("sessions-changed", "start:one"); }, pageOf({ ...start, doing: "Starting Codex on build-box" }, "Inspect the project", null));
  await expect(live).toHaveText("Starting Codex on build-box");
  await expect(banner).toContainText("Starting Codex on build-box");

  // Its machine named the session: it takes the view over, and keeps the message while its machine has logged nothing.
  await page.evaluate(event => { window.__navigation.respond("agent_view", event.first); window.__navigation.emit("agent-started", event.started); },
    { first: pageOf(named, null, null), started: { start: "start:one", session: named, first_message: "Inspect the project" } });
  await expect(page.locator("#agmsg")).toBeVisible();
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "followed"))).toBe("started");
  await expect(sent).toContainText("Inspect the project");
  await expect(page.locator('[data-log-part="starting"]')).toHaveCount(0);
  await expect(banner).toHaveCount(0);

  // The draft and the start left no page behind: Back is the project, and forward the session.
  await page.keyboard.press(`${modifier}+[`);
  await expect(page.locator(".pj-strip")).toBeVisible();
  await page.keyboard.press(`${modifier}+]`);
  await expect(page.locator("#agmsg")).toBeVisible();
  await expect(sent).toContainText("Inspect the project");
});

test("a start that failed says why, runs again on Retry and goes on Cancel", async ({ page }) => {
  const failed = { ...start, status: "idle", doing: null };
  await openDesktop(page, false, { agent_view: pageOf(failed, "Inspect the project", "build-box does not have that model.") });
  await page.evaluate(agent => window.__navigation.open({ agent: { kind: "agent", ...agent } }), failed);
  const banner = page.locator("#agconv .dbanner");
  await expect(page.locator('#agbody [data-log-part="error"]')).toContainText("build-box does not have that model.");
  await expect(page.locator("#agbody .dcoderen.user")).toContainText("Inspect the project");
  await expect(page.locator("#agbody .dlive")).toHaveCount(0);

  await page.evaluate(next => window.__navigation.respond("agent_view", next), pageOf(start, "Inspect the project", null));
  await banner.getByRole("button", { name: "Retry" }).click();
  expect(await calls(page, "agent_begin_retry")).toEqual([{ session: "start:one" }]);
  await expect(banner).toContainText("Waiting for build-box");
  await expect(banner.getByRole("button", { name: "Retry" })).toHaveCount(0);

  await page.evaluate(next => window.__navigation.respond("agent_view", next), pageOf({ ...kept, id: "notice-one", title: "Next session", status: "working", doing: null }, null, null));
  await banner.getByRole("button", { name: "Cancel" }).click();
  await expect.poll(() => calls(page, "agent_steer")).toEqual([{ machineId: "build-box", agent: "codex", session: "start:one", steer: "close" }]);
  await expect.poll(async () => (await calls(page, "agent_view")).map(args => args.session)).toContain("notice-one");
  await expect(page.locator("#agconv .dtakeover")).toHaveCount(0);
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
  expect(await calls(page, "agent_begin")).toEqual([]);
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
  await page.evaluate(host => window.__navigation.notify("machines", [{ command: "agent_hosts", value: [host] }]), host);
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

test("the model shortcut returns focus to the project task after using the picker", { tag: "@windows" }, async ({ page }, info) => {
  await openDesktop(page, false, { agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }] });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const harness = form.locator('button[title="Harness, model and thinking"]');
  await expect(harness).toBeEnabled();
  const input = form.getByRole("textbox", { name: "Task for a coding agent" });
  await expect(input).toBeFocused();
  const key = info.project.name === "Windows" ? "Control+Shift+m" : "Meta+Shift+m";
  await page.keyboard.press(key);
  await expect(harness).toHaveAttribute("aria-expanded", "true");
  await expect(form.locator(".pick .agc-search")).toBeVisible();
  await page.keyboard.press(key);
  await expect(harness).toHaveAttribute("aria-expanded", "false");
  await expect(input).toBeFocused();
  await page.keyboard.type("Continue typing");
  await expect(input).toHaveValue("Continue typing");
  await page.keyboard.press(key);
  await expect(form.locator(".pick .agc-search")).toBeFocused();
  await page.keyboard.press(key);
  await expect(input).toBeFocused();
});

test("⇧⌘M and the pill open the model picker in a project's new conversation", { tag: "@windows" }, async ({ page }, info) => {
  await openDesktop(page, false, { agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }] });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator(".pj-strip")).toBeVisible();
  const modifier = info.project.name === "Windows" ? "Control" : "Meta";
  await page.keyboard.press(`${modifier}+n`);
  await expect(page.locator("#agdraft textarea")).toBeFocused();
  const harness = page.locator('#agdraft button[title="Harness, model and thinking"]');
  await expect(harness).toBeEnabled();

  await page.keyboard.press(`${modifier}+Shift+m`);
  await expect(harness).toHaveAttribute("aria-expanded", "true");
  await expect(page.locator("#agdraft .pick .agc-search")).toBeVisible();
  await page.keyboard.press(`${modifier}+Shift+m`);
  await expect(harness).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator("#agdraft textarea")).toBeFocused();

  await harness.click();
  await expect(page.locator("#agdraft .pick .agc-search")).toBeVisible();
});

test("the project bar starts as the account picked for the project", async ({ page }) => {
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    agent_account_names: [{ agent: "codex", name: "Work", token: { last4: "4a2f", created_at: 0, unreadable: false } }],
    agent_begin: { session: start, why: "" },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const account = form.locator('[data-composer="account"]');
  await expect(account).toHaveText("as default");
  await account.click();
  const menu = page.getByRole("menu", { name: "Account" });
  await expect(menu.getByRole("menuitemcheckbox", { name: "Default" })).toHaveAttribute("aria-checked", "true");
  await menu.getByRole("menuitemcheckbox", { name: "Work" }).click();
  await expect(account).toHaveText("as Work");
  const input = form.getByRole("textbox", { name: "Task for a coding agent" });
  await input.fill("Inspect the project");
  await input.press("Enter");
  await expect.poll(async () => (await calls(page, "agent_begin")).length).toBe(1);
  const [args] = await calls(page, "agent_begin");
  expect(args?.account).toBe("Work");
});
