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
  const details = page.getByRole("navigation", { name: "Git views" }).getByRole("button", { name: "Details" });
  await details.click();
  await expect(page.locator(".gh-facts")).toContainText("abcdef123");
  await expect(details).toBeFocused();
  await details.evaluate(element => { element.dataset.identity = "kept"; });
  await page.locator(".gh-nav-head button").filter({ hasText: "Refresh" }).evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await expect(details).toHaveAttribute("data-identity", "kept");
  await expect(details).toBeFocused();
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
  await page.evaluate(() => {
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: () => new Promise<void>(resolve => window.addEventListener("test-copy-ready", () => resolve(), { once: true })) } });
  });
  await page.getByRole("button", { name: "Copy SHA" }).click();
  const tabs = page.getByRole("navigation", { name: "Git views" });
  await tabs.getByRole("button", { name: "Details" }).click();
  await expect(page.locator(".gh-facts")).toContainText("First author");
  await page.locator('details[data-key="commit:def456"] > summary').click();
  await expect(page.locator(".gh-title-row h3")).toHaveText("Second commit");
  await page.evaluate(() => window.dispatchEvent(new Event("test-copy-ready")));
  await expect(page.getByRole("button", { name: "Copy SHA" })).toBeVisible();
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

test("diff summary updates without replacing the filter or scrolling the diff", async ({ page }) => {
  const lines = Array.from({ length: 150 }, (_, i) => ({ k: " ", t: `const value${i} = ${i};` }));
  const diff = { root: "/srv/project", base: "base-branch", merge_base: "", working_tree: false, additions: 3, deletions: 2, more: 0, files: [{ path: "sample.ts", old_path: null, status: "modified", additions: 3, deletions: 2, left_out: null, hunks: [{ old_start: 1, new_start: 1, old_lines: 150, new_lines: 150, section: "", lines }] }] };
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null },
    project_git_files: [{ path: "sample.ts", old_path: null, status: "modified" }], project_git_diff: diff,
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const summary = page.locator(".dv-sum");
  await expect(summary).toContainText("+3");
  await expect(summary).toContainText("-2");
  await expect(summary).toContainText("base-branch");
  const filter = page.getByRole("textbox", { name: "Filter files" });
  await filter.focus();
  await filter.evaluate(element => { element.dataset.testIdentity = "same"; });
  const files = page.locator(".dv-files");
  const before = await files.evaluate(element => { element.scrollTop = 500; return element.scrollTop; });
  expect(before).toBeGreaterThan(0);
  await page.evaluate(value => window.__navigation.respond("project_git_diff", value), { ...diff, additions: 8, deletions: 4 });
  await page.getByRole("button", { name: "Refresh", exact: true }).last().evaluate(button => { if (button instanceof HTMLElement) button.click(); });
  await expect(summary).toContainText("+8");
  await expect(summary).toContainText("-4");
  await expect(filter).toHaveAttribute("data-test-identity", "same");
  await expect(filter).toBeFocused();
  await expect(files).toHaveJSProperty("scrollTop", before);
});

test("diff toolbar keeps filtering, keyboard focus, split and folding controls working", async ({ page }) => {
  const lines = [{ k: " ", t: "one line" }];
  const file = (path: string) => ({ path, old_path: null, status: "modified", additions: 1, deletions: 0, left_out: null, hunks: [{ old_start: 1, new_start: 1, old_lines: 1, new_lines: 1, section: "", lines }] });
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null },
    project_git_files: ["alpha.ts", "beta.ts"].map(path => ({ path, old_path: null, status: "modified" })),
    project_git_diff: { root: "/srv/project", base: "main", merge_base: "", working_tree: false, additions: 2, deletions: 0, more: 0, files: [file("alpha.ts"), file("beta.ts")] },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const diff = page.locator(".gh-detail .dv");
  await expect(diff.locator(".dv-file")).toHaveCount(2);
  await diff.focus();
  await page.keyboard.press("/");
  const filter = diff.getByRole("textbox", { name: "Filter files" });
  await expect(filter).toBeFocused();
  await filter.fill("alpha");
  await expect(filter).toHaveValue("alpha");
  await expect(diff.locator('.dv-file[data-path="alpha.ts"]')).toBeVisible();
  await expect(diff.locator('.dv-file[data-path="beta.ts"]')).toBeHidden();
  await page.keyboard.press("Escape");
  await expect(diff).toBeFocused();
  const split = diff.getByRole("button", { name: "Side by side" });
  await split.click();
  await expect(split).toHaveAttribute("aria-pressed", "true");
  await diff.getByRole("button", { name: "Collapse all" }).click();
  await expect(diff.locator('.dv-file[data-path="alpha.ts"]')).toHaveClass(/closed/);
  await diff.getByRole("button", { name: "Expand all" }).click();
  await expect(diff.locator('.dv-file[data-path="alpha.ts"]')).not.toHaveClass(/closed/);
  await filter.fill("");
  await expect(diff.locator('.dv-file[data-path="beta.ts"]')).toBeVisible();
});

