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

test("voice dictation turns on, lists only live models and points to the key in Credentials", async ({ page }) => {
  await openDesktop(page, false, {
    ...responses,
    list_credentials: [{ id: "k1", kind: "api_key", provider: "mistral", last4: "9c12", account: null, env_var: "MISTRAL_API_KEY", created_at: 0 }],
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const toggle = settings.getByRole("switch", { name: "Voice dictation" });
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await expect(page.locator("#dictationModel")).toHaveCount(0);
  await toggle.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_settings_save" && JSON.stringify(call.args.settings) === JSON.stringify({ enabled: true, model: null })))).toBe(true);
  await expect(settings.getByText("Live via Mistral · mic button or hold Option in composers")).toBeVisible();
  const chip = settings.locator(".dgchip", { hasText: "Mistral key …9c12" });
  await expect(chip).toBeVisible();
  const triggerBox = await page.locator("#dictationModel").boundingBox();
  const chipBox = await chip.boundingBox();
  expect(triggerBox && triggerBox.height <= 28).toBe(true);
  expect(chipBox && chipBox.width < 200).toBe(true);

  await page.locator("#dictationModel").click();
  const menu = page.getByRole("menu", { name: "Dictation model" });
  await expect(menu).toBeVisible();
  await expect(menu.getByText("On this computer")).toBeVisible();
  await expect(menu.getByText("Cloud live API")).toBeVisible();
  await expect(menu.getByText("OpenRouter")).toHaveCount(0);

  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(settings).toBeVisible();

  await page.locator("#dictationModel").click();
  await page.getByRole("menuitemradio", { name: /Scribe v3 Live/ }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_settings_save" && JSON.stringify(call.args.settings) === JSON.stringify({ enabled: true, model: "elevenlabs:scribe_v3_live" })))).toBe(true);
  const scribeBox = await page.locator("#dictationModel").boundingBox();
  expect(scribeBox && scribeBox.height <= 28).toBe(true);
  await expect(settings.getByText("Add your ElevenLabs API key in Credentials to start dictating")).toBeVisible();
  await settings.getByRole("button", { name: "Add API key…" }).click();
  await expect(settings.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Credentials", exact: true })).toHaveAttribute("aria-current", "true");
});

test("local dictation models download on demand and are ready immediately when already on disk", async ({ page }) => {
  await openDesktop(page, false, {
    ...responses,
    dictation_settings_get: { enabled: true, model: "local:parakeet-unified" },
    dictation_local_models: [
      { id: "local:parakeet-unified", bytes: 731_357_568, installed: false, downloading: false, downloaded_bytes: 0 },
      { id: "local:nemotron-3.5-asr", bytes: 751_094_240, installed: true, downloading: false, downloaded_bytes: 751_094_240 },
    ],
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });

  const download = settings.getByRole("button", { name: "Download (731 MB)" });
  await expect(download).toBeVisible();
  await expect(settings.getByRole("button", { name: "Add API key…" })).toHaveCount(0);

  await download.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_model_download" && call.args.model === "local:parakeet-unified"))).toBe(true);

  await page.evaluate(() => {
    const call = window.__navigation.calls().findLast(item => item.command === "dictation_model_download");
    const channel = call?.args.onEvent as { onmessage(event: unknown): void } | undefined;
    channel?.onmessage({ kind: "progress", downloaded_bytes: 314_483_754, total_bytes: 731_357_568 });
  });
  await expect(settings.getByLabel("Download progress 43%")).toBeVisible();
  await expect(settings.getByRole("button", { name: "Cancel" })).toBeVisible();

  await page.evaluate(() => {
    const call = window.__navigation.calls().findLast(item => item.command === "dictation_model_download");
    const channel = call?.args.onEvent as { onmessage(event: unknown): void } | undefined;
    channel?.onmessage({ kind: "done" });
  });
  await expect(settings.getByText("On device · 731 MB")).toBeVisible();
  await expect(settings.getByRole("button", { name: "Download (731 MB)" })).toHaveCount(0);

  await page.locator("#dictationModel").click();
  await page.getByRole("menuitemradio", { name: /Nemotron 3\.5/ }).click();
  await expect(settings.getByText("On device · 751 MB")).toBeVisible();
  await expect(settings.getByRole("button", { name: /Download/ })).toHaveCount(0);
});
