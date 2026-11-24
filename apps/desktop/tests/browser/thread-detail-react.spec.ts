import { expect, test } from "@playwright/test";
import { allowSelection, openDesktop, projectName } from "./harness.ts";

test("issue detail keeps comment draft, caret and scroll during project refresh", async ({ page }) => {
  const now = Date.now(), issue = { number: 1, title: "Example issue", url: "https://example.test/issues/1", author: "Author", opened_at: now, comments: 1, labels: [] };
  const detail = { kind: "issue", number: 1, title: issue.title, url: issue.url, author: issue.author, state: "open", draft: false, labels: [], opened_at: now, body_html: "<p>Issue description</p>", comments: [{ author: "Writer", at: now, html: "<p>Existing comment</p>" }], earlier: 0, review: "none", checks: null, linked_prs: [] };
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    center_project_lists: { issues: [issue], prs: [], issues_error: null, prs_error: null }, center_thread_detail: detail,
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "issues" } }), projectName);
  await page.locator('.pj-det .pj-li[title="Example issue"]').click();
  const pane = page.locator(".pj-split > .pj-det"), input = pane.locator(".pj-reply textarea");
  await expect(input).toBeVisible();
  const action = pane.getByRole("button", { name: "Work on it" });
  await action.focus();
  await action.evaluate(element => { (element as HTMLElement).dataset.identity = "kept"; });
  const actionReads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "projects_on_top").length);
  await page.evaluate(() => window.__navigation.notify("projects"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "projects_on_top").length)).toBeGreaterThan(actionReads);
  await expect(action).toHaveAttribute("data-identity", "kept");
  await expect(action).toBeFocused();
  await input.fill("Draft reply");
  await input.evaluate(element => { if (element instanceof HTMLTextAreaElement) { element.dataset.identity = "kept"; element.setSelectionRange(2, 5); } });
  await pane.evaluate(element => { element.style.maxHeight = "150px"; element.scrollTop = 80; });
  const top = await pane.evaluate(element => element.scrollTop);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "projects_on_top").length);
  await page.evaluate(() => window.__navigation.notify("projects"));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "projects_on_top").length)).toBeGreaterThan(reads);
  await expect(input).toHaveAttribute("data-identity", "kept");
  await expect(input).toBeFocused();
  await expect(input).toHaveValue("Draft reply");
  await expect(input).toHaveJSProperty("selectionStart", 2);
  await expect(pane).toHaveJSProperty("scrollTop", top);
  await page.evaluate(() => window.__navigation.hold("center_comment_thread"));
  await pane.getByRole("button", { name: "Comment", exact: true }).click();
  await expect(input).toBeDisabled();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_comment_thread").map(call => call.args.body))).toEqual(["Draft reply"]);
  await page.evaluate(() => window.__navigation.release("center_comment_thread"));
  await expect(input).toBeEnabled();
  await expect(input).toHaveValue("");
  const paragraph = pane.locator(".pj-md p").first();
  await allowSelection(page);
  await paragraph.evaluate(element => { Reflect.set(window, "threadParagraph", element); const range = document.createRange(); range.selectNodeContents(element); window.getSelection()?.removeAllRanges(); window.getSelection()?.addRange(range); });
  const detailReads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_thread_detail").length);
  await page.evaluate(value => { window.__navigation.respond("center_thread_detail", value); const now = Date.now(); Date.now = () => now + 61000; window.__navigation.notify("projects"); }, { ...detail, body_html: "<p>Issue description</p><p>New information</p>" });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_thread_detail").length)).toBeGreaterThan(detailReads);
  await expect(pane.getByText("New information")).toBeVisible();
  expect(await paragraph.evaluate(element => element === Reflect.get(window, "threadParagraph"))).toBe(true);
  expect(await page.evaluate(() => String(window.getSelection()))).toBe("Issue description");
});

