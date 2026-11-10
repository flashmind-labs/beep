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

const subscribed = (() => {
  const project = (name: string, tokens: number) => ({ project: name, cost_usd: 0, unpriced: [], metered_tokens: 0, subscription: [{ name: "claude", tokens }] });
  const period = {
    total: { project: "", cost_usd: 0, unpriced: [], metered_tokens: 0, subscription: [] },
    tokens: 39_300_000,
    projects: [project("acme/widget", 38_000_000), project("acme/relay", 1_300_000)],
    harnesses: [{ agent: "claude", subscription: true, cost_usd: null, tokens: 39_300_000, models: [{ name: "claude-sonnet", tokens: 39_300_000 }] }],
  };
  const window = (minutes: number, used: number) => ({ minutes, used_percent: used, resets_at: null, scope: null });
  return {
    limits: [
      { agent: "claude", plan: "max", machine_name: "build-box", as_of: Date.now(), windows: [window(300, 38), window(10_080, 76)], credits: [] },
      { agent: "codex", plan: "pro", machine_name: "build-box", as_of: Date.now(), windows: [window(10_080, 20)], credits: [] },
    ],
    today: period, week: period, month: period, metered: true,
  };
})();

for (const [name, size] of [["desktop", { width: 1280, height: 900 }], ["phone", { width: 390, height: 844 }]] as const) {
  test(`the ${name} pill shows each harness's binding share and the popover its values`, async ({ page }) => {
    await page.setViewportSize(size);
    await openDesktop(page, false, { usage_get: subscribed });
    const pill = page.getByRole("button", { name: "Plan usage: Claude Code 76%, Codex 20%" });
    await expect(pill).toBeVisible();
    await expect(pill.locator(".us-plan")).toHaveCount(2);
    await expect(pill.locator(".us-plan svg")).toHaveCount(2);
    await expect(pill.locator(".bar")).toHaveCount(0);
    await expect(pill.locator("b.warn")).toHaveText("76%");
    await expect(pill.locator(".us-plan").nth(1).locator("b")).not.toHaveClass(/warn/);

    await pill.click();
    const pop = page.getByRole("dialog", { name: "Plan usage and cost" });
    await expect(pop.getByText("76% used")).toBeVisible();
    await expect(pop.locator(".us-row", { hasText: "acme/widget" }).getByText("38M")).toBeVisible();
    await expect(pop.locator(".us-row", { hasText: "Claude Code, subscription" }).getByText("39.3M")).toBeVisible();
    await expect(pop.locator(".us-row", { hasText: "claude-sonnet" }).getByText("39.3M")).toBeVisible();
    await expect(pop.locator(".us-row", { hasText: "Total" }).getByText("39.3M")).toBeVisible();
    await expect(pop.locator(".us-note")).toHaveCount(0);
    await page.screenshot({ path: `../../.artifacts/usage-${name}.png` });
  });
}
