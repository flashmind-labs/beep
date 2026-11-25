import { expect, test, type Page } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

const now = Date.now();
const record = {
  repo: "example.test/example/widget", me: "dana", pickup: true, pickup_limit: null, asks: true, removed: ["lee"],
  members: [{ login: "dana", mesh: "mesh-a", joined_at: now - 2000 }, { login: "kim", mesh: "mesh-b", joined_at: now - 1000 }],
};
const issue = (number: number, assignees: string[]) => ({ number, title: `Issue ${number}`, url: `https://example.test/example/widget/issues/${number}`, author: "Example", opened_at: now - number * 1000, comments: 0, labels: [], assignees });
const lists = { issues: [issue(47, ["kim"]), issue(52, ["kim"]), issue(53, [])], prs: [], issues_error: null, prs_error: null };
const teammate = { id: "kim-1", title: "Issue 47", lane: "working", login: "kim", machine: "kim-laptop", issue: 47, pr: null, updated_at: now };
const queued = { id: "queued:53", title: "Issue 53", lane: "queued", login: "dana", machine: "build-box", issue: 53, pr: null, updated_at: now };

async function openProject(page: Page, tile: string | null, responses: Record<string, unknown> = {}) {
  await openDesktop(page, false, { team_record: record, team_cards: { needs_you: [], teammates: [teammate], own_queued: [queued], online: ["kim"] }, center_project_lists: lists, ...responses });
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
  await expect(pane.locator(".tm-invite")).toContainText("for anyone with push access to example/widget");
  await expect(pane.locator(".tm-code")).toContainText("beepteam1abc");
  await expect(page.locator(".pj-strip button").filter({ hasText: "Team" })).toContainText("2 online");
});

test("a followed teammate with no cards is online", async ({ page }) => {
  await openProject(page, "team", { team_cards: { needs_you: [], teammates: [], own_queued: [], online: ["kim"] } });

  const kim = page.locator(".pj-split > .pj-det .tm-members .cc-line").nth(1);
  await expect(kim).toContainText("online");
  await expect(kim).not.toContainText("offline");
  await expect(page.locator(".pj-strip button").filter({ hasText: "Team" })).toContainText("2 online");
});

test("teammate card opens the issue", async ({ page }) => {
  await openProject(page, null);

  const card = page.locator(".bd-card").filter({ hasText: "Issue 47" });
  await expect(card.locator(".bd-who")).toHaveText("kim");
  const mine = page.locator(".bd-lane").filter({ hasText: "Working" }).locator(".bd-card:not(.theirs)").filter({ hasText: "Issue 53" });
  await expect(mine.locator(".ft")).toHaveText("queued");
  await expect(page.locator(".tm-filter").getByRole("button", { name: "kim" })).toBeVisible();
  await card.click();
  await expect(page.locator('.pj-list .pj-li[title="Issues"]')).toHaveAttribute("aria-current", "true");
  await expect(page.getByRole("button", { name: "All issues" })).toBeVisible();
  expect(await called(page, "agent_start")).toEqual([]);
});

test("a teammate with no cards keeps the filter on the board", async ({ page }) => {
  await openProject(page, null, { team_cards: { needs_you: [], teammates: [], own_queued: [queued], online: ["kim"] } });

  const filter = page.locator(".tm-filter");
  await expect(page.locator(".bd-card").filter({ hasText: "Issue 53" })).toBeVisible();
  await filter.getByRole("button", { name: "kim" }).click();
  await expect(page.locator(".pj-board .bd-card")).toHaveCount(0);
  await expect(filter).toBeVisible();

  await filter.getByRole("button", { name: "Everyone" }).click();
  await expect(page.locator(".bd-card").filter({ hasText: "Issue 53" })).toBeVisible();
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

test("a batch start before the team answers leaves out a held issue and takes a free one", async ({ page }) => {
  const hosts = [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }];
  await openDesktop(page, false, { team_record: record, team_cards: { needs_you: [], teammates: [], own_queued: [], online: [] }, center_project_lists: lists, agent_hosts: hosts,
    agent_start: { session: { machine_id: "build-box", agent: "codex", id: "s53" }, why: "" } });
  expect(await called(page, "team_record")).toEqual([]);
  await page.evaluate(() => window.__navigation.hold("team_record"));
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "issues" } }), projectName);

  const detail = page.locator(".pj-split > .pj-det");
  await detail.getByRole("button", { name: "Select", exact: true }).click();
  for (const title of ["Issue 52", "Issue 53"]) await detail.locator(".pj-prow").filter({ has: page.locator(`.pj-li[title="${title}"]`) }).getByRole("checkbox", { name: "Pick this issue" }).check();
  await detail.getByRole("button", { name: "Start 2 agents" }).click();
  await detail.locator(".pj-start.ask").getByRole("button", { name: "Start 2 agents" }).click();
  await page.evaluate(() => window.__navigation.release("team_record"));

  await expect(detail.locator(".pj-start").filter({ hasText: "Started" })).toContainText("Started example/widget #53. Left out: example/widget #52 (kim has #52).");
  expect(await called(page, "issue_assign")).toEqual([{ projectId: expect.any(String), number: 53, logins: [] }]);
  expect((await called(page, "agent_start")).map(args => args.prompt)).toEqual([expect.stringContaining("#53")]);
});

