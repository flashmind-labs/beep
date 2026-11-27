import { expect, test } from "@playwright/test";
import type { HarnessView } from "../../src/stores/settings/providers-state.ts";
import { openDesktop } from "./harness.ts";

const harness: HarnessView = {
  agent: "claude", installed: true, runnable: true, version: "1.0", latest: null, program: "claude", home_var: "CLAUDE_CONFIG_DIR", home_default: "~/.claude",
  sign_in: { state: "signed_in", account: null, plan: null, last4: null }, install: null,
  settings: { enabled: true, program: "", home: "", args: [], env: [] },
};

test("shared compaction retains its draft on refresh and permits retry after failure", async ({ page }) => {
  await openDesktop(page, false, {
    settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
    list_machines: [{ id: "build", name: "build-box", online: true }],
    agent_harnesses: { machine_id: "build", machine_name: "build-box", checked_at: null, harnesses: [harness] },
    harness_settings_all: [], harness_compact: null, harness_model_compacts: [], harness_default_model: null, harness_driver: "acp", agent_models: null,
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const navigation = settings.getByRole("navigation", { name: "Settings pages" });
  await navigation.getByRole("button", { name: "Agents", exact: true }).click();
  await settings.getByRole("tab", { name: "Configuration", exact: true }).click();
  await settings.locator('#set-agent-overview [data-sub="agent:claude"]').click();
  const field = settings.getByRole("textbox", { name: "Auto-compact threshold" });
  await field.fill("70%");
  await field.evaluate(element => {
    Reflect.set(window, "sharedCompactField", element);
    if (element instanceof HTMLInputElement) element.setSelectionRange(1, 2);
  });
  const before = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length);
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(before);
  await expect(field).toHaveValue("70%");
  await expect(field).toBeFocused();
  expect(await field.evaluate(element => element === Reflect.get(window, "sharedCompactField") && element instanceof HTMLInputElement && element.selectionStart === 1 && element.selectionEnd === 2)).toBe(true);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "harness_compact").length)).toBe(1);
  const save = settings.getByRole("button", { name: "Save auto-compact", exact: true });
  await page.evaluate(() => window.__navigation.rejectOnce("harness_compact_save"));
  await save.click();
  await expect(settings.getByText("Requested test failure", { exact: true })).toBeVisible();
  await expect(save).toBeEnabled();
  await page.evaluate(() => window.__navigation.respond("harness_compact", { enabled: true, percent: 70, tokens: null }));
  await save.click();
  await expect(field).toBeEnabled();
  await expect(save).toBeDisabled();
  expect(await field.evaluate(element => element === Reflect.get(window, "sharedCompactField"))).toBe(true);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "harness_compact_save").length)).toBe(2);
});
