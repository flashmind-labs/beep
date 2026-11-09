import { expect, test } from "@playwright/test";
import type { HomeView } from "../../src/center/home-state.ts";
import { openDesktop } from "./harness.ts";

const status = { link: "linked", signed_out: false, server: true, live: false, session: null };
const home: HomeView = { machines: [], projects: [], running: [], needs_you: [], coming_up: null, updates: [] };

test("Home updates retain the composer draft, selection and form", async ({ page }) => {
  await openDesktop(page, false, { status, center_home: home });
  const input = page.getByRole("textbox", { name: "Ask Beep", exact: true });
  await input.fill("Draft to keep");
  await input.evaluate(element => {
    Reflect.set(window, "homeInput", element);
    if (element instanceof HTMLInputElement) element.setSelectionRange(2, 6);
  });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length);
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, { ...home, updates: [{ id: 1, summary: "A new update", app: "app", app_name: "App", at: 1 }] });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length)).toBeGreaterThan(reads);
  await expect(input).toHaveValue("Draft to keep");
  await expect(input).toBeFocused();
  expect(await input.evaluate(element => element === Reflect.get(window, "homeInput") && element instanceof HTMLInputElement && element.selectionStart === 2 && element.selectionEnd === 6)).toBe(true);
  await expect(page.locator("#home-comp")).toHaveCount(1);
});

test("Home submit sends the trimmed prompt and clears its retained draft", async ({ page }) => {
  await openDesktop(page, false, { status, center_home: home, session_agents: [], session_files: [], session_sources: [], list_questions: [] });
  await page.evaluate(() => { const invoke = window.__TAURI__.core.invoke; window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => { if (command === "send_prompt") { Reflect.set(window, "homeSentPrompt", args?.message); return new Promise<T>(() => {}); } return invoke<T>(command, args); }; });
  const input = page.getByRole("textbox", { name: "Ask Beep", exact: true });
  await input.fill("  Start from Home  ");
  await input.press("Enter");
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "homeSentPrompt"))).toBe("Start from Home");
  await expect(page.locator("#ask")).toBeVisible();
  await expect(page.locator("#body")).toContainText("Start from Home");
  await page.evaluate(() => window.__navigation.open("home"));
  await expect(input).toHaveValue("");
});

test("Home submit restores the prompt and error when chat startup fails", async ({ page }) => {
  await openDesktop(page, false, { status, center_home: home });
  await page.evaluate(() => { const invoke = window.__TAURI__.core.invoke; window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => { if (command === "new_chat") throw new Error("Cannot start chat"); return invoke<T>(command, args); }; });
  const input = page.getByRole("textbox", { name: "Ask Beep", exact: true });
  await input.fill("Restore this prompt");
  await input.press("Enter");
  await expect(input).toBeVisible();
  await expect(input).toHaveValue("Restore this prompt");
  await expect(page.locator("#home-err")).toContainText("Cannot start chat");
});

test("Home Agent hands its draft to the agent composer", async ({ page }) => {
  const machine = { id: "build-box", name: "build-box", online: true, can_host: true, seen_at: 1, version: "1.0", env: null, joined: true, apps: [], gpus: [], capabilities: [], agents: 0, stats: null };
  await openDesktop(page, false, { status, center_home: { ...home, machines: [machine] }, machine_clocks: [], agent_hosts: [{ machine_id: machine.id, machine_name: machine.name, agents: ["codex"], workspace: "/srv/project", home: "/srv", capabilities: [] }] });
  const input = page.getByRole("textbox", { name: "Ask Beep", exact: true });
  await input.fill("  Investigate this project  ");
  await page.locator("#home-agent").click();
  await expect(page.getByRole("textbox", { name: "Task for a coding agent" })).toHaveValue("Investigate this project");
  await page.evaluate(() => window.__navigation.open("home"));
  await expect(input).toHaveValue("");
});
