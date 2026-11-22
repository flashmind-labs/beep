import { expect, test, type Page, type TestInfo } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const host = { machine_id: "build-box", machine_name: "build-box", agents: ["claude", "flsh"], workspace: null, home: "/srv", capabilities: [], assistant: true, moves_chats: true };
const folder = "/srv/.beep/assistant/chat-one";
const chat = { machine_id: "build-box", machine_name: "build-box", id: "chat-one", agent: "flsh", title: "Plan the launch", cwd: folder, status: "idle", doing: null as string | null, started_at: 1, last_activity: 5, external: false, question: null, assistant: true, parent: null as { machine_id: string; id: string; key: string } | null };
const under = { machine_id: "build-box", id: "chat-one", key: "" };
const research = { ...chat, id: "thread-one", agent: "claude", title: "Pricing research", cwd: `${folder}/pricing`, status: "working", doing: "Reading the price list", started_at: 2, assistant: false, parent: under };
const notes = { ...chat, id: "thread-two", agent: "claude", title: "Competitor notes", cwd: `${folder}/notes`, status: "ended", started_at: 3, last_activity: 4, assistant: false, parent: under };
const view = (row: typeof chat) => ({ session: row, plan: [], files: [], entries: [{ kind: "agent", text: "On it." }], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 1 });

async function openChat(page: Page, sessions: (typeof chat)[]) {
  await openDesktop(page, false, { agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null, fast: null }, agent_sessions: sessions, agent_view: view(chat) });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  await page.locator(".asst-list").getByRole("button", { name: /Plan the launch/ }).click();
}

const mod = (info: TestInfo) => info.project.name === "Windows" ? "Control" : "Meta";

test("a chat with threads lists them in place of the sidebar, and ⌘E swaps them", async ({ page }, info) => {
  await openChat(page, [chat, research, notes]);
  const rail = page.locator("#rail");

  await expect(page.locator(".cwin")).toHaveClass(/rail-on/);
  await expect(rail.locator(".rail-live .pj-li")).toHaveText([/Plan the launch/, /Pricing research.*Reading the price list/]);
  // The running threads are the chat's subitems, indented under it.
  await expect(rail.locator(".rail-threads .pj-li")).toHaveText([/Pricing research/]);
  expect(await rail.locator(".rail-threads .pj-li").evaluate(el => el.getBoundingClientRect().left)).toBeGreaterThan(await rail.locator(".rail-live > .pj-li").first().evaluate(el => el.getBoundingClientRect().left));
  await expect(rail.locator(".rail-scroll > .pj-li")).toHaveText([/Competitor notes/]);
  await expect(rail.locator('[aria-current="true"]')).toContainText("Plan the launch");
  await expect(page.locator(".asst-list")).toBeHidden();

  await page.evaluate(() => window.__navigation.respond("agent_view", null));
  await rail.getByRole("button", { name: /Pricing research/ }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_view").at(-1)?.args.session)).toBe("thread-one");
  await expect(rail).toBeVisible();

  await page.keyboard.press(`${mod(info)}+e`);
  await expect(page.locator(".cwin")).not.toHaveClass(/rail-on/);
  await expect(page.locator("#side")).toBeVisible();
  await expect(page.locator(".asst-list")).toBeVisible();
});

test("a chat without threads keeps the chats beside it", async ({ page }) => {
  await openChat(page, [chat]);
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator(".cwin")).not.toHaveClass(/rail-on/);
});

test("⌘N in a chat starts a new chat and ⇧⌘N asks it for a new thread", async ({ page }, info) => {
  await openChat(page, [chat, research]);
  await expect(page.locator("#agmsg")).toBeVisible();

  await page.keyboard.press(`${mod(info)}+Shift+n`);
  await expect(page.locator("#agmsg")).toHaveValue(/^New thread: /);

  await page.keyboard.press(`${mod(info)}+n`);
  await expect(page.locator("#agprompt")).toHaveAttribute("placeholder", "Ask the assistant.");
});
