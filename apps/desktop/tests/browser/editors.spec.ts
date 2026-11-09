import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("Open in Zed reports a launch failure in the Codex session", async ({ page }) => {
  await openDesktop(page, true, {
    editors: [{ id: "zed", name: "Zed" }],
    session_folder_here: "/srv/project",
  });
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex",
    title: "First notification", cwd: "/srv/project", status: "working", doing: null,
    started_at: 1, last_activity: 1, external: false,
  } }));

  await page.evaluate(() => {
    const nativeInvoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => nativeInvoke<T>(command, args).then(value => {
      if (command === "open_in_editor") throw new Error("Cannot launch application");
      return value;
    });
  });

  await page.getByRole("button", { name: "Open in Zed", exact: true }).click();

  await expect(page.locator("#saved")).toContainText("Could not open Zed: Error: Cannot launch application");
  await expect(page.locator("#saved")).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "open_in_editor"))).toEqual([
    { command: "open_in_editor", args: { editor: "zed", path: "/srv/project" } },
  ]);
});
