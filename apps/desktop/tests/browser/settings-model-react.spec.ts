import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const one = { choices: [{ id: "one", name: "One", input_price: null, output_price: null, sees_images: false, default: true }, { id: "two", name: "Two", input_price: 1, output_price: 2, sees_images: true, default: false }], selected: "one", thinking: "low", auto: false, auto_available: true };
const base = { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, list_machines: [], list_devices: { devices: [], server: "answered" }, list_credentials: [], list_github_accounts: [], get_models: one };
test("model changes keep controls mounted, report errors and retry", async ({ page }) => {
  await openDesktop(page, false, base);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Models", exact: true }).click();
  const model = settings.getByLabel("Model", { exact: true });
  await model.focus();
  await model.evaluate(el => Reflect.set(window, "settingsModel", el));
  await page.evaluate(value => window.__navigation.respond("set_model", value), { ...one, selected: "two" });
  await model.selectOption("two");
  await expect(model).toHaveValue("two");
  expect(await model.evaluate(el => el === Reflect.get(window, "settingsModel") && document.activeElement === el)).toBe(true);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_model" && call.args.id === "two"))).toBe(true);
  await page.evaluate(() => window.__navigation.rejectOnce("set_auto_model"));
  const auto = settings.getByRole("switch", { name: "Auto thinking" });
  await auto.click();
  await expect(settings.locator("#setModelErr")).toContainText("Requested test failure");
  await expect(auto).toBeEnabled();
  await page.evaluate(value => window.__navigation.respond("set_auto_model", value), { ...one, selected: "two", auto: true });
  await auto.click();
  await expect(auto).toHaveAttribute("aria-checked", "true");
  await expect(settings.getByRole("button", { name: "High", exact: true })).toBeDisabled();
});

test("a stale model read cannot overwrite a completed update", async ({ page }) => {
  await openDesktop(page, false, base);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Models", exact: true }).click();
  await page.evaluate(() => window.__navigation.holdNext("get_models"));
  await page.evaluate(() => window.__navigation.emit("link-state", "linked"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "get_models").length)).toBeGreaterThan(1);
  await page.evaluate(value => window.__navigation.respond("set_model", value), { ...one, selected: "two" });
  const model = settings.getByLabel("Model", { exact: true });
  await model.selectOption("two");
  await expect(model).toHaveValue("two");
  await page.evaluate(() => window.__navigation.release("get_models"));
  await expect(model).toHaveValue("two");
});

test("a pending thinking change remains serialized across reopening", async ({ page }) => {
  await openDesktop(page, false, base);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Models", exact: true }).click();
  await page.evaluate(value => { window.__navigation.respond("set_thinking", value); window.__navigation.hold("set_thinking"); }, { ...one, thinking: "high" });
  await settings.getByRole("button", { name: "High", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "set_thinking" && call.args.level === "high").length)).toBe(1);
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const high = settings.getByRole("button", { name: "High", exact: true });
  await expect(high).toBeDisabled();
  await page.evaluate(() => window.__navigation.release("set_thinking"));
  await expect(high).toBeEnabled();
  await expect(high).toHaveAttribute("aria-pressed", "true");
});

test("a failed write from a closed model page does not show a stale error", async ({ page }) => {
  await openDesktop(page, false, base);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Models", exact: true }).click();
  await page.evaluate(() => window.__navigation.hold("set_thinking"));
  await settings.getByRole("button", { name: "High", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "set_thinking"))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  await expect(settings.getByRole("button", { name: "High", exact: true })).toBeDisabled();
  await page.evaluate(() => { window.__navigation.rejectOnce("set_thinking"); window.__navigation.release("set_thinking"); });
  await expect(settings.getByRole("button", { name: "High", exact: true })).toBeEnabled();
  await expect(settings.locator("#setModelErr")).toHaveText("");
});
