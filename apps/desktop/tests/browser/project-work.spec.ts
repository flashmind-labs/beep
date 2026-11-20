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

test("a project card says what its sessions do, and a row opens its session", async ({ page }) => {
  await openDesktop(page, false, { center_home: home });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  const busy = page.locator(".pj-card", { hasText: "widget" }), quiet = page.locator(".pj-card", { hasText: "gadget" });

  await expect(busy.locator(".pj-work .cc-line")).toHaveCount(2);
  await expect(busy).toContainText("Fix the login redirect");
  await expect(busy).toContainText("Edit src/login.ts");
  await expect(busy).toContainText("Done. Tests pass.");
  await expect(quiet.locator(".pj-work")).toHaveCount(0);

  await busy.getByRole("button", { name: "Update the changelog" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_view").map(call => call.args.session))).toContain("two");
  await expect(page.locator(".pj-card")).toHaveCount(0);
});

test("Home lists the open sessions of each project, and keeps the choice", async ({ page }) => {
  const now = Date.now();
  const ended = { machine_id: "build-box", machine_name: "build-box", id: "three", agent: "codex", title: "Bump the parser", cwd: "/srv/example/gadget", status: "ended", doing: null, started_at: now - 1000, last_activity: now, external: false, reviewer: false, parent: null };
  await openDesktop(page, false, { center_home: home, session_moves: [], session_copies: [], projects_sessions: [ended] });
  await expect(page.locator("#sec-running").getByText("Bump the parser")).toBeVisible();

  await page.getByRole("radio", { name: "Projects" }).click();
  const box = page.locator(".bd-projects > div", { hasText: projectName });
  await expect(box.locator(".cc-line")).toHaveCount(2);
  await expect(box).toContainText("build-box · Edit src/login.ts");
  await expect(box).toContainText("Done. Tests pass.");
  await expect(page.locator(".bd-lanes")).toHaveCount(0);
  // A project whose sessions all ended is left out.
  await expect(page.locator(".bd-projects > div")).toHaveCount(1);
  await expect(page.locator("#sec-running").getByText("Bump the parser")).toHaveCount(0);

  // An idle session closes from its row.
  const idle = box.locator(".cc-line", { hasText: "Update the changelog" });
  await expect(idle.getByRole("button", { name: "Close" })).toBeHidden();
  await idle.hover();
  await idle.getByRole("button", { name: "Close" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_steer").map(call => call.args))).toContainEqual({ machineId: "build-box", agent: "codex", session: "two", steer: "close" });

  await page.reload();
  await expect(page.locator(".bd-projects .cc-line").first()).toBeVisible();
  await expect(page.getByRole("radio", { name: "Projects" })).toHaveAttribute("aria-checked", "true");
});
