import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const host = { machine_id: "build-box", machine_name: "build-box", agents: ["codex", "flsh"], workspace: null, home: "/srv", capabilities: [], assistant: true };
const session = { machine_id: "build-box", machine_name: "build-box", id: "assistant-one", agent: "flsh", title: null as string | null, cwd: "/srv", status: "working", doing: null, started_at: 1, last_activity: 1, external: false, question: null, assistant: true };

const view = (row: typeof session, text: string) => ({ session: row, plan: [], files: [], entries: [{ kind: "agent", text }], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 1 });

test("the sidebar's Assistant lists the chats and opens one beside them", async ({ page }) => {
  const other = { ...session, id: "assistant-two", title: "Check the open pull requests", status: "idle" };
  await openDesktop(page, false, {
    agent_hosts: [host], assistant_settings_get: { agent: "flsh", model: null, effort: null },
    agent_sessions: [{ ...session, title: "Set up the widget project" }, other],
    agent_view: view(other, "Two pull requests wait for review."),
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  const list = page.locator(".asst-list");
  await expect(list.locator(".pj-li")).toHaveCount(2);
  await expect(page.locator("#agprompt")).toHaveAttribute("placeholder", "Ask the assistant.");

  await list.getByRole("button", { name: /Check the open pull requests/ }).click();
  await expect(page.getByText("Two pull requests wait for review.")).toBeVisible();
  await expect(list).toBeVisible();
  await expect(list.locator('[aria-current="true"]')).toContainText("Check the open pull requests");
  await expect(page.locator("#side").getByRole("button", { name: "Assistant", exact: true })).toHaveAttribute("aria-current", "true");
});

test("Cmd+N on Home opens the Assistant page and starts the assistant with its harness in the home folder", async ({ page }, info) => {
  await openDesktop(page, false, {
    agent_hosts: [host],
    assistant_settings_get: { agent: "flsh", model: null, effort: "high" },
    agent_start: { session, why: "" },
    agent_view: { session, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 0 },
  });
  await expect(page.locator("#sec-needs")).toBeVisible();
  await page.keyboard.press(info.project.name === "Windows" ? "Control+n" : "Meta+n");

  const prompt = page.locator("#agprompt");
  await expect(prompt).toBeFocused();
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator("#title")).toContainText("Assistant");
  await expect(page.locator("#agpillb .flsh")).toBeVisible();
  await expect(page.locator("#agpillb")).toContainText("High");
  await expect(page.locator("#aghostb")).toHaveText("build-box");
  await expect(page.locator("#agfoldb")).toBeHidden();

  await prompt.fill("How is the widget project doing?");
  await prompt.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_start").map(call => call.args))).toEqual([
    // No machine picked: Auto picks one, this computer first.
    { machineId: null, agent: "flsh", cwd: null, prompt: "How is the widget project doing?", images: [], model: null, effort: "high", checkout: null, assistant: true },
  ]);
  // The new chat opens on the page, picked in its list.
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator("#agshell")).toBeVisible();
});

test.describe("on a phone", () => {
  test.use({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } });

  test("New assistant on Home opens the chats, and New chat the assistant's start form", async ({ page }) => {
    await openDesktop(page, false, { agent_hosts: [host], agent_sessions: [session], assistant_settings_get: { agent: "flsh", model: null, effort: null } });
    await expect(page.locator("body")).toHaveClass(/mobile/);
    await page.getByRole("button", { name: "New assistant" }).tap();
    const list = page.locator(".asst-list");
    await expect(list.locator(".pj-li")).toHaveCount(1);
    await expect(page.locator("#agprompt")).toBeHidden();
    await list.getByRole("button", { name: "New chat" }).tap();
    await expect(page.locator("#agprompt")).toBeVisible();
    await expect(list).toBeHidden();
    await expect(page.locator("#agpillb .flsh")).toBeVisible();
    await expect(page.locator("#agfoldb")).toBeHidden();
  });
});

test("the first start's harness shows the assistant setting saved in Settings", async ({ page }) => {
  const models = { pick: "listed", current: null, models: [{ id: "model-b", name: "Example Model B", description: null, provider: null }] };
  await openDesktop(page, false, {
    center_home: { machines: [], projects: [], running: [], needs_you: [] },
    settings: { name: "build-box", version: "0.9.0", autostart: false, agent_notices: false },
    agent_hosts: [host], agent_models: models, mac_control: { id: "build-box" },
    assistant_settings_get: { agent: "flsh", model: null, effort: null },
  });
  await expect(page.locator(".painted-empty .pill .flsh")).toBeVisible();

  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Assistant", exact: true }).click();
  await settings.locator("#assistantSetting").getByRole("button").click();
  // Saved in Settings: Home reads the setting again.
  await page.evaluate(() => window.__navigation.respond("assistant_settings_get", { agent: "codex", model: null, effort: null }));
  await page.getByRole("dialog", { name: "Harness, model and thinking" }).getByText("Example Model B").click();
  await expect(page.locator(".painted-empty .pill .codex")).toBeVisible();
});
