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

test("the project's sessions scroll under a header that stays put, and an idle one shows its last reply and changed lines", async ({ page }) => {
  const sessions = Array.from({ length: 40 }, (_, i) => ({ machine_id: "build-box", machine_name: "build-box", id: `past-${i}`, agent: "codex", title: `Past session ${i}`, cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 100 - i, external: false, question: null, assistant: false }));
  const idle = { ...sessions[0], id: "idle", title: "Idle session", status: "idle", reply_html: "All <strong>42</strong> tests pass in <code>beep-node</code>.", started_at: 0, changes: { additions: 146, deletions: 21 } };
  await openDesktop(page, false, { project_sessions: [...sessions, idle] });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  await page.locator(".pj-det .pv-row").filter({ hasText: "Past session 1" }).first().click();
  const rail = page.locator("#rail"), list = rail.locator(".rail-scroll"), filter = rail.getByPlaceholder("Filter sessions");
  await expect(rail).toBeVisible();
  // An idle running session shows the first line of its last reply.
  const idleRow = rail.locator(".rail-live > .pj-li").filter({ hasText: "Idle session" });
  await expect(idleRow.locator(".rail-reply")).toHaveText("All 42 tests pass in beep-node.");
  await expect(idleRow.locator(".rail-reply code")).toHaveText("beep-node");
  // Its checkout's changed lines sit under its time, added then removed.
  await expect(idleRow.locator(".rail-diff i")).toHaveText(["+146", "-21"]);
  await expect(rail.locator(".rail-live > .pj-li").filter({ hasText: "First notification" }).locator(".rail-diff")).toHaveCount(0);
  const before = await filter.boundingBox();

  await list.evaluate(node => { node.scrollTop = node.scrollHeight; });
  await expect.poll(() => list.evaluate(node => node.scrollTop)).toBeGreaterThan(0);
  expect(await rail.evaluate(node => node.scrollTop)).toBe(0);
  // Rows over the painting dim it.
  await expect(list).not.toHaveAttribute("data-clear");
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

  // The last one hands over round to the first; the last one left goes to the project, its
  // sessions still beside it.
  await page.keyboard.press(`${modifier}+Shift+w`);
  await expect(current).toHaveText(/First notification/);
  await page.keyboard.press(`${modifier}+Shift+w`);
  await expect(rail.locator(".rail-live")).toHaveCount(0);
  await expect(page.getByRole("heading", { level: 2, name: projectName })).toBeVisible();
});

test("the project's sessions end on its painting and a card of its pull requests, issues, Actions and machines", async ({ page }) => {
  const past = { machine_id: "build-box", machine_name: "build-box", id: "past-0", agent: "codex", title: "Past session 0", cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 100, external: false, question: null, assistant: false };
  await openDesktop(page, false, { project_sessions: [past] });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  await page.locator(".pj-det .pv-row").filter({ hasText: "Past session 0" }).click();
  const rail = page.locator("#rail"), card = rail.locator(".rail-card");
  await expect(rail.locator("canvas.painted-art")).toHaveCount(1);
  // With no row over it, the painting shows clearer.
  await expect(rail.locator(".rail-scroll[data-clear]")).toHaveCount(1);
  await expect(card.locator(".rail-card-li")).toHaveText([/Pull requests/, /Issues/, /Actions/, /Machines\s*1\/1/]);

  // Each opens that part of the project's page, with the sessions still beside it.
  await card.getByRole("button", { name: /Issues/ }).click();
  await expect(page.getByRole("heading", { level: 2, name: projectName })).toBeVisible();
  await expect(rail).toBeVisible();
  await card.getByRole("button", { name: /Pull requests/ }).click();
  await expect(page.locator(".pj-det")).toContainText("No open pull requests.");
  await expect(rail).toBeVisible();
  // The project's name goes back to the sidebar.
  await rail.locator(".rail-back").click();
  await expect(rail).toBeHidden();
  await expect(page.locator("#side")).toBeVisible();
});

test("a project window shows its sessions in the sidebar's place, and its Back clears the traffic lights without room for them", async ({ page }, info) => {
  const modifier = info.project.name === "Windows" ? "Control" : "Meta";
  await page.setViewportSize({ width: 1400, height: 800 });
  await openDesktop(page, false, { center_window_project: projectName });
  const rail = page.locator("#rail");
  await expect(rail).toBeVisible();
  await expect(rail.locator(".cc-side-toggle")).toBeHidden();
  await expect(page.locator("#side")).toBeHidden();

  await rail.locator(".rail-live > .pj-li").first().click();
  await expect(rail.locator('.pj-li[aria-current="true"]')).toHaveText(/First notification/);
  await expect(page.locator("#pw-back")).toBeHidden();
  // There is no sidebar to swap them for.
  await page.keyboard.press(`${modifier}+e`);
  await expect(rail).toBeVisible();

  await page.setViewportSize({ width: 640, height: 800 });
  await expect(rail).toBeHidden();
  const back = page.locator("#pw-back");
  await expect(back).toBeVisible();
  expect((await back.boundingBox())?.x ?? 0).toBeGreaterThanOrEqual(78);
});
