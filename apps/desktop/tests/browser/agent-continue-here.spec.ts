import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const outside = {
  machine_id: "build-box", machine_name: "build-box", id: "outside-session", agent: "claude",
  title: "Started in a terminal", cwd: "/srv/project", status: "working", external: true,
  doing: null, question: null, started_at: 1, last_activity: 1,
};
const view = (status: string) => ({
  session: { ...outside, status }, model: null, plan: [], files: [], entries: [{ kind: "user", text: "Fix the parser" }],
  earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 1,
});
const closes = (page: import("@playwright/test").Page) => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_close_external"));

test("Continue here waits for the running turn, then closes the session outside Beep", async ({ page }) => {
  await openDesktop(page, false, { agent_view: view("working") });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), outside);
  const button = page.locator("#agcontinue");
  await expect(page.locator(".dtakeover")).toContainText("Running outside Beep.");
  await expect(button).toHaveText("Continue here");

  await button.click();
  await expect(page.locator(".am-sheet")).toContainText("Claude Code closes in the terminal it runs in");
  await page.locator(".am-sheet").getByRole("button", { name: "Continue here" }).click();
  await expect(button).toHaveText("Waiting for the turn to end");
  await expect(button).toBeDisabled();
  await page.locator(".dtakeover").getByRole("button", { name: "Cancel" }).click();
  await expect(button).toHaveText("Continue here");

  await button.click();
  await page.locator(".am-sheet").getByRole("button", { name: "Continue here" }).click();
  await expect(button).toHaveText("Waiting for the turn to end");
  expect(await closes(page)).toEqual([]);

  await page.evaluate(next => window.__navigation.respond("agent_view", next), view("waiting"));
  await expect.poll(() => closes(page)).toEqual([{ command: "agent_close_external", args: { machineId: "build-box", agent: "claude", session: "outside-session" } }]);

  await page.evaluate(next => window.__navigation.respond("agent_view", next), view("ended"));
  await expect(page.locator("#agmsg")).toBeVisible();
  await expect(page.locator(".dtakeover")).toHaveCount(0);
});

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
