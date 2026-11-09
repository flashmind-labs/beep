import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const status = { link: "unlinked", signed_out: false, server: true, live: false, session: null };
async function openBar(page: import("@playwright/test").Page, hosted: boolean) {
  await openDesktop(page, false, { status, login_hosted: hosted, link_offer: { uri: "beep://link", code: "ABCD", qr_svg: "<svg></svg>", expires_in: 60 } });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar")).toBeVisible();
  await page.evaluate(() => window.__navigation.emit("bar-shown", "link"));
}
test("link view expires without replacing a later screen", async ({ page }) => {
  await page.clock.install();
  await openBar(page, false);
  await expect(page.getByText("Waiting for phone…")).toBeVisible();
  await page.getByRole("button", { name: "Have a join code?" }).click();
  await expect(page.locator("#jcode")).toBeFocused();
  await page.clock.fastForward("01:01");
  await expect(page.getByText("Join an account")).toBeVisible();
});

test("hosted sign-in shows failures and supports own-server retry", async ({ page }) => {
  await openBar(page, true);
  await expect(page.getByText("Sign in to Beep")).toBeVisible();
  await page.getByRole("button", { name: "Use my own server" }).click();
  const code = page.locator("#lcode");
  await expect(code).toBeFocused();
  await code.fill("sample-code");
  await page.evaluate(() => window.__navigation.rejectOnce("use_own_server"));
  await code.press("Enter");
  await expect(page.locator(".note")).toContainText("Requested test failure");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "use_own_server").map(call => call.args))).toEqual([{ code: "sample-code" }]);
  await page.getByRole("button", { name: "Back" }).click();
  await page.evaluate(() => window.__navigation.emit("login-failed", "bad request: expired"));
  await expect(page.getByText("That sign-in expired or was for another device.")).toBeVisible();
});

test("join sends the entered code and reports errors", async ({ page }) => {
  await openBar(page, false);
  await page.getByRole("button", { name: "Have a join code?" }).click();
  await page.locator("#jcode").fill("sample-code");
  await page.evaluate(() => window.__navigation.rejectOnce("join_account"));
  await page.locator("#jcode").press("Enter");
  await expect(page.locator(".note")).toContainText("Requested test failure");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "join_account").map(call => call.args))).toEqual([{ code: "sample-code" }]);
});

test("late link offer cannot replace a newer hosted login", async ({ page }) => {
  await openDesktop(page, false, { status, login_hosted: false, link_offer: { uri: "beep://link", code: "OLD", qr_svg: "<svg></svg>", expires_in: 60 } });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar")).toBeVisible();
  await page.evaluate(() => { window.__navigation.hold("link_offer"); window.__navigation.emit("bar-shown", "link"); });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "link_offer"))).toBe(true);
  await page.evaluate(() => { window.__navigation.respond("login_hosted", true); window.__navigation.emit("bar-shown", "link"); });
  await expect(page.getByText("Sign in to Beep")).toBeVisible();
  await page.evaluate(() => window.__navigation.release("link_offer"));
  await expect(page.getByText("Sign in to Beep")).toBeVisible();
  await expect(page.getByText("Waiting for phone…")).toHaveCount(0);
});

test("phone invite expires and can request a replacement", async ({ page }) => {
  await page.clock.install();
  await openDesktop(page, false, { status: { ...status, link: "linked" }, list_devices: { devices: [{ id: "local", name: "Local", kind: "desktop", primary: true, this_device: true }], server: "answered" }, settings: { name: "Local", version: "1", autostart: false, notice_voice: "off", send_on_pause: false, agent_notices: false }, invite_phone: { code: "PHONE", qr_svg: "<svg></svg>", expires_in: 2 } });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar")).toBeVisible();
  await page.evaluate(() => window.__navigation.emit("bar-shown", "settings"));
  await page.locator("#invite").click();
  await expect(page.locator(".code.big")).toHaveText("PHONE");
  await page.clock.fastForward("00:03");
  await expect(page.getByRole("button", { name: "New code" })).toBeVisible();
  await page.evaluate(() => window.__navigation.respond("invite_phone", { code: "NEXT", qr_svg: "<svg></svg>", expires_in: 60 }));
  await page.getByRole("button", { name: "New code" }).click();
  await expect(page.locator(".code.big")).toHaveText("NEXT");
});

test("switching to another React bar page cancels the link countdown", async ({ page }) => {
  await page.clock.install();
  await openBar(page, false);
  await expect(page.getByText("Waiting for phone…")).toBeVisible();
  await page.evaluate(() => { window.__navigation.respond("status", { ...{ link: "linked", signed_out: false, server: true, live: false, session: null } }); window.__navigation.respond("list_researches", []); window.__navigation.emit("bar-shown", "research"); });
  await expect(page.getByText("No research yet.")).toBeVisible();
  await page.clock.fastForward("01:01");
  await expect(page.getByText("No research yet.")).toBeVisible();
  await expect(page.getByText("Code expired")).toHaveCount(0);
});

test("synchronous screen replacement remounts a detached account page", async ({ page }) => {
  await openBar(page, false);
  await expect(page.getByText("Waiting for phone…")).toBeVisible();
  await page.evaluate(() => { document.querySelector("#bar")?.replaceChildren(); window.__navigation.emit("link-state", "revoked"); });
  await expect(page.getByText("This computer was removed")).toBeVisible();
  await page.evaluate(() => { window.__navigation.respond("status", { link: "unlinked", signed_out: false, server: true, live: false, session: null }); window.__navigation.emit("bar-shown", "link"); });
  await expect(page.getByText("Waiting for phone…")).toBeVisible();
});
