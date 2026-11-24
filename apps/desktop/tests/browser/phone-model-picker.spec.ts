import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test.use({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } });

const session = {
  machine_id: "build-box", machine_name: "build-box", id: "s", agent: "codex",
  title: "Restyle the app", cwd: "/srv/project", status: "idle", external: false,
  doing: null, question: null, started_at: 1, last_activity: 1,
};
const view = {
  session, model: null, plan: [], entries: [{ kind: "user", text: "Hello" }, { kind: "assistant", text: "Hi" }],
  earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1,
};

test("a phone picks a model while typing in an agent conversation", async ({ page }) => {
  await openDesktop(page, false, {
    agent_view: view, agent_steer: [],
    agent_models: { pick: "listed", current: null, models: [{ id: "test-model", name: "Test model", description: null }] },
  });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  await expect(page.locator("body")).toHaveClass(/mobile/);
  await page.locator("#agmsg").tap();
  await page.locator("#agmsg").fill("Draft");
  await page.locator("#agmodelb").tap();
  await page.locator('[data-agmo="test-model"]').tap();
  await expect(page.locator("#agmodelb")).toContainText("Test model");
  await expect(page.locator("#agmsg")).toHaveValue("Draft");
});

test("typing on a phone leaves the chat the room the head and tabs took", async ({ page }) => {
  await openDesktop(page, false, { agent_view: view, agent_steer: [] });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  const chat = page.locator("#agconv .dbody"), height = async () => (await chat.boundingBox())?.height ?? 0;
  await expect(page.locator("#agmnav")).toBeVisible();
  const before = await height();

  // The keyboard is up: the app marks the body when the visual viewport shrinks.
  await page.locator("#agmsg").tap();
  await page.evaluate(() => document.body.classList.add("mb-kbd"));
  await expect(page.locator("#agmnav")).toBeHidden();
  await expect(page.locator("#agstate")).toBeHidden();
  await expect(page.locator(".mb-back")).toBeHidden();
  await expect(page.locator("#agtitle")).toBeVisible();
  expect(await height()).toBeGreaterThan(before + 60);

  await page.evaluate(() => { document.body.classList.remove("mb-kbd"); (document.activeElement as HTMLElement | null)?.blur(); });
  await expect(page.locator("#agmnav")).toBeVisible();
});
