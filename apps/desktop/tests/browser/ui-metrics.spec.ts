import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";
import type { PerfBatch } from "../../src/ui/perf-state.ts";

const batches = (page: Page) => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "desktop_ui_metrics").map(call => call.args.batch)) as Promise<PerfBatch[]>;

async function setHidden(page: Page, hidden: boolean) {
  await page.evaluate(hidden => {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => hidden ? "hidden" : "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
  }, hidden);
}

test("hiding the window sends its UI timings once, with fixed labels and no content", async ({ page }) => {
  // Page fades under the blocked frames below would report long frames after the flush.
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openDesktop(page);
  await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible();
  const side = page.locator("#side");
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator('.pj-li[title="Running"]')).toBeVisible();
  await side.getByRole("button", { name: "Home", exact: true }).click();
  await expect(page.getByRole("tab", { name: "Overview" })).toBeVisible();

  // A click whose handler blocks, and a long task with no input.
  await page.evaluate(() => document.addEventListener("click", () => { const end = performance.now() + 600; while (performance.now() < end); }, { once: true }));
  await page.getByRole("tab", { name: "Overview" }).click();
  await page.evaluate(() => { const end = performance.now() + 250; while (performance.now() < end); });
  await page.waitForTimeout(300);

  await setHidden(page, true);
  await expect.poll(async () => (await batches(page)).length).toBeGreaterThan(0);
  const sent = await batches(page);
  const histograms = sent.flatMap(batch => batch.histograms);
  const last = sent[sent.length - 1];

  expect(sent.every(batch => batch.window === "main")).toBe(true);
  // WebKit reports no long tasks or long frames: only what it observes is checked there.
  if (page.context().browser()?.browserType().name() !== "webkit") expect(last.supported).toEqual(expect.arrayContaining(["event", "longtask", "long-animation-frame"]));
  const interactions = histograms.filter(h => h.kind === "interaction");
  if (last.supported.includes("event")) {
    expect(interactions.some(h => h.input === "pointer" && h.max_ms >= 500 && h.counts[7] + h.counts[8] >= 1)).toBe(true);
    expect(sent.flatMap(batch => batch.slow).some(s => s.kind === "interaction" && s.ms >= 500)).toBe(true);
  }
  if (last.supported.includes("longtask")) expect(histograms.some(h => h.kind === "long_task" && h.view === "home" && h.max_ms >= 200)).toBe(true);
  expect(histograms.some(h => h.kind === "open" && h.view === "home")).toBe(true);
  for (const h of histograms) {
    expect(h.counts).toHaveLength(11);
    expect(h.counts.reduce((a, b) => a + b, 0)).toBeGreaterThan(0);
    expect(h.sum_ms).toBeGreaterThanOrEqual(h.max_ms);
  }
  expect(sent.flatMap(batch => batch.shown_ms).reduce((sum, s) => sum + s.ms, 0)).toBeGreaterThan(0);

  const text = JSON.stringify(sent);
  for (const content of [projectName, "build-box", "/srv/project", "notification", "notice-one"]) expect(text).not.toContain(content);

  // Hidden, nothing new: no more flushes.
  const count = sent.length;
  await page.evaluate(() => window.dispatchEvent(new Event("pagehide")));
  await page.waitForTimeout(500);
  expect((await batches(page)).length).toBe(count);

  const overhead = sent.reduce((sum, batch) => sum + batch.overhead_ms, 0);
  test.info().annotations.push({ type: "overhead", description: `${overhead.toFixed(2)} ms over ${histograms.reduce((n, h) => n + h.counts.reduce((a, b) => a + b, 0), 0)} samples` });
  expect(overhead).toBeLessThan(20);
});
