import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("coding agent overview preserves focus on refresh and opens with Enter", async ({ page }) => {
  await openDesktop(page, false, { settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false }, list_machines: [], list_credentials: [], list_github_accounts: [] });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Agent accounts", exact: true }).click();
  const row = settings.locator('#set-agent-overview [data-sub="agent:codex"]');
  await row.focus();
  await row.evaluate(el => Reflect.set(window, "codexOverview", el));
  const arrow = row.locator(":scope > svg");
  expect(await arrow.evaluate(el => { const sample = document.createElement("span"); sample.style.color = "var(--muted)"; el.parentElement?.append(sample); const matches = getComputedStyle(el).color === getComputedStyle(sample).color; sample.remove(); return matches; })).toBe(true);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length);
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "settings").length)).toBeGreaterThan(reads);
  expect(await row.evaluate(el => el === Reflect.get(window, "codexOverview") && document.activeElement === el)).toBe(true);
  await row.press("Enter");
  await expect(settings.getByRole("button", { name: "Back to Agent accounts" })).toBeVisible();
  await expect(settings.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Codex", exact: true })).toHaveAttribute("aria-current", "true");
});
