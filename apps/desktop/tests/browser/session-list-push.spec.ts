import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const chat = { machine_id: "build-box", machine_name: "Build box", id: "chat-one", agent: "flsh", title: "Plan the launch", cwd: "/srv/chat", status: "idle", doing: null, started_at: 1, last_activity: 5, external: false, question: null, assistant: true, parent: null, model: null };
const view = { session: chat, plan: [], entries: [{ kind: "agent", text: "On it." }], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1 };
const reads = (page: import("@playwright/test").Page) => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_sessions").length);

async function waitingForChat(page: import("@playwright/test").Page) {
  await openDesktop(page, false, { center_window_alone: { kind: "chat", machine_id: chat.machine_id, session: chat.id }, agent_sessions: [], agent_view: view }, "chat-window");
  await expect(page.getByText("Waiting for the machine to list this chat…")).toBeVisible();
}

test("a session list that waits on a move answer still draws", async ({ page }) => {
  await waitingForChat(page);
  await page.evaluate(row => {
    window.__navigation.hold("session_moves");
    window.__navigation.respond("agent_sessions", [row]);
    window.__navigation.emit("reconnected", null);
  }, chat);
  await expect(page.locator("#agtitle")).toHaveText(chat.title);
  await page.evaluate(() => window.__navigation.release("session_moves"));
});

test("a chat that is not listed yet opens from the pushed list and reads nothing on a timer", async ({ page }) => {
  await page.clock.install();
  await waitingForChat(page);
  const before = await reads(page);
  await page.clock.fastForward(60_000);
  expect(await reads(page)).toBe(before);

  await page.evaluate(row => {
    window.__navigation.respond("agent_sessions", [row]);
    window.__navigation.notify("home");
  }, chat);
  await expect(page.locator("#agtitle")).toHaveText(chat.title);
  expect(await reads(page)).toBe(before);
});
