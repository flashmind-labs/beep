import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const machineId = "build-box";
const machine = { id: machineId, name: machineId, online: true, can_host: true, seen_at: 1_000, version: "0.9.0", env: null, apps: [], gpus: [], capabilities: [], agents: 2, stats: null };
const project = { name: "example/widget", url: "https://example.test/example/widget", machine_id: machineId, machine_name: machineId, folder: "/srv/example/widget", cloned: true };

test("Home Review deck shows diff counts, sends quick replies, and closes sessions", async ({ page }) => {
  const idle = {
    kind: "agent", machine_id: machineId, machine_name: machineId, id: "idle-one", agent: "codex",
    title: "Refactor token parser", cwd: project.folder, status: "idle", doing: null,
    reply_html: "Tests pass.", changes: { additions: 42, deletions: 11 },
    started_at: 1_000, last_activity: 2_000, external: false, question: null,
  };
  await openDesktop(page, false, { center_home: { machines: [machine], projects: [project], running: [idle], needs_you: [] }, agent_steer: [], agent_close: null });
  await page.getByRole("radio", { name: "Review" }).click();

  const deck = page.locator(".bd-deck");
  await expect(deck).toContainText("Refactor token parser");
  await expect(deck.locator(".rail-diff")).toContainText("+42");
  await expect(deck.locator(".rail-diff")).toContainText("-11");

  const reply = deck.getByRole("textbox", { name: "Quick reply" });
  await reply.fill("Please squash and push");
  await deck.getByRole("button", { name: "Send" }).click();

  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer").map(c => c.args))).toEqual([
    expect.objectContaining({ machineId, session: "idle-one", steer: { prompt: { text: "Please squash and push" } } }),
  ]);
  await expect(reply).toHaveValue("");

  await deck.getByRole("button", { name: "Close" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(c => c.command === "agent_steer" && c.args.session === "idle-one" && c.args.steer === "close"))).toBe(true);
  await expect(deck).toBeHidden();
});

test("palette runs new navigation and shortcut actions", async ({ page }) => {
  const index = {
    actions: [
      { kind: "action", id: "terminals", title: "Terminals", detail: "", keys: ["terminals", "shell"] },
      { kind: "action", id: "shortcuts", title: "Keyboard shortcuts", detail: "", keys: ["keyboard shortcuts", "keys"] },
    ],
    settings: [], code: [], projects: [], sessions: [], machines: [],
  };
  await openDesktop(page, false, { center_home: { machines: [machine], projects: [project], running: [], needs_you: [] }, center_palette_index: index, center_find_prompts: [], terminals_all: [] });

  await page.getByRole("button", { name: /^Search/ }).click();
  const dialog = page.getByRole("dialog", { name: "Search" });
  await dialog.getByRole("combobox").fill("shell");
  await expect(dialog.getByRole("option").first()).toContainText("Terminals");
  await page.keyboard.press("Enter");
  await expect(page.locator("#title")).toHaveText("Terminals");

  await page.keyboard.press(test.info().project.name === "Windows" ? "Control+k" : "Meta+k");
  await dialog.getByRole("combobox").fill("keyboard");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("dialog", { name: /Keyboard shortcuts/i })).toBeVisible();
});

