import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const card = (online: boolean, connecting: boolean) => ({ id: "build-box", name: "build-box", online, connecting, can_host: true, seen_at: 1, version: "", env: null, apps: [], gpus: [], capabilities: [], agents: 1, stats: null });
const agent = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Fix the build script", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } as const;
const home = (online: boolean, connecting: boolean) => ({ machines: [card(online, connecting)], projects: [], running: [agent], needs_you: [] });

/// Holds `command` from the first call until `window.release(command)`.
async function holdFromStart(page: Page, command: string) {
  await page.addInitScript(name => {
    const define = Object.defineProperty;
    let release = () => {};
    const gate = new Promise<void>(resolve => { release = resolve; });
    Reflect.set(window, "release", () => release());
    Object.defineProperty = (target, key, descriptor) => {
      if (target === window && key === "__TAURI__") {
        const core = descriptor.value.core, invoke = core.invoke;
        core.invoke = async (cmd: string, args: unknown) => { if (cmd === name) await gate; return invoke(cmd, args); };
        Object.defineProperty = define;
      }
      return define(target, key, descriptor);
    };
  }, command);
}

test("Home kept from before says its machines are connecting until Home is read", async ({ page }) => {
  await holdFromStart(page, "center_home");
  await openDesktop(page, false, { center_cached: { home: home(true, false) }, center_home: home(true, false) });
  const strip = page.locator(".cc-mstrip .m").filter({ hasText: "build-box" });
  const row = page.locator("#sec-running .bd-card").filter({ hasText: "Fix the build script" });

  await expect(strip).toContainText("connecting");
  await expect(strip.locator(".ddot.dial")).toHaveCount(1);
  await expect(row).toContainText("connecting");
  await expect(page.locator(".cc-foot")).toContainText("Connecting to build-box…");
  // A dot, not the pill a bare .wait would draw.
  const dot = await page.locator(".cc-foot .ddot").boundingBox();
  expect(dot && Math.round(dot.width)).toBe(7);

  await page.evaluate(() => (Reflect.get(window, "release") as () => void)());
  await expect(strip).not.toContainText("connecting");
  await expect(strip.locator(".ddot.dial")).toHaveCount(0);
  await expect(row).not.toContainText("connecting");
  await expect(page.locator(".cc-foot")).toContainText("1 of 1 machine online");
});

test("a machine the mesh still dials reads connecting, then online once reached", async ({ page }) => {
  await openDesktop(page, false, { center_home: home(false, true) });
  const strip = page.locator(".cc-mstrip .m").filter({ hasText: "build-box" });
  await expect(strip).toContainText("connecting");
  // Not offline: no note that its sessions left.
  await expect(page.getByText("is offline.")).toHaveCount(0);

  await page.evaluate(view => { window.__navigation.respond("center_home", view); window.__navigation.notify("home"); }, home(true, false));
  await expect(strip).not.toContainText("connecting");
});

test("a session drawn from what was kept says it is connecting until the first live update", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args: Record<string, unknown> = {}) => {
      if (command === "transcript_kept") {
        const base = await invoke<Record<string, unknown>>("agent_view", { session: "notice-one" });
        return { page: { ...base, entries: [{ kind: "user", text: "Kept question" }, { kind: "assistant", text: "Kept answer" }], earlier: 0 }, cursor: { start: 0, next: 2, fingerprint: "kept" } } as T;
      }

      if (command !== "agent_follow") return invoke<T>(command, args);
      const channel = args.onEvent as { onmessage(value: unknown): void };
      const base = await invoke<Record<string, unknown>>("agent_view", { session: "notice-one" });
      Reflect.set(window, "live", () => channel.onmessage({ kind: "delta", resumed: false, cursor: { start: 0, next: 3, fingerprint: "kept" }, page: { ...base, entries: [{ kind: "assistant", text: "Live answer" }], earlier: 2 }, modes: null, tasks: [] }));
      return new Promise<T>(() => {});
    };
  });

  await page.evaluate(session => window.__navigation.open({ agent: session }), agent);
  const body = page.locator("#agbody");
  await expect(body).toContainText("Kept answer");
  await expect(body.getByText("Connecting to build-box...")).toBeVisible();

  await page.evaluate(() => (Reflect.get(window, "live") as () => void)());
  await expect(body).toContainText("Live answer");
  await expect(body.getByText("Connecting to build-box...")).toHaveCount(0);
});