test("diff tree preserves focused files and scroll when refreshed", async ({ page }) => {
  await page.setViewportSize({ width: 1800, height: 900 });
  const file = (path: string) => ({ path, old_path: null, status: "modified", additions: 1, deletions: 0, left_out: null, hunks: [] });
  const files = Array.from({ length: 80 }, (_, i) => file(`src/file-${i}.ts`));
  const diff = { root: "/srv/project", base: "main", merge_base: "", working_tree: false, additions: 80, deletions: 0, more: 0, files };
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null },
    project_git_files: files.map(({ path }) => ({ path, old_path: null, status: "modified" })), project_git_diff: diff,
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const tree = page.locator(".gh-detail .dv-tree");
  const row = tree.locator('.dv-tr[data-path="src/file-70.ts"]');
  await expect(row).toBeVisible();
  await row.focus();
  await row.evaluate(element => { element.dataset.testIdentity = "same"; });
  const before = await tree.evaluate(element => { element.scrollTop = element.scrollHeight; return element.scrollTop; });
  expect(before).toBeGreaterThan(0);
  await page.evaluate(value => window.__navigation.respond("project_git_diff", value), { ...diff, files: [...files, file("src/new.ts")] });
  await page.locator('.gh-detail .dv-btn[aria-label="Refresh"]').evaluate(button => { if (button instanceof HTMLElement) button.click(); });
  await expect(tree.locator('.dv-tr[data-path="src/new.ts"]')).toBeVisible();
  await expect(row).toHaveAttribute("data-test-identity", "same");
  await expect(row).toBeFocused();
  await expect(tree).toHaveJSProperty("scrollTop", before);
  await row.click({ button: "right" });
  await expect(page.getByRole("menu", { name: "src/file-70.ts" }).getByRole("menuitem", { name: "Copy path" })).toBeVisible();
});

test("diff keyboard navigation reveals a filtered closed folder", async ({ page }) => {
  await page.setViewportSize({ width: 1800, height: 900 });
  const file = (path: string) => ({ path, old_path: null, status: "modified", additions: 1, deletions: 0, left_out: null, hunks: [] });
  const files = [file("src/first.ts"), file("src/second.ts")];
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null },
    project_git_files: files.map(({ path }) => ({ path, old_path: null, status: "modified" })),
    project_git_diff: { root: "/srv/project", base: "main", merge_base: "", working_tree: false, additions: 2, deletions: 0, more: 0, files },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const diff = page.locator(".gh-detail .dv");
  await diff.getByRole("textbox", { name: "Filter files" }).fill("src");
  const folder = diff.locator('.dv-dir[data-folder="src/"]');
  await folder.locator(":scope > summary").click();
  await expect(folder).not.toHaveAttribute("open", "");
  await diff.focus();
  await page.keyboard.press("j");
  await expect(folder).toHaveAttribute("open", "");
  await expect(folder.locator(".dv-tr.on")).toBeVisible();
});

test("diff file header actions do not toggle the file", async ({ page }) => {
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null },
    project_git_files: [{ path: "sample.ts", old_path: null, status: "modified" }],
    project_git_diff: { root: "/srv/project", base: "main", merge_base: "", working_tree: false, additions: 1, deletions: 0, more: 0, files: [{ path: "sample.ts", old_path: null, status: "modified", additions: 1, deletions: 0, left_out: null, hunks: [] }] },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const section = page.locator('.gh-detail .dv-file[data-path="sample.ts"]');
  const header = section.locator(".dv-fh");
  const copy = header.getByTitle("Copy path");
  await expect(header).toHaveAttribute("aria-expanded", "true");
  await copy.click();
  await expect(header).toHaveAttribute("aria-expanded", "true");
  await copy.focus();
  await page.keyboard.press("Enter");
  await expect(header).toHaveAttribute("aria-expanded", "true");
  await header.focus();
  await page.keyboard.press("Enter");
  await expect(header).toHaveAttribute("aria-expanded", "false");
});

test("diff file header opens a comment without folding", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const section = page.locator('.dv-file[data-path="sample.ts"]');
  const header = section.locator(".dv-fh");
  await header.getByRole("button", { name: "Comment" }).click();
  await expect(header).toHaveAttribute("aria-expanded", "true");
  const comment = section.getByRole("textbox", { name: "Comment on this file" });
  await comment.fill("Please review this file.");
  await section.locator(".dv-cmt.open").getByRole("button", { name: "Add" }).click();
  await expect(section.locator(".dv-fcs")).toContainText("Please review this file.");
  await expect(page.getByRole("button", { name: /Send review/ })).toContainText("1");
});

