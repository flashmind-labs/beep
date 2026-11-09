import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = {
  machine_id: "build-box", machine_name: "build-box", id: "imported-session", agent: "codex",
  title: "Imported conversation", cwd: "/srv/project", status: "ended", external: true,
  doing: null, question: null, started_at: 1, last_activity: 1,
};
const view = (row = session) => ({
  session: row, model: null, plan: [], files: [], entries: Array.from({ length: 40 }, (_, i) => ({ kind: i % 2 ? "assistant" : "user", text: `Message ${i}\n${"Conversation text. ".repeat(30)}` })),
  earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 20,
});

test("an imported session with an unknown model can choose before continuing", async ({ page }) => {
  const resumed = { ...session, id: "resumed-session", external: false, status: "idle" };
  await openDesktop(page, false, {
    agent_view: view(), agent_resume: { session: resumed, why: "" }, agent_steer: [],
    agent_models: { pick: "listed", current: null, models: [{ id: "test-model", name: "Test model", description: null }] },
  });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  await expect(page.locator("#agmodelb")).toBeEnabled();
  await expect(page.locator("#agmodelb")).toContainText("Model unknown");
  await page.locator("#agmodelb").click();
  await page.locator('[data-agmo="test-model"]').click();
  await expect(page.locator("#agmodelb")).toContainText("Test model");
  expect(await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer"))).toEqual([]);
  await page.locator("#agmsg").fill("Continue with this model");
  await page.locator("#agmsg").press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer").length)).toBe(2);
  const calls = await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_resume" || c.command === "agent_steer"));
  expect(calls.map(c => c.command)).toEqual(["agent_resume", "agent_steer", "agent_steer"]);
  expect(calls[0].args.prompt).toBeNull();
  expect(calls[1].args).toMatchObject({ session: "resumed-session", steer: { configure: { model: "test-model", effort: null } } });
  expect(calls[2].args).toMatchObject({ session: "resumed-session", steer: { prompt: { text: "Continue with this model" } } });
});

test("touch scrolling releases the log before the next animation frame", async ({ page }) => {
  const active = { ...session, status: "working", external: false };
  await openDesktop(page, false, { agent_view: view(active) });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
  const body = page.locator("#agbody");
  const details = page.locator("#agshortcuts"), summary = details.locator("summary");
  await summary.click();
  await summary.focus();
  await details.evaluate(el => Reflect.set(window, "sessionHintsDisclosure", el));
  await expect(body).toContainText("Message 39");
  await expect.poll(() => body.evaluate(el => el.scrollHeight > el.clientHeight)).toBe(true);
  const state = await body.evaluate((el: HTMLElement & { stuck?: boolean; touching?: boolean }) => {
    el.scrollTop = el.scrollHeight;
    el.dispatchEvent(new Event("scroll"));
    el.dispatchEvent(new Event("touchstart"));
    el.dispatchEvent(new Event("touchmove"));
    el.scrollTop -= 200;
    el.dispatchEvent(new Event("scroll"));
    return { stuck: el.stuck, touching: el.touching, top: el.scrollTop, height: el.clientHeight, total: el.scrollHeight };
  });
  expect(state.stuck).toBe(false);
  expect(state.touching).toBe(true);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_view").length);
  await page.evaluate(next => window.__navigation.respond("agent_view", next), {
    ...view(active), entries: [...view(active).entries, { kind: "assistant", text: "New reply while reading older messages" }],
  });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_view").length)).toBeGreaterThan(reads);
  await expect.poll(() => body.evaluate((el: HTMLElement & { stuck?: boolean }) => el.stuck)).toBe(false);
  expect(await details.evaluate(el => el === Reflect.get(window, "sessionHintsDisclosure") && el.hasAttribute("open"))).toBe(true);
  await expect(summary).toBeFocused();
  await body.dispatchEvent("touchend");
  await expect.poll(() => body.evaluate((el: HTMLElement & { touching?: boolean }) => el.touching)).toBe(false);
  await body.evaluate(el => { el.scrollTop = el.scrollHeight; el.dispatchEvent(new Event("scroll")); });
  await expect.poll(() => body.evaluate((el: HTMLElement & { stuck?: boolean }) => el.stuck)).toBe(true);
});

for (const custom of [
  { id: "gpt-6.1-sol", label: "6.1 Sol", models: [{ id: "gpt-6.1-sol", name: "6.1 Sol", description: null }] },
  { id: "custom-model", label: "custom-model", models: [] },
]) {
  test(`a session without a reported model shows the custom default ${custom.id}`, async ({ page }) => {
    const active = { ...session, status: "working", external: false };
    await openDesktop(page, false, {
      agent_view: view(active),
      agent_models: { pick: "listed", current: custom.id, models: custom.models },
    });
    await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
    await expect(page.locator("#agmodelb")).toContainText(custom.label);
    await expect(page.locator("#agmodelb")).not.toContainText("Model unknown");
    expect(await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer"))).toEqual([]);
  });
}

test("a reported session model takes precedence over the configured default", async ({ page }) => {
  const active = { ...session, status: "working", external: false };
  await openDesktop(page, false, {
    agent_view: { ...view(active), model: "session-model" },
    agent_models: { pick: "listed", current: "custom-model", models: [{ id: "session-model", name: "Session model", description: null }] },
  });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), active);
  await expect(page.locator("#agmodelb")).toContainText("Session model");
});
