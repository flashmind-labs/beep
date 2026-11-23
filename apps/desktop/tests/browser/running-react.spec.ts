import { expect, test } from "@playwright/test";
import { allowSelection, openDesktop } from "./harness.ts";

test("running process rows retain focus through updates and stop safely", async ({ page }) => {
  await openDesktop(page, true);
  const process = (pid: number, command: string) => ({ pid, command, started_at: 1, ports: [], left_group: false, parent: 0, pages: [] });
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
  await allowSelection(page);
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
        return { processes: [{ pid: 41, command: "old process", started_at: 1, ports: [], left_group: false, parent: 0, pages: [] }] } as T;
      }
      return { processes: [] } as T;
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
  await page.evaluate(() => window.__navigation.processes([{ pid: 41, command: "initial process", started_at: 1, ports: [], left_group: false, parent: 0, pages: [] }]));
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
      return { processes: [{ pid: 41, command: read === 1 ? "old process" : "new process", started_at: 1, ports: [], left_group: false, parent: 0, pages: [] }] } as T;
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

test("running pane keeps its scroll and handlers after leaving and reopening the session", async ({ page }) => {
  await openDesktop(page, true);
  const agent = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false };
  const rows = (last: string) => Array.from({ length: 30 }, (_, i) => ({ pid: 100 + i, command: i === 29 ? last : `process ${i}`, started_at: 1, ports: i === 0 ? [3000] : [], left_group: false, parent: 0, pages: [] }));
  await page.evaluate(value => window.__navigation.open({ agent: value }), agent);
  await page.evaluate(value => window.__navigation.processes(value), rows("last process"));
  await page.getByRole("tab", { name: /Running/ }).click();
  const list = page.locator(".agrunninglist");
  await expect(page.getByText("last process")).toBeAttached();
  await list.evaluate(element => { element.style.maxHeight = "120px"; element.style.overflow = "auto"; element.scrollTop = 200; });
  const stop = page.locator(".agrunningrow").filter({ hasText: "process 12" }).getByRole("button", { name: "Stop", exact: true });
  await stop.focus();
  const scroll = await list.evaluate(element => element.scrollTop);
  expect(scroll).toBeGreaterThan(0);
  await page.evaluate(value => window.__navigation.processes(value), rows("newest process"));
  await page.evaluate(() => window.dispatchEvent(new Event("agent-pane-shown")));
  await expect(page.getByText("newest process")).toBeAttached();
  await expect(stop).toBeFocused();
  expect(await list.evaluate(element => element.scrollTop)).toBe(scroll);
  await page.evaluate(() => window.__navigation.open("home"));
  await page.evaluate(value => window.__navigation.open({ agent: value }), agent);
  await page.getByRole("tab", { name: /Running/ }).click();
  await expect(page.locator(".agpview.running")).toHaveCount(1);
  const first = page.locator(".agrunningrow").filter({ hasText: "process 0" }).first();
  await first.locator("[data-whole]").click();
  await expect(first).toHaveClass(/whole/);
  const url = page.url();
  await first.getByRole("link", { name: ":3000" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_url").map(call => call.args))).toEqual([{ url: "http://localhost:3000/" }]);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "machine_forward_port").map(call => call.args))).toEqual([{ machineId: "build-box", port: 3000 }]);
  expect(page.url()).toBe(url);
  await expect(first).toHaveClass(/whole/);
  await first.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(first).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_stop_process").length)).toBe(1);
});
