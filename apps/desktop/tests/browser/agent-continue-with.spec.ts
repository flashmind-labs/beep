import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const base = { machine_id: "build-box", machine_name: "build-box", cwd: "/srv/project", external: false, doing: null, question: null, started_at: 1, last_activity: 1, assistant: false, persona: null };
const earlier = { ...base, id: "early", agent: "claude", title: "Plan the work", status: "ended" };
const later = { ...base, id: "late", agent: "codex", title: "Plan the work", status: "idle", continues: "early" };
const host = { machine_id: "build-box", machine_name: "build-box", agents: ["claude", "codex"], workspace: null, home: null, capabilities: [], assistant: false, moves_chats: true };
const view = (session: typeof earlier | typeof later) => ({
  session, model: null, plan: [], entries: [{ kind: "user", text: "Hello" }, { kind: "assistant", text: "Hi" }],
  earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1,
});
const models = { pick: "listed", current: null, models: [{ id: "test-model", name: "Test model", description: null }] };

test("a chat that continues another links back to it, and the earlier one links forward", async ({ page }) => {
  await openDesktop(page, false, { agent_sessions: [earlier, later], agent_view: view(later), agent_hosts: [host] });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), later);
  const line = page.locator(".dcontinued button");
  await expect(line).toHaveText("Continued from Plan the work · Claude Code");

  await page.evaluate(next => window.__navigation.respond("agent_view", next), view(earlier));
  await line.click();
  await expect(page.locator(".dcontinued button")).toHaveText("Continued in Codex");
});

test("a model of another harness waits for the next message instead of switching in place", async ({ page }) => {
  await openDesktop(page, false, { agent_sessions: [earlier], agent_view: view({ ...earlier, status: "idle" }), agent_hosts: [host], agent_steer: [], agent_models: models });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), { ...earlier, status: "idle" });
  await page.locator("#agmodelb").click();
  await page.locator('[data-agh="codex"]').first().click();
  await page.locator('[data-agmo="test-model"]').click();
  await expect(page.locator("#agmodelb")).toContainText("Test model");
  await expect(page.locator("#agmodelb")).toHaveAttribute("title", /continues this chat with/);
  const steers = await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer"));
  expect(steers).toEqual([]);
});

test("the next message starts the chat under the harness picked, naming the one it continues", async ({ page }) => {
  const idle = { ...earlier, status: "idle" };
  const started = { ...base, id: "new", agent: "codex", title: null, status: "starting", continues: "early" };
  await openDesktop(page, false, { agent_sessions: [idle], agent_view: view(idle), agent_hosts: [host], agent_models: models, agent_begin: { session: started, why: "" } });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), idle);
  await page.locator("#agmodelb").click();
  await page.locator('[data-agh="codex"]').click();
  await page.locator('[data-agmo="test-model"]').click();
  await page.locator("#agmsg").fill("Carry on");
  await page.locator("#agmsg").press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_begin").map(c => c.args))).toMatchObject([
    { machineId: "build-box", agent: "codex", cwd: "/srv/project", model: "test-model", continues: "early" },
  ]);
});

test("an attached file uploads to the chat's folder and goes with the continued message", async ({ page }) => {
  const idle = { ...earlier, status: "idle" };
  const started = { ...base, id: "new", agent: "codex", title: null, status: "starting", continues: "early" };
  await openDesktop(page, false, { agent_sessions: [idle], agent_view: view(idle), agent_hosts: [host], agent_models: models, agent_begin: { session: started, why: "" }, agent_tree_upload: "/srv/project/.cache/notes.txt" });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), idle);
  await page.locator("#agmodelb").click();
  await page.locator('[data-agh="codex"]').click();
  await page.locator('[data-agmo="test-model"]').click();
  await page.locator('#agfield input[type="file"]').setInputFiles({ name: "notes.txt", mimeType: "text/plain", buffer: Buffer.from("remember this") });
  await expect(page.locator("#agfield .attachment-count")).toHaveText("1 file");
  await page.locator("#agmsg").fill("Carry on");
  await page.locator("#agmsg").press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_begin").map(c => c.args))).toMatchObject([
    { machineId: "build-box", agent: "codex", cwd: "/srv/project", continues: "early", prompt: expect.stringContaining("/srv/project/.cache/notes.txt") },
  ]);
  const uploads = await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_tree_upload").map(c => c.args));
  expect(uploads).toMatchObject([{ machineId: "build-box", root: "/srv/project", path: "notes.txt", size: 13 }]);
});
