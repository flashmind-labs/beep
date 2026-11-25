import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const mac = (over: Record<string, unknown>) => ({ id: "mac-9", name: "Laptop", enabled: false, status: "off", error: null, permissions: { screen_recording: false, accessibility: false }, ...over });
const reads = (page: import("@playwright/test").Page) => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "mac_control").length);

test("this computer comes from the machines push, to the sidebar and the Add a machine sheet alike", async ({ page }) => {
  await openDesktop(page, false, { mac_control: mac({}), list_machines: [] });
  await page.evaluate(() => window.__navigation.notify("machines"));
  await page.getByRole("button", { name: "Add a machine", exact: true }).first().click();
  const sheet = page.getByRole("dialog", { name: "Add a machine" });
  await expect(sheet.getByRole("button", { name: "Use this computer" })).toBeVisible();
  const before = await reads(page);

  await page.evaluate(next => {
    window.__navigation.respond("mac_control", next);
    window.__navigation.notify("machines");
  }, mac({ enabled: true, status: "failed", error: "No route" }));
  await expect(sheet).toContainText("Couldn't connect: No route");
  await expect(page.locator(".cc-grp.machines")).toContainText("No route");
  expect(await reads(page)).toBe(before);
});
