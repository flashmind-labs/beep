import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("running process rows retain focus through updates and stop safely", async ({ page }) => {
  await openDesktop(page, true);
  const process = (pid: number, command: string) => ({ pid, command, started_at: 1, ports: [], left_group: false });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.evaluate(rows => window.__navigation.processes(rows), [process(41, "first process"), process(42, "second process")]);
  await page.getByRole("tab", { name: /Running/ }).click();
  const first = page.locator(".agrunningrow").filter({ hasText: "first process" });
  const stop = first.getByRole("button", { name: "Stop", exact: true });
  await expect(stop).toBeVisible();
  await stop.focus();
  await stop.evaluate(element => { element.dataset.identity = "kept"; });
  await page.evaluate(rows => window.__navigation.processes(rows), [process(41, "first process"), process(42, "updated process")]);
  await expect(page.locator(".agrunningrow").filter({ hasText: "updated process" })).toBeVisible();
  await expect(stop).toHaveAttribute("data-identity", "kept");
  await expect(stop).toBeFocused();
  await first.locator("[data-whole]").click();
  await expect(first).toHaveClass(/whole/);
  await first.locator("b").evaluate(element => { const range = document.createRange(); range.selectNodeContents(element); const selection = document.getSelection(); selection?.removeAllRanges(); selection?.addRange(range); });
  await expect.poll(() => page.evaluate(() => document.getSelection()?.toString())).toBe("first process");
  await page.evaluate(rows => window.__navigation.processes(rows), [process(41, "first process"), process(42, "newer process")]);
  await expect(page.locator(".agrunningrow").filter({ hasText: "newer process" })).toBeVisible();
  await expect.poll(() => page.evaluate(() => document.getSelection()?.toString())).toBe("first process");
  await page.evaluate(() => document.getSelection()?.removeAllRanges());
  await page.evaluate(() => window.__navigation.hold("agent_stop_process"));
  await stop.click();
  await expect(first.getByRole("button", { name: "Stopping…" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Stop all" })).toBeDisabled();
  await page.evaluate(() => window.__navigation.release("agent_stop_process"));
  await expect(first).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().find(call => call.command === "agent_stop_process")?.args)).toEqual({ machineId: "build-box", session: "notice-one", pid: 41 });
  await page.evaluate(() => window.__navigation.open("home"));
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: /Running/ }).click();
  await expect(page.locator(".agrunningrow").filter({ hasText: "newer process" })).toBeVisible();
});

test("a pending process read cannot update a different session", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "agent_processes") return invoke<T>(command, args);
      if (args?.session === "notice-one") {
        Reflect.set(window, "processReadPending", true);
        await new Promise<void>(resolve => window.addEventListener("test-release-process", () => resolve(), { once: true }));
        return { processes: [{ pid: 41, command: "old process", started_at: 1, ports: [], left_group: false }], local: false } as T;
      }
      return { processes: [], local: false } as T;
    };
  });
  const open = async (id: string) => page.evaluate(session => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: session, agent: "codex", title: session, cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }), id);
  await open("notice-one");
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "processReadPending"))).toBe(true);
  await open("notice-two");
  await page.evaluate(() => window.dispatchEvent(new Event("test-release-process")));
  await expect(page.getByText("old process")).toHaveCount(0);
  await expect(page.getByRole("tab", { name: /Running/ })).toHaveCount(0);
});

test("an older process read cannot replace a newer result in the same session", async ({ page }) => {
  await openDesktop(page, true);
  const agent = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false };
  await page.evaluate(value => window.__navigation.open({ agent: value }), agent);
  await page.evaluate(() => window.__navigation.processes([{ pid: 41, command: "initial process", started_at: 1, ports: [], left_group: false }]));
  await page.getByRole("tab", { name: /Running/ }).click();
  await expect(page.getByText("initial process")).toBeVisible();
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    let reads = 0;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "agent_processes") return invoke<T>(command, args);
      const read = ++reads;
      Reflect.set(window, "orderedProcessReads", read);
      if (read === 1) {
        await new Promise<void>(resolve => Reflect.set(window, "releaseOldProcesses", resolve));
        Reflect.set(window, "oldProcessesDelivered", true);
      }
      return { processes: [{ pid: 41, command: read === 1 ? "old process" : "new process", started_at: 1, ports: [], left_group: false }], local: false } as T;
    };
    window.dispatchEvent(new Event("agent-pane-shown"));
  });
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, "releaseOldProcesses"))).toBe("function");
  await page.evaluate(() => window.dispatchEvent(new Event("agent-pane-shown")));
  await expect(page.getByText("new process")).toBeVisible();
  await page.evaluate(() => Reflect.get(window, "releaseOldProcesses")());
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "oldProcessesDelivered"))).toBe(true);
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => resolve())));
  await expect(page.getByText("new process")).toBeVisible();
  await expect(page.getByText("old process")).toHaveCount(0);
});
