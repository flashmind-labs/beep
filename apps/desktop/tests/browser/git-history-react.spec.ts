import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("Git switches the displayed checkout and keeps the picker open on refusal", async ({ page }) => {
  const root = "/srv/project", machine = "build-box";
  const row = { machine_id: machine, branch: "feature", upstream: null, ahead: 0, behind: 0, error: null };
  const history = {
    machine_id: machine, machine_name: machine, root,
    refs: [
      { name: "main", head: "abc123", kind: "local" },
      { name: "feature", head: "abc123", kind: "local" },
      { name: "origin/main", head: "abc123", kind: "remote" },
    ],
    worktrees: [{ path: root, head: "abc123", branch: "feature", main: true }],
    commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null,
  };
  await openDesktop(page, false, {
    project_git_history: history, project_git_state: [row], project_git_files: [],
    project_git_diff: { root, base: "", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0, files: [] },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const branchBadge = page.locator(".pj-strip button").filter({ hasText: "branch" });
  await expect(branchBadge).toContainText("feature");
  await branchBadge.click();
  const checkout = page.locator(".gh-checkout");
  await expect(checkout.locator("code")).toHaveText("feature");
  await checkout.getByRole("button", { name: "Switch branch" }).click();
  const picker = page.getByRole("dialog", { name: "Switch branch" });
  await expect(picker).toContainText(`${machine} · ${root}`);
  const input = picker.getByRole("searchbox", { name: "Search local branches" });
  await expect(input).toBeFocused();
  await input.fill("missing");
  await expect(picker.getByText("No local branches match.")).toBeVisible();
  await input.fill("main");
  await expect(picker.getByRole("option")).toHaveCount(1);
  await picker.getByRole("option", { name: "main", exact: true }).click();
  const submit = picker.getByRole("button", { name: "Switch branch" });
  await page.evaluate(() => window.__navigation.rejectOnce("project_git_switch", "Git refused to overwrite a local change."));
  await submit.click();
  await expect(picker.getByRole("alert")).toHaveText("Git refused to overwrite a local change.");
  await expect(checkout.locator("code")).toHaveText("feature");

  const updated = { ...row, branch: "main" };
  await page.evaluate(value => {
    window.__navigation.respond("project_git_switch", value);
    window.__navigation.hold("project_git_switch");
  }, updated);
  await submit.click();
  await expect(picker.getByRole("button", { name: "Switching…" })).toBeDisabled();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_git_switch").map(call => call.args))).toEqual(
    Array.from({ length: 2 }, () => ({ project: projectName, machineId: machine, root, current: "feature", branch: "main" })),
  );
  await page.evaluate(value => {
    window.__navigation.respond("project_git_history", value);
    window.__navigation.release("project_git_switch");
  }, { ...history, worktrees: history.worktrees.map(tree => ({ ...tree, branch: "main" })) });
  await expect(picker).toBeHidden();
  await expect(checkout.locator("code")).toHaveText("main");
  await expect(branchBadge).toContainText("main");
  await expect(checkout.getByRole("button", { name: "Switch branch" })).toBeFocused();
});

test("diff status shows loading, empty, errors and omitted files", async ({ page }) => {
  const empty = { root: "/srv/project", base: "main", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0, files: [] };
  await openDesktop(page, false, { project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null }, project_git_files: [], project_git_diff: empty });
  await page.evaluate(() => window.__navigation.hold("project_git_diff"));
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  await expect(page.locator(".dv-empty").getByText("Reading the changes…")).toBeVisible();
  await page.evaluate(() => window.__navigation.release("project_git_diff"));
  await expect(page.getByText("Nothing changed")).toBeVisible();
  await expect(page.locator(".dv-keys")).toContainText("next, previous file");
  await page.evaluate(value => window.__navigation.respond("project_git_diff", value), { ...empty, more: 3, files: [{ path: "sample.ts", old_path: null, status: "modified", additions: 0, deletions: 0, left_out: null, hunks: [] }] });
  await page.locator(".dv-head").getByRole("button", { name: "Refresh" }).click();
  await expect(page.getByText("And 3 more files not shown.")).toBeVisible();
  await page.evaluate(() => window.__navigation.rejectOnce("project_git_diff"));
  await page.locator(".dv-head").getByRole("button", { name: "Refresh" }).click();
  await expect(page.locator(".dv-err")).toBeVisible();
  await expect(page.getByText("And 3 more files not shown.")).toBeVisible();
});