test("Open beside mounts a second session in the right dock and sends messages", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const first = { kind: "agent", machine_id: machineId, machine_name: machineId, id: "sess-a", agent: "codex", title: "Primary session", cwd: project.folder, status: "idle", doing: null, started_at: 1_000, last_activity: 2_000, external: false, question: null, assistant: false, persona: null };
  const second = { ...first, id: "sess-b", title: "Secondary session", agent: "claude" };
  const viewA = { session: first, plan: [], entries: [{ kind: "agent", text: "Primary ready." }], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1, usage: null };
  const viewB = { session: second, plan: [], entries: [{ kind: "agent", text: "Secondary output here." }], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1, usage: null };

  await openDesktop(page, false, {
    center_home: { machines: [machine], projects: [project], running: [first, second], needs_you: [] },
    agent_sessions: [first, second],
    project_sessions: [first, second],
    agent_views: { "sess-a": viewA, "sess-b": viewB },
    agent_steer: [],
  });

  await page.evaluate(target => window.__navigation.open({ agent: target }), first);
  await expect(page.getByText("Primary ready.")).toBeVisible();

  const secondRow = page.locator("#rail").getByRole("button", { name: /Secondary session/ });
  await expect(secondRow).toBeVisible();
  await secondRow.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Open beside" }).click();

  const split = page.locator("#agpane-right .agsplit");
  await expect(split).toBeVisible();
  await expect(split).toContainText("Secondary output here.");

  const box = split.getByRole("textbox", { name: /Message Secondary session/ });
  await box.fill("Check edge cases");
  await box.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer" && c.args.session === "sess-b").map(c => c.args.steer))).toEqual([
    { prompt: { text: "Check edge cases" } },
  ]);

  await split.getByRole("button", { name: "Open main" }).click();
  await expect(page.locator(".ag-head")).toContainText("Secondary session");
});

test("a split session on a node without the follow stream reads it once and retries the follow", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const first = { kind: "agent", machine_id: machineId, machine_name: machineId, id: "sess-a", agent: "codex", title: "Primary session", cwd: project.folder, status: "idle", doing: null, started_at: 1_000, last_activity: 2_000, external: false, question: null, assistant: false, persona: null };
  const second = { ...first, id: "sess-b", title: "Secondary session", agent: "claude", status: "working" };
  const view = (session: typeof first, text: string) => ({ session, plan: [], entries: [{ kind: "agent", text }], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1, usage: null });

  await openDesktop(page, false, {
    center_home: { machines: [machine], projects: [project], running: [first, second], needs_you: [] },
    agent_sessions: [first, second],
    project_sessions: [first, second],
    agent_views: { "sess-a": view(first, "Primary ready."), "sess-b": view(second, "Secondary output here.") },
  });

  await page.evaluate(target => window.__navigation.open({ agent: target }), first);
  await page.locator("#rail").getByRole("button", { name: /Secondary session/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Open beside" }).click();
  await expect(page.locator("#agpane-right .agsplit")).toContainText("Secondary output here.");

  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_follow" && c.args.session === "sess-b").length), { timeout: 10_000 }).toBeGreaterThanOrEqual(2);
  expect(await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_view" && c.args.session === "sess-b").length)).toBe(1);
});

test("a split session follows the push stream and never reads the session", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification", cwd: "/srv/example/widget", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.locator("#rail").getByRole("button", { name: /Second notification/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Open beside" }).click();

  const split = page.locator("#agpane-right .agsplit");
  await expect(split).toContainText("No messages yet.");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_follow").map(c => c.args.session))).toContain("notice-two");
  expect(await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_view" && c.args.session === "notice-two").length)).toBe(0);
});

test("light skins render light card surfaces instead of dark foreground tints", async ({ page }) => {
  const running = [{ kind: "agent", machine_id: machineId, machine_name: machineId, id: "one", agent: "codex", title: "Light card check", cwd: project.folder, status: "working", doing: "Building", started_at: 1_000, last_activity: 2_000, external: false, question: null }];
  await page.addInitScript(() => localStorage.setItem("beep.skin", "paper"));
  await openDesktop(page, false, { center_home: { machines: [machine], projects: [project], running, needs_you: [] } });
  await page.getByRole("radio", { name: "Lanes" }).click();
  const card = page.locator(".bd-card").first();
  await expect(card).toBeVisible();

  const rgb = await card.evaluate(el => {
    const bg = getComputedStyle(el).backgroundColor;
    const m = bg.match(/\d+/g)?.map(Number) ?? [0, 0, 0];
    return m.slice(0, 3);
  });
  expect(rgb[0]).toBeGreaterThan(240);
  expect(rgb[1]).toBeGreaterThan(240);
  expect(rgb[2]).toBeGreaterThan(240);
});
