import { expect, test, type Page } from "@playwright/test";
import { openDesktop, pageErrors, projectName } from "./harness.ts";

const reads = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).length, command);

// A page error means React and a retained host fought over a node.
let errors: string[] = [];
test.beforeEach(({ page }) => {
  errors = pageErrors(page);
});
test.afterEach(() => { expect(errors).toEqual([]); });

async function refreshProjects(page: Page) {
  const before = await reads(page, "projects_on_top");
  await page.evaluate(() => window.__navigation.notify("projects"));
  await expect.poll(() => reads(page, "projects_on_top")).toBeGreaterThan(before);
}

test("your threads keep the page, detail column, scroll and focus through refreshes", async ({ page }) => {
  const now = Date.now();
  const threads = Array.from({ length: 12 }, (_, i) => ({ kind: i % 2 ? "issue" : "pr", repo: projectName, number: i + 1, title: `Thread ${i + 1}`, url: `https://example.test/pull/${i + 1}`, last_other: "Reviewer", needs_reply: i === 0, updated_at: now }));
  await openDesktop(page, false, { center_project_lists: { issues: [], prs: [], issues_error: null, prs_error: null, synced_at: now }, center_project_threads: threads, center_thread_chats: [{ chat_id: "chat-one", kind: "pr", repo: projectName, number: 1, title: "About PR 1" }] });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "threads" } }), projectName);
  await page.locator('.pj-list .pj-li[title="Your pull requests and issues"]').click();

  const view = page.locator("#main .cc-view.split"), det = page.locator(".pj-split > .pj-det");
  await expect(det.locator(".pj-task.pj-issue")).toHaveCount(12);
  await expect(det.locator(".pj-task.pj-issue").first().locator(".chip")).toHaveText("needs reply");
  await expect(det.getByRole("button", { name: "Let the agent reply" })).toHaveClass("pri");
  await expect(det.locator(".cc-sec h4").filter({ hasText: "Chats about one thread" })).toBeVisible();
  await view.evaluate(el => { el.dataset.identity = "view"; });
  await det.evaluate(el => { el.dataset.identity = "det"; el.style.maxHeight = "160px"; el.scrollTop = 120; });
  const top = await det.evaluate(el => el.scrollTop);
  expect(top).toBeGreaterThan(0);
  const open = det.locator(".pj-task.pj-issue").nth(6).getByRole("button", { name: "Open", exact: true });
  await open.evaluate(el => { el.dataset.identity = "kept"; });
  await open.focus();
  await det.evaluate(el => { el.scrollTop = 120; });

  await refreshProjects(page);
  await refreshProjects(page);
  await expect(view).toHaveAttribute("data-identity", "view");
  await expect(det).toHaveAttribute("data-identity", "det");
  await expect(open).toHaveAttribute("data-identity", "kept");
  await expect(open).toBeFocused();
  await expect(det).toHaveJSProperty("scrollTop", top);

  await det.getByRole("button", { name: "Refresh" }).click();
  await expect.poll(() => reads(page, "center_project_threads")).toBeGreaterThan(1);
  await open.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_url").map(call => call.args.url))).toContain("https://example.test/pull/7");
});

test("needs you cards answer once and keep their buttons through refreshes", async ({ page }) => {
  const card = { run_id: "run-1", query_id: "query-1", approval: { app: "Forge", action: "Open a pull request", fields: [{ label: "Title", value: "Example change" }] } };
  const agent = { project_id: "project-1", name: projectName, chat_id: "chat-1", open_tasks: 0, open_workers: 0 };
  await openDesktop(page, false, {
    status: { link: "linked", signed_out: false, server: true, live: false, session: null }, center_projects: [agent],
    center_project: { tasks: [], workers: [], turns: [], cards: [card] },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "needs" } }), projectName);
  await page.locator('.pj-list .pj-li[title="Needs you"]').click();

  const section = page.locator(".pj-det > #sec-project-needs");
  await expect(section.locator(".pj-plan b")).toHaveText("Open a pull request");
  await expect(section.locator(".pj-plan ol li")).toHaveText("Title Example change");
  const approve = section.getByRole("button", { name: "Approve" });
  await approve.evaluate(el => { el.dataset.identity = "kept"; });
  await approve.focus();
  await refreshProjects(page);
  await expect(approve).toHaveAttribute("data-identity", "kept");
  await expect(approve).toBeFocused();

  await page.evaluate(() => window.__navigation.hold("answer_approval"));
  await approve.click();
  await expect(section.locator(".pj-plan .ok")).toHaveText("Approved");
  await expect(section.locator(".pj-plan")).toHaveCSS("opacity", "0.55");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "answer_approval").map(call => call.args.decision))).toEqual(["allow"]);
  await page.evaluate(() => window.__navigation.release("answer_approval"));
});

test("sessions tab keeps its list host while tabs switch", async ({ page }) => {
  const past = [{ machine_id: "build-box", machine_name: "build-box", id: "past-one", agent: "codex", title: "Past session", cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 1, external: false, question: null }];
  await openDesktop(page, false, { project_sessions: past });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  const det = page.locator(".pj-split > .pj-det"), tabs = det.locator(":scope > .cc-tabs.pj-tabs");
  await expect(tabs.getByRole("tab", { name: /Sessions/ })).toHaveAttribute("aria-selected", "true");
  await expect(det.locator(":scope > .cc-panel .pv-row").first()).toBeVisible();
  const list = det.locator(":scope > .cc-panel").first();
  await list.evaluate(el => { el.dataset.identity = "list"; });
  await tabs.getByRole("tab", { name: /Agents/ }).click();
  await expect(det.locator(":scope > #sec-custom-agents")).toBeVisible();
  await expect(det.locator('[data-identity="list"]')).toHaveCount(0);
  await tabs.getByRole("tab", { name: /Sessions/ }).click();
  await expect(det.locator(':scope > [data-identity="list"]')).toBeVisible();
  await expect(det.locator(":scope > .cc-tabs.pj-tabs + .cc-panel")).toHaveAttribute("data-identity", "list");
});

test("projects page shows its empty state with an Add a project button", async ({ page }) => {
  await openDesktop(page, false, { found_machines: [], center_home: { machines: [], projects: [], running: [], needs_you: [], coming_up: null, updates: null } });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  const empty = page.locator(`#main .cc-view[data-project=""] > .cc-panel.cc-empty`);
  await expect(empty.locator("b")).toHaveText("No projects yet");
  await expect(empty.locator("span")).toHaveText("A folder or a repository to clone.");
  await expect(empty.locator("button.pri svg")).toHaveCount(1);
  await expect(empty.getByRole("button", { name: "Add a project" })).toBeVisible();
  await expect(page.locator("#main .cc-view[data-project]")).toHaveCount(1);
});