test("Git navigation keeps search focus and tree across refreshes", async ({ page }) => {
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Example commit" }], next: null },
    project_git_files: [], project_git_diff: { root: "/srv/project", base: "", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0, files: [] },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const input = page.getByRole("searchbox", { name: "Search commit messages" });
  const tree = page.locator(".gh-tree");
  await input.fill("example");
  await input.evaluate(element => { if (element instanceof HTMLInputElement) { element.dataset.identity = "kept"; element.setSelectionRange(2, 2); } });
  await tree.evaluate(element => { element.dataset.identity = "kept"; });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "project_git_history" && call.args.search === "example"))).toBe(true);
  await page.locator(".gh-nav-head button").filter({ hasText: "Refresh" }).evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await expect(input).toHaveValue("example");
  await expect(input).toHaveAttribute("data-identity", "kept");
  await expect(input).toBeFocused();
  await expect(input).toHaveJSProperty("selectionStart", 2);
  await expect(tree).toHaveAttribute("data-identity", "kept");
  await expect(page.locator(".gh-nav-foot")).toHaveText("All branches · build-box");
  await page.evaluate(() => window.__navigation.open("home"));
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  await expect(input).toHaveValue("example");
  await expect(input).toHaveAttribute("data-identity", "kept");
  await expect(tree).toHaveAttribute("data-identity", "kept");
  await expect(page.locator(".gh-nav-foot")).toHaveText("All branches · build-box");
});

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
  const showAll = page.getByRole("button", { name: "Show all files" });
  await showAll.focus();
  await showAll.evaluate(element => { element.dataset.identity = "kept"; });
  await page.locator(".gh-nav-head button").filter({ hasText: "Refresh" }).evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await expect(showAll).toHaveAttribute("data-identity", "kept");
  await expect(showAll).toBeFocused();
  await page.getByRole("navigation", { name: "Git views" }).getByRole("button", { name: "Sessions" }).click();
  await expect(page.locator(".gh-session-view")).toContainText("Sessions in this worktree");
  const session = page.locator(".gh-session-link").first();
  await expect(session).toBeVisible();
  await session.focus();
  await session.evaluate(element => { element.dataset.identity = "kept"; });
  await page.locator(".gh-nav-head button").filter({ hasText: "Refresh" }).evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await expect(session).toHaveAttribute("data-identity", "kept");
  await expect(session).toBeFocused();
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
  const actions = page.locator(".gh-nav-head");
  await expect(tree.locator(":scope > details[open]")).toHaveCount(0);
  await actions.getByRole("button", { name: "Expand all" }).press("Enter");
  await expect(tree.locator(":scope > details[open]")).toHaveCount(3);
  await expect(tree.locator('details[data-key="branch:local:feature"]')).not.toHaveAttribute("open", "");
  const collapse = actions.getByRole("button", { name: "Collapse all" });
  await expect(collapse).toBeFocused();
  await collapse.click();
  await expect(tree.locator(":scope > details[open]")).toHaveCount(0);
  await tree.locator('details[data-key="history"] > summary').click();
  await expect(collapse).toBeVisible();
  await tree.locator('details[data-key="history"] > summary').click();
  await expect(actions.getByRole("button", { name: "Expand all" })).toBeVisible();
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

