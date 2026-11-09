import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const base = { settings: { name: "build-box", version: "0.1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, list_machines: [], list_credentials: [], list_github_accounts: [], skill_sync_get: true };
async function openServer(page: Page, extra: Record<string, unknown>) {
  await openDesktop(page, false, { ...base, ...extra });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Beep server", exact: true }).click();
  return settings;
}
const linked = { link: "linked", signed_out: false, server: true, live: false, session: null };
test("device controls and sign-out confirmation survive refresh", async ({ page }) => {
  const devices = [{ id: "one", name: "This device", kind: "desktop", primary: true, this_device: true }, { id: "two", name: "Phone", kind: "phone", primary: false, this_device: false }];
  const settings = await openServer(page, { status: linked, list_devices: { devices, server: "answered" } });
  await expect(settings.getByText("Signed in", { exact: true })).toBeVisible();
  await settings.getByRole("button", { name: "Remove Phone" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "remove_device" && call.args.id === "two"))).toBe(true);
  await settings.getByRole("button", { name: "Sign out…" }).click();
  await expect(settings.getByText("Sign out of Beep server?")).toBeVisible();
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length);
  await page.evaluate(() => window.__navigation.emit("link-state", "linked"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(reads);
  await expect(settings.getByText("Sign out of Beep server?")).toBeVisible();
  await settings.getByRole("button", { name: "Sign out", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "sign_out"))).toBe(true);
});

test("unlink requires confirmation and reports errors", async ({ page }) => {
  const devices = [{ id: "one", name: "This device", kind: "desktop", primary: false, this_device: true }];
  const settings = await openServer(page, { status: linked, list_devices: { devices, server: "answered" } });
  await settings.getByRole("button", { name: "Unlink this computer" }).click();
  await expect(settings.getByText("Unlink this computer?", { exact: true })).toBeVisible();
  await page.evaluate(() => window.__navigation.rejectOnce("unlink"));
  await settings.getByRole("button", { name: "Unlink", exact: true }).click();
  await expect(settings.locator("#unlinkErr")).toContainText("Requested test failure");
  await settings.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(settings.getByText("Unlink this computer?", { exact: true })).toHaveCount(0);
  await settings.getByRole("button", { name: "Unlink this computer" }).click();
  await expect(settings.locator("#unlinkErr")).toHaveText("");
});

test("browser sign-in accepts a pasted token", async ({ page }) => {
  const settings = await openServer(page, { status: linked, list_devices: { devices: [], server: "answered" }, login_hosted: true });
  await page.evaluate(() => window.__navigation.emit("link-state", "unlinked"));
  await expect(settings.getByText("No account", { exact: true })).toBeVisible();
  await settings.getByRole("button", { name: "Sign in", exact: true }).click();
  const token = settings.getByPlaceholder("Token");
  await expect(token).toBeFocused();
  await token.fill("sample");
  await token.evaluate(el => { if (el instanceof HTMLInputElement) { el.setSelectionRange(1, 4); Reflect.set(window, "loginToken", el); } });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length);
  await page.evaluate(() => window.__navigation.emit("link-state", "unlinked"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(reads);
  expect(await token.evaluate(el => el === Reflect.get(window, "loginToken") && document.activeElement === el && el instanceof HTMLInputElement && el.selectionStart === 1 && el.selectionEnd === 4)).toBe(true);
  await token.fill("sample.token");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "complete_login" && call.args.token === "sample.token"))).toBe(true);
  await page.evaluate(() => window.__navigation.emit("link-state", "linked"));
  await expect(settings.getByText("Signed in", { exact: true })).toBeVisible();
  await expect(settings.getByPlaceholder("Token")).toHaveCount(0);
});

test("late browser sign-in cannot show a token field after linking", async ({ page }) => {
  const settings = await openServer(page, { status: linked, list_devices: { devices: [], server: "answered" }, login_hosted: true });
  await page.evaluate(() => window.__navigation.emit("link-state", "unlinked"));
  await expect(settings.getByText("No account", { exact: true })).toBeVisible();
  await page.evaluate(() => window.__navigation.hold("start_login"));
  await settings.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "start_login"))).toBe(true);
  await page.evaluate(() => window.__navigation.emit("link-state", "linked"));
  await expect(settings.getByText("Signed in", { exact: true })).toBeVisible();
  await page.evaluate(() => window.__navigation.release("start_login"));
  await expect(settings.getByPlaceholder("Token")).toHaveCount(0);
});
