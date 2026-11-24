import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test.use({ hasTouch: true, isMobile: true, viewport: { width: 390, height: 844 } });

test("a phone sorts projects by the activity their cards show", async ({ page }) => {
  const now = Date.now();
  const projects = ["alpha", "beta"].map(name => ({ name: `example/${name}`, url: "", machine_id: "build-box", machine_name: "Build box", folder: `/srv/${name}`, cloned: true }));
  const running = [{ kind: "agent", machine_id: "build-box", machine_name: "Build box", id: "active", agent: "codex", title: "Task", cwd: "/srv/alpha", status: "working", doing: null, started_at: 1, last_activity: now - 2 * 86_400_000, external: false, question: null }];
  const ended = [{ machine_id: "build-box", machine_name: "Build box", id: "past", agent: "codex", title: "Done", cwd: "/srv/beta", status: "ended", doing: null, started_at: 1, last_activity: now - 3_600_000, external: false, question: null }];
  await openDesktop(page, false, { center_home: { machines: [], projects, running, needs_you: [] }, project_sessions: ended });
  await page.evaluate(() => window.__navigation.open({ projects: { project: "example/beta" } }));
  await expect(page.locator("body")).toHaveClass(/mobile/);
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));

  const sort = page.getByRole("button", { name: "Sort projects" });
  await sort.tap();
  await page.getByRole("menuitemradio", { name: "Recent activity", exact: true }).tap();
  await expect(sort).toHaveText("Recent activity");
  await expect(page.locator(".pj-card .h b")).toHaveText(["beta", "alpha"]);
  await expect(page.locator(".pj-card").first().locator(".ago")).not.toHaveText("No activity");
});
