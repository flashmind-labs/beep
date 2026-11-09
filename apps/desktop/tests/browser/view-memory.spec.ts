import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("starting a project draft keeps its terminal and dock mounted", async ({ page }, info) => {
  const session = { machine_id: "build-box", machine_name: "build-box", id: "started", agent: "codex", title: "Started session", cwd: "/srv/project", status: "working", external: false, question: null };
  await openDesktop(page, true, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: "/srv/project", home: "/srv", capabilities: [] }],
    agent_start: { session },
    agent_view: { session, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 0 },
    terminal_open: { id: "shell-one", project: projectName, machine_id: "build-box", machine_name: "build-box", name: "shell", command: "", cwd: "/srv/project", state: "running", code: null, started_at: 1 },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator('.pj-li[title="Running"]')).toBeVisible();
  await page.keyboard.press(info.project.name === "Windows" ? "Control+n" : "Meta+n");
  const draft = page.locator("#agdraft textarea"); await expect(draft).toBeFocused();
  await page.getByRole("button", { name: "+ Terminal", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_attach").length)).toBeGreaterThan(0);
  const tab = page.locator('[role="tab"][data-mtab^="term:"]');
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".agpview.term")).toBeAttached();
  await page.evaluate(() => { Reflect.set(window, "draftDock", document.querySelector("#agpane-right")); Reflect.set(window, "draftTerminal", document.querySelector('.agpview.term')); });
  await draft.fill("Inspect the project"); await draft.press("Enter");
  await expect(page.locator("#agmsg")).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_start").length)).toBe(1);
  expect(await page.evaluate(() => document.querySelector("#agpane-right") === Reflect.get(window, "draftDock") && document.querySelector('.agpview.term') === Reflect.get(window, "draftTerminal"))).toBe(true);
  const calls = await page.evaluate(() => window.__navigation.calls());
  expect(calls.filter(call => call.command === "terminal_open")).toHaveLength(1);
  expect(calls.filter(call => call.command === "terminal_close")).toHaveLength(0);
});

test("a reopened session restores its file tab", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("beep.projectView.build-box\nnotice-one", JSON.stringify({
    on: { right: "text:sample.ts", bottom: "text:sample.ts" }, tabs: [{ root: "/srv/project", rel: "sample.ts" }],
  })));
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  }}));
  await expect(page.getByRole("tab", { name: /^sample.ts Close/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /^sample.ts Close/ })).toHaveAttribute("aria-selected", "true");
  const code = page.locator(".agcode:visible");
  await code.evaluate(node => { node.scrollTop = 700; });
  await expect.poll(() => code.evaluate(node => node.scrollTop)).toBe(700);
  await page.reload();
  await page.waitForFunction(() => window.__navigation.ready());
  await page.evaluate(() => window.__navigation.open({agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  }}));
  await expect.poll(() => page.locator(".agcode:visible").evaluate(node => node.scrollTop)).toBe(700);
  await page.screenshot({ path: "test-results/restored-file-tab.png" });
});

test("diff context can be expanded and reviewed", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  }}));
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const above = page.getByRole("button", { name: "Show 10 lines above", exact: true });
  const below = page.getByRole("button", { name: "Show 10 lines below", exact: true });
  await expect(above).toHaveText("");
  await expect(below).toHaveText("");
  await expect(below).toBeVisible();
  await page.screenshot({ path: "test-results/diff-context-arrows.png" });
  await above.click();
  const row = page.locator(".dv-l").filter({ hasText: "const value7 = 7;" });
  await expect(row).toBeVisible();
  await row.locator(".dv-g").click();
  await expect(page.getByPlaceholder("Comment on this line")).toBeVisible();
  await page.screenshot({ path: "test-results/expanded-diff-context.png" });
});

test("returning to a session restores its expanded diff and pane layout", async ({ page }) => {
  await openDesktop(page, true);
  const open = async (id: string) => page.evaluate(id => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: id,
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }), id);
  await open("notice-one");
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  await page.keyboard.press("Shift+Escape");
  await expect(page.locator("#agshell")).toHaveClass(/zoomed/);
  await open("notice-two");
  await expect(page.getByRole("tab", { name: "Files", exact: true })).toHaveAttribute("aria-selected", "true");
  await page.getByRole("button", { name: "Hide the pane", exact: true }).click();
  await open("notice-one");
  await expect(page.getByRole("tab", { name: "Changes", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("#agshell")).toHaveClass(/zoomed/);
  await expect(page.locator(".agpane.zoomed")).toBeVisible();
  await page.reload();
  await page.waitForFunction(() => window.__navigation.ready());
  await open("notice-one");
  await expect(page.getByRole("tab", { name: "Changes", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("#agshell")).toHaveClass(/zoomed/);
});

test("returning and reloading reconnects the same terminal without stopping it", async ({ page }) => {
  await openDesktop(page, true, { terminal_open: {
    id: "shell-one", project: "", machine_id: "build-box", machine_name: "build-box", name: "shell",
    command: "", cwd: "/srv/project", state: "running", code: null, started_at: 1,
  } });
  const open = async (id: string) => page.evaluate(id => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: id,
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }), id);
  const attaches = () => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "terminal_attach").length);
  await open("notice-one");
  await page.getByRole("button", { name: "+ Terminal", exact: true }).click();
  await expect.poll(attaches).toBeGreaterThan(0);
  const tab = page.locator('[role="tab"][data-mtab^="term:"]');
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await open("notice-two");
  await open("notice-one");
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await expect.poll(attaches).toBeGreaterThan(1);
  const calls = await page.evaluate(() => window.__navigation.calls());
  expect(calls.filter(c => c.command === "terminal_open")).toHaveLength(1);
  expect(calls.filter(c => c.command === "terminal_close")).toHaveLength(0);
  await page.reload();
  await page.waitForFunction(() => window.__navigation.ready());
  await open("notice-one");
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await expect.poll(attaches).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "terminal_open"))).toHaveLength(0);
  await tab.locator("[data-mclose]").click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "terminal_close").length)).toBe(1);
});