test("diff context and omitted-file controls request additional lines", async ({ page }) => {
  const files = [
    { path: "source.ts", old_path: null, status: "modified", additions: 1, deletions: 0, left_out: null, hunks: [{ old_start: 1, new_start: 1, old_lines: 1, new_lines: 1, section: "", lines: [{ k: "+", t: "added" }] }, { old_start: 40, new_start: 40, old_lines: 1, new_lines: 1, section: "", lines: [{ k: " ", t: "later" }] }] },
    { path: "lock.json", old_path: null, status: "modified", additions: 1, deletions: 0, left_out: "lockfile", hunks: [] },
  ];
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null },
    project_git_files: files.map(({ path }) => ({ path, old_path: null, status: "modified" })),
    project_git_diff: { root: "/srv/project", base: "main", merge_base: "", working_tree: false, additions: 2, deletions: 0, more: 0, files },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const diff = page.locator(".gh-detail .dv");
  await diff.getByRole("button", { name: "Show the previous 10 lines" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "project_git_diff" && call.args.context === 13))).toBe(true);
  await expect(diff.getByRole("button", { name: "Show 10 lines above" })).toBeVisible();
  const lock = diff.locator('.dv-file[data-path="lock.json"]');
  await lock.locator(".dv-fh").click();
  await lock.getByRole("button", { name: "Show", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "project_git_diff" && call.args.lockfiles === true && Array.isArray(call.args.only) && call.args.only.includes("lock.json")))).toBe(true);
});

test("diff tree compacts single-child folders and matches file order", async ({ page }) => {
  await page.setViewportSize({ width: 1800, height: 900 });
  const paths = ["src/deep/first.ts", "README.md", "src/deep/second.ts"];
  const files = paths.map(path => ({ path, old_path: null, status: "modified", additions: 1, deletions: 0, left_out: null, hunks: [] }));
  await openDesktop(page, false, {
    project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null },
    project_git_files: files.map(({ path }) => ({ path, old_path: null, status: "modified" })),
    project_git_diff: { root: "/srv/project", base: "main", merge_base: "", working_tree: false, additions: 3, deletions: 0, more: 0, files },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const diff = page.locator(".gh-detail .dv");
  const folder = diff.locator('.dv-dir[data-folder="src/deep/"]');
  await expect(folder.locator(":scope > summary")).toContainText("src/deep");
  await expect(diff.locator('.dv-dir[data-folder="src/"]')).toHaveCount(0);
  await expect(diff.locator(".dv-file")).toHaveCount(3);
  expect(await diff.locator(".dv-file").evaluateAll(rows => rows.map(row => row.getAttribute("data-path")))).toEqual([paths[0], paths[2], paths[1]]);
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
  // The list draws only the rows near its viewport.
  await tree.evaluate(element => { element.scrollTop = element.scrollHeight; });
  await expect(row).toBeVisible();
  // Rows are measured as they mount, which shrinks the list and clamps the
  // bottom offset, so read it once two frames pass without a change.
  const before = await tree.evaluate(async element => {
    for (;;) {
      element.scrollTop = element.scrollHeight;
      const top = element.scrollTop;
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      if (element.scrollTop === top && top === element.scrollHeight - element.clientHeight) return top;
    }
  });
  await row.focus();
  await row.evaluate(element => { element.dataset.testIdentity = "same"; });
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
  await expect(diff.locator(".dv-tree .dv-tr.on")).toBeVisible();
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
  await header.evaluate(element => { element.dataset.identity = "kept"; });
  await page.keyboard.press("Enter");
  await expect(header).toHaveAttribute("aria-expanded", "false");
  await expect(header).toBeFocused();
  await expect(header).toHaveAttribute("data-identity", "kept");
  await header.click({ button: "right" });
  await expect(page.getByRole("menu", { name: "sample.ts" }).getByRole("menuitem", { name: "Copy path" })).toBeVisible();
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

test("session changes let diff text be selected", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const text = page.locator(".dv-l .dv-c").filter({ hasText: "const value20 = 20;" });
  await expect(text).toBeVisible();
  await expect(text).toHaveCSS("user-select", "text");
  await text.dblclick();
  await expect.poll(() => page.evaluate(() => document.getSelection()?.toString())).not.toBe("");
});

test("unchanged diff reload keeps a focused comment editor and file mounted", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const section = page.locator('.dv-file[data-path="sample.ts"]');
  await section.locator(".dv-l").filter({ hasText: "const value20 = 20;" }).locator(".dv-g").click();
  const editor = section.getByRole("textbox", { name: "Comment on this line" });
  await editor.fill("Keep this draft");
  await editor.evaluate(element => { if (element instanceof HTMLTextAreaElement) { element.dataset.identity = "kept"; element.setSelectionRange(3, 3); } });
  await section.evaluate(element => { element.dataset.identity = "kept"; });
  const files = page.locator(".dv-files");
  const top = await files.evaluate(element => element.scrollTop);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "work_diff").length);
  await page.evaluate(() => window.__navigation.hold("work_diff"));
  await page.locator('.dv-head .dv-btn[aria-label="Refresh"]').evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "work_diff").length)).toBeGreaterThan(reads);
  await page.evaluate(() => window.__navigation.release("work_diff"));
  await expect(page.locator('.dv-head .dv-btn[aria-label="Refresh"]')).toBeEnabled();
  await expect(section).toHaveAttribute("data-identity", "kept");
  await expect(editor).toHaveAttribute("data-identity", "kept");
  await expect(editor).toHaveValue("Keep this draft");
  await expect(editor).toBeFocused();
  await expect(editor).toHaveJSProperty("selectionStart", 3);
  await expect(files).toHaveJSProperty("scrollTop", top);
});

