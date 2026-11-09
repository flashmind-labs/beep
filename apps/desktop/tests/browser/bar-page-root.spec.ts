import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("bar pages reuse their root and initialize each view", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, list_skills: [], list_researches: [], list_sessions: [], list_groups: [] });
  await page.evaluate(() => window.__navigation.open("skills"));
  await expect(page.getByText("No skills yet.")).toBeVisible();
  await page.locator("#bar > div").evaluate(element => { Reflect.set(window, "sharedPageRoot", element); });
  await page.evaluate(() => window.__navigation.open("research"));
  await expect(page.getByPlaceholder("What should the team research?")).toBeVisible();
  expect(await page.locator("#bar > div").evaluate(element => element === Reflect.get(window, "sharedPageRoot"))).toBe(true);
  await page.evaluate(() => window.__navigation.open("chats"));
  await expect(page.getByPlaceholder("Search chats and groups")).toBeVisible();
  expect(await page.locator("#bar > div").evaluate(element => element === Reflect.get(window, "sharedPageRoot"))).toBe(true);
  await page.evaluate(() => window.__navigation.open("skills"));
  await expect(page.getByText("No skills yet.")).toBeVisible();
  expect(await page.locator("#bar > div").evaluate(element => element === Reflect.get(window, "sharedPageRoot"))).toBe(true);
});