test("pane refresh preserves composer focus and terminal selection focuses its shell", async ({ page }) => {
  await openDesktop(page, true, { terminal_open: { id: "shell-one", project: "", machine_id: "build-box", machine_name: "build-box", name: "shell", command: "", cwd: "/srv/project", state: "running", code: null, started_at: 1 } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("button", { name: "+ Terminal", exact: true }).click();
  const terminalTab = page.locator('[role="tab"][data-mtab^="term:"]');
  await expect(terminalTab).toHaveAttribute("aria-selected", "true");
  const screen = page.locator(".xterm-screen").first();
  await expect(screen).toBeVisible();
  await screen.evaluate(element => { element.dataset.identity = "kept"; });
  const composer = page.locator("#agmsg");
  await composer.focus();
  const dockIcon = page.locator('#agpane-right [data-pact="dock"] svg');
  await dockIcon.evaluate(element => { (element as SVGElement).dataset.identity = "kept"; });
  const processes = [{ pid: 41, command: "example process", started_at: 1, ports: [], left_group: false }];
  await page.evaluate(rows => window.__navigation.processes(rows), processes);
  await expect(page.getByRole("tab", { name: /Running/ })).toBeVisible();
  await expect(composer).toBeFocused();
  await expect(dockIcon).toHaveAttribute("data-identity", "kept");
  await expect(screen).toHaveAttribute("data-identity", "kept");
  expect((await screen.boundingBox())?.width).toBeGreaterThan(0);
  await terminalTab.focus();
  await terminalTab.evaluate(element => { element.dataset.identity = "kept"; });
  await page.evaluate(rows => window.__navigation.processes(rows), [...processes, { ...processes[0], pid: 42, command: "another process" }]);
  await expect(page.getByRole("tab", { name: /Running/ })).toContainText("2");
  await expect(terminalTab).toHaveAttribute("data-identity", "kept");
  await expect(terminalTab).toBeFocused();
  await expect(screen).toHaveAttribute("data-identity", "kept");
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await terminalTab.click();
  await expect(page.locator(".xterm-helper-textarea:focus")).toHaveCount(1);
  await composer.focus();
  await page.evaluate(() => window.__navigation.processes([]));
  await expect(page.getByRole("tab", { name: /Running/ })).toHaveCount(0);
  await expect(composer).toBeFocused();
});

test("terminal tab menu and dragging preserve the running screen", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  await openDesktop(page, true, { terminal_open: { id: "shell-one", project: "", machine_id: "build-box", machine_name: "build-box", name: "shell", command: "", cwd: "/srv/project", state: "running", code: null, started_at: 1 } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("button", { name: "+ Terminal", exact: true }).click();
  const tab = page.locator('[role="tab"][data-mtab^="term:"]');
  await expect(tab).toBeVisible();
  const screen = page.locator(".xterm-screen").first();
  await expect(screen).toBeVisible();
  await screen.evaluate(element => { element.dataset.identity = "kept"; });
  await tab.click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Close", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  const start = await tab.boundingBox(), shell = await page.locator("#agshell").boundingBox();
  const from = await tab.evaluate(element => element.closest<HTMLElement>(".agpane")?.dataset.dock);
  expect(start).not.toBeNull(); expect(shell).not.toBeNull();
  if (!start || !shell) return;
  const target = from === "right" ? { x: shell.x + shell.width * 0.35, y: shell.y + shell.height - 8 } : { x: shell.x + shell.width - 8, y: shell.y + shell.height * 0.4 };
  await page.mouse.move(start.x + start.width / 2, start.y + start.height / 2);
  await page.mouse.down();
  await page.mouse.move(target.x, target.y, { steps: 12 });
  await page.mouse.up();
  await expect.poll(() => tab.evaluate(element => element.closest<HTMLElement>(".agpane")?.dataset.dock)).not.toBe(from);
  await expect(screen).toHaveAttribute("data-identity", "kept");
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_open").length)).toBe(1);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_close").length)).toBe(0);
});
