import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("command suggestions keep focus and insert the selected command", async ({ page }) => {
  const commands = [{ name: "review", description: "Review work", hint: "[path]" }, { name: "run", description: "Run checks", hint: "" }];
  await openDesktop(page, true, { agent_commands: commands });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const field = page.locator("#agmsg");
  await expect(field).toBeVisible();
  await field.fill("/");
  const menu = page.locator(".cm-menu");
  await expect(menu.getByRole("option")).toHaveCount(2);
  await expect(field).toBeFocused();
  await field.press("ArrowDown");
  await expect(menu.getByRole("option", { selected: true })).toContainText("/run");
  await field.press("Enter");
  await expect(field).toHaveValue("/run ");
  await expect(field).toBeFocused();
  await field.fill("/re");
  await menu.getByRole("option", { name: /review/ }).click();
  await expect(field).toHaveValue("/review ");
  await expect(field).toBeFocused();
});

test("late command results cannot remount a removed composer", async ({ page }) => {
  await openDesktop(page, true, { agent_commands: [{ name: "review", description: "Review work", hint: "" }] });
  await page.evaluate(() => { window.__navigation.hold("agent_commands"); window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }); });
  await page.locator("#agmsg").fill("/");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_commands").length)).toBe(1);
  await page.evaluate(() => window.__navigation.open("home"));
  await page.evaluate(() => window.__navigation.release("agent_commands"));
  await expect(page.locator(".cm-menu")).toHaveCount(0);
});

test("agent file picker preserves composer focus and attaches an image", async ({ page }) => {
  await openDesktop(page, true, { agent_tree_upload: "/cache/uploads/example.png" });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const input = page.locator("#agmsg"), picker = page.getByRole("button", { name: "Attach files" });
  await input.focus();
  await picker.dispatchEvent("pointerdown", { button: 0 });
  await expect(input).toBeFocused();
  await page.locator('#agfield input[type="file"]').setInputFiles({ name: "example.png", mimeType: "image/png", buffer: Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=", "base64") });
  const count = page.locator("#agfield .attachment-count");
  await expect(count).toHaveText("1 file");
  await expect(input).toBeFocused();
  await count.click();
  await expect(page.getByRole("dialog", { name: "Attachments", exact: true }).getByRole("button", { name: "Preview example.png", exact: true })).toBeVisible();
});

test("a failed background session close shows a dismissible error", { tag: "@windows" }, async ({ page }, info) => {
  await openDesktop(page, true);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => command === "agent_steer" ? Promise.reject(new Error("Machine is offline\n\nMore details")) : invoke<T>(command, args);
    window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false } });
  });
  await page.keyboard.press(info.project.name === "Windows" ? "Control+Shift+W" : "Meta+Shift+W");
  const alert = page.getByRole("alert").filter({ hasText: "Could not close session" });
  await expect(alert).toBeVisible();
  await alert.getByText("Details", { exact: true }).click();
  await expect(alert.locator("pre")).toHaveText("More details");
  await alert.getByRole("button", { name: "Dismiss" }).click();
  await expect(alert).toHaveCount(0);
});

test("the project start field offers the commands of the harness picked", async ({ page }) => {
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    agent_commands: [{ name: "simplify", description: "Simplify the code", hint: "" }],
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const input = form.getByRole("textbox", { name: "Task for a coding agent" });
  await expect(form.locator('[data-composer="machine"]')).toBeEnabled();
  await input.fill("/simp");
  const menu = form.locator(".cm-menu");
  await expect(menu.getByRole("option")).toHaveCount(1);
  await input.press("Enter");
  await expect(input).toHaveValue("/simplify ");
  await expect(input).toBeFocused();
});

test("the project start field asks a running session of the harness on another machine", async ({ page }) => {
  const row = (id: string, machine: string, status: string, last_activity: number) => ({ kind: "agent", machine_id: machine, machine_name: machine, id, agent: "codex", title: id, cwd: "/srv/project", status, doing: null, started_at: 1, last_activity, external: false, question: null });
  const rows = [row("ended-here", "build-box", "ended", 9), row("running-there", "gpu-box", "idle", 2)];
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    agent_sessions: rows, project_sessions: rows, projects_sessions: rows,
    agent_commands: [{ name: "simplify", description: "Simplify the code", hint: "" }],
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const input = form.getByRole("textbox", { name: "Task for a coding agent" });
  await expect(form.locator('[data-composer="machine"]')).toBeEnabled();
  await input.fill("/simp");
  await expect(form.locator(".cm-menu").getByRole("option")).toHaveCount(1);
  const asked = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_commands").map(call => call.args));
  expect(asked).toContainEqual({ machineId: "gpu-box", session: "running-there" });
});

test("the file menu reads the folder once and takes its changes from the folder's updates", async ({ page }) => {
  const listing = (...names: string[]) => ({ folders: [{ path: "", entries: names.map(name => ({ name, folder: false, size: 1, ignored: false, change: null })), more: 0 }], truncated: false });
  await openDesktop(page, true, { agent_tree: listing("alpha.ts") });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const field = page.locator("#agmsg"), menu = page.locator(".cm-menu");
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree" && call.args.deep === true).length);
  await field.fill("@");
  await expect(menu.getByRole("option")).toHaveCount(1);
  const first = await reads();

  // A listing pushed with the folder's update joins the index without a read.
  const view = { command: "agent_tree", args: { machineId: "build-box", root: "/srv/project", path: "", ignored: false, deep: true }, value: listing("alpha.ts", "beta.ts") };
  await page.evaluate(([topic, value]) => window.__navigation.publish(topic, 3000, { views: [value] }), ["files.%2Fsrv%2Fproject", view] as const);
  await field.fill("");
  await field.fill("@");
  await expect(menu.getByRole("option")).toHaveCount(2);
  expect(await reads()).toBe(first);

  // An update with no listing makes the next menu read the folder again.
  await page.evaluate(() => window.__navigation.publish("files.%2Fsrv%2Fproject", 3001));
  await field.fill("");
  await field.fill("@");
  await expect.poll(reads).toBeGreaterThan(first);
});
