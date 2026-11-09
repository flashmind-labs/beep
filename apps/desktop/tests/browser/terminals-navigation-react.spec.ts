import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

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
  const screen = page.locator(".tm-panel .xterm-screen");
  await expect(screen).toBeVisible();
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
  await expect(page.locator(".tm-panel .xterm-helper-textarea")).toBeFocused();
  await page.evaluate(value => { window.__navigation.respond("terminals_all", value); window.__navigation.notify("terminals"); }, [terminal("terminal-0", "renamed active shell"), ...rows.slice(1), terminal("terminal-new", "renamed shell")]);
  await expect(list.getByRole("button", { name: /renamed shell/ })).toBeVisible();
  await expect(page.locator(".tm-head")).toContainText("renamed active shell");
  await expect(screen).toHaveAttribute("data-identity", "kept");
  await expect(page.locator(".tm-panel .xterm-helper-textarea")).toBeFocused();
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
  const screen = page.locator(".tm-panel .xterm-screen");
  await expect(screen).toBeVisible();
  await screen.evaluate(element => { element.dataset.identity = "first"; });
  await page.locator(".tm-head").click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Kill…" })).toBeVisible();
  await page.keyboard.press("Escape");
  await list.getByRole("button", { name: /second/ }).click();
  await expect(page.locator(".tm-head")).toContainText("second");
  await list.getByRole("button", { name: /first/ }).click();
  await expect(screen).toHaveAttribute("data-identity", "first");
  await page.getByRole("button", { name: "Kill…" }).click();
  const dialog = page.getByRole("dialog").filter({ hasText: "Stop first?" });
  await expect(dialog).toBeVisible();
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_close").length)).toBe(0);
  await page.keyboard.press("Escape");
  await expect(screen).toHaveAttribute("data-identity", "first");
  await page.evaluate(() => window.__navigation.open("home"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_detach").length)).toBe(2);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_close").length)).toBe(0);
});