test("Work on it picks a model under the button and starts the issue's agent at once", async ({ page }) => {
  const now = Date.now(), issue = { number: 7, title: "Example issue", url: "https://example.test/issues/7", author: "Author", opened_at: now, comments: 0, labels: [] };
  const detail = { kind: "issue", number: 7, title: issue.title, url: issue.url, author: issue.author, state: "open", draft: false, labels: [], opened_at: now, body_html: "<p>Issue description</p>", comments: [], earlier: 0, review: "none", checks: null, linked_prs: [] };
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    agent_models: { pick: "listed", current: "alpha", models: [{ id: "alpha", name: "Alpha", description: null }, { id: "beta", name: "Beta", description: null }] },
    agent_begin: { session: { machine_id: "build-box", machine_name: "build-box", id: "start:one", agent: "codex", cwd: "/srv/project", status: "starting", external: false, question: null } },
    center_project_lists: { issues: [issue], prs: [], issues_error: null, prs_error: null }, center_thread_detail: detail,
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "issues" } }), projectName);
  await page.locator('.pj-det .pj-li[title="Example issue"]').click();
  const work = page.locator(".pj-split > .pj-det").getByRole("button", { name: "Work on it" });
  await expect(work).toBeEnabled();
  await work.click();
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  await expect(picker).toBeVisible();
  // It asks the machine first, nothing picked for you.
  await expect(picker.locator(".agc-stephead")).toHaveAttribute("data-agstep", "machine");
  await expect(picker.locator("[data-agmo]")).toHaveCount(0);
  await picker.locator('[data-agmachine="build-box"]').click();
  await picker.locator('[data-agh="codex"]').click();
  await picker.locator('[data-agmo="beta"]').click();
  await picker.locator('[data-age=""]').click();
  await expect(picker).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin").length)).toBe(1);
  const args = await page.evaluate(() => window.__navigation.calls().find(call => call.command === "agent_begin")?.args);
  expect(args).toMatchObject({ machineId: "build-box", agent: "codex", model: "beta", checkout: { named_worktree: { branch: "issue-7-example-issue" } } });
  expect(String(args?.prompt)).toContain("https://example.test/issues/7");
  await expect(page.locator(".cc-pjc:visible").getByRole("textbox", { name: "Task for a coding agent" })).toHaveCount(0);

  // The session is named for the issue once its machine names it.
  const session = { machine_id: "build-box", machine_name: "build-box", id: "worker", agent: "codex", cwd: "/srv/project/.worktrees/issue-7-example-issue", status: "working", external: false, question: null };
  await page.evaluate(session => window.__navigation.emit("agent-started", { start: "start:one", session, first_message: "Work on it" }), session);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_title_save").map(call => call.args))).toEqual([{ machineId: "build-box", agent: "codex", session: "worker", title: "Issue #7" }]);
});

test("an issue opens the session that worked on it, one started before sessions were named found by its worktree and named then", async ({ page }) => {
  const now = Date.now(), issue = { number: 7, title: "Example issue", url: "https://example.test/issues/7", author: "Author", opened_at: now, comments: 0, labels: [] };
  const detail = { kind: "issue", number: 7, title: issue.title, url: issue.url, author: issue.author, state: "open", draft: false, labels: [], opened_at: now, body_html: "<p>Issue description</p>", comments: [], earlier: 0, review: "none", checks: null, linked_prs: [] };
  const row = (id: string, title: string | null, status: string) => ({ kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title, cwd: "/srv/project/.worktrees/issue-7-example-issue", status, doing: null, started_at: 1, last_activity: 1, external: false, question: null });
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    agent_sessions: [row("earlier", null, "ended")], project_sessions: [row("earlier", null, "ended")],
    center_project_lists: { issues: [issue], prs: [], issues_error: null, prs_error: null }, center_thread_detail: detail,
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "issues" } }), projectName);
  await page.locator('.pj-det .pj-li[title="Example issue"]').click();
  const pane = page.locator(".pj-split > .pj-det");
  await expect(pane.getByRole("button", { name: "Open session" })).toBeVisible();
  await expect(pane.getByRole("button", { name: "Work on it" })).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_title_save").map(call => call.args))).toEqual([{ machineId: "build-box", agent: "codex", session: "earlier", title: "Issue #7" }]);
});

