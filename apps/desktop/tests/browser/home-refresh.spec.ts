import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = (id: string) => ({ kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: id, cwd: "/srv/project",
  status: "working", doing: null, started_at: 1, last_activity: 1, external: false, question: null });
const home = (running: number, needs_you: unknown[] = []) => ({ machines: [], projects: [], running: Array.from({ length: running }, (_, i) => session(`agent-${i}`)), needs_you });
const approval = { kind: "agent", machine_id: "build-box", machine_name: "build-box", agent: "codex", session: "agent-asks", title: "Deploy notes", question: { id: 1, text: "Send the draft?", detail: null, options: [] } };

const reads = (page: Page) => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length);

/// Counts the sidebar's DOM updates from now on.
const watchSidebar = (page: Page) => page.evaluate(() => {
  const w = window as unknown as { __sideDraws: number };
  w.__sideDraws = 0;
  new MutationObserver(() => { w.__sideDraws++; }).observe(document.querySelector("#side")!, { subtree: true, childList: true, characterData: true, attributes: true });
});
const sidebarDraws = (page: Page) => page.evaluate(() => (window as unknown as { __sideDraws: number }).__sideDraws);

/// Twenty Home changes 40 ms apart, each with a different answer, as during an agent's turn.
async function burst(page: Page, needs: unknown[] = []) {
  for (let i = 0; i < 20; i++) {
    await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, home(20 - i, needs));
    await page.clock.runFor(40);
  }
}

test("a burst of Home changes draws the newest pushed Home and the sidebar once, with no read", async ({ page }) => {
  await openDesktop(page, false, { center_home: home(0) });
  await expect(page.getByRole("tab", { name: "Board" })).toBeVisible();
  await page.clock.install();
  await page.clock.pauseAt(Date.now() + 1000);
  await page.clock.runFor(2000);
  const before = await reads(page);
  await watchSidebar(page);

  // The question gives the sidebar's Home link a count, the one thing there that changes.
  await burst(page, [approval]);
  expect(await reads(page)).toBe(before);
  await page.clock.runFor(1000);
  await expect(page.locator("#side")).toContainText("Home 1");
  await expect(page.locator("#sec-running .bd-lane > h4", { hasText: "Working" })).toHaveText("Working1");
  await page.clock.runFor(2000);
  expect(await reads(page)).toBe(before);
  expect(await sidebarDraws(page)).toBe(1);

  // The same answer again draws nothing.
  await page.evaluate(() => window.__navigation.notify("home"));
  await page.clock.runFor(1200);
  expect(await reads(page)).toBe(before);
  expect(await sidebarDraws(page)).toBe(1);
});

test("a first Home that failed to load is read again after five seconds while it shows, or by its first push", async ({ page }) => {
  await page.clock.install();
  await openDesktop(page, false, { failing: ["center_home"] });
  await page.clock.runFor(1000);
  const before = await reads(page);
  expect(before).toBeGreaterThan(0);
  await page.clock.runFor(4000);
  expect(await reads(page)).toBeGreaterThan(before);
  const retried = await reads(page);
  await page.clock.runFor(1000);
  expect(await reads(page)).toBe(retried);
  await page.evaluate(value => { window.__navigation.allow("center_home"); window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, home(1));
  await expect(page.getByRole("tab", { name: "Board" })).toBeVisible();
  await expect(page.locator("#sec-running .bd-lane > h4", { hasText: "Working" })).toHaveText("Working1");
});
