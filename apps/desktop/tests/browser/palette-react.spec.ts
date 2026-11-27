import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("palette search keeps its field and selected result across index refreshes", async ({ page }) => {
  const project = (id: string, title: string) => ({ kind: "project", id, title, detail: "Project", keys: [title.toLowerCase()] });
  const index = { actions: [], settings: [], code: [], projects: [project("one", "example/tool"), project("two", "example/example")], sessions: [], machines: [] };
  await openDesktop(page, false, { center_palette_index: index, center_find_prompts: [] });
  await page.getByRole("button", { name: /^Search/ }).click();
  const dialog = page.getByRole("dialog", { name: "Search" }), input = dialog.getByRole("combobox");
  await input.fill("Example");
  const options = dialog.getByRole("option");
  await expect(options).toHaveCount(2);
  await expect(options.first()).toContainText("example/example");
  await expect(options.first()).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowDown");
  await expect(options.nth(1)).toHaveAttribute("aria-selected", "true");
  await input.evaluate(element => { element.dataset.identity = "kept"; });
  await options.nth(1).evaluate(element => { element.dataset.identity = "kept"; });
  await page.evaluate(next => window.__navigation.emit("palette-changed", next), JSON.stringify({ ...index, projects: [...index.projects, project("three", "other/widget")] }));
  await expect(input).toHaveAttribute("data-identity", "kept");
  await expect(input).toBeFocused();
  await expect(options.nth(1)).toHaveAttribute("data-identity", "kept");
  await expect(options.nth(1)).toHaveAttribute("aria-selected", "true");
  await input.fill("widget");
  await expect(options).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("button", { name: /^Search/ })).toBeFocused();
});

test("the right arrow on a project lists its sessions and Enter opens one", async ({ page }) => {
  const session = (id: string, title: string, project: string) => ({
    kind: "agent_session", id: `build-box ${id}`, title, detail: `Codex · ${project} · build-box`, keys: [title.toLowerCase(), project], project,
    session: { machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title, cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false },
  });
  const index = {
    actions: [], settings: [], code: [], machines: [],
    projects: [{ kind: "project", id: "example/widget", title: "example/widget", detail: "build-box", keys: ["example/widget"] }],
    sessions: [session("one", "Fix the redirect", "example/widget"), session("two", "Tidy the docs", "example/widget"), session("three", "Another widget", "example/relay")],
  };
  await openDesktop(page, false, { center_palette_index: index, center_find_prompts: [] });
  await page.getByRole("button", { name: /^Search/ }).click();
  const dialog = page.getByRole("dialog", { name: "Search" }), options = dialog.getByRole("option");
  await dialog.getByRole("combobox").fill("widget");
  await expect(options.first()).toHaveText(/example\/widget/);
  await expect(dialog.locator(".foot")).toContainText("sessions");

  await page.keyboard.press("ArrowRight");
  await expect(options).toHaveText([/Fix the redirect/, /Tidy the docs/]);
  await expect(dialog.locator(".foot")).toContainText("back");

  await page.keyboard.press("ArrowLeft");
  await expect(options).toHaveCount(4);
  await expect(options.first()).toHaveAttribute("aria-selected", "true");

  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(dialog).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_view").map(call => JSON.stringify(call.args)))).toEqual([expect.stringContaining('"two"')]);
});

