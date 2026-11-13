import { expect, test } from "@playwright/test";
import { openDesktop, pageErrors } from "./harness.ts";

for (const width of [1280, 390]) {
  test(`conversation send times stay discreet at width ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const errors = pageErrors(page);
    await openDesktop(page);
    await page.evaluate(() => {
      const invoke = window.__TAURI__.core.invoke;
      window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
        const value = await invoke<T>(command, args);
        if (command !== "agent_view" || !value || typeof value !== "object") return value;
        return { ...value, entries: [
          { kind: "user", text: "Check the build", sent_at: 1794579242 },
          { kind: "agent", text: "Checking the tests.", html: "<p>Checking the tests.</p>", sent_at: 1794579245 },
          { kind: "tool", text: "Run: cargo test [in_progress]", sent_at: 1794579248 },
        ] } as T;
      };
    });
    await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
    for (const selector of [".dcoderen.user time", ".dcoderen.agent time", ".dcoderen.tool time"]) {
      const time = page.locator(selector);
      await expect(time).toBeVisible();
      await expect(time).toHaveCSS("font-size", "10px");
      await expect(time).toHaveAttribute("title", /2026/);
      await expect(time).toHaveText(/^\d{2}:\d{2}$/);
    }
    await page.screenshot({ path: `test-results/send-times-${width}.png` });
    expect(errors).toEqual([]);
  });
}