test("join sheet automatically adds the project and joins from an invite", async ({ page }) => {
  const invite = { from: "mesh-b", login: "kim", repo: "example.test/org/api", ticket: "beepteam1xyz" };
  await openDesktop(page, false, {
    team_invites: [invite],
    team_ticket: { host: "example.test", repo: "org/api", project: null },
    team_prepare: { project: "org/api", machine_name: "build-box", folder: "/srv/org/api", added: true, cloned: true },
    team_join: { ...record, repo: "example.test/org/api", me: "dana" },
    team_invite_drop: null,
  });

  const card = page.locator("#sec-needs .cc-line").filter({ hasText: "kim invites you to org/api" });
  await expect(card).toBeVisible();
  await card.getByRole("button", { name: "Accept" }).click();
  const sheet = page.getByRole("dialog", { name: "Join org/api" });
  await expect(sheet).toContainText("org/api is on build-box");
  await expect(sheet).toContainText("Signed in to example.test as dana");
  await expect(sheet).toContainText("Teammates see your cards on this project's board and can message your agents. They never see your sessions, files, terminals or other projects.");
  await expect(sheet.getByRole("button", { name: "Open project" })).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Done" })).toBeVisible();
  expect(await called(page, "team_prepare")).toEqual([{ ticket: "beepteam1xyz" }]);
  expect(await called(page, "team_join")).toEqual([{ ticket: "beepteam1xyz" }]);
});

test("join team from Projects pastes a code and shows manual import when adding fails", async ({ page }) => {
  await openDesktop(page, false, {
    team_ticket: { host: "example.test", repo: "org/api", project: null },
  });
  await page.evaluate(() => window.__navigation.rejectOnce("team_prepare"));
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));

  await page.getByRole("button", { name: "Join team" }).click();
  const sheet = page.getByRole("dialog", { name: "Join a team" });
  await sheet.getByRole("textbox", { name: "Team code" }).fill("beepteam1xyz");
  await sheet.getByRole("button", { name: "Join", exact: true }).click();

  const named = page.getByRole("dialog", { name: "Join org/api" });
  await expect(named).toContainText("Could not add org/api");
  await expect(named.locator(".am-msg.bad")).toHaveText("Requested test failure");
  await expect(named.getByRole("button", { name: "Import manually" })).toBeVisible();
  await expect(named.getByRole("button", { name: "Try again" })).toBeVisible();
  expect(await called(page, "team_join")).toEqual([]);
});

test("a held issue's start that fails says why in the sheet", async ({ page }) => {
  const detail = { kind: "issue", number: 52, title: "Issue 52", url: issue(52, []).url, author: "Example", state: "open", draft: false, labels: [], opened_at: now, body_html: "<p>Fee model</p>", comments: [], earlier: 0, review: "none", checks: null, linked_prs: [] };
  await openProject(page, "issues", {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    agent_models: { pick: "listed", current: "alpha", models: [{ id: "alpha", name: "Alpha", description: null }] },
    center_thread_detail: detail,
  });

  await page.locator('.pj-det .pj-li[title="Issue 52"]').click();
  await page.locator(".pj-split > .pj-det").getByRole("button", { name: "Work on it" }).click();
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  await picker.locator('[data-agmachine="build-box"]').click();
  await picker.locator('[data-agh="codex"]').click();
  await picker.locator('[data-agmo="alpha"]').click();
  await picker.locator('[data-age=""]').click();
  const sheet = page.getByRole("dialog", { name: "kim has #52" });
  await expect(sheet).toBeVisible();

  await page.evaluate(() => window.__navigation.rejectOnce("agent_begin"));
  await sheet.getByRole("button", { name: "Work on it anyway" }).click();

  await expect(sheet.locator(".am-msg.bad")).toHaveText("Requested test failure");
  await expect(sheet.getByRole("button", { name: "Work on it anyway" })).toBeEnabled();
  expect(await called(page, "issue_assign")).toHaveLength(1);
});

