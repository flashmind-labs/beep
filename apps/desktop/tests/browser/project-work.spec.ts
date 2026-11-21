import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

const row = (id: string, title: string, status: string, doing: string | null, cwd: string) => ({ kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title, cwd, status, doing, reply_html: status === "idle" ? "Done. Tests pass." : null, started_at: 1, last_activity: 2, external: false, question: null });
const project = (name: string) => ({ name, url: `https://example.test/${name}`, machine_id: "build-box", machine_name: "build-box", folder: `/srv/${name}`, cloned: true, imported_at: 1 });
const home = {
  machines: [{ id: "build-box", name: "build-box", online: true, can_host: true, seen_at: 1, version: "0.9.0", env: null, joined: true, apps: [], gpus: [], capabilities: [], agents: 2, stats: null }],
  projects: [project(projectName), project("example/gadget")],
  running: [row("one", "Fix the login redirect", "working", "Edit src/login.ts", `/srv/${projectName}`), row("two", "Update the changelog", "idle", null, `/srv/${projectName}`)],
  needs_you: [],
};

test("the Projects page lists the active projects on top, and a row opens its session", async ({ page }) => {
  await openDesktop(page, false, { center_home: home });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  const boxes = page.locator(".bd-projects > div"), box = boxes.filter({ hasText: projectName });

  // A project with nothing open has its card and no box.
  await expect(boxes).toHaveCount(1);
  await expect(box.locator(".cc-line")).toHaveCount(2);
  await expect(box).toContainText("build-box · Edit src/login.ts");
  await expect(box).toContainText("Done. Tests pass.");
  await expect(page.locator(".pj-card")).toHaveCount(2);
  await expect(page.locator(".pj-card .cc-line")).toHaveCount(0);

  // A search is for the cards.
  await page.getByRole("searchbox", { name: "Search projects" }).fill("gadget");
  await expect(boxes).toHaveCount(0);
  await expect(page.locator(".pj-card")).toHaveCount(1);
  await page.getByRole("searchbox", { name: "Search projects" }).fill("");

  await box.getByRole("button", { name: "Update the changelog" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_view").map(call => call.args.session))).toContain("two");
  await expect(page.locator(".pj-card")).toHaveCount(0);
});

test("Home lists the open sessions, the finished ones from the latest or the oldest, and keeps the choice", async ({ page }) => {
  const now = Date.now();
  const ended = { machine_id: "build-box", machine_name: "build-box", id: "three", agent: "codex", title: "Bump the parser", cwd: "/srv/example/gadget", status: "ended", doing: null, started_at: now - 1000, last_activity: now, external: false, reviewer: false, parent: null };
  const later = { ...row("four", "Sort the tables", "idle", null, "/srv/example/gadget"), last_activity: now };
  await openDesktop(page, false, { center_home: { ...home, running: [...home.running, later] }, session_moves: [], session_copies: [], projects_sessions: [ended] });
  await expect(page.locator("#sec-running").getByText("Bump the parser")).toBeVisible();

  await page.getByRole("radio", { name: "List" }).click();
  const sections = page.locator(".bd-list > div"), working = sections.nth(0), finished = sections.nth(1).locator(".cc-line");
  await expect(working).toContainText("build-box · Edit src/login.ts");
  await expect(working.locator(".bd-tag")).toHaveText(projectName);
  await expect(finished).toHaveText([/^Sort the tables/, /^Update the changelog/]);
  await expect(finished.first().locator(".bd-tag")).toHaveText("example/gadget");
  await expect(page.locator(".bd-lanes")).toHaveCount(0);
  // An ended session is left out.
  await expect(page.locator("#sec-running").getByText("Bump the parser")).toHaveCount(0);

  // A row has no Close: its time stays in place.
  const idle = finished.filter({ hasText: "Update the changelog" });
  await idle.hover();
  await expect(idle.getByRole("button", { name: "Close" })).toHaveCount(0);

  await page.getByRole("radio", { name: "Oldest" }).click();
  await expect(finished).toHaveText([/^Update the changelog/, /^Sort the tables/]);

  await page.reload();
  await expect(finished).toHaveText([/^Update the changelog/, /^Sort the tables/]);
  await expect(page.getByRole("radio", { name: "List" })).toHaveAttribute("aria-checked", "true");
});
