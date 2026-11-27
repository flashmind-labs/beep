import { expect, test, type Locator, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const projects = [{ name: "example/widget", url: "", machine_id: "build-box", machine_name: "build-box", folder: "/srv/project", cloned: true }];
const row = (id: string, title: string, status: string) => ({ kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title, cwd: "/srv/project",
  status, doing: status === "working" ? "running the tests" : null, started_at: 1, last_activity: 1, external: false, question: null });
const question = { id: 7, text: "Restart the web server?", detail: "systemctl restart web", options: [{ id: "yes", name: "Allow", allow: true }, { id: "no", name: "Deny", allow: false }] };
const home = {
  machines: [], projects,
  running: [row("asks", "Rotate certs", "asking"), row("done-turn", "Lazy-load charts", "idle"), row("busy", "Add rate limits", "working")],
  needs_you: [{ kind: "agent", machine_id: "build-box", machine_name: "build-box", agent: "codex", session: "asks", title: "Rotate certs", question }],
};

const calls = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).map(call => call.args), command);

test("Home answers a question in place", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, projects_sessions: [] });
  const queue = page.locator("#sec-needs");
  await expect(queue).toContainText("Restart the web server?");
  await expect(queue).toContainText("systemctl restart web");

  await page.evaluate(() => window.__navigation.hold("center_answer_agent"));
  await queue.getByRole("button", { name: "Allow", exact: true }).click();
  await expect(queue.getByRole("button", { name: "Allow", exact: true })).toBeDisabled();
  await expect(queue.getByRole("button", { name: "Deny", exact: true })).toBeDisabled();
  expect(await calls(page, "center_answer_agent")).toEqual([expect.objectContaining({ machineId: "build-box", agent: "codex", session: "asks", question: 7, reply: "yes" })]);
  // The answer stays on Home; the chat does not open.
  await expect(queue).toBeVisible();
});

test("a failed answer gives the buttons back", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, projects_sessions: [] });
  await page.evaluate(() => window.__navigation.rejectOnce("center_answer_agent"));
  const allow = page.locator("#sec-needs").getByRole("button", { name: "Allow", exact: true });
  await allow.click();
  await expect(allow).toBeEnabled();
});

test("Home's lanes have no project rows or sort", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, projects_sessions: [] });
  await expect(page.locator("#sec-running .bd-lane > h4")).toHaveText(["Working1", "Ready to review1", "Done today"]);
  await expect(page.locator(".bd-rows")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Sort projects" })).toHaveCount(0);
  const working = page.locator("#sec-running .bd-card", { hasText: "Add rate limits" });
  await expect(working).toContainText("running the tests");
  // The project's dot is drawn, on a card and on a queue row.
  const width = (dot: Locator) => dot.evaluate(element => element.getBoundingClientRect().width);
  expect(await width(working.locator(".cc-pdot"))).toBeGreaterThan(4);
  expect(await width(page.locator("#sec-needs .cc-row .cc-pdot"))).toBeGreaterThan(4);
});

test("a review card closes its session", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, projects_sessions: [] });
  const card = page.locator("#sec-running .bd-card", { hasText: "Lazy-load charts" });
  await expect(card.getByRole("button", { name: "Close" })).toBeHidden();
  await card.hover();
  await card.getByRole("button", { name: "Close" }).click();
  await expect.poll(() => calls(page, "agent_steer")).toContainEqual(expect.objectContaining({ session: "done-turn", steer: "close" }));
});

test("Close shows without hover on a phone", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 800 });
  await openDesktop(page, false, { center_home: home, projects_sessions: [] });
  await expect(page.locator("#sec-running .bd-card", { hasText: "Lazy-load charts" }).getByRole("button", { name: "Close" })).toBeVisible();
});

test("on a phone a question shows no answer buttons and keeps its text readable", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 800 });
  await openDesktop(page, false, { center_home: home, projects_sessions: [] });
  const row = page.locator("#sec-needs .cc-row").first();
  await expect(row.getByRole("button", { name: "Allow", exact: true })).toBeHidden();
  expect((await row.locator(".t b").boundingBox())?.width).toBeGreaterThan(250);
});

test("the session's next question gets working buttons", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, projects_sessions: [] });
  const queue = page.locator("#sec-needs");
  await queue.getByRole("button", { name: "Allow", exact: true }).click();
  await expect(queue.getByRole("button", { name: "Allow", exact: true })).toBeDisabled();
  const next = { ...home, needs_you: [{ ...home.needs_you[0], question: { ...question, id: 8, text: "Restart the cache too?" } }] };
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, next);
  await expect(queue).toContainText("Restart the cache too?");
  await expect(queue.getByRole("button", { name: "Allow", exact: true })).toBeEnabled();
});

test("a question with no option to allow opens the chat to answer", async ({ page }) => {
  const text = { ...question, text: "Which port should it use?", detail: null, options: [{ id: "skip", name: "Skip", allow: false }] };
  await openDesktop(page, false, { center_home: { ...home, needs_you: [{ ...home.needs_you[0], question: text }] }, projects_sessions: [] });
  const queue = page.locator("#sec-needs");
  await expect(queue.getByRole("button", { name: "Skip" })).toHaveCount(0);
  await expect(queue.getByRole("button", { name: "Answer" })).toBeVisible();
});

test("long option names leave the question room", async ({ page }) => {
  const long = { ...question, options: ["Allow this command once for this session", "Always allow commands like this one here", "Allow for every project on this machine", "Deny and tell the agent what to do instead"].map((name, index) => ({ id: `o${index}`, name, allow: index < 3 })) };
  await openDesktop(page, false, { center_home: { ...home, needs_you: [{ ...home.needs_you[0], question: long }] }, projects_sessions: [] });
  expect((await page.locator("#sec-needs .cc-row > .t").boundingBox())?.width).toBeGreaterThan(250);
});

