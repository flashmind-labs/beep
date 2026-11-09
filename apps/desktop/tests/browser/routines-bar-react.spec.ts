import { build } from "esbuild";
import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const routines = Array.from({ length: 25 }, (_, index) => ({ id: `routine-${index}`, label: `Routine ${index}`, prompt: "Summarise", schedule: "0 8 * * *", enabled: true, created_at: "2025-01-01T00:00:00Z", run_once: false, last_run_at: null, next_run_at: null }));

test("routine list and editor keep focus and scroll through updates", async ({ page }) => {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, list_crons: routines, list_grants: [] });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar")).toBeVisible();
  const bundle = await build({ entryPoints: ["tests/browser/routines-bar-fixture.ts"], bundle: true, write: false, platform: "browser", format: "iife" });
  await page.addScriptTag({ content: bundle.outputFiles[0].text });
  await page.evaluate(() => Reflect.get(window, "openTestRoutines")());
  const list = page.getByRole("listbox", { name: "Routines" });
  await expect(list.getByRole("option")).toHaveCount(25);
  const row = list.getByRole("option").first();
  await row.evaluate(el => Reflect.set(window, "routineRow", el));
  await list.evaluate(el => { el.style.height = "100px"; el.style.overflow = "auto"; el.scrollTop = 200; });
  const top = await list.evaluate(el => el.scrollTop);
  await page.getByRole("button", { name: /Grants/ }).evaluate(el => Reflect.set(window, "routineGrants", el));
  await page.getByRole("switch").first().evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  await expect(list.getByRole("option").first()).toHaveClass(/off/);
  expect(await row.evaluate(el => el === Reflect.get(window, "routineRow"))).toBe(true);
  await expect(list).toHaveJSProperty("scrollTop", top);
  await page.getByRole("button", { name: /^New/ }).first().click();
  const name = page.locator("#nrlabel");
  await name.fill("Draft");
  await name.evaluate(el => { if (el instanceof HTMLInputElement) { el.setSelectionRange(2, 2); Reflect.set(window, "routineName", el); } });
  await page.getByRole("button", { name: "Weekly" }).evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  expect(await name.evaluate(el => el === Reflect.get(window, "routineName") && document.activeElement === el && el instanceof HTMLInputElement && el.selectionStart === 2)).toBe(true);
  await expect(page.locator("[data-day]")).toHaveCount(7);
  await page.locator("#nrprompt").fill("Summarise");
  await page.locator("#nrgo").click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "create_cron").map(call => call.args))).toEqual([{ label: "Draft", prompt: "Summarise", schedule: "0 8 * * 1" }]);
});

test("grant icon returns after cancelling revoke", async ({ page }) => {
  const grant = { id: "grant", routine_id: "routine", routine: "Morning", app: "Example", action: "Read", fields: [], per_run: 1, expires_at: 0, created_at: 0, last_used_at: null };
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, list_crons: routines, list_grants: [grant], list_apps: [] });
  await page.goto("https://app.test/index.html");
  const bundle = await build({ entryPoints: ["tests/browser/routines-bar-fixture.ts"], bundle: true, write: false, platform: "browser", format: "iife" });
  await page.addScriptTag({ content: bundle.outputFiles[0].text });
  await page.evaluate(() => Reflect.get(window, "openTestRoutines")());
  await page.getByRole("button", { name: /Grants/ }).click();
  const icon = page.locator("#glist .dapp");
  await expect(icon).toHaveText("E");
  await page.getByRole("button", { name: /Revoke/ }).click();
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(icon).toHaveText("E");
});