test("selected diff text survives reload until selection clears", async ({ page }) => {
  const file = (text: string) => ({ path: "sample.ts", old_path: null, status: "modified", additions: 1, deletions: 0, left_out: null, hunks: [{ old_start: 1, new_start: 1, old_lines: 1, new_lines: 1, section: "", lines: [{ k: "+", t: text }] }] });
  const diff = { root: "/srv/project", base: "main", merge_base: "", working_tree: false, additions: 1, deletions: 0, more: 0, files: [file("selected original text")] };
  await openDesktop(page, false, { project_git_history: { machine_id: "build-box", machine_name: "build-box", root: "/srv/project", refs: [], worktrees: [], commits: [{ id: "abc123", parents: [], author: "Example", committed_at: 1, subject: "Commit" }], next: null }, project_git_files: [], project_git_diff: diff });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  const section = page.locator('.dv-file[data-path="sample.ts"]');
  const text = section.locator(".dv-c").filter({ hasText: "selected original text" });
  await expect(text).toBeVisible();
  await text.evaluate(element => { const range = document.createRange(); range.selectNodeContents(element); const selection = document.getSelection(); selection?.removeAllRanges(); selection?.addRange(range); });
  await expect.poll(() => page.evaluate(() => document.getSelection()?.toString())).toBe("selected original text");
  await section.evaluate(element => { element.dataset.identity = "kept"; });
  for (let i = 1; i <= 2; i++) {
    await page.evaluate(value => window.__navigation.respond("project_git_diff", value), { ...diff, files: [file(`updated line ${i}`)] });
    await page.locator('.dv-head .dv-btn[aria-label="Refresh"]').evaluate(element => { if (element instanceof HTMLElement) element.click(); });
    await expect(page.locator('.dv-head .dv-btn[aria-label="Refresh"]')).toBeEnabled();
    await expect(section).toHaveAttribute("data-identity", "kept");
    await expect.poll(() => page.evaluate(() => document.getSelection()?.toString())).toBe("selected original text");
  }
  await page.evaluate(() => document.getSelection()?.removeAllRanges());
  await expect(section).toContainText("updated line 2");
  await expect(section).not.toHaveAttribute("data-identity", "kept");
});