test("Work on it asks the machine first when the project is on several", async ({ page }) => {
  const now = Date.now(), issue = { number: 7, title: "Example issue", url: "https://example.test/issues/7", author: "Author", opened_at: now, comments: 0, labels: [] };
  const detail = { kind: "issue", number: 7, title: issue.title, url: issue.url, author: issue.author, state: "open", draft: false, labels: [], opened_at: now, body_html: "<p>Issue description</p>", comments: [], earlier: 0, review: "none", checks: null, linked_prs: [] };
  const host = (id: string, agents: string[]) => ({ machine_id: id, machine_name: id, agents, workspace: null, home: "/srv", capabilities: [] });
  await openDesktop(page, false, {
    agent_hosts: [host("build-box", ["codex"]), host("gpu-box", ["claude", "codex"]), host("spare-box", ["codex"])],
    center_home: { machines: [], running: [], needs_you: [], projects: ["build-box", "gpu-box"].map(id => ({ display_name: null, name: projectName, url: "https://example.test/example/widget", machine_id: id, machine_name: id, folder: "/srv/project", cloned: true })) },
    agent_models: { pick: "listed", current: "alpha", models: [{ id: "alpha", name: "Alpha", description: null }] },
    agent_begin: { session: { machine_id: "gpu-box", machine_name: "gpu-box", id: "start:one", agent: "claude", cwd: "/srv/project", status: "starting", external: false, question: null } },
    center_project_lists: { issues: [issue], prs: [], issues_error: null, prs_error: null }, center_thread_detail: detail,
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "issues" } }), projectName);
  await page.locator('.pj-det .pj-li[title="Example issue"]').click();
  await page.locator(".pj-split > .pj-det").getByRole("button", { name: "Work on it" }).click();
  const picker = page.getByRole("dialog", { name: "Machine, harness, model and thinking" });
  // A machine without the project's checkout is listed but cannot be picked.
  await expect(picker.locator('[data-agmachine="spare-box"]')).toBeDisabled();
  await picker.locator('[data-agmachine="gpu-box"]').click();
  await expect(picker.locator(".agc-stephead")).toHaveAttribute("data-agstep", "harness");
  await picker.locator('[data-agh="claude"]').click();
  await picker.locator('[data-agmo="alpha"]').click();
  await picker.locator('[data-age="high"]').click();
  await expect(picker).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin").length)).toBe(1);
  const args = await page.evaluate(() => window.__navigation.calls().find(call => call.command === "agent_begin")?.args);
  expect(args).toMatchObject({ machineId: "gpu-box", agent: "claude", model: "alpha", effort: "high" });
});

test("Review stays in the checkout and Work on PR targets the branch when a session only mentions the PR", async ({ page }) => {
  const reviewer = { machine_id: "build-box", machine_name: "build-box", id: "reviewer", agent: "codex", cwd: "/srv/project", status: "working", external: false, question: null };
  const now = Date.now(), pr = { number: 9, title: "Example change", url: "https://github.com/example/widget/pull/9", author: "Author", draft: false, mine: false, created_at: now, labels: [] };
  const detail = { kind: "pr", number: 9, title: pr.title, url: pr.url, author: pr.author, state: "open", draft: false, labels: [], opened_at: now, body_html: "<p>Change</p>", comments: [], earlier: 0, review: "none", checks: null, linked_prs: [] };
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
    agent_models: { pick: "listed", current: "alpha", models: [{ id: "alpha", name: "Alpha", description: null }] },
    agent_begin: { session: { ...reviewer, id: "start:review", status: "starting" } },
    center_find_prompts: [{ session: reviewer }],
    agent_older: { earlier: 0, entries: [
      { kind: "agent", text: `Reviewed ${pr.url}` },
      { kind: "tool", text: `Bash: gh pr view 9 --json url [completed]\n${pr.url}` },
      { kind: "tool", text: `Bash: gh pr create [failed]\n${pr.url}` },
      { kind: "tool", text: `Bash: gh pr create [completed]\n${pr.url}0` },
    ] },
    center_pull_request_head: { branch: "example-change", fork: false },
    center_project_lists: { issues: [], prs: [pr], issues_error: null, prs_error: null }, center_thread_detail: detail,
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "1 PR" }).click();
  await page.locator('.pj-split:not(.gh-layout) > .pj-list .pj-li[title="Example change"] .t > b').click();
  await page.locator(".pj-split > .pj-det").getByRole("button", { name: "Review with agent" }).click();
  await page.getByRole("dialog", { name: "Machine, harness, model and thinking" }).locator('[data-agmachine="build-box"]').click();
  await page.getByRole("dialog", { name: "Machine, harness, model and thinking" }).locator('[data-agh="codex"]').click();
  await page.getByRole("dialog", { name: "Machine, harness, model and thinking" }).locator('[data-agmo="alpha"]').click();
  await page.getByRole("dialog", { name: "Machine, harness, model and thinking" }).locator('[data-age=""]').click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin").length)).toBe(1);
  const args = await page.evaluate(() => window.__navigation.calls().find(call => call.command === "agent_begin")?.args);
  expect(args).toMatchObject({ agent: "codex", model: "alpha", cwd: "/srv/project", checkout: null });
  expect(String(args?.prompt)).toContain("Review PR #9");
  expect(String(args?.prompt)).toContain("refs/pull/9/head");
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "center_pull_request_head"))).toEqual([]);

  // The review is named once its machine names the session.
  const titles = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_title_save").map(call => call.args));
  expect(await titles()).toEqual([]);
  await page.evaluate(session => window.__navigation.emit("agent-started", { start: "start:review", session, first_message: "Review PR #9" }), reviewer);
  await expect.poll(titles).toEqual([{ machineId: "build-box", agent: "codex", session: "reviewer", title: "Review #9" }]);

  // Back on the PR, Work on PR starts on its branch.
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator(".pj-strip button").filter({ hasText: "1 PR" }).click();
  await page.locator('.pj-split:not(.gh-layout) > .pj-list .pj-li[title="Example change"] .t > b').click();
  await page.locator(".pj-split > .pj-det").getByRole("button", { name: "Work on PR" }).click();
  await page.getByRole("dialog", { name: "Machine, harness, model and thinking" }).locator('[data-agmachine="build-box"]').click();
  await page.getByRole("dialog", { name: "Machine, harness, model and thinking" }).locator('[data-agh="codex"]').click();
  await page.getByRole("dialog", { name: "Machine, harness, model and thinking" }).locator('[data-agmo="alpha"]').click();
  await page.getByRole("dialog", { name: "Machine, harness, model and thinking" }).locator('[data-age=""]').click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin").length)).toBe(2);
  const next = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin")[1]?.args);
  expect(next).toMatchObject({ agent: "codex", checkout: { branch_worktree: { branch: "example-change" } } });
  expect(String(next?.prompt)).toContain("Work on PR #9");
});

