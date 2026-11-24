import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("sidebar navigation keeps focus and scroll during Home updates", async ({ page }) => {
  await openDesktop(page);
  const home = page.locator("#side").getByRole("button", { name: "Home", exact: true });
  await expect(home).toBeVisible();
  const scroll = await page.locator("#side-content").evaluate(element => { element.style.maxHeight = "80px"; element.scrollTop = 30; return element.scrollTop; });
  expect(scroll).toBeGreaterThan(0);
  await home.evaluate(element => (element as HTMLElement).focus({ preventScroll: true }));
  await home.evaluate(element => { (window as any).__sidebarHome = element; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length);
  await page.evaluate(() => window.__navigation.notify("home"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length)).toBeGreaterThan(reads);
  await expect(home).toBeFocused();
  expect(await page.locator("#side-content").evaluate(element => element.scrollTop)).toBe(scroll);
  expect(await home.evaluate(element => element === (window as any).__sidebarHome)).toBe(true);
  const toggle = page.getByRole("button", { name: "Collapse sidebar" });
  await toggle.click();
  await expect(page.getByRole("button", { name: "Expand sidebar" })).toBeVisible();
  await expect(home.locator("svg")).toBeVisible();
  await page.getByRole("button", { name: "Expand sidebar" }).click();
  await expect(home.getByText("Home")).toBeVisible();
});

test("sidebar group rows keep focus on refresh", async ({ page }) => {
  await openDesktop(page);
  const settings = page.locator("#side .cc-grp.views").getByRole("button", { name: "Settings", exact: true });
  await expect(settings).toBeVisible();
  await settings.focus();
  await settings.evaluate(element => { (window as any).__settingsRow = element; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length);
  await page.evaluate(() => window.__navigation.notify("home"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length)).toBeGreaterThan(reads);
  await expect(settings).toBeFocused();
  expect(await settings.evaluate(element => element === (window as any).__settingsRow)).toBe(true);
});

test("machine sidebar rows remain focused when another machine changes", async ({ page }) => {
  const machine = (id: string, name: string) => ({ id, name, online: true, can_host: true, seen_at: 0, version: "1", env: null, apps: [], gpus: [], capabilities: [], agents: 0, stats: null });
  const home = { machines: [machine("one", "Build box"), machine("two", "Test box")], projects: [], running: [], needs_you: [] };
  await openDesktop(page, false, { center_home: null });
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, home);
  const row = page.locator("#side .cc-grp.machines").getByRole("button", { name: "Test box" });
  await expect(row).toBeVisible();
  await row.focus();
  await row.evaluate(element => { (window as any).__sidebarMachine = element; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length);
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, { ...home, machines: [{ ...home.machines[0], agents: 2 }, home.machines[1]] });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_home").length)).toBeGreaterThan(reads);
  await expect(page.locator("#side .cc-grp.machines").getByRole("button", { name: "Build box" })).toContainText("2");
  await expect(row).toBeFocused();
  expect(await row.evaluate(element => element === (window as any).__sidebarMachine)).toBe(true);
});

test("sidebar project rows retain focus and update another project", async ({ page }) => {
  const project = (name: string) => ({ name, url: `https://example.test/${name}`, machine_id: "box", machine_name: "Build box", folder: `/srv/${name}`, cloned: true });
  const home = { machines: [], projects: [project("team/alpha"), project("team/beta")], running: [], needs_you: [] };
  await openDesktop(page, false, { center_home: home });
  const beta = page.locator("#side .cc-grp.projects").getByRole("button", { name: "team/beta", exact: true });
  await expect(beta).toBeVisible();
  await beta.focus();
  await beta.evaluate(element => { (window as any).__sidebarProject = element; });
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, { ...home, projects: [{ ...home.projects[0], machine_name: "Changed box" }, home.projects[1]] });
  await expect(page.locator("#side .cc-grp.projects").getByRole("button", { name: "team/alpha", exact: true })).toHaveAttribute("title", /Changed box/);
  await expect(beta).toBeFocused();
  expect(await beta.evaluate(element => element === (window as any).__sidebarProject)).toBe(true);
});

test("sidebar project headings fold and middle-click opens a project window", async ({ page }) => {
  const project = { name: "team/widget", url: "https://example.test/team/widget", machine_id: "box", machine_name: "Build box", folder: "/srv/widget", cloned: true };
  await openDesktop(page, false, { center_home: { machines: [], projects: [project], running: [], needs_you: [] } });
  const group = page.locator("#side .cc-grp.projects"), heading = group.getByRole("button", { name: "team/", exact: true });
  const row = group.getByRole("button", { name: "team/widget", exact: true });
  await expect(row).toBeVisible();
  await heading.click();
  await expect(row).toHaveCount(0);
  await heading.click();
  await expect(row).toBeVisible();
  await row.click({ button: "middle" });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "center_open_window" && JSON.stringify(call.args.alone) === JSON.stringify({ kind: "project", project: "team/widget" })))).toBe(true);
});

test("project counts line up with the counts of the links above", async ({ page }) => {
  const terminal = { id: "shell-one", project: "", machine_id: "build-box", machine_name: "build-box", name: "shell-one", command: "shell", cwd: "/srv/project", state: "running", code: null, started_at: 1 };
  await openDesktop(page, false, { terminals_all: [terminal] });
  const link = page.locator("#side .cc-si", { hasText: "Terminals" }).locator(".n");
  const project = page.locator("#side .cc-chat .n").first();
  await expect(project).toBeVisible();
  const right = async (n: typeof link) => Math.round((await n.boundingBox())!.x + (await n.boundingBox())!.width);
  expect(await right(project)).toBe(await right(link));
});
