import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("agent terminal footer retains its screen and confirms close", async ({ page }) => {
  const terminal = { id: "term-one", project: "", machine_id: "build-box", machine_name: "Build box", name: "shell", command: "shell", cwd: "/srv/project", state: "running", code: null, started_at: 1 };
  await openDesktop(page, true, { terminal_open: terminal });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => command === "terminal_attach" ? new Promise<T>(() => {}) : invoke<T>(command, args);
    window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "Build box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } });
  });
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.getByRole("button", { name: "+ Terminal" }).filter({ visible: true }).click();
  const terminalView = page.locator(".agpview.term:visible");
  await expect(terminalView.locator(".xterm")).toBeVisible();
  const screen = terminalView.locator(".xterm");
  await screen.evaluate(element => { element.dataset.identity = "kept"; });
  await terminalView.locator(".agpacts").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Detach" }).click();
  const attach = terminalView.getByRole("button", { name: "Attach", exact: true });
  await expect(attach).toBeVisible();
  await attach.click();
  await expect(attach).toBeHidden();
  await expect(screen).toHaveAttribute("data-identity", "kept");
  const close = terminalView.getByRole("button", { name: "Close", exact: true });
  await expect(close).toBeVisible();
  await close.click();
  const dialog = page.getByRole("dialog").filter({ hasText: "Stop" });
  await expect(dialog).toBeVisible();
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "terminal_close").length)).toBe(0);
});