test("Continue opens the PR creator's session without a model picker", async ({ page }) => {
  const now = Date.now();
  const pr = { number: 9, title: "Example change", url: "https://github.com/example/widget/pull/9", author: "Author", draft: false, mine: true, created_at: now, labels: [] };
  const detail = { kind: "pr", number: pr.number, title: pr.title, url: pr.url, author: pr.author, state: "open", draft: false, labels: [], opened_at: now, body_html: "<p>Change</p>", comments: [], earlier: 0, review: "none", checks: null, linked_prs: [] };
  const creator = { machine_id: "build-box", machine_name: "build-box", id: "creator", agent: "codex", title: "Original work", cwd: "/srv/project", status: "ended", external: false, question: null };
  await openDesktop(page, false, {
    center_project_lists: { issues: [], prs: [pr], issues_error: null, prs_error: null },
    center_thread_detail: detail,
    center_find_prompts: [{ session: creator }],
    agent_older: { earlier: 0, entries: [{
      kind: "tool",
      text: "Open a pull request [completed]",
      tool: { name: "exec_command", input: JSON.stringify({ cmd: "git push && gh -R example/widget pr create" }), input_truncated: false, result: JSON.stringify(pr.url), result_truncated: false },
    }] },
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "pr:9" } }), projectName);
  await page.locator(".pj-split > .pj-det").getByRole("button", { name: "Continue", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "agent_view" && call.args.session === "creator"))).toBe(true);
  await expect(page.getByRole("dialog", { name: "Machine, harness, model and thinking" })).toHaveCount(0);
  const starts = await page.evaluate(() => window.__navigation.calls().filter(call => ["agent_begin", "agent_resume", "agent_steer", "center_pull_request_head"].includes(call.command)));
  expect(starts).toEqual([]);
});

test("Open on GitHub opens the current pull request in the browser", async ({ page }) => {
  const now = Date.now();
  const pr = { number: 9, title: "Example change", url: "https://github.com/example/widget/pull/9", author: "Author", draft: false, mine: false, created_at: now, labels: [] };
  const detail = { kind: "pr", number: pr.number, title: pr.title, url: pr.url, author: pr.author, state: "open", draft: false, labels: [], opened_at: now, body_html: "<p>Change</p>", comments: [], earlier: 0, review: "none", checks: null, linked_prs: [] };
  await openDesktop(page, false, { center_project_lists: { issues: [], prs: [pr], issues_error: null, prs_error: null }, center_thread_detail: detail });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "pr:9" } }), projectName);
  const pane = page.locator(".pj-split > .pj-det");
  await expect(pane.getByText("Change", { exact: true })).toBeVisible();
  await pane.getByRole("button", { name: "Open on GitHub", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_url").map(call => call.args))).toEqual([{ url: pr.url }]);
});

