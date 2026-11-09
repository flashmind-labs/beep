import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("agent command row keeps focus while opening its details", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      return { ...value, entries: [{ kind: "user", text: "Check" }, { kind: "tool", text: "Timer to recheck the build [in_progress]", tool: { name: "Bash", input: '{"command":"sleep 45"}', input_truncated: false, result: null, result_truncated: false } }] } as T;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const row = page.locator('.dcoderen.tool[data-at="1"]');
  await expect(row).toContainText("Timer to recheck the build");
  await row.focus();
  await row.evaluate(element => { (element as HTMLElement).dataset.identity = "kept"; });
  await page.keyboard.press("Enter");
  await expect(row).toHaveAttribute("aria-expanded", "true");
  await expect(row.locator(".call-details")).toContainText("sleep 45");
  await expect(row).toHaveAttribute("data-identity", "kept");
  await expect(row).toBeFocused();
  await row.locator("[data-tool-content]").evaluate(element => {
    const link = document.createElement("a"); link.className = "imgl"; link.dataset.img = "0"; link.textContent = "example.png";
    const pictures = document.createElement("span"); pictures.className = "pics";
    const image = document.createElement("img"); image.className = "pic"; image.src = "data:image/png;base64,iVBORw0KGgo=";
    pictures.append(image); element.append(link, pictures);
  });
  await row.locator("a.imgl").click();
  await expect(page.getByRole("tab", { name: /example.png/ })).toBeVisible();
});

test("grouped agent tools open without replacing the group control", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      const session = Reflect.get(value, "session");
      return { ...value, session: { ...session, status: "idle" }, entries: [{ kind: "user", text: "Check" }, { kind: "tool", text: "Read: first.ts [completed]" }, { kind: "tool", text: "Read: second.ts [completed]" }] } as T;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("button", { name: "Read 2 files", exact: true }).click();
  const group = page.locator(".dtrun");
  const button = group.locator("[data-run]");
  await expect(button).toContainText("Read 2 files");
  await button.focus();
  await button.evaluate(element => { (element as HTMLElement).dataset.identity = "kept"; });
  await button.click();
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await expect(button).toHaveAttribute("data-identity", "kept");
  await expect(group.locator(".dcoderen.tool")).toHaveCount(2);
});

test("made-file activity keeps its control when expanded", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      return { ...value, entries: [{ kind: "user", text: "Check" }, { kind: "other", text: "(made first.md)\n(made second.md)" }] } as T;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const button = page.locator("[data-madeat]");
  await expect(button).toContainText("Made 2 files");
  await button.focus();
  await button.evaluate(element => { (element as HTMLElement).dataset.identity = "kept"; });
  await button.click();
  await expect(button).toHaveAttribute("aria-expanded", "true");
  await expect(button).toHaveAttribute("data-identity", "kept");
  await expect(page.locator("[data-made]")).toHaveCount(2);
});

test("folded activity retains its image tile", async ({ page }) => {
  await openDesktop(page, false, { agent_thumb: "data:image/png;base64,iVBORw0KGgo=" });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      const session = Reflect.get(value, "session");
      return { ...value, session: { ...session, status: "idle" }, entries: [{ kind: "user", text: "Check" }, { kind: "tool", text: "Read: first.ts [completed]\n[image: image-0123456789abcdef]" }, { kind: "tool", text: "Read: second.ts [completed]" }] } as T;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await expect(page.locator(".dfoldpics img.pic")).toHaveCount(1);
});