test("changing another file keeps the focused diff section and editor mounted", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    let updated = false, changedAgain = false, removed = false, missing = false, returned = false;
    window.addEventListener("test-update-diff", () => { updated = true; });
    window.addEventListener("test-update-again", () => { changedAgain = true; });
    window.addEventListener("test-remove-line", () => { removed = true; });
    window.addEventListener("test-remove-file", () => { missing = true; });
    window.addEventListener("test-return-file", () => { returned = true; });
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "work_diff") return invoke<T>(command, args);
      const file = (path: string, text: string) => ({ path, old_path: null, status: "modified", additions: 1, deletions: 0, left_out: null, hunks: [{ old_start: 1, new_start: 1, old_lines: 1, new_lines: 1, section: "", lines: [{ k: "+", t: text }] }] });
      const first = file("first.ts", changedAgain ? "latest first line" : updated ? "updated first line" : "first line"), second = file("second.ts", updated ? "updated line" : "old line");
      if (removed) { first.hunks[0].old_start = 2; first.hunks[0].new_start = 2; }
      return { root: "/srv/project", base: "main", merge_base: "", working_tree: true, additions: 2, deletions: 0, more: 0, files: missing ? returned ? [second] : [] : updated ? [second, first] : [first, second] } as T;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const first = page.locator('.dv-file[data-path="first.ts"]'), second = page.locator('.dv-file[data-path="second.ts"]');
  await expect(second).toContainText("old line");
  await first.locator(".dv-l").filter({ hasText: "first line" }).locator(".dv-g").click();
  const editor = first.getByRole("textbox", { name: "Comment on this line" });
  await editor.fill("Unfinished comment");
  await editor.evaluate(element => { if (element instanceof HTMLTextAreaElement) { element.dataset.identity = "kept"; element.setSelectionRange(4, 4); } });
  await first.evaluate(element => { element.dataset.identity = "kept"; });
  await second.evaluate(element => { element.dataset.identity = "old"; });
  const files = page.locator(".dv-files"), top = await files.evaluate(element => element.scrollTop);
  await page.evaluate(() => window.dispatchEvent(new Event("test-update-diff")));
  await page.locator('.dv-head .dv-btn[aria-label="Refresh"]').evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await expect(second).toContainText("updated line");
  await expect(page.locator(".dv-file").first()).toHaveAttribute("data-path", "second.ts");
  await expect(second).not.toHaveAttribute("data-identity", "old");
  await expect(first).toHaveAttribute("data-identity", "kept");
  await expect(editor).toHaveAttribute("data-identity", "kept");
  await expect(editor).toHaveValue("Unfinished comment");
  await expect(editor).toBeFocused();
  await expect(editor).toHaveJSProperty("selectionStart", 4);
  await expect(files).toHaveJSProperty("scrollTop", top);
  await expect(first).toContainText("first line");
  await expect(first).not.toContainText("updated first line");
  await editor.press("Control+Enter");
  await expect(first).toContainText("updated first line");
  await expect(first).toContainText("Unfinished comment");
  await first.getByRole("button", { name: "Edit" }).click();
  await page.evaluate(() => window.dispatchEvent(new Event("test-update-again")));
  await page.locator('.dv-head .dv-btn[aria-label="Refresh"]').evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await first.locator(".dv-cmt.open").getByRole("button", { name: "Cancel" }).click();
  await expect(first).toContainText("latest first line");
  await first.getByRole("button", { name: "Edit" }).click();
  const editing = first.getByRole("textbox", { name: "Comment on this line" });
  await editing.fill("Unsaved edit");
  await editing.evaluate(element => { if (element instanceof HTMLTextAreaElement) element.setSelectionRange(3, 3); });
  await page.evaluate(() => window.dispatchEvent(new Event("test-remove-line")));
  await page.locator('.dv-head .dv-btn[aria-label="Refresh"]').evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  const lost = page.locator(".dv-lost");
  await expect(lost).toContainText("On files or lines no longer in the changes");
  const restored = lost.getByRole("textbox", { name: "Comment on this line" });
  await expect(restored).toHaveValue("Unsaved edit");
  await expect(restored).toBeFocused();
  await expect(restored).toHaveJSProperty("selectionStart", 3);
  await restored.press("Control+Enter");
  await expect(lost).toContainText("Unsaved edit");
  await first.locator(".dv-l").filter({ hasText: "latest first line" }).locator(".dv-g").click();
  const fresh = first.getByRole("textbox", { name: "Comment on this line" });
  await fresh.fill("New draft on removed file");
  await fresh.evaluate(element => { if (element instanceof HTMLTextAreaElement) element.setSelectionRange(5, 5); });
  await page.evaluate(() => window.dispatchEvent(new Event("test-remove-file")));
  await page.locator('.dv-head .dv-btn[aria-label="Refresh"]').evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await expect(first).toHaveCount(0);
  const orphan = lost.getByRole("textbox", { name: "Comment on this line" });
  await expect(orphan).toHaveValue("New draft on removed file");
  await expect(orphan).toBeFocused();
  await expect(orphan).toHaveJSProperty("selectionStart", 5);
  await orphan.fill("Edited after the file disappeared");
  await orphan.evaluate(element => { if (element instanceof HTMLTextAreaElement) element.setSelectionRange(7, 7); });
  await page.evaluate(() => window.dispatchEvent(new Event("test-return-file")));
  await page.locator('.dv-head .dv-btn[aria-label="Refresh"]').evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await expect(second).toBeVisible();
  const returnedEditor = lost.getByRole("textbox", { name: "Comment on this line" });
  await expect(returnedEditor).toHaveValue("Edited after the file disappeared");
  await expect(returnedEditor).toBeFocused();
  await expect(returnedEditor).toHaveJSProperty("selectionStart", 7);
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
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  await expect(picker).toBeVisible();
  // The picker places itself as it opens, next to its button.
  const placed = await page.locator(".dv-fbw .agc-pick").evaluate(element => element instanceof HTMLElement ? [element.style.top, element.style.left] : []);
  expect(placed).toHaveLength(2);
  expect(placed.every(value => /^-?\d+(\.\d+)?px$/.test(value))).toBe(true);
  await picker.getByRole("button", { name: "Codex", exact: true }).click();
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
  await expect(page.getByRole("dialog", { name: "Machine, harness, model and thinking" })).toBeVisible();
  await page.evaluate(() => document.addEventListener("keydown", event => { if (event.key === "Escape") document.body.dataset.escapeReached = "yes"; }));
  await open("notice-two");
  await expect(page.getByRole("dialog", { name: "Machine, harness, model and thinking" })).toHaveCount(0);
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
    await review.click();
    await expect.poll(() => pickerUrl).not.toBe("");
    await page.evaluate(() => document.addEventListener("keydown", event => { if (event.key === "Escape") document.body.dataset.escapeReached = "yes"; }));
    await open("notice-two");
    release();
    await page.evaluate(async url => { await import(url); }, pickerUrl);
    await page.keyboard.press("Escape");
    await expect(page.locator("body")).toHaveAttribute("data-escape-reached", "yes");
    await expect(page.getByRole("dialog", { name: "Machine, harness, model and thinking" })).toHaveCount(0);
  } finally { release(); }
});

