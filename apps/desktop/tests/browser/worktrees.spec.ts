import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("the worktree picker can pause following and search branches or paths", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await openDesktop(page, true);
  await page.evaluate(() => {
    window.__navigation.worktrees([
      {path: "/srv/project", branch: "main", head: "abc1234", own: true, created: false},
      {path: "/srv/project/.worktrees/fix", branch: "fix-login", head: "abc1234", own: false, created: true},
    ]);
    window.__navigation.open({agent: {
      kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification",
      cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
    }});
  });
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const picker = page.locator(".agwt:visible");
  await picker.click();
  const follow = page.getByRole("menuitemcheckbox", {name: "Follow the agent"});
  await expect(follow).toBeChecked();
  await follow.click();
  await picker.click();
  await expect(follow).not.toBeChecked();
  const search = page.getByRole("searchbox", {name: "Search worktrees"});
  await expect(search).toBeFocused();
  for (const query of ["LOGIN", ".worktrees"]) {
    await search.fill(query);
    await expect(page.getByRole("menuitemradio")).toHaveCount(1);
    await expect(page.getByRole("menuitemradio")).toContainText("fix-login");
    await expect(follow).toBeVisible();
  }
  await search.fill("missing");
  await expect(page.locator(".agwtm:visible").getByText("No matching worktrees", {exact: true})).toBeVisible();
  await search.fill("");
  await expect(page.getByRole("menuitemradio")).toHaveCount(2);
  await follow.click();
  await picker.click();
  await expect(follow).toBeChecked();
  await page.keyboard.press("Escape");
  await expect(search).toHaveCount(0);
  await page.evaluate(() => window.__navigation.open("home"));
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator(".agwt:visible").click();
  await expect(page.getByRole("menuitemcheckbox", { name: "Follow the agent" })).toBeChecked();
  expect(errors).toEqual([]);
});
