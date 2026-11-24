import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

// Long names, folders, many machines, groups and outside items: everything
// that can push the Projects overview wider than a phone.
const long = "a-very-long-repository-name-that-keeps-going-without-any-break-at-all";
const machines = Array.from({ length: 6 }, (_, i) => ({ id: `box-${i}`, name: `build-box-with-a-long-name-${i}` }));
const rows = [
  ...machines.map(m => ({ name: `example/${long}`, url: `https://example.test/example/${long}`, machine_id: m.id, machine_name: m.name, folder: `/srv/work/deeply/nested/folder/${long}`, cloned: true })),
  { name: `other-org-with-a-long-name/${long}-2`, url: "https://example.test/o/p", machine_id: "box-0", machine_name: machines[0].name, folder: `/srv/other/${long}-2`, cloned: true },
  { name: "plain-folder-without-an-org-and-a-long-name-too", url: "", machine_id: "box-1", machine_name: machines[1].name, folder: "/srv/plain", cloned: false },
];
const home = { machines: [], projects: rows, running: [], needs_you: [] };
const groups = [{ id: "g", name: `A group with a long name ${long}`, paths: [`work/deeply/nested/folder`, `another/very/long/group/folder/path/${long}`] }];
const found = [{ name: `found-org/${long}`, remote: "https://example.test/found", places: machines.map(m => ({ machine_id: m.id, machine_name: m.name, folder: `/srv/found/${long}` })), agents: [{ agent: "codex", sessions: 2 }, { agent: "claude", sessions: 9 }], last_activity: 1 }];
const item = { host: "example.test", repo: `outside-org/${long}`, number: 12345, kind: "pull_request", title: `Outside change ${long}`, url: "https://example.test/outside/pull/7", updated_at: 1, authored: false, assigned: false, review_requested: true, draft: false, review: "none", checks_failing: false, labels: ["review", "a-long-label-name"] };

async function openOverview(page: Page, width: number) {
  await page.setViewportSize({ width, height: 800 });
  await openDesktop(page, false, { center_home: home, project_groups: groups, found_projects: found, git_items: { items: [item], hosts: [] } });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await expect(page.locator("body")).toHaveClass(/mobile/);
  await expect(page.locator(".pj-card").first()).toBeVisible();
  await expect(page.locator("#sec-found")).toBeVisible();
}

/// What sticks out past the window's right edge without an ancestor clipping
/// it, and what scrolls sideways.
const overflowing = (page: Page) => page.evaluate(() => {
  const width = document.documentElement.clientWidth;
  const clips = (style: CSSStyleDeclaration) => style.overflowX !== "visible";
  const clipped = (el: HTMLElement) => {
    for (let up = el.parentElement; up; up = up.parentElement) if (clips(getComputedStyle(up)) && up.getBoundingClientRect().right <= width + 0.5) return true;
    return false;
  };
  const name = (el: HTMLElement) => `${el.tagName.toLowerCase()}.${String(el.className)}`;
  const all = [...document.querySelectorAll<HTMLElement>(".cwin *")].filter(el => el.getClientRects().length);
  const outside = all.filter(el => el.getBoundingClientRect().right > width + 0.5 && !clipped(el)).map(name);
  const scrolls = all.filter(el => /auto|scroll/.test(getComputedStyle(el).overflowX) && el.scrollWidth > el.clientWidth + 1).map(el => `scroll ${name(el)}`);
  return [...outside, ...scrolls];
});

for (const width of [375, 320]) {
  test(`projects overview fits a ${width} px phone`, async ({ page }) => {
    await openOverview(page, width);
    expect(await overflowing(page)).toEqual([]);
  });
}

test("projects overview end fits a phone", async ({ page }) => {
  await openOverview(page, 375);
  await page.locator(".cc-view[data-project]").evaluate(el => { el.scrollTop = el.scrollHeight; });
  expect(await overflowing(page)).toEqual([]);
});