test("virtualized diff keeps inline comments editable after refresh", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const section = page.locator('.dv-file[data-path="sample.ts"]');
  const row = section.locator(".dv-l").filter({ hasText: "const value20 = 20;" });
  await row.locator(".dv-g").click();
  const editor = section.locator(".dv-cmt.open");
  await editor.getByRole("textbox", { name: "Comment on this line" }).fill("Review this line.");
  await editor.getByRole("textbox", { name: "Comment on this line" }).press("Control+Enter");
  await expect(section.locator(".dv-cmt")).toContainText("Review this line.");
  await page.locator('.dv-head .dv-btn[aria-label="Refresh"]').click();
  const saved = section.locator(".dv-cmt").filter({ hasText: "Review this line." });
  await expect(saved).toBeVisible();
  await saved.getByRole("button", { name: "Edit" }).click();
  const editing = section.locator(".dv-cmt.open");
  await editing.getByRole("textbox", { name: "Comment on this line" }).fill("Discarded edit.");
  await editing.getByRole("textbox", { name: "Comment on this line" }).press("Escape");
  await expect(section.locator(".dv-cmt")).toContainText("Review this line.");
  await section.locator(".dv-cmt").getByRole("button", { name: "Edit" }).click();
  await editing.getByRole("textbox", { name: "Comment on this line" }).fill("Updated line review.");
  await editing.getByRole("button", { name: "Add" }).click();
  await expect(section.locator(".dv-cmt")).toContainText("Updated line review.");
  await section.getByRole("button", { name: "Remove the comment" }).click();
  await expect(section.locator(".dv-cmt")).toHaveCount(0);
});

test("virtualized diff evicts and restores saved inline comments", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await openDesktop(page, true);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const result = await invoke<T>(command, args);
      if (command === "work_diff" && result && typeof result === "object") {
        const lines = Array.from({ length: 1500 }, (_, i) => ({ k: " ", t: `const value${i} = ${i};` }));
        Object.assign(result, { files: [{ path: "sample.ts", old_path: null, status: "modified", additions: 1, deletions: 0, left_out: null,
          hunks: [{ old_start: 1, new_start: 1, old_lines: lines.length, new_lines: lines.length, section: "", lines }] }] });
      }
      return result;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const section = page.locator('.dv-file[data-path="sample.ts"]');
  const firstChunk = section.locator(".dv-chunk").first();
  await firstChunk.locator(".dv-l").filter({ hasText: "const value1 = 1;" }).locator(".dv-g").click();
  const editor = firstChunk.locator(".dv-cmt.open");
  await editor.getByRole("textbox", { name: "Comment on this line" }).fill("Saved comment.");
  await editor.getByRole("button", { name: "Add" }).click();
  await expect(firstChunk.locator(".dv-cmt")).toHaveCount(1);
  const scroll = page.locator(".dv-files");
  await scroll.evaluate(element => { element.scrollTop = element.scrollHeight; });
  await expect(firstChunk.locator(".dv-l")).toHaveCount(0);
  await expect(firstChunk.locator(".dv-cmt")).toHaveCount(0);
  await scroll.evaluate(element => { element.scrollTop = 0; });
  await expect(firstChunk.locator(".dv-cmt")).toHaveCount(1);
  await expect(firstChunk.locator(".dv-cmt")).toContainText("Saved comment.");
  await firstChunk.getByRole("button", { name: "Remove the comment" }).click();
  await scroll.evaluate(element => { element.scrollTop = element.scrollHeight; });
  await expect(firstChunk.locator(".dv-l")).toHaveCount(0);
  await scroll.evaluate(element => { element.scrollTop = 0; });
  await expect(firstChunk.locator(".dv-l").first()).toBeVisible();
  await expect(firstChunk.locator(".dv-cmt")).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("lost diff comments remain editable without duplicate hosts on refresh", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("beep.diffReview.build-box\nnotice-one", JSON.stringify([
    { path: "removed.ts", side: "new", line: 4, text: "Review a removed file." },
  ])));
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const lost = page.locator(".dv-lost");
  await expect(lost.locator(".dv-cmt")).toHaveCount(1);
  await page.locator('.dv-head .dv-btn[aria-label="Refresh"]').click();
  await expect(lost.locator(".dv-cmt")).toHaveCount(1);
  await expect(lost.locator(".dv-comment-slot")).toHaveCount(1);
  await lost.getByRole("button", { name: "Edit" }).click();
  await lost.getByRole("textbox", { name: "Comment on this line" }).fill("Updated removed file review.");
  await lost.getByRole("button", { name: "Add" }).click();
  await expect(lost.locator(".dv-cmt")).toContainText("Updated removed file review.");
  await lost.getByRole("button", { name: "Remove the comment" }).click();
  await expect(lost.locator(".dv-cmt")).toHaveCount(0);
});