test("detached project windows focus Command K on items and actions inside the project", { tag: "@windows" }, async ({ page }) => {
  const paletteKey = test.info().project.name === "Windows" ? "Control+k" : "Meta+k";
  const widgetSession = {
    kind: "agent_session",
    id: "build-box s-widget",
    title: "Fix redirect loop",
    detail: projectName,
    keys: ["fix redirect loop", projectName, "codex", "build-box"],
    project: projectName,
    session: { machine_id: "build-box", machine_name: "build-box", id: "s-widget", agent: "codex", title: "Fix redirect loop", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 20, external: false },
  };
  const otherSession = {
    kind: "agent_session",
    id: "build-box s-other",
    title: "Other redirect task",
    detail: "other/repo",
    keys: ["other redirect task", "other/repo", "codex", "build-box"],
    project: "other/repo",
    session: { machine_id: "build-box", machine_name: "build-box", id: "s-other", agent: "codex", title: "Other redirect task", cwd: "/srv/other", status: "idle", doing: null, started_at: 1, last_activity: 30, external: false },
  };
  const index = {
    actions: [
      { kind: "action", id: "new_session", title: "New session", detail: "", keys: ["new session"] },
      { kind: "action", id: "home", title: "Home", detail: "", keys: ["home"] },
      { kind: "action", id: "settings", title: "Settings", detail: "", keys: ["settings"] },
    ],
    settings: [{ kind: "setting", id: "creds", title: "GitHub token", detail: "Settings · Credentials", keys: ["github token"] }],
    machines: [{ kind: "machine", id: "build-box", title: "build-box", detail: "online", keys: ["build-box"] }],
    projects: [
      { kind: "project", id: projectName, title: projectName, detail: "build-box", keys: [projectName] },
      { kind: "project", id: "other/repo", title: "other/repo", detail: "build-box", keys: ["other/repo"] },
    ],
    code: [
      { kind: "branch", id: `${projectName} fix-redirect`, title: `${projectName} · fix-redirect`, detail: "Branch · 2 ahead · Guard redirect URL", keys: ["fix-redirect", projectName] },
      { kind: "branch", id: "other/repo fix-redirect", title: "other/repo · fix-redirect", detail: "Branch · 1 ahead · Unrelated", keys: ["fix-redirect", "other/repo"] },
    ],
    sessions: [widgetSession, otherSession],
  };
  const lists = {
    prs: [{ number: 42, title: "Tighten redirect check", url: "https://example.test/example/widget/pull/42", author: "alice", draft: false, mine: false, created_at: 100, labels: ["security"] }],
    prs_error: null,
    issues: [{ number: 19, title: "Redirect loop on sign-out", labels: ["bug"], author: "bob", opened_at: 90, comments: 2, url: "https://example.test/example/widget/issues/19", assignees: ["carol"] }],
    issues_error: null,
  };
  const terminals = [{ id: "t1", project: projectName, machine_id: "build-box", machine_name: "build-box", name: "redirect-watch", command: "npm test", cwd: "/srv/project", state: "running", code: null, started_at: 50 }];

  await openDesktop(page, false, {
    center_window_alone: { kind: "project", project: projectName },
    center_palette_index: index,
    center_find_prompts: [widgetSession, otherSession],
    center_project_lists: lists,
    terminals_list: terminals,
    project_skills_get: { libraries: [], keep_out_of_git: true },
  }, "project-widget");

  await expect(page.locator(".pj-strip")).toBeVisible();
  await page.keyboard.press(paletteKey);

  const dialog = page.getByRole("dialog", { name: "Search" });
  const input = dialog.getByRole("combobox");
  const options = dialog.getByRole("option");
  await expect(input).toHaveAttribute("placeholder", `Search ${projectName}: sessions, PRs, issues, branches…`);
  await expect(options.first()).toContainText("New session");
  await expect(dialog).toContainText("Pull requests");
  await expect(dialog).toContainText("Skills");
  await expect(dialog).not.toContainText("Home");
  await expect(dialog).not.toContainText("Settings");

  await input.fill("other/repo");
  await expect(dialog.locator(".none")).toContainText("Nothing matches");

  await input.fill("redirect");
  await expect(options).toContainText([
    /#19 Redirect loop on sign-out/,
    /Fix redirect loop/,
    /#42 Tighten redirect check/,
    /fix-redirect/,
    /redirect-watch/,
  ]);
  await expect(dialog).not.toContainText("Other redirect task");
  await expect(dialog).not.toContainText("other/repo");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "center_find_prompts").map(c => c.args))).toContainEqual({
    query: "redirect",
    project: projectName,
  });

  await dialog.getByRole("option", { name: /#42 Tighten redirect check/ }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.locator(".pj-det")).toContainText("Tighten redirect check");
  expect(await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "center_open"))).toEqual([]);

  await page.keyboard.press(paletteKey);
  await input.fill("Skills");
  await page.keyboard.press("Enter");
  const sheet = page.getByRole("dialog", { name: `Skills in ${projectName}` });
  await expect(sheet).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
});
