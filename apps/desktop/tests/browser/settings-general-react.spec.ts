import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const responses = { settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false, developer: false }, mac_control: { id: "one", name: "build-box", enabled: false, status: "off", error: null, permissions: { screen_recording: false, accessibility: false } }, list_machines: [], skill_sync_get: true, project_sync_get: true, docker_containers_get: true, dictation_settings_get: { enabled: false, model: null } };
test("skill sync preserves state and serializes a save across Settings reopening", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const toggle = settings.getByRole("switch", { name: "Sync coding agent skills" });
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await page.evaluate(() => window.__navigation.hold("skill_sync_set"));
  await toggle.click();
  await expect(toggle).toBeDisabled();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "skill_sync_set" && call.args.enabled === false))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  await expect(toggle).toBeDisabled();
  await page.evaluate(() => window.__navigation.release("skill_sync_set"));
  await expect(toggle).toBeEnabled();
  await expect(toggle).toHaveAttribute("aria-checked", "false");
});

test("general settings use their native commands and retain focus on refresh", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const toggle = settings.getByRole("switch", { name: "Coding agent notifications" });
  await toggle.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_agent_notices" && call.args.enabled === true))).toBe(true);
  await expect(toggle).toBeEnabled();
  await settings.getByRole("switch", { name: "Update machines automatically" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_node_auto_update" && call.args.enabled === true))).toBe(true);
  await settings.getByRole("switch", { name: "Developer mode" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_developer_mode" && call.args.enabled === true))).toBe(true);
  const skill = settings.getByRole("switch", { name: "Sync coding agent skills" });
  await expect(skill).toBeEnabled();
  await skill.focus();
  await skill.evaluate(el => Reflect.set(window, "skillToggle", el));
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length);
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(reads);
  expect(await skill.evaluate(el => el === Reflect.get(window, "skillToggle") && document.activeElement === el)).toBe(true);
});

test("skill sync retries an unsuccessful read", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.evaluate(() => window.__navigation.rejectOnce("skill_sync_get"));
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const toggle = settings.getByRole("switch", { name: "Sync coding agent skills" });
  await expect(settings.getByRole("alert")).toContainText("Requested test failure");
  await expect(toggle).toBeDisabled();
  await settings.getByRole("button", { name: "Retry" }).click();
  await expect(toggle).toBeEnabled();
  await expect(toggle).toHaveAttribute("aria-checked", "true");
});

test("phone General hides what belongs to desktops", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.evaluate(() => { Reflect.set(window, "__BEEP_PHONE__", true); });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.getByRole("button", { name: "Report…" })).toBeVisible();
  for (const name of ["Launch at login", "Sync coding agent skills", "Project sync", "Always use Beep sync over Syncthing", "Developer mode"]) {
    await expect(settings.getByRole("switch", { name })).toHaveCount(0);
  }
  await expect(settings.getByRole("button", { name: "Show…" })).toHaveCount(0);
});


test("project sync stays serialized across reopening and rereads partial failures", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const toggle = settings.getByRole("switch", { name: "Project sync", exact: true });
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await page.evaluate(() => window.__navigation.hold("project_sync_set"));
  await toggle.click();
  await expect(toggle).toBeDisabled();
  await page.keyboard.press("Escape");
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  await expect(toggle).toBeDisabled();
  await page.evaluate(() => window.__navigation.release("project_sync_set"));
  await expect(toggle).toBeEnabled();
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await expect(settings.getByText("Paused for every project on all machines. Turn on to reconcile your projects.")).toBeVisible();
  await page.evaluate(() => {
    window.__navigation.respond("project_sync_get", true);
    window.__navigation.rejectOnce("project_sync_set");
  });
  await toggle.click();
  await expect(settings.getByRole("alert")).toContainText("Requested test failure");
  await expect(toggle).toHaveAttribute("aria-checked", "true");
  await expect(toggle).toBeEnabled();
});

test("voice dictation turns on, picks a model and points to the key in Credentials", async ({ page }) => {
  await openDesktop(page, false, { ...responses, list_credentials: [{ id: "k1", kind: "api_key", provider: "openrouter", last4: "9c12", account: null, env_var: "OPENROUTER_API_KEY", created_at: 0 }] });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const toggle = settings.getByRole("switch", { name: "Voice dictation" });
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await expect(settings.getByRole("combobox", { name: "Dictation model" })).toHaveCount(0);
  await toggle.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_settings_save" && JSON.stringify(call.args.settings) === JSON.stringify({ enabled: true, model: null })))).toBe(true);
  await expect(settings.getByText("OpenRouter · default model · mic button or hold Option in prompt composers")).toBeVisible();
  await expect(settings.getByText("OpenRouter key …9c12")).toBeVisible();
  await settings.getByRole("combobox", { name: "Dictation model" }).selectOption("elevenlabs:scribe_v3_live");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_settings_save" && JSON.stringify(call.args.settings) === JSON.stringify({ enabled: true, model: "elevenlabs:scribe_v3_live" })))).toBe(true);
  await expect(settings.getByText("Add your ElevenLabs API key in Credentials to start dictating")).toBeVisible();
  await settings.getByRole("button", { name: "Add API key…" }).click();
  await expect(settings.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Credentials", exact: true })).toHaveAttribute("aria-current", "true");
});
