import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

const now = Date.now();
const record = {
  repo: "example.test/example/widget", me: "dana", pickup: true, pickup_limit: null, asks: true, removed: ["lee"],
  members: [{ login: "dana", mesh: "mesh-a", joined_at: now - 2000 }, { login: "kim", mesh: "mesh-b", joined_at: now - 1000 }],
};
const issue = (number: number, assignees: string[]) => ({ number, title: `Issue ${number}`, url: `https://example.test/example/widget/issues/${number}`, author: "Example", opened_at: now - number * 1000, comments: 0, labels: [], assignees });
const lists = { issues: [issue(47, ["kim"]), issue(52, ["kim"]), issue(53, [])], prs: [], issues_error: null, prs_error: null, synced_at: now };
const teammate = { id: "kim-1", title: "Issue 47", lane: "working", login: "kim", machine: "kim-laptop", issue: 47, pr: null, updated_at: now };

async function openProject(page: Page, tile: string | null, responses: Record<string, unknown> = {}) {
  await openDesktop(page, false, { team_record: record, team_cards: { needs_you: [], teammates: [teammate] }, center_project_lists: lists, ...responses });
  if (tile) await page.evaluate(([project, tile]) => window.__navigation.open({ projects: { project, tile } }), [projectName, tile]);
  else await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await expect(page.locator(".pj-strip")).toBeVisible();
}

const called = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).map(call => call.args), command);

test("team page shows members and invite", async ({ page }) => {
  await openProject(page, "team", { team_invite: "beepteam1abc" });

  const pane = page.locator(".pj-split > .pj-det");
  await expect(page.locator('.pj-list .pj-li[title="Team"]')).toHaveAttribute("aria-current", "true");
  await expect(pane.locator(".tm-members .cc-line")).toHaveCount(2);
  await expect(pane.locator(".tm-members .cc-line").first()).toContainText("dana");
  await expect(pane.locator(".tm-members .cc-line").first()).toContainText("you");
  await expect(pane.locator(".tm-members .cc-line").nth(1)).toContainText("kim-laptop");
  await expect(pane.getByRole("button", { name: "Allow back" })).toBeVisible();

  await pane.getByRole("button", { name: "Invite", exact: true }).click();
  await expect(pane).toContainText("Anyone with push access to example/widget can join with this code.");
  await expect(pane.locator(".tm-code")).toContainText("beepteam1abc");
  await expect(page.locator(".pj-strip button").filter({ hasText: "Team" })).toContainText("2 online");
});

test("teammate card opens the issue", async ({ page }) => {
  await openProject(page, null);

  const card = page.locator(".bd-card").filter({ hasText: "Issue 47" });
  await expect(card.locator(".bd-who")).toHaveText("kim");
  await expect(page.locator(".tm-filter").getByRole("button", { name: "kim" })).toBeVisible();
  await card.click();
  await expect(page.locator('.pj-list .pj-li[title="Issues"]')).toHaveAttribute("aria-current", "true");
  await expect(page.getByRole("button", { name: "All issues" })).toBeVisible();
  expect(await called(page, "agent_start")).toEqual([]);
});

test("work on held issue asks first", async ({ page }) => {
  await openProject(page, "issues");

  await page.locator('.pj-det .pj-li[title="Issue 52"]').click({ button: "right" });
  await page.getByRole("menuitem", { name: "Work on it" }).click();
  const sheet = page.getByRole("dialog", { name: "kim has #52" });
  await expect(sheet).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Ask kim" })).toBeVisible();
  expect(await called(page, "issue_assign")).toEqual([]);

  await sheet.getByRole("button", { name: "Work on it anyway" }).click();
  await expect(sheet).toBeHidden();
  expect(await called(page, "issue_assign")).toEqual([{ projectId: expect.any(String), number: 52, logins: [] }]);

  await page.locator('.pj-det .pj-li[title="Issue 53"]').click({ button: "right" });
  await page.getByRole("menuitem", { name: "Work on it" }).click();
  await expect.poll(() => called(page, "issue_assign")).toHaveLength(2);
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("join sheet shows the import step when not imported", async ({ page }) => {
  const invite = { from: "mesh-b", login: "kim", repo: "example.test/org/api", ticket: "beepteam1xyz" };
  await openDesktop(page, false, { team_invites: [invite], team_ticket: { host: "example.test", repo: "org/api", project: null } });

  const card = page.locator("#sec-needs .cc-line").filter({ hasText: "kim invites you to org/api" });
  await expect(card).toBeVisible();
  await card.getByRole("button", { name: "Accept" }).click();
  const sheet = page.getByRole("dialog", { name: "Join org/api" });
  await expect(sheet).toContainText("Import org/api first");
  await expect(sheet.getByRole("button", { name: "Import" })).toBeVisible();
  await expect(sheet).toContainText("Teammates see your cards on this project's board and can message your agents. They never see your sessions, files, terminals or other projects.");
  expect(await called(page, "team_join")).toEqual([]);
});
