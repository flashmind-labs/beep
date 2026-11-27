import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

/// Counts timer and frame callbacks from now on, and the animations running.
function countCallbacks() {
  const w = window as unknown as { __idle: { timers: number; frames: number } };
  w.__idle = { timers: 0, frames: 0 };
  const timeout = window.setTimeout, interval = window.setInterval, frame = window.requestAnimationFrame;
  const wrap = (fn: TimerHandler, kind: "timers" | "frames") => typeof fn === "function" ? (...args: unknown[]) => { w.__idle[kind]++; (fn as (...a: unknown[]) => void)(...args); } : fn;
  window.setTimeout = ((fn: TimerHandler, ms?: number, ...rest: unknown[]) => timeout(wrap(fn, "timers"), ms, ...rest)) as typeof setTimeout;
  window.setInterval = ((fn: TimerHandler, ms?: number, ...rest: unknown[]) => interval(wrap(fn, "timers"), ms, ...rest)) as typeof setInterval;
  window.requestAnimationFrame = (fn => frame(wrap(fn, "frames") as FrameRequestCallback)) as typeof requestAnimationFrame;
}

async function idle(page: Page, ms: number) {
  await page.evaluate(() => { const w = window as unknown as { __idle: { timers: number; frames: number } }; w.__idle.timers = 0; w.__idle.frames = 0; });
  await page.waitForTimeout(ms);
  return page.evaluate(() => {
    const w = window as unknown as { __idle: { timers: number; frames: number } };
    const running = document.getAnimations().filter(a => a.playState === "running" && a.effect?.getTiming().iterations === Infinity);
    return { ...w.__idle, infinite: running.length };
  });
}

async function setHidden(page: Page, hidden: boolean) {
  await page.evaluate(hidden => {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => hidden ? "hidden" : "visible" });
    window.__navigation.emit(hidden ? "center-hidden" : "center-shown", null);
    document.dispatchEvent(new Event("visibilitychange"));
  }, hidden);
}

test("an idle window with reduced motion runs few callbacks, and nothing once hidden", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.addInitScript(countCallbacks);
  await openDesktop(page);
  await expect(page.getByRole("tab", { name: "Board" })).toBeVisible();

  const home = await idle(page, 3000);
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator(".pj-strip")).toBeVisible();
  await page.waitForTimeout(1000);
  const project = await idle(page, 3000);
  await setHidden(page, true);
  await page.waitForTimeout(500);
  const hidden = await idle(page, 3000);
  test.info().annotations.push({ type: "idle", description: JSON.stringify({ home, project, hidden }) });

  for (const shown of [home, project]) {
    expect(shown.frames).toBeLessThan(10);
    expect(shown.timers).toBeLessThan(15);
  }
  expect(hidden.frames).toBe(0);
  expect(hidden.infinite).toBe(0);
  expect(hidden.timers).toBeLessThan(5);
});

test("center-hidden stops paintings and infinite animations even when document.hidden stays false", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.addInitScript(countCallbacks);
  await openDesktop(page);
  await expect(page.getByRole("tab", { name: "Board" })).toBeVisible();

  await page.evaluate(() => { window.__navigation.emit("center-hidden", null); });
  await page.waitForTimeout(300);
  const hidden = await idle(page, 2000);
  test.info().annotations.push({ type: "idle-normal-motion-hidden", description: JSON.stringify(hidden) });

  expect(hidden.frames).toBe(0);
  expect(hidden.infinite).toBe(0);
});