test("Done today rows keep to one line", async ({ page }) => {
  const ended = { ...row("old", "Fix the login test", "ended"), reviewer: false, parent: null, last_activity: Date.now() };
  await openDesktop(page, false, { center_home: home, projects_sessions: [ended], session_moves: [], session_copies: [] });
  const done = page.locator("#sec-running .bd-log .cc-row", { hasText: "Fix the login test" });
  await expect(done).toBeVisible();
  await expect(done.locator(".cc-pdot")).toHaveCount(0);
});

test.describe("on a touch screen without hover", () => {
  test.use({ hasTouch: true, isMobile: true, viewport: { width: 1180, height: 820 } });
  test("Close shows on a review card", async ({ page }) => {
    await openDesktop(page, false, { center_home: home, projects_sessions: [] });
    await expect(page.locator("body")).not.toHaveClass(/mobile/);
    await expect(page.locator("#sec-running .bd-card", { hasText: "Lazy-load charts" }).getByRole("button", { name: "Close" })).toBeVisible();
  });
});

test("a full lane scrolls on its own and Home fits the window", async ({ page }) => {
  const idle = Array.from({ length: 25 }, (_, index) => row(`idle-${index}`, `Finished turn ${index}`, "idle"));
  await openDesktop(page, false, { center_home: { ...home, running: [...home.running, ...idle] }, projects_sessions: [] });
  const lane = page.locator("#sec-running .bd-lane.cell").nth(1);
  await expect(lane.locator(".bd-card")).toHaveCount(26);
  await expect.poll(() => lane.evaluate(element => element.scrollHeight > element.clientHeight)).toBe(true);
  await expect.poll(() => page.locator("#view").evaluate(element => element.scrollHeight - element.clientHeight)).toBeLessThanOrEqual(1);
});

test("a crowded lane spreads its cards over columns and an empty one stays narrow", async ({ page }) => {
  await page.setViewportSize({ width: 1800, height: 1000 });
  const idle = Array.from({ length: 25 }, (_, index) => row(`idle-${index}`, `Finished turn ${index}`, "idle"));
  await openDesktop(page, false, { center_home: { ...home, needs_you: [], running: [row("busy", "Add rate limits", "working"), ...idle] }, projects_sessions: [] });
  const lanes = page.locator("#sec-running .bd-lanes > .bd-lane");
  const review = lanes.nth(1), done = lanes.nth(2);
  await expect(review.locator(".bd-card")).toHaveCount(25);
  const columns = await review.locator(".bd-card").evaluateAll(cards => new Set(cards.map(card => Math.round(card.getBoundingClientRect().left))).size);
  expect(columns).toBeGreaterThanOrEqual(2);
  const [reviewBox, doneBox] = await Promise.all([review.boundingBox(), done.boundingBox()]);
  expect(doneBox?.width ?? 0).toBeLessThan((reviewBox?.width ?? 0) / 2);
});

test("Working takes one wide column, Ready to review two of the same width", async ({ page }) => {
  await page.setViewportSize({ width: 2000, height: 1300 });
  const busy = Array.from({ length: 6 }, (_, index) => row(`busy-${index}`, `Working on ${index}`, "working"));
  const idle = Array.from({ length: 23 }, (_, index) => row(`idle-${index}`, `Finished turn ${index}`, "idle"));
  const ended = Array.from({ length: 6 }, (_, index) => ({ ...row(`ended-${index}`, `Ended ${index}`, "ended"), reviewer: false, parent: null, last_activity: Date.now() - index }));
  await openDesktop(page, false, { center_home: { ...home, needs_you: [], running: [...busy, ...idle] }, projects_sessions: ended, session_moves: [], session_copies: [] });
  const lanes = page.locator("#sec-running .bd-lanes > .bd-lane");
  await expect(lanes.nth(2).locator(".cc-row")).toHaveCount(6);
  const columns = (lane: number) => lanes.nth(lane).locator(".bd-card").evaluateAll(cards => [...new Map(cards.map(card => { const box = card.getBoundingClientRect(); return [Math.round(box.left), box.width]; })).values()]);
  const [working, review] = await Promise.all([columns(0), columns(1)]);
  expect(working).toHaveLength(1);
  expect(review).toHaveLength(2);
  expect(Math.abs(working[0] - review[0])).toBeLessThan(working[0] * 0.1);
});

test("the queue's buttons share one width", async ({ page }) => {
  const pull = (number: number, repo: string, extra: Record<string, unknown>) => ({ host: "example.test", repo, number, kind: "pull_request", title: `Pull ${number}`, url: `https://example.test/${repo}/pull/${number}`,
    updated_at: number, authored: false, assigned: false, review_requested: true, draft: false, review: "none", checks_failing: false, ...extra });
  await openDesktop(page, false, { git_items: { items: [pull(1, "example/widget", {}), pull(2, "other/site", {})], hosts: [] },
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }] });
  const queue = page.locator("#sec-needs");
  await expect(queue.getByRole("button", { name: "Review", exact: true })).toBeVisible();
  const widths = await queue.locator(".cc-row > .r :is(.dbtn, .dv-fb):visible").evaluateAll(buttons => buttons.map(button => Math.round(button.getBoundingClientRect().width)));
  expect(widths.length).toBeGreaterThan(1);
  expect(new Set(widths).size).toBe(1);
});
