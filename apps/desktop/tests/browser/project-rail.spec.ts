import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("a project's session swaps the sidebar for the project's sessions in the same column", async ({ page }, info) => {
  const modifier = info.project.name === "Windows" ? "Control" : "Meta";
  const sessions = Array.from({ length: 4 }, (_, i) => ({ machine_id: "build-box", machine_name: "build-box", id: `past-${i}`, agent: "codex", title: `Past session ${i}`, cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 100 - i, external: false, question: null, assistant: false }));
  const busy = { ...sessions[0], id: "busy", title: "Busy session", status: "working", doing: "Running the tests", started_at: 5, last_activity: 1 };
  await openDesktop(page, false, { project_sessions: [...sessions, busy] });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  const win = page.locator(".cwin"), rail = page.locator("#rail"), side = page.locator("#side"), main = page.locator("#main");
  await expect(rail).toBeHidden();

  await page.locator(".pj-det .pv-row").filter({ hasText: "Past session 1" }).click();
  await expect(win).toHaveClass(/rail-on/);
  await expect(side).toBeHidden();
  const past = rail.locator(".pj-li").filter({ hasText: "Past session" });
  await expect(past).toHaveCount(4);
  await expect(rail.locator('.pj-li[aria-current="true"]')).toHaveText(/Past session 1/);
  const at = await main.boundingBox();

  // The sessions take the sidebar's width, which the grip sets and keeps.
  expect(Math.round((await rail.boundingBox())?.width ?? 0)).toBe(300);
  await page.locator("#side-grip").press("ArrowRight");
  await expect.poll(async () => Math.round((await rail.boundingBox())?.width ?? 0)).toBe(316);
  expect(await page.evaluate(() => localStorage.getItem("beep.sideWidth"))).toBe("316");
  await page.locator("#side-grip").press("ArrowLeft");

  // Running rows keep the line for what the agent is doing while it is idle, so they do not jump.
  const heights = await rail.locator(".rail-live > .pj-li").evaluateAll(rows => rows.map(row => Math.round(row.getBoundingClientRect().height)));
  expect(new Set(heights).size).toBe(1);

  // Running sessions keep the order they started in, newest first, whatever their activity.
  await expect(rail.locator(".rail-live > .pj-li .t b")).toHaveText(["Busy session", "First notification", "Second notification"]);

  // ⌃Tab steps through them in that order, round to the first.
  const current = rail.locator('.pj-li[aria-current="true"]');
  await page.keyboard.press("Control+Tab");
  await expect(current).toHaveText(/Busy session/);
  await page.keyboard.press("Control+Tab");
  await expect(current).toHaveText(/First notification/);
  await page.keyboard.press("Control+Tab");
  await expect(current).toHaveText(/Second notification/);
  await page.keyboard.press("Control+Tab");
  await expect(current).toHaveText(/Busy session/);
  await page.keyboard.press("Control+Shift+Tab");
  await expect(current).toHaveText(/Second notification/);

  await rail.getByPlaceholder("Filter sessions").fill("session 3");
  await expect(past).toHaveText([/Past session 3/]);

  await rail.locator(".cc-side-toggle").click();
  await expect(rail).toBeHidden();
  await expect(side).toBeVisible();
  expect(await main.boundingBox()).toEqual(at);
  await side.locator(".cc-side-toggle").click();
  await expect(rail).toBeVisible();
  await expect(side).toBeHidden();

  // ⌘E swaps them too.
  await page.keyboard.press(`${modifier}+e`);
  await expect(side).toBeVisible();
  await expect(rail).toBeHidden();
  await page.keyboard.press(`${modifier}+e`);
  await expect(rail).toBeVisible();

  await rail.locator(".rail-back").click();
  await expect(rail).toBeHidden();
  await expect(side).toBeVisible();
});

test("the project's sessions scroll under a header that stays put", async ({ page }) => {
  const sessions = Array.from({ length: 40 }, (_, i) => ({ machine_id: "build-box", machine_name: "build-box", id: `past-${i}`, agent: "codex", title: `Past session ${i}`, cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 100 - i, external: false, question: null, assistant: false }));
  await openDesktop(page, false, { project_sessions: sessions });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  await page.locator(".pj-det .pv-row").filter({ hasText: "Past session 1" }).first().click();
  const rail = page.locator("#rail"), list = rail.locator(".rail-scroll"), filter = rail.getByPlaceholder("Filter sessions");
  await expect(rail).toBeVisible();
  const before = await filter.boundingBox();

  await list.evaluate(node => { node.scrollTop = node.scrollHeight; });
  await expect.poll(() => list.evaluate(node => node.scrollTop)).toBeGreaterThan(0);
  expect(await rail.evaluate(node => node.scrollTop)).toBe(0);
  expect((await filter.boundingBox())?.y).toBe(before?.y);
  // No row slides up under the filter and the window buttons.
  const top = await list.evaluate(node => node.getBoundingClientRect().top);
  expect(top).toBeGreaterThanOrEqual((before?.y ?? 0) + (before?.height ?? 0));
});

test("closing a project's session opens its next running one, then the project once none is left", async ({ page }, info) => {
  const modifier = info.project.name === "Windows" ? "Control" : "Meta";
  const past = { machine_id: "build-box", machine_name: "build-box", id: "past-0", agent: "codex", title: "Past session 0", cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 100, external: false, question: null, assistant: false };
  await openDesktop(page, false, { project_sessions: [past] });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  await page.locator(".pj-det .pv-row").filter({ hasText: "Past session 0" }).click();
  const rail = page.locator("#rail"), current = rail.locator('.pj-li[aria-current="true"]');
  await expect(rail.locator(".rail-live > .pj-li .t b")).toHaveText(["First notification", "Second notification"]);
  await rail.locator(".rail-live > .pj-li").filter({ hasText: "Second notification" }).click();
  await expect(current).toHaveText(/Second notification/);

  // The last one hands over round to the first; the last one left goes to the project.
  await page.keyboard.press(`${modifier}+Shift+w`);
  await expect(current).toHaveText(/First notification/);
  await page.keyboard.press(`${modifier}+Shift+w`);
  await expect(rail).toBeHidden();
  await expect(page.getByRole("heading", { level: 2, name: projectName })).toBeVisible();
});
