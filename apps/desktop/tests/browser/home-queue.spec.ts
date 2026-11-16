import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const projects = [{ name: "example/widget", url: "", machine_id: "build-box", machine_name: "build-box", folder: "/srv/project", cloned: true, imported_at: 1 }];
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
  await expect(working.locator(".cc-pdot")).toHaveCount(1);
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
