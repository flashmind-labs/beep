import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("project sort separates activity from visits and survives a reload", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("beep.projects.visited", JSON.stringify({ "example/beta": 400 }));
  });
  const projects = ["alpha", "beta"].map(name => ({ name: `example/${name}`, url: "", machine_id: "build-box", machine_name: "Build box", folder: `/srv/${name}`, cloned: true, imported_at: 1 }));
  const running = [{ kind: "agent", machine_id: "build-box", machine_name: "Build box", id: "active", agent: "codex", title: "Task", cwd: "/srv/alpha", status: "working", doing: null, started_at: 1, last_activity: 300, external: false, question: null }];
  await openDesktop(page, false, { center_home: { machines: [], projects, running, needs_you: [] } });
  const open = () => page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await open();
  const sort = page.getByRole("combobox", { name: "Sort projects" });
  const names = page.locator(".pj-card .h b");
  await expect(sort).toHaveValue("default");
  await expect(names).toHaveText(["beta", "alpha"]);
  await sort.selectOption("activity");
  await expect(names).toHaveText(["alpha", "beta"]);
  await page.reload();
  await expect.poll(() => page.evaluate(() => window.__navigation?.ready())).toBe(true);
  await open();
  await expect(sort).toHaveValue("activity");
  await expect(names).toHaveText(["alpha", "beta"]);
  await sort.selectOption("opened");
  await expect(names).toHaveText(["beta", "alpha"]);
  await sort.selectOption("default");
  await expect(names).toHaveText(["beta", "alpha"]);
});
