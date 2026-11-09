import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("session title keeps its draft when saving fails and retries", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const title = page.getByRole("button", { name: "Rename session" });
  await title.focus();
  await page.keyboard.press("Enter");
  const input = page.getByRole("textbox", { name: "Session title" });
  await expect(input).toBeFocused();
  await input.fill("New title");
  await page.evaluate(() => window.__navigation.rejectOnce("agent_title_save"));
  await input.press("Enter");
  await expect(page.getByRole("alert")).toContainText("Requested test failure");
  await expect(input).toHaveValue("New title");
  await expect(input).toBeFocused();
  await input.press("Enter");
  await expect(input).toHaveCount(0);
  await expect(title).toBeFocused();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_title_save").at(-1)?.args)).toMatchObject({ title: "New title" });
});
