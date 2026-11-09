import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("Git details retain behavior and refresh preserves the mounted diff", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  const commits = [
    { id: "abcdef123", parents: ["parent123"], author: "Example", committed_at: 1, subject: "Latest" },
    { id: "parent123", parents: [], author: "Example", committed_at: 1, subject: "Parent" },
  ];
  const lines = Array.from({ length: 150 }, (_, i) => ({ k: " ", t: `const value${i} = ${i};` }));
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits, next: null },
    project_git_files: [{ path: "sample.ts", old_path: null, status: "modified" }],
    project_git_diff: { root: "/srv/project", base: "", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0, files: [{ path: "sample.ts", old_path: null, status: "modified", additions: 0, deletions: 0, left_out: null, hunks: [{ old_start: 1, new_start: 1, old_lines: 150, new_lines: 150, section: "", lines }] }] },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  await expect(page.locator(".gh-title-row h3")).toHaveText("Latest");
  const files = page.locator(".dv-files");
  await expect(files).toBeVisible();
  await files.evaluate(element => { element.scrollTop = 500; element.dataset.testIdentity = "mounted"; });
  const before = await files.evaluate(element => element.scrollTop);
  expect(before).toBeGreaterThan(0);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_git_history").length);
  await page.locator(".gh-nav-head button").filter({ hasText: "Refresh" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_git_history").length)).toBeGreaterThan(reads);
  await expect(files).toHaveAttribute("data-test-identity", "mounted");
  await expect(files).toHaveJSProperty("scrollTop", before);
  await page.getByRole("navigation", { name: "Git views" }).getByRole("button", { name: "Details" }).click();
  await expect(page.locator(".gh-facts")).toContainText("abcdef123");
  await page.getByRole("button", { name: "parent1" }).click();
  await expect(page.locator(".gh-title-row h3")).toHaveText("Parent");
  expect(errors).toEqual([]);
});

test("Git tree preserves expansion and focus while history updates", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  const makeCommit = (id: string) => ({ id, parents: [], author: "Example", committed_at: 1, subject: `Commit ${id}` });
  const history = { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [makeCommit("first"), makeCommit("second")], next: null };
  await openDesktop(page, false, {
    project_git_history: history,
    project_git_files: [{ path: "sample.ts", old_path: null, status: "modified" }],
    project_git_diff: { root: "/srv/project", base: "", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0, files: [] },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const first = page.locator('details[data-key="commit:first"]');
  await expect(first).toHaveAttribute("open", "");
  const firstSummary = first.locator(":scope > summary");
  await firstSummary.focus();
  await firstSummary.evaluate(element => { element.dataset.testIdentity = "same"; });
  await page.evaluate(value => window.__navigation.respond("project_git_history", value), { ...history, commits: [...history.commits, makeCommit("third")] });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_git_history").length);
  await page.locator(".gh-nav-head button").filter({ hasText: "Refresh" }).evaluate(button => { if (button instanceof HTMLElement) button.click(); });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_git_history").length)).toBeGreaterThan(reads);
  await expect(page.locator(".gh-tree")).toContainText("Commit third");
  await expect(first).toHaveAttribute("open", "");
  await expect(firstSummary).toHaveAttribute("data-test-identity", "same");
  await expect(firstSummary).toBeFocused();
  await firstSummary.click();
  await expect(first).not.toHaveAttribute("open", "");
  await firstSummary.click();
  await expect(first).toHaveAttribute("open", "");
  await page.locator(".gh-nav-head button").filter({ hasText: "Collapse all" }).click();
  await expect(page.locator('details[data-key="history"]')).not.toHaveAttribute("open", "");
  expect(errors).toEqual([]);
});

test("Git tree opens branches, worktrees and changed files", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [{ name: "feature", head: "abc123", kind: "local" }], worktrees: [{ path: "/srv/project", head: "abc123", branch: "feature", main: true }], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null },
    project_git_files: [{ path: "sample.ts", old_path: null, status: "modified" }],
    project_git_diff: { root: "/srv/project", base: "", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0, files: [] },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const tree = page.locator(".gh-tree");
  await tree.locator('details[data-key="branches"] > summary').click();
  await tree.locator('details[data-key="branch:local:feature"] > summary').click();
  await expect(tree.locator('details[data-key="feature:commit:abc123"]')).toBeVisible();
  await tree.locator('details[data-key="worktrees"] > summary').click();
  await tree.locator('details[data-key="worktree:/srv/project"] > summary').click();
  await expect(tree.locator(".gh-location")).toContainText("/srv/project");
  const file = tree.locator('.gh-file[data-path="sample.ts"]').first();
  await file.click();
  await expect(page.locator(".gh-diff-toolbar code")).toHaveText("sample.ts");
  expect(errors).toEqual([]);
});

