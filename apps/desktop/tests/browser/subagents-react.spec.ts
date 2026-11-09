import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("subagent pane shows native child actions and keeps rows on redraw", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      return { ...value, entries: [{ kind: "tool", text: "Inspect files [completed]\n[steps] 1\n[agent] Explorer\n[session] subagent:codex:child\nDone" }] } as T;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: /Subagents/ }).click();
  const row = page.locator(".agpview.subs .dsubrow");
  await expect(row).toContainText("Inspect files");
  await expect(row).toHaveAttribute("data-native", "0");
  await expect(row.locator("[data-subclose]")).toHaveCount(1);
  await row.focus();
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.getByRole("tab", { name: /Subagents/ }).click();
  await expect(row).toBeVisible();
});