test("an issue's linked pull requests open here or on the host", async ({ page }) => {
  const now = Date.now(), issue = { number: 1, title: "Example issue", url: "https://github.com/example/widget/issues/1", author: "Author", opened_at: now, comments: 0, labels: [], linked_pr: true };
  const pr = { number: 9, title: "Example change", url: "https://github.com/example/widget/pull/9", author: "Author", draft: false, mine: false, created_at: now, labels: [] };
  const other = { number: 4, title: "Elsewhere", url: "https://github.com/example/other/pull/4", state: "merged" };
  const detail = { kind: "issue", number: 1, title: issue.title, url: issue.url, author: issue.author, state: "open", draft: false, labels: [], opened_at: now, body_html: "<p>Issue</p>", comments: [], earlier: 0, review: "none", checks: null, linked_prs: [{ number: 9, title: pr.title, url: pr.url, state: "open" }, other] };
  await openDesktop(page, false, { center_project_lists: { issues: [issue], prs: [pr], issues_error: null, prs_error: null }, center_thread_detail: detail });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "issues" } }), projectName);
  await page.locator('.pj-det .pj-li[title="Example issue"]').click();
  const pane = page.locator(".pj-split > .pj-det");
  await pane.locator(".cc-line").filter({ hasText: "Elsewhere" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "open_url").map(call => call.args))).toEqual([{ url: other.url }]);
  await pane.locator(".cc-line").filter({ hasText: "Example change" }).click();
  await expect(page.locator('.pj-list .pj-li[title="Example change"]')).toHaveAttribute("aria-current", "true");
});

for (const { kind, subject, path } of [
  { kind: "issue", subject: "issue", path: "issues" },
  { kind: "pr", subject: "pull request", path: "pull" },
]) {
  test(`Discuss with agent fills an editable ${subject} prompt before sending`, async ({ page }) => {
    const now = Date.now(), number = 7, title = "Example change";
    const url = `https://github.com/example/widget/${path}/${number}`;
    const thread = { number, title, url, author: "Author", opened_at: now, labels: [] };
    const issue = { ...thread, comments: 0 };
    const pr = { ...thread, draft: false, mine: false, created_at: now };
    const detail = { ...thread, kind, state: "open", draft: false, body_html: "<p>Change</p>", comments: [], earlier: 0, review: "none", checks: null, linked_prs: [] };
    await openDesktop(page, false, {
      agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
      agent_models: { pick: "listed", current: "alpha", models: [{ id: "alpha", name: "Alpha", description: null }] },
      agent_begin: { session: { machine_id: "build-box", machine_name: "build-box", id: "start:discussion", agent: "codex", cwd: "/srv/project", status: "starting", external: false, question: null } },
      center_project_lists: { issues: kind === "issue" ? [issue] : [], prs: kind === "pr" ? [pr] : [], issues_error: null, prs_error: null },
      center_thread_detail: detail,
    });
    await page.evaluate(tile => window.__navigation.open({ projects: { project: tile.project, tile: tile.key } }), { project: projectName, key: `${kind}:${number}` });
    await page.locator(".pj-split > .pj-det").getByRole("button", { name: "Discuss with agent" }).click();
    const composer = page.locator(".cc-pjc:visible");
    const input = composer.getByRole("textbox", { name: "Task for a coding agent" });
    const prompt = `I want to discuss about ${subject} #${number}: ${title} (${url}).\n\n`;
    await expect(input).toHaveValue(prompt);
    await expect(input).toBeEditable();
    await expect(input).toBeFocused();

    const starts = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin").map(call => call.args));
    expect(await starts()).toEqual([]);
    const followUp = "What options do you see?";
    await page.keyboard.insertText(followUp);
    const edited = prompt + followUp;
    await expect(input).toHaveValue(edited);
    if (await page.locator("body").evaluate(body => body.classList.contains("mobile"))) {
      await composer.getByRole("button", { name: "Start", exact: true }).click();
    } else {
      await input.press("Enter");
    }
    await expect.poll(starts).toMatchObject([{ cwd: "/srv/project", checkout: null, prompt: edited }]);
  });
}