test("a review arriving after its diff closes is saved for reopening", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.hold("agent_reviews"));
  const open = async (id: string) => page.evaluate(id => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: id,
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }), id);
  await open("notice-one");
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_reviews").length)).toBeGreaterThan(0);
  await open("notice-two");
  await page.evaluate(() => {
    window.__navigation.respond("agent_reviews", [{ id: "review-one", parent: "notice-one", cwd: "/srv/project", base: "main", agent: "codex", started_at: 1, error: null,
      review: { verdict: "changes", summary: "Review", findings: [{ file: "src/example.ts", line: 1, severity: "high", summary: "Background finding", scenario: "" }] } }]);
    window.__navigation.release("agent_reviews");
  });
  await expect.poll(() => page.evaluate(() => localStorage.getItem("beep.diffReview.build-box\nnotice-one"))).toContain("Background finding");
  await expect(page.locator(".dv-cmt")).toHaveCount(0);
  await open("notice-one");
  const tab = page.getByRole("tab", { name: "Changes", exact: true });
  if (await tab.getAttribute("aria-selected") !== "true") await tab.click();
  await expect(page.locator(".dv")).toContainText("Background finding");
  expect(errors).toEqual([]);
});

test("the session's reviews come from the reviews update, not from reading them again", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "notice-one",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_reviews" || call.command === "agent_review").length);
  await expect.poll(reads).toBe(1);
  await expect(page.locator(".dv")).toContainText("sample.ts");
  const run = { id: "review-one", parent: "notice-one", cwd: "/srv/project", base: "main", agent: "codex", started_at: 1, error: null,
    review: { verdict: "changes", summary: "Review", findings: [{ file: "sample.ts", line: 20, severity: "high", summary: "Pushed finding", scenario: "" }] } };
  // The subscription reaches native code a moment after the diff opens, so the update repeats until drawn.
  await expect(async () => {
    await page.evaluate(data => window.__navigation.publish("reviews.*", null, data, "build-box"), { Sessions: { Reviews: { runs: [run] } } });
    await expect(page.locator(".dv-cmt")).toContainText("Pushed finding", { timeout: 300 });
  }).toPass();
  expect(await reads()).toBe(1);
});