test("a project whose team cannot be read is asked again on the next projects push, and a pushed team shows", async ({ page }) => {
  await page.clock.install();
  await openDesktop(page, false, { center_project_lists: lists });
  await page.evaluate(() => window.__navigation.rejectAll("team_record"));
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "machines" } }), projectName);
  await expect(page.locator(".pj-strip")).toBeVisible();

  // No timer asks again.
  await page.clock.fastForward(120_000);
  const asked = await called(page, "team_record");
  expect(asked.length).toBe(1);
  await expect(page.locator('.pj-list .pj-li[title="Team"]')).toHaveCount(0);
  await expect(page.locator(".pj-strip button").filter({ hasText: "Team" })).toHaveCount(0);

  // A projects push says the record may have landed: the next read works.
  await page.evaluate(team => { window.__navigation.respond("team_record", team); window.__navigation.allow("team_record"); window.__navigation.notify("projects"); }, record);
  await expect.poll(async () => (await called(page, "team_record")).length).toBe(2);

  await expect(page.locator(".pj-strip button").filter({ hasText: "Team" })).toBeVisible();
  await page.evaluate(args => window.__navigation.emit("team-changed", { project_id: args.projectId, record: args.record, cards: { needs_you: [], teammates: [], own_queued: [], online: ["kim"] } }), { projectId: asked[0].projectId, record });
  await expect(page.locator(".pj-strip button").filter({ hasText: "Team" })).toContainText("2 online");
});

test("a teammate's card arrives from the push, with no read on a timer", async ({ page }) => {
  await page.clock.install();
  await openProject(page, null);
  await expect(page.locator(".bd-card").filter({ hasText: "Issue 47" })).toBeVisible();
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "team_record" || call.command === "team_cards").length);
  const before = await reads();
  const projectId = (await called(page, "team_record"))[0].projectId;

  await page.clock.fastForward(120_000);
  expect(await reads()).toBe(before);

  const arrived = { ...teammate, id: "kim-2", title: "Issue 52", issue: 52 };
  await page.evaluate(args => window.__navigation.emit("team-changed", { project_id: args.projectId, record: args.record, cards: { needs_you: [], teammates: [args.card], own_queued: [], online: ["kim"] } }), { projectId, record, card: arrived });
  await expect(page.locator(".bd-card").filter({ hasText: "Issue 52" })).toBeVisible();
  await expect(page.locator(".bd-card").filter({ hasText: "Issue 47" })).toHaveCount(0);
  expect(await reads()).toBe(before);
});

test("an invite arrives from the push, with no read on a timer", async ({ page }) => {
  await page.clock.install();
  await openDesktop(page, false, { team_invites: [] });
  const asked = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "team_invites").length);
  const before = await asked();
  await page.clock.fastForward(300_000);
  expect(await asked()).toBe(before);

  const invite = { from: "mesh-b", login: "kim", repo: "example.test/org/api", ticket: "beepteam1xyz" };
  await page.evaluate(list => window.__navigation.emit("team-invites", list), [invite]);
  await expect(page.locator("#sec-needs .cc-line").filter({ hasText: "kim invites you to org/api" })).toBeVisible();
  expect(await asked()).toBe(before);
});

test("a project without a team still shows Team in the strip", async ({ page }) => {
  await openProject(page, null, { team_record: null });

  const pill = page.locator(".pj-strip button").filter({ hasText: "Team" });
  await expect(pill).toHaveText("Team");
  await pill.click();
  await expect(page.locator('.pj-list .pj-li[title="Team"]')).toHaveAttribute("aria-current", "true");
  await expect(page.locator(".pj-split > .pj-det")).toContainText("No team yet");
});

test("a contact is invited or removed from the team page", async ({ page }) => {
  const lee = { mesh: "mesh-c", logins: [{ repo: "example.test/example/api", login: "lee" }] };
  await openProject(page, "team", { contacts: [lee], contact_remove: null });

  const row = page.locator(".tm-contacts .cc-line").filter({ hasText: "lee" });
  await expect(row).toContainText("example/api");
  await row.hover();
  await row.getByRole("button", { name: "Remove" }).click();
  const sheet = page.getByRole("dialog", { name: "Remove lee?" });
  await sheet.getByRole("button", { name: "Remove" }).click();

  await expect(row).toHaveCount(0);
  expect(await called(page, "contact_remove")).toEqual([{ contact: "mesh-c" }]);
});

test("an ask card carries the whole ask", async ({ page }) => {
  const text = "Who owns the fee model? ".repeat(12).trim();
  const ask = { id: "ask:a1", title: text, lane: "needs_you", login: "kim", machine: "", issue: null, pr: null, updated_at: now };
  await openProject(page, null, { team_cards: { needs_you: [ask], teammates: [], own_queued: [] } });

  const card = page.locator(".bd-card").filter({ hasText: "asks you" });
  await expect(card).toHaveAttribute("title", text);
});
