import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const outside = {
  machine_id: "build-box", machine_name: "build-box", id: "outside-session", agent: "claude",
  title: "Started in a terminal", cwd: "/srv/project", status: "working", external: true,
  doing: null, question: null, started_at: 1, last_activity: 1,
};
const view = (status: string) => ({
  session: { ...outside, status }, model: null, plan: [], entries: [{ kind: "user", text: "Fix the parser" }],
  earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1,
});
const closes = (page: import("@playwright/test").Page) => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_close_external"));

test("an idle session closes from the confirm sheet, which shows why it could not", async ({ page }) => {
  await openDesktop(page, false, { agent_view: view("waiting") });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), outside);
  await page.evaluate(() => window.__navigation.rejectOnce("agent_close_external"));
  await page.locator("#agcontinue").click();
  const confirm = page.locator(".am-sheet").getByRole("button", { name: "Continue here" });
  await confirm.click();
  await expect(page.locator(".am-sheet")).toContainText("Requested test failure");

  await confirm.click();
  await expect(page.locator(".am-sheet")).toHaveCount(0);
  expect((await closes(page)).length).toBe(2);
});