test("Git tree shows failed file reads and retries", async ({ page }) => {
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null },
    project_git_files: [{ path: "sample.ts", old_path: null, status: "modified" }],
    project_git_diff: { root: "/srv/project", base: "", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0, files: [] },
  });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    let failed = false;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "project_git_files" && !failed) { failed = true; throw new Error("File read failed"); }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const changes = page.locator('details[data-key="changes:abc123"]');
  await expect(changes.locator(".cc-err")).toHaveText("File read failed");
  await changes.getByRole("button", { name: "Try again" }).click();
  await expect(changes.locator('.gh-file[data-path="sample.ts"]')).toBeVisible();
});

test("Git tree stays collapsed when a pending branch read finishes", async ({ page }) => {
  const history = { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [{ name: "feature", head: "abc123", kind: "local" }], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null };
  await openDesktop(page, false, {
    project_git_history: history, project_git_files: [],
    project_git_diff: { root: "/srv/project", base: "", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0, files: [] },
  });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "project_git_history" && args?.revision === "abc123") {
        await new Promise<void>(resolve => window.addEventListener("test-git-read", () => resolve(), { once: true }));
      }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const tree = page.locator(".gh-tree");
  await tree.locator('details[data-key="branches"] > summary').click();
  await tree.locator('details[data-key="branch:local:feature"] > summary').click();
  await expect(tree).toContainText("Reading history");
  await page.locator(".gh-nav-head button").filter({ hasText: "Collapse all" }).click();
  await expect(tree.locator('details[data-key="branches"]')).not.toHaveAttribute("open", "");
  await page.evaluate(() => window.dispatchEvent(new Event("test-git-read")));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_git_history").length)).toBeGreaterThan(1);
  await expect(tree.locator('details[data-key="branches"]')).not.toHaveAttribute("open", "");
});

test("Git details update when changing commits", async ({ page }) => {
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "First author", committed_at: 1, subject: "First commit" }, { id: "def456", parents: [], author: "Second author", committed_at: 1, subject: "Second commit" }], next: null },
    project_git_files: [], project_git_diff: { root: "/srv/project", base: "", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0, files: [] },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const tabs = page.getByRole("navigation", { name: "Git views" });
  await tabs.getByRole("button", { name: "Details" }).click();
  await expect(page.locator(".gh-facts")).toContainText("First author");
  await page.locator('details[data-key="commit:def456"] > summary').click();
  await expect(page.locator(".gh-title-row h3")).toHaveText("Second commit");
  await tabs.getByRole("button", { name: "Details" }).click();
  await expect(page.locator(".gh-facts")).toContainText("Second author");
  await expect(page.locator(".gh-facts")).toContainText("def456");
  await expect(page.locator(".gh-facts")).toContainText("Initial commit");
});

test("Git history retries a failed read and loads older commits", async ({ page }) => {
  const commit = (id: string) => ({ id, parents: [], author: "Example", committed_at: 1, subject: `Commit ${id}` });
  const history = { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [commit("first")], next: 1 };
  await openDesktop(page, false, {
    project_git_history: history, project_git_files: [],
    project_git_diff: { root: "/srv/project", base: "", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0, files: [] },
  });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    let failed = false;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "project_git_history" && !failed) { failed = true; throw new Error("History read failed"); }
      return invoke<T>(command, args);
    };
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const tree = page.locator(".gh-tree");
  await expect(tree.locator(".cc-err")).toHaveText("History read failed");
  await tree.getByRole("button", { name: "Try again" }).click();
  await expect(tree).toContainText("Commit first");
  await page.evaluate(value => window.__navigation.respond("project_git_history", value), { ...history, commits: [commit("older")], next: null });
  await tree.getByRole("button", { name: "Load older commits" }).click();
  await expect(tree).toContainText("Commit first");
  await expect(tree).toContainText("Commit older");
  await expect(tree.getByRole("button", { name: "Load older commits" })).toHaveCount(0);
});
