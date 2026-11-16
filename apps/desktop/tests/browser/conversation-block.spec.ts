import { expect, test } from "@playwright/test";
import { openDesktop, pageErrors } from "./harness.ts";

for (const width of [1280, 390]) {
  test(`conversation block is red and separate from the reply at width ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const errors = pageErrors(page);
    await openDesktop(page);
    await page.evaluate(() => {
      const invoke = window.__TAURI__.core.invoke;
      window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
        const value = await invoke<T>(command, args);
        if (command !== "agent_view" || !value || typeof value !== "object") return value;
        const block = "This request was blocked by our safety systems. Reason: Potentially unintended activity.";
        const session = Reflect.get(value, "session");
        return { ...value, session: { ...session, status: "idle" }, entries: [{ kind: "user", text: "Push after merge" }, { kind: "agent", text: `The changes are committed.${block}`, html: `<p>The changes are <strong>committed</strong>.${block}</p>` }] } as T;
      };
    });
    await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false } }));
    const error = page.getByRole("alert");
    await expect(error).toBeVisible();
    await expect(error.locator("b")).toHaveText("Conversation blocked");
    await expect(error.locator("span")).toContainText("Potentially unintended activity");
    await expect(error.locator("span")).toHaveCSS("color", "rgb(235, 87, 87)");
    await expect(page.locator(".dcoderen.agent .reply-body")).toHaveText("The changes are committed.");
    await expect(page.locator(".dcoderen.agent strong")).toHaveText("committed");
    expect(errors).toEqual([]);
  });
}
