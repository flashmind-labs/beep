import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("usage updates retain the open popover and selected period", async ({ page }) => {
  const spend = { project: "Example", cost_usd: 2, unpriced: [] };
  const period = { total: spend, tokens: 0, projects: [spend], harnesses: [] };
  const usage = { limits: [{ agent: "codex", plan: "pro", machine_name: "Build box", as_of: 0, windows: [{ minutes: 300, used_percent: 20, resets_at: null, scope: null }] }], today: period, week: period, month: null, metered: true };
  await openDesktop(page, false, { usage_get: usage });
  const button = page.getByRole("button", { name: /Codex/ });
  await button.click();
  const pop = page.getByRole("dialog", { name: "Plan usage and cost" });
  await expect(pop).toBeVisible();
  await pop.getByRole("button", { name: "This week" }).click();
  await pop.evaluate(element => { element.dataset.identity = "kept"; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "usage_get").length);
  await page.evaluate(value => { window.__navigation.respond("usage_get", value); window.__navigation.notify("usage"); }, usage);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "usage_get").length)).toBeGreaterThan(reads);
  await expect(pop).toHaveAttribute("data-identity", "kept");
  await expect(pop.getByRole("button", { name: "This week" })).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Escape");
  await expect(pop).toHaveCount(0);
});
