import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const project = (name: string, folder: string, url = `https://example.test/${name}`) => ({ name, url, machine_id: "build-box", machine_name: "build-box", folder, cloned: true, imported_at: 1 });
const agent = (id: string, cwd: string, last_activity: number) => ({ kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: id, cwd, status: "working", doing: null, started_at: 1, last_activity, external: false, question: null });
const projects = [
  project("acme/alpha", "/srv/acme/alpha"),
  project("acme/beta", "/srv/acme/beta"),
  project("acme/gamma", "/srv/acme/gamma"),
  project("tools/delta", "/srv/tools/delta", "https://forge.test/team/delta-remote"),
];
const home = { machines: [], projects, running: [agent("old", "/srv/acme/alpha", 1_000), agent("new", "/srv/acme/gamma", 5_000)], needs_you: [] };

async function openOverview(page: Page) {
  await openDesktop(page, false, { center_home: home });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await expect(page.locator(".pj-card")).toHaveCount(4);
}

const cards = (page: Page) => page.locator(".pj-card .h b").allTextContents();

test("projects are listed by recent activity within their org", async ({ page }) => {
  await openOverview(page);
  expect(await cards(page)).toEqual(["gamma", "alpha", "beta", "delta"]);
});

test("search filters across groups and keeps its text across pages", async ({ page }) => {
  await openOverview(page);
  const field = page.getByRole("searchbox", { name: "Search projects" });
  const reads = await page.evaluate(() => window.__navigation.calls().length);

  await field.fill("BETA");
  await expect(page.locator(".pj-card")).toHaveCount(1);
  expect(await cards(page)).toEqual(["beta"]);
  await expect(page.locator(".pj-org")).toHaveCount(1);

  await field.fill("delta-remote");
  expect(await cards(page)).toEqual(["delta"]);
  await field.fill("/srv/acme");
  await expect(page.locator(".pj-card")).toHaveCount(3);
  expect(await page.evaluate(() => window.__navigation.calls().length)).toBe(reads);

  await field.fill("nothing");
  await expect(page.getByText("No projects match.")).toBeVisible();
  await expect(page.locator(".pj-org")).toHaveCount(0);

  await field.fill("gamma");
  await page.evaluate(() => window.__navigation.open("home"));
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await expect(field).toHaveValue("gamma");
  expect(await cards(page)).toEqual(["gamma"]);

  await field.focus();
  await field.press("Escape");
  await expect(field).toHaveValue("");
  await expect(page.locator(".pj-card")).toHaveCount(4);
  await expect(page.locator(".cc-view[data-project]")).toBeVisible();
});

test("slash and the find shortcut focus the search", { tag: "@windows" }, async ({ page }, info) => {
  await openOverview(page);
  const field = page.getByRole("searchbox", { name: "Search projects" });
  await page.locator(".pj-card").first().focus();
  await page.keyboard.press("/");
  await expect(field).toBeFocused();
  await expect(field).toHaveValue("");

  await page.locator(".pj-card").first().focus();
  await page.keyboard.press(info.project.name === "Windows" ? "Control+f" : "Meta+f");
  await expect(field).toBeFocused();
});