test("diff comment drafts survive a session reload", async ({ page }) => {
  const consoleErrors: string[] = [];
  page.on("console", message => { if (message.type() === "error") consoleErrors.push(message.text()); });
  await openDesktop(page, true);
  const open = async () => page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await open();
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const section = page.locator('.dv-file[data-path="sample.ts"]');
  await section.locator(".dv-l").filter({ hasText: "const value20 = 20;" }).locator(".dv-g").click();
  const editor = section.locator(".dv-cmt.open");
  await editor.getByRole("textbox", { name: "Comment on this line" }).fill("Keep this review draft.");
  await editor.getByRole("button", { name: "Add" }).click();
  await expect(section.locator(".dv-cmt")).toContainText("Keep this review draft.");
  await page.reload();
  await page.waitForFunction(() => window.__navigation.ready());
  await open();
  const tab = page.getByRole("tab", { name: "Changes", exact: true });
  if (await tab.getAttribute("aria-selected") !== "true") await tab.click();
  await expect(section.locator(".dv-cmt")).toContainText("Keep this review draft.");
  expect(consoleErrors.filter(message => message.includes("flushSync") || message.includes("Cannot update"))).toEqual([]);
});

test("review picker remains usable through diff refresh and closes with Escape", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await openDesktop(page, true, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: "/srv/project", home: "/path/to/work", capabilities: [] }],
  });
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const review = page.getByRole("button", { name: "Review with agent" });
  await expect(review).toBeEnabled();
  await review.click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  await expect(picker).toBeVisible();
  const search = picker.locator(".agc-search");
  await expect(search).toBeFocused();
  await search.fill("example");
  await page.locator('.dv-head .dv-btn[aria-label="Refresh"]').evaluate(button => { if (button instanceof HTMLElement) button.click(); });
  await expect(picker).toBeVisible();
  await expect(picker.locator(".agc-search")).toHaveValue("example");
  await page.keyboard.press("Escape");
  await expect(picker).toHaveCount(0);
  await expect(review).toBeFocused();
  expect(errors).toEqual([]);
});

test("closing a diff removes the review picker's keyboard handler", async ({ page }) => {
  await openDesktop(page, true, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: "/srv/project", home: "/path/to/work", capabilities: [] }],
  });
  const open = async (id: string) => page.evaluate(id => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: id,
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }), id);
  await open("notice-one");
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  await page.getByRole("button", { name: "Review with agent" }).click();
  await expect(page.getByRole("dialog", { name: "Harness, model and thinking" })).toBeVisible();
  await page.evaluate(() => document.addEventListener("keydown", event => { if (event.key === "Escape") document.body.dataset.escapeReached = "yes"; }));
  await open("notice-two");
  await expect(page.getByRole("dialog", { name: "Harness, model and thinking" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(page.locator("body")).toHaveAttribute("data-escape-reached", "yes");
});

test("a pending review picker does not open after its diff closes", async ({ page }) => {
  await openDesktop(page, true, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: "/srv/project", home: "/path/to/work", capabilities: [] }],
  });
  let release = () => {};
  const blocked = new Promise<void>(resolve => { release = resolve; });
  let pickerUrl = "";
  await page.route(/\/agentpicker-[A-Z0-9]+\.js$/, async route => {
    pickerUrl = route.request().url();
    await blocked;
    await route.fallback();
  });
  const open = async (id: string) => page.evaluate(id => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: id,
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }), id);
  try {
    await open("notice-one");
    await page.getByRole("tab", { name: "Changes", exact: true }).click();
    const review = page.getByRole("button", { name: "Review with agent" });
    await expect(review).toBeEnabled();
    await expect.poll(() => pickerUrl).not.toBe("");
    await review.click();
    await page.evaluate(() => document.addEventListener("keydown", event => { if (event.key === "Escape") document.body.dataset.escapeReached = "yes"; }));
    await open("notice-two");
    release();
    await page.evaluate(async url => { await import(url); }, pickerUrl);
    await page.keyboard.press("Escape");
    await expect(page.locator("body")).toHaveAttribute("data-escape-reached", "yes");
    await expect(page.getByRole("dialog", { name: "Harness, model and thinking" })).toHaveCount(0);
  } finally { release(); }
});
