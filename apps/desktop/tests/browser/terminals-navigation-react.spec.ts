import { expect, test } from "@playwright/test";
import { allowSelection, openDesktop } from "./harness.ts";

test("terminal list refresh keeps its rows and mounted screen", async ({ page }) => {
  const terminal = (id: string, name: string) => ({ id, project: "", machine_id: "build-box", machine_name: "build-box", name, command: "shell", cwd: "/srv/project", state: "running", code: null, started_at: 1 });
  const rows = Array.from({ length: 20 }, (_, i) => terminal(`terminal-${i}`, `shell-${i}`));
  await openDesktop(page, false, { terminals_all: rows });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => command === "terminal_attach" ? new Promise<T>(() => {}) : invoke<T>(command, args);
  });
  await page.evaluate(() => window.__navigation.open("terminals"));
  const list = page.locator(".tl-list");
  const first = list.getByRole("button", { name: /shell-0/ });
  await expect(first).toBeVisible();
  await first.click();
  const screen = page.locator(".tm-panel .tm-term");
  await expect(screen).toBeVisible();
  // Opening it gives the screen the keyboard once the terminal has loaded.
  await expect(page.locator(".tm-panel .tm-keys-input")).toBeFocused();
  await first.focus();
  await first.evaluate(element => { element.dataset.identity = "kept"; });
  await screen.evaluate(element => { element.dataset.identity = "kept"; });
  await list.evaluate(element => { element.scrollTop = 100; });
  const top = await list.evaluate(element => element.scrollTop);
  await page.evaluate(value => { window.__navigation.respond("terminals_all", value); window.__navigation.notify("terminals"); }, [...rows, terminal("terminal-new", "new shell")]);
  await expect(list.getByRole("button", { name: /new shell/ })).toBeVisible();
  await expect(first).toHaveAttribute("data-identity", "kept");
  await expect(first).toBeFocused();
  await expect(screen).toHaveAttribute("data-identity", "kept");
  await expect(list).toHaveJSProperty("scrollTop", top);
  await page.getByRole("button", { name: "Focus", exact: true }).click();
  await expect(page.locator(".tm-panel .tm-keys-input")).toBeFocused();
  await page.evaluate(value => { window.__navigation.respond("terminals_all", value); window.__navigation.notify("terminals"); }, [terminal("terminal-0", "renamed active shell"), ...rows.slice(1), terminal("terminal-new", "renamed shell")]);
  await expect(list.getByRole("button", { name: /renamed shell/ })).toBeVisible();
  await expect(page.locator(".tm-head")).toContainText("renamed active shell");
  await expect(screen).toHaveAttribute("data-identity", "kept");
  await expect(page.locator(".tm-panel .tm-keys-input")).toBeFocused();
  await page.getByRole("button", { name: "View only", exact: true }).click();
  await expect(page.getByRole("button", { name: "Take control", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Take control", exact: true }).click();
  await page.getByRole("button", { name: "Detach", exact: true }).click();
  await expect(page.getByRole("button", { name: "Attach", exact: true })).toBeVisible();
  await expect(screen).toHaveAttribute("data-identity", "kept");
  await page.getByRole("button", { name: "Attach", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_detach").length)).toBe(1);
  await expect(screen).toHaveAttribute("data-identity", "kept");
});

test("terminal switching reuses screens and stopping requires confirmation", async ({ page }) => {
  const terminal = (id: string) => ({ id, project: "", machine_id: "build-box", machine_name: "build-box", name: id, command: "shell", cwd: "/srv/project", state: "running", code: null, started_at: 1 });
  await openDesktop(page, false, { terminals_all: [terminal("first"), terminal("second")] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => command === "terminal_attach" ? new Promise<T>(() => {}) : invoke<T>(command, args);
  });
  await page.evaluate(() => window.__navigation.open("terminals"));
  const list = page.locator(".tl-list");
  await list.getByRole("button", { name: /first/ }).click();
  const screen = page.locator(".tm-panel .tm-term");
  await expect(screen).toBeVisible();
  await screen.evaluate(element => { element.dataset.identity = "first"; });
  await page.locator(".tm-head").click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Kill" })).toBeVisible();
  await page.keyboard.press("Escape");
  await list.getByRole("button", { name: /second/ }).click();
  await expect(page.locator(".tm-head")).toContainText("second");
  await list.getByRole("button", { name: /first/ }).click();
  await expect(screen).toHaveAttribute("data-identity", "first");
  await page.getByRole("button", { name: "Kill" }).click();
  const dialog = page.getByRole("dialog").filter({ hasText: "Stop first?" });
  await expect(dialog).toBeVisible();
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_close").length)).toBe(0);
  await page.keyboard.press("Escape");
  await expect(screen).toHaveAttribute("data-identity", "first");
  await page.evaluate(() => window.__navigation.open("home"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_detach").length)).toBe(2);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_close").length)).toBe(0);
});

test("new terminal form retains draft and caret through refresh and starts once", async ({ page }) => {
  const errors: string[] = [];
  page.on("console", message => { if (message.type() === "error") errors.push(message.text()); });
  const machine = { id: "build-box", name: "Build box", online: true, can_host: true, seen_at: 0, version: "1", env: null, apps: [], gpus: [], capabilities: [], agents: 0, stats: null };
  const terminal = { id: "new-terminal", project: "", machine_id: "build-box", machine_name: "Build box", name: "my shell", command: "printf test", cwd: "/srv/project", state: "running", code: null, started_at: 1 };
  await openDesktop(page, false, { center_home: { machines: [machine], projects: [], running: [], needs_you: [] }, terminals_all: [], terminal_open: terminal });
  await page.evaluate(() => window.__navigation.open("terminals"));
  await page.getByRole("button", { name: "New terminal", exact: true }).first().click();
  const form = page.locator(".tm-form"), command = form.getByRole("textbox", { name: "Command" });
  await command.fill("printf test");
  await command.evaluate(element => { if (element instanceof HTMLInputElement) { element.dataset.identity = "kept"; element.setSelectionRange(3, 3); } });
  await form.getByRole("textbox", { name: "Name" }).fill("my shell");
  await command.focus();
  await command.evaluate(element => { if (element instanceof HTMLInputElement) element.setSelectionRange(3, 3); });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminals_all").length);
  await page.evaluate(next => { window.__navigation.respond("terminals_all", next); window.__navigation.notify("terminals"); }, [{ ...terminal, id: "pushed", name: "pushed shell" }]);
  await expect(page.locator(".tl-list").getByRole("button", { name: /pushed shell/ })).toBeVisible();
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminals_all").length)).toBe(reads);
  await expect(command).toHaveAttribute("data-identity", "kept");
  await expect(command).toBeFocused();
  await expect(command).toHaveJSProperty("selectionStart", 3);
  await expect(command).toHaveValue("printf test");
  await page.evaluate(() => window.__navigation.hold("terminal_open"));
  await form.getByRole("button", { name: "Open", exact: true }).click();
  await expect(form.getByRole("button", { name: "Starting…" })).toBeDisabled();
  await form.dispatchEvent("submit", { bubbles: true, cancelable: true });
  await form.dispatchEvent("submit", { bubbles: true, cancelable: true });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_open").length)).toBe(1);
  await page.evaluate(() => window.__navigation.release("terminal_open"));
  await expect(page.locator(".tm-panel .tm-term")).toBeVisible();
  expect(await page.evaluate(() => window.__navigation.calls().find(call => call.command === "terminal_open")?.args.terminal)).toMatchObject({ machineId: "build-box", name: "my shell", command: "printf test" });
  await page.getByRole("button", { name: "New terminal", exact: true }).first().click();
  await page.locator(".tm-form").getByRole("button", { name: "Cancel" }).click();
  await expect(page.locator(".tm-form")).toHaveCount(0);
  expect(errors.filter(message => message.includes("unmount") || message.includes("flushSync"))).toEqual([]);
});

test("new terminal cannot submit after its machine goes offline", async ({ page }) => {
  const machine = { id: "build-box", name: "Build box", online: true, can_host: true, seen_at: 0, version: "1", env: null, apps: [], gpus: [], capabilities: [], agents: 0, stats: null };
  const home = { machines: [machine], projects: [], running: [], needs_you: [] };
  await openDesktop(page, false, { center_home: home, terminals_all: [] });
  await page.evaluate(() => window.__navigation.open("terminals"));
  await page.getByRole("button", { name: "New terminal", exact: true }).first().click();
  const form = page.locator(".tm-form");
  await expect(form.getByRole("button", { name: "Build box" })).toHaveAttribute("aria-pressed", "true");
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, { ...home, machines: [{ ...machine, online: false }] });
  await expect(page.locator("#side .cc-foot")).toContainText("0 of 1 machine online");
  await page.evaluate(() => window.__navigation.notify("terminals"));
  await expect(form.getByText("No machine is online")).toBeVisible();
  await expect(form.getByRole("button", { name: "Open", exact: true })).toBeDisabled();
  await form.dispatchEvent("submit", { bubbles: true, cancelable: true });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_open").length)).toBe(0);
});

test("opening a new terminal again resets the previous form", async ({ page }) => {
  const machine = { id: "build-box", name: "Build box", online: true, can_host: true, seen_at: 0, version: "1", env: null, apps: [], gpus: [], capabilities: [], agents: 0, stats: null };
  await openDesktop(page, false, { center_home: { machines: [machine], projects: [], running: [], needs_you: [] }, terminals_all: [] });
  await page.evaluate(() => window.__navigation.open("terminals"));
  const create = page.getByRole("button", { name: "New terminal", exact: true }).first();
  await create.click();
  await page.getByRole("textbox", { name: "Command" }).fill("stale command");
  await create.click();
  await expect(page.getByRole("textbox", { name: "Command" })).toHaveValue("");
});

test("view-only output keeps its screen, focus, selection and scroll across reads", async ({ page }) => {
  const terminal = { id: "watched", project: "", machine_id: "build-box", machine_name: "build-box", name: "watched", command: "tail", cwd: "/srv/project", state: "running", code: null, started_at: 1 };
  const output = (count: number) => ({ terminal, earlier: true, lines: Array.from({ length: count }, (_, i) => `line ${i}`) });
  await openDesktop(page, false, { terminals_all: [terminal], terminal_output: output(100) });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "terminal_attach") throw new Error("The node needs an update to attach");
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(() => window.__navigation.open("terminals"));
  await page.locator(".tl-list").getByRole("button", { name: /watched/ }).click();
  const screen = page.getByLabel("Terminal output");
  await expect(screen).toContainText("line 99");
  await expect(screen.locator(".tm-cmd")).toHaveText("build-box /srv/project $ tail");
  await expect.poll(() => screen.evaluate(element => element.scrollHeight - element.scrollTop - element.clientHeight)).toBeLessThan(4);
  await screen.evaluate(element => { element.dataset.identity = "kept"; element.scrollTop = 0; element.dispatchEvent(new Event("scroll")); });
  await screen.focus();
  await allowSelection(page);
  await screen.evaluate(element => {
    const text = element.lastChild;
    if (text) getSelection()?.setBaseAndExtent(text, 0, text, 6);
  });

  await page.evaluate(next => { window.__navigation.respond("terminals_all", next); window.__navigation.notify("terminals"); }, [terminal, { ...terminal, id: "pushed", name: "pushed shell" }]);
  await expect(page.locator(".tl-list").getByRole("button", { name: /pushed shell/ })).toBeVisible();
  await expect(screen).toHaveAttribute("data-identity", "kept");
  await expect(screen).toBeFocused();
  expect(await page.evaluate(() => getSelection()?.toString())).toBe("line 0");

  // The open terminal's own topic pushes its screen.
  await page.evaluate(value => window.__navigation.publish("terminal-output.watched", null, { views: [{ command: "terminal_output", args: { id: "watched" }, value }] }), output(101));
  await expect(screen).toContainText("line 100");
  await expect(screen).toHaveAttribute("data-identity", "kept");
  await expect(screen).toBeFocused();
  await expect(screen).toHaveJSProperty("scrollTop", 0);
});
