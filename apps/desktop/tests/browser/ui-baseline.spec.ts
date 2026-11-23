// UI baseline: a "no visual or DOM change" check for refactors.
//
// Each test opens one representative state through the browser harness with
// fixed fixture data and a frozen clock, then compares two things against the
// baselines in `ui-baseline.spec.ts-snapshots/`:
//   - a normalized DOM snapshot of the window (tags, sorted classes, ids,
//     roles, aria and other stable attributes, text, focus), committed, and
//   - a screenshot with a small tolerance, recorded locally and never committed.
// Generated ids, `data-sig`, inline style values and other volatile details
// are left out of the DOM snapshot.
//
// Chromium only (the macOS project); WebKit renders fonts differently.
//
// Run from the repository root:
//   flock .artifacts/browser.lock sh -c 'npm --prefix apps/desktop run build && cd apps/desktop && npx playwright test ui-baseline --project=macOS --workers=6 --reporter=line'
//
// When a change to the UI is intentional, rerun with `--update-snapshots`,
// review the changed files under `ui-baseline.spec.ts-snapshots/` with
// `git diff`, and commit them with the change.

import { EMPTY } from "../../src/settings/flsh-state.ts";
import { expect, test, type Page } from "@playwright/test";
import { openDesktop, openStartForm, projectName } from "./harness.ts";
import { audit, auditing } from "./ui-audit.ts";
import { summaryPrompt } from "../../src/agents/report-state.ts";

/// The frozen wall clock every state renders at.
const NOW = Date.UTC(2026, 0, 15, 12, 0, 0);
const PHONE = { width: 375, height: 812 };

test.describe.configure({ mode: "parallel" });
test.use({ timezoneId: "UTC", locale: "en-US", deviceScaleFactor: auditing ? 2 : 1 });
test.beforeEach(async ({ page, browserName }, info) => {
  test.skip(!process.env.WK && (browserName !== "chromium" || info.project.name !== "macOS"), "The baseline is recorded once, on Chromium");
  await page.clock.setFixedTime(NOW);
  await page.emulateMedia({ reducedMotion: "reduce", colorScheme: "light" });
  await page.addInitScript(() => localStorage.setItem("beep.starPrompt.dismissed", "1"));
});

// Fixtures

const machineId = "build-box";
const folder = "/srv/project";

const machine = (id: string, name: string, online: boolean, agents: number) => ({ id, name, online, can_host: true, seen_at: NOW - 3_600_000, version: "0.9.0", env: null, joined: true, apps: [], gpus: [], capabilities: [], agents, stats: null });
const project = (name: string, url = `https://example.test/${name}`, box = machineId) => ({ name, url, machine_id: box, machine_name: box, folder: `/srv/${name}`, cloned: true, imported_at: NOW - 86_400_000 });
const agentRow = (id: string, title: string, status: string, ago: number, cwd = folder) => ({ kind: "agent", machine_id: machineId, machine_name: machineId, id, agent: "codex", title, cwd, status, doing: status === "working" ? "cargo test -p beep-node --workspace --all-features -- --nocapture --test-threads 8 2>&1 | tail -40" : null, started_at: NOW - 7_200_000, last_activity: NOW - ago, external: false, question: null });

const projects = [project(projectName), project("example/gadget"), project("tools/delta", "https://forge.test/team/delta")];
const running = [agentRow("notice-one", "Fix the login redirect", "working", 30_000, "/srv/example/widget"), agentRow("notice-two", "Update the changelog", "idle", 600_000, "/srv/example/gadget")];
const home = { machines: [machine("build-box", "build-box", true, 2), machine("test-box", "test-box", false, 0)], projects, running, needs_you: [] };

const session = { kind: "agent", machine_id: machineId, machine_name: machineId, id: "notice-one", agent: "codex", title: "Fix the login redirect", cwd: folder, status: "working", doing: null, started_at: NOW - 7_200_000, last_activity: NOW - 30_000, external: false };
const transcript = [
  { kind: "user", text: "The login page redirects to a blank page after sign in. Find out why and fix it." },
  { kind: "tool", text: "Read: src/login.ts [completed]" },
  { kind: "tool", text: "Bash: npm test [completed]" },
  { kind: "agent", text: "The redirect reads `next` before the session cookie is set.\n\n- Moved the read after the cookie write\n- Added a test for the empty `next` case\n\nAll tests pass." },
  { kind: "other", text: "Switched to thinking medium from the next turn" },
  { kind: "user", text: "Also keep the query string." },
  { kind: "agent", text: "Done. The query string now survives the redirect." },
];
const agentView = { session: { ...session, question: null }, plan: [], files: [], entries: transcript, earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 2, usage: null };
const sessionUsage = { context: { used: 250_000, size: 1_000_000 }, tokens: 54_200_000, cost_usd: 34.9 };
const listing = { folders: [{ path: "", entries: [{ name: "src", folder: true, size: 0, ignored: false, change: null }, { name: "README.md", folder: false, size: 1200, ignored: false, change: null }, { name: "package.json", folder: false, size: 640, ignored: false, change: "modified" }, { name: "sample.ts", folder: false, size: 300, ignored: false, change: "modified" }], more: 0, git: true }], truncated: false };

const commits = [
  { id: "c3d4e5f6a7b8", parents: ["b2c3d4e5f6a7"], author: "Example", committed_at: NOW - 3_600_000, subject: "Keep the query string on redirect" },
  { id: "b2c3d4e5f6a7", parents: ["a1b2c3d4e5f6"], author: "Example", committed_at: NOW - 86_400_000, subject: "Read next after the cookie write" },
  { id: "a1b2c3d4e5f6", parents: [], author: "Example", committed_at: NOW - 172_800_000, subject: "Initial commit" },
];
const gitHistory = { machine_id: machineId, machine_name: machineId, root: folder, refs: [{ name: "main", head: "c3d4e5f6a7b8", kind: "local" }], worktrees: [], commits, next: null };
const emptyDiff = { root: folder, base: "", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0, files: [] };
const pastSessions = Array.from({ length: 6 }, (_, i) => ({ ...agentRow(`past-${i}`, `Past session ${i + 1}`, "ended", (i + 1) * 3_600_000), external: false }));
const projectPage = { project_git_history: gitHistory, project_git_files: [], project_git_diff: emptyDiff, project_sessions: pastSessions };

// DOM snapshot

/// Serializes `selector` as an indented outline of the stable parts of each
/// element. Runs in the page.
function serializeDom(selector: string): string {
  const generated = /react-aria[\w-]*|«[^»]*»|:r[0-9a-z]+:|_r_[0-9a-z]+_|[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g;
  const clean = (value: string) => value.replace(generated, "<id>").replace(/\s+/g, " ").trim();
  const volatile = new Set(["class", "id", "style", "data-sig", "data-identity", "data-tauri-drag-region"]);
  const stable = (name: string) => !volatile.has(name) && !name.startsWith("on");
  const skipped = new Set(["script", "style", "link", "meta", "noscript", "template"]);
  const leaves = new Set(["svg", "canvas", "iframe"]);
  const lines: string[] = [];

  const walk = (element: Element, depth: number) => {
    const tag = element.tagName.toLowerCase();
    if (skipped.has(tag)) return;

    let line = tag;
    if (element.id) line += `#${clean(element.id)}`;
    const classes = [...element.classList].sort();
    if (classes.length) line += `.${classes.join(".")}`;
    const attributes = [...element.attributes].filter(attribute => stable(attribute.name))
      .map(attribute => {
        const value = clean(attribute.value);
        const shown = value.startsWith("data:") ? `${value.slice(0, 40)}…` : value;
        return shown === "" ? attribute.name : `${attribute.name}=${JSON.stringify(shown)}`;
      })
      .sort();
    if (attributes.length) line += ` [${attributes.join(" ")}]`;
    if (element instanceof HTMLElement && element.style.length) {
      const names = [...element.style].sort();
      line += ` style(${names.join(",")}${element.style.display === "none" ? ";display:none" : ""})`;
    }
    if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement || element instanceof HTMLSelectElement) line += ` value=${JSON.stringify(element.value)}`;
    if (element instanceof HTMLInputElement && (element.type === "checkbox" || element.type === "radio")) line += element.checked ? " checked" : " unchecked";
    if (document.activeElement === element && element !== document.body) line += " (focused)";
    const text = [...element.childNodes].filter(node => node.nodeType === Node.TEXT_NODE).map(node => node.textContent ?? "").join(" ");
    const cleanText = clean(text);
    if (cleanText) line += ` "${cleanText}"`;
    lines.push(`${"  ".repeat(depth)}${line}`);

    if (leaves.has(tag)) return;
    for (const child of element.children) walk(child, depth + 1);
  };

  const root = document.querySelector(selector);
  if (!root) return `missing ${selector}`;
  walk(root, 0);
  return `${lines.join("\n")}\n`;
}

/// Waits until fonts are in and the DOM stops changing, then compares the DOM
/// snapshot and a screenshot against the baselines named `name`.
async function baseline(page: Page, name: string, root = "body") {
  await page.evaluate(() => document.fonts.ready);
  let previous = "";
  await expect.poll(async () => {
    await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
    const current = await page.evaluate(serializeDom, root);
    const same = current === previous;
    previous = current;
    return same;
  }, { intervals: [100, 150, 200, 250], timeout: 10_000 }).toBe(true);

  if (auditing) return audit(page, name);

  expect(previous).toMatchSnapshot(`${name}.txt`);
  await expect(page).toHaveScreenshot(`${name}.png`, { maxDiffPixelRatio: 0.002, animations: "disabled", caret: "hide" });
}

async function openAgent(page: Page, responses: Record<string, unknown> = {}) {
  await openDesktop(page, false, { agent_view: { ...agentView, usage: sessionUsage }, agent_tree: listing, ...responses });
  await page.evaluate(target => window.__navigation.open({ agent: target }), session);
  await expect(page.getByText("The query string now survives the redirect.")).toBeVisible();
}

async function openProject(page: Page, responses: Record<string, unknown> = {}) {
  await openDesktop(page, false, { center_home: home, ...projectPage, ...responses });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name } }), projectName);
  await expect(page.locator(".pj-strip")).toBeVisible();
}

/// Assistant sessions: two that run, one ended.
const linux = { os: "linux", os_version: "Ubuntu 26.04", arch: "x86_64", hostname: "build-box", cpus: 32, memory_mb: 124_000, home: "/srv", tools: [], logins: [], avds: [], gpus: [] };
const assistantHome = { ...home, machines: [{ ...home.machines[0], env: linux }, home.machines[1]] };
const assistantSessions = [
  { ...agentRow("helper-one", "Set up the widget project", "working", 60_000, "/srv"), agent: "flsh", assistant: true, parent: null },
  { ...agentRow("helper-two", "Check the open pull requests", "idle", 900_000, "/srv"), agent: "claude", assistant: true, parent: null, external: true },
  { ...agentRow("helper-old", "Import my projects", "ended", 7_200_000, "/srv"), agent: "flsh", assistant: true, parent: null },
];
const assistantHost = { machine_id: machineId, machine_name: machineId, agents: ["claude", "codex", "flsh"], workspace: null, home: "/srv", capabilities: [], assistant: true };

const gitItem = (repo: string, number: number, kind: string, title: string, hours: number, extra = {}) => ({ host: "example.test", repo, number, kind, title, url: `https://example.test/${repo}/${number}`, updated_at: NOW - hours * 3_600_000, authored: true, assigned: false, review_requested: false, draft: false, review: "none", checks_failing: false, comments: 0, labels: [], ...extra });
const gitItems = [
  gitItem("agent/plugins", 412, "pull_request", "feat: add background and foreground task switch", 3, { labels: ["plugins"] }),
  gitItem("agent/plugins", 398, "pull_request", "fix: keep a session's model when it resumes", 24, { checks_failing: true }),
  gitItem("agent/plugins", 377, "issue", "Tool calls should be shown in one place", 96),
  gitItem("docs/handbook", 88, "pull_request", "docs: a shorter README and a manual", 30),
  gitItem("docs/handbook", 85, "pull_request", "docs: sync, explained in one page", 50),
  gitItem("cli/runner", 31, "pull_request", "fix: timeouts other than the default are honoured", 48),
  gitItem("cli/runner", 14, "issue", "Exit code is lost in a pipe", 60),
  gitItem("lib/parser", 12, "issue", "Tables break on nested pipes", 168),
];

// Desktop window

test("home overview", async ({ page }) => {
  await openDesktop(page, false, { center_home: home });
  await expect(page.locator("#sec-running .bd-card").first()).toBeVisible();
  await baseline(page, "home-overview");
});

test("home review", async ({ page }) => {
  const widget = [agentRow("notice-three", "Add CSV export to reports", "idle", 1_200_000, "/srv/example/widget"), agentRow("notice-four", "Bump the parser", "working", 60_000, "/srv/example/widget")];
  const replied = [...running, ...widget].map(row => row.status === "idle" ? { ...row, reply_html: "Done. The changelog lists <code>0.9.0</code>." } : row);
  await openDesktop(page, false, { center_home: { ...home, running: replied } });
  await page.getByRole("radio", { name: "Review" }).click();
  await expect(page.locator(".bd-review .bd-log .cc-line")).toHaveCount(2);
  await expect(page.locator(".bd-review .bd-card")).toHaveCount(2);
  await baseline(page, "home-review");
});

test("assistant page", async ({ page }) => {
  const chat = assistantSessions[0];
  const transcript = [
    { kind: "user", text: "Set up the widget project on build-box." },
    { kind: "agent", text: "Cloned example/widget into ~/Projects/example/widget and imported it." },
  ];
  await openDesktop(page, false, {
    center_home: assistantHome, agent_sessions: assistantSessions, agent_hosts: [assistantHost],
    assistant_settings_get: { agent: "flsh", model: null, effort: "high", fast: null },
    agent_view: { ...agentView, session: { ...chat, question: null }, entries: transcript },
  });
  await page.locator("#side").getByRole("button", { name: /^Assistant/ }).click();
  const list = page.locator(".asst-list");
  await list.getByRole("button", { name: /Set up the widget project/ }).click();
  await expect(page.getByText("Cloned example/widget")).toBeVisible();
  await expect(list.locator('[aria-current="true"]')).toContainText("Set up the widget project");
  await baseline(page, "assistant-page");
});

test("assistant chat with threads", async ({ page }) => {
  const chat = { ...assistantSessions[0], title: "Battery chemistries after 2030", cwd: "/srv/.beep/assistant/ab12cd" };
  const thread = (id: string, title: string, over: object) => ({ ...chat, id, title, status: "idle", assistant: false, parent: { machine_id: machineId, id: chat.id, key: "" }, ...over });
  const threads = [
    thread("thread-one", "Sodium-ion cost curve", { status: "working", doing: "Reading the supply reports" }),
    thread("thread-two", "Solid-state timelines", { reply_html: "Pilot lines in 2027, no volume before 2030" }),
    thread("thread-three", "Recycling regulation", { status: "asking", question: { id: 1, text: "EU only, or US too?", detail: null, options: [] } }),
    // One that ended, found in its agent's files once its node started again: no link to its chat but its folder.
    { ...thread("thread-four", "Cathode prices", { status: "ended", external: true, parent: null }), cwd: `${chat.cwd}/cathode-prices` },
  ];
  const view = (row: object, text: string) => ({ ...agentView, session: { ...row, question: null }, entries: [{ kind: "agent", text }] });
  await openDesktop(page, false, {
    center_home: assistantHome, agent_sessions: [chat, ...threads, session], agent_hosts: [assistantHost],
    assistant_settings_get: { agent: "flsh", model: null, effort: "high", fast: null },
    agent_view: view(chat, "Three threads are on it. I will merge what they find."),
  });
  await page.locator("#side").getByRole("button", { name: /^Assistant/ }).click();
  const list = page.locator(".asst-list");
  await list.getByRole("button", { name: /Battery chemistries/ }).click();
  // The chat's threads take the sidebar's place; the one that ended is under Past.
  const rail = page.locator("#rail");
  await expect(rail.locator(".rail-threads")).toContainText("Solid-state timelines");
  await expect(rail.locator(".rail-scroll > .pj-li")).toContainText("Cathode prices");
  await baseline(page, "assistant-threads");

  // New thread starts the message that asks the chat's agent for one.
  await rail.getByRole("button", { name: "New thread" }).click();
  await expect(page.locator("#agmsg")).toHaveValue("New thread: ");
  await expect(page.locator("#agmsg")).toBeFocused();

  // The chat's own menu offers it too, which is how a phone reaches it: its list hides while a chat shows.
  await page.locator("#agmsg").fill("");
  await rail.getByRole("button", { name: /Battery chemistries/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "New thread" }).click();
  await expect(page.locator("#agmsg")).toHaveValue("New thread: ");

  // A thread opens on the page, with its own files and no Changes beside it, as the current item of its chat's rail.
  await page.evaluate(next => window.__navigation.respond("agent_view", next), view(threads[1], "Pilot lines in 2027, no volume before 2030."));
  await rail.getByRole("button", { name: /Solid-state timelines/ }).click();
  await expect(page.getByText("Pilot lines in 2027, no volume before 2030.")).toBeVisible();
  await expect(rail.locator('[aria-current="true"]')).toContainText("Solid-state timelines");
  await expect(page.getByRole("tab", { name: "Changes", exact: true })).toHaveCount(0);
});

test("assistant page empty", async ({ page }, info) => {
  await openDesktop(page, false, { center_home: home, agent_sessions: [], agent_hosts: [assistantHost], assistant_settings_get: { agent: "flsh", model: null, effort: "high", fast: null } });
  await expect(page.locator("#sec-running")).toBeVisible();
  // Cmd+N on Home lands on the page with a new chat.
  await page.keyboard.press(info.project.name === "Windows" ? "Control+n" : "Meta+n");
  await expect(page.locator(".asst-list")).toBeVisible();
  await expect(page.locator("#agpillb .flsh")).toBeVisible();
  await expect(page.locator("#aghostb")).toHaveText(machineId);
  await baseline(page, "assistant-page-empty");
});

test("home first start", async ({ page }) => {
  const place = (dir: string) => ({ machine_id: machineId, machine_name: machineId, folder: dir });
  const found = [
    { name: "example/alpha", remote: "https://example.test/alpha", places: [place("/srv/example/alpha")], agents: [{ agent: "claude", sessions: 12 }], last_activity: NOW - 2 * 86_400_000 },
    { name: "tools/echo", remote: "https://example.test/echo", places: [place("/srv/tools/echo")], agents: [], last_activity: NOW - 7 * 86_400_000 },
  ];
  await openDesktop(page, false, { center_home: { ...home, projects: [], running: [] }, found_projects: found, agent_hosts: [assistantHost], assistant_settings_get: { agent: "flsh", model: null, effort: "high", fast: null } });
  const prompt = page.getByRole("textbox", { name: "Message for the assistant" });
  await expect(prompt).toBeVisible();
  await expect(page.locator("#sec-found .pl-row")).toHaveCount(2);
  await expect(page.locator("#sec-found").getByRole("button", { name: "Import" })).toHaveCount(2);
  await expect(page.locator(".painted-empty .pill .flsh")).toBeVisible();
  await baseline(page, "home-first-start");

  await page.getByRole("button", { name: "Import my projects" }).click();
  await expect(prompt).toHaveValue("Import my projects");

  await page.evaluate(() => window.__navigation.hold("import_project"));
  await page.locator("#sec-found").getByRole("button", { name: "Import" }).first().click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "import_project").length)).toBe(1);

  await prompt.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin").map(call => call.args))).toEqual([
    { machineId: null, agent: "flsh", cwd: null, prompt: "Import my projects", images: [], model: null, effort: "high", fast: null, checkout: null, assistant: true, account: null },
  ]);
});

test("sidebar star prompt", async ({ page }) => {
  await page.addInitScript(() => localStorage.removeItem("beep.starPrompt.dismissed"));
  await openDesktop(page, false, { center_home: home });
  await expect(page.locator(".cc-star")).toBeVisible();
  await baseline(page, "sidebar-star-prompt", ".cc-side-list");
});

test("search palette", async ({ page }) => {
  const hit = (kind: string, id: string, title: string, detail: string) => ({ kind, id, title, detail, keys: [title.toLowerCase()] });
  const index = {
    actions: [hit("action", "new", "New example session", "")], settings: [hit("setting", "look", "Example look", "Settings")], code: [],
    projects: [hit("project", "one", "Example one", "Project"), hit("project", "two", "Example two", "Project")],
    sessions: [hit("session", "s1", "Fix the example redirect", "example/widget @ build-box")], machines: [hit("machine", "m1", "example-box", "online")],
  };
  const session = { machine_id: "m1", machine_name: "build-box", id: "s2", agent: "claude", cwd: "/srv/widget", status: "idle", external: false };
  const prompt = { ...hit("agent_session", "s2", "Tidy the example docs", "example/widget"), keys: [], session, snippet: "…rewrite the example in the README so it matches the new widget options and drop the old flags…" };
  await openDesktop(page, false, { center_home: home, center_palette_index: index, center_find_prompts: [prompt] });
  await page.getByRole("button", { name: /^Search/ }).click();
  await page.getByRole("dialog", { name: "Search" }).getByRole("combobox").fill("example");
  await page.locator(".cc-pal .it", { hasText: "Tidy the example docs" }).scrollIntoViewIfNeeded();
  await baseline(page, "palette");
});

test("palette project sessions", async ({ page }) => {
  const session = (id: string, title: string) => ({ kind: "agent_session", id, title, detail: "example/widget", keys: [title.toLowerCase()], project: "example/widget", session: { machine_id: "m1", machine_name: "build-box", id, agent: id === "s2" ? "codex" : "claude", cwd: "/srv/widget", status: "idle", external: false } });
  const index = {
    actions: [], settings: [], code: [], machines: [],
    projects: [{ kind: "project", id: "example/widget", title: "example/widget", detail: "build-box", keys: ["example/widget"] }],
    sessions: [session("s1", "Fix the example redirect"), session("s2", "Tidy the example docs"), session("s3", "Measure the widget's start")],
  };
  await openDesktop(page, false, { center_home: home, center_palette_index: index, center_find_prompts: [] });
  await page.getByRole("button", { name: /^Search/ }).click();
  await page.getByRole("dialog", { name: "Search" }).getByRole("combobox").fill("widget");
  await page.keyboard.press("ArrowRight");
  await expect(page.locator(".cc-pal .it")).toHaveCount(3);
  await baseline(page, "palette-sessions");
});

test("start screen", async ({ page }) => {
  await openDesktop(page, false, { center_home: home });
  // The headline is drawn by lot when the screen opens; the screenshot needs the same one each run.
  await page.evaluate(() => { Math.random = () => 0; });
  await openStartForm(page);
  await expect(page.locator(".cc-pjc.centered .agc-intro h3")).toBeVisible();
  await baseline(page, "start");
});

test("home machines", async ({ page }) => {
  const stats = { sampled_at: NOW, disks: [{ path: folder, free_bytes: 412e9, total_bytes: 1e12 }, { path: "/mnt/data", free_bytes: 1.8e12, total_bytes: 2e12 }], memory_used_mb: 38_000, memory_total_mb: 124_000, load: 1.4 };
  const online = { ...machine("build-box", "build-box", true, 2), env: { os: "linux", os_version: "Ubuntu 26.04", arch: "x86_64", hostname: "build-box", cpus: 32, memory_mb: 124_000, home: "/srv", tools: [], logins: [], avds: [], gpus: [] }, stats, capabilities: ["docker", "gpu", "android"] };
  const names = ["example/widget", "example/gadget", "example/alpha", "example/bravo", "example/charlie", "tools/echo", "tools/foxtrot", "site/landing", "site/docs"];
  const sync = [{ project: "tools/delta", machine_id: "build-box", status: "syncing", files: 62, pending: 38, error: null }, ...names.map(name => ({ project: name, machine_id: "build-box", status: "in_sync", files: 90, pending: 0, error: null }))];
  const terminal = { id: "t1", project: "", machine_id: "build-box", machine_name: "build-box", name: "dev server", command: "shell", cwd: folder, state: "running", code: null, started_at: NOW - 600_000 };
  await openDesktop(page, false, { center_home: { ...home, machines: [online, home.machines[1]] }, terminals_all: [terminal], project_sync: sync });
  await page.getByRole("tab", { name: "Machines" }).click();
  await expect(page.locator("#m-build-box .mfold")).toBeVisible();
  await baseline(page, "home-machines");
});

test("home PRs and issues sorted", async ({ page }) => {
  const items = gitItems.map(item => ({ ...item, opened_at: NOW - item.number * 3_600_000 }));
  const sortBy = async (label: string) => {
    await page.getByRole("button", { name: /Sort PRs and issues/ }).click();
    await page.getByRole("menuitemradio", { name: label }).click();
  };
  await openDesktop(page, false, { center_home: home, git_items: { items, hosts: [] } });
  await page.getByRole("tab", { name: /PRs and issues/ }).click();
  await expect(page.locator("#sec-git .pl-row.repo")).toHaveCount(4);
  // The sort orders the cards, then each card's items.
  await sortBy("Recent activity");
  await expect(page.locator("#sec-git .pl-row.it .num")).toHaveText(["#412", "#398", "#377", "#88", "#85", "#31", "#14", "#12"]);
  await baseline(page, "home-git-activity");
  await sortBy("Newest");
  await expect(page.locator("#sec-git .pl-row.it .num")).toHaveText(["#12", "#14", "#31", "#85", "#88", "#377", "#398", "#412"]);
});

test("sidebar groups folded and collapsed", async ({ page }) => {
  await openDesktop(page, false, { center_home: home });
  const projectsGroup = page.locator("#side .cc-grp.projects");
  await expect(projectsGroup.getByRole("button", { name: "tools/delta", exact: true })).toBeVisible();
  await projectsGroup.getByRole("button", { name: "example/", exact: true }).click();
  await expect(projectsGroup.getByRole("button", { name: projectName, exact: true })).toHaveCount(0);
  await baseline(page, "sidebar-folded");
  await page.getByRole("button", { name: "Collapse sidebar" }).click();
  await expect(page.getByRole("button", { name: "Expand sidebar" })).toBeVisible();
  await baseline(page, "sidebar-collapsed");
});

const groups = [{ id: "g", name: "Client work", paths: ["srv/example"] }];

test("projects overview grouped", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, project_groups: groups });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await expect(page.locator(".pj-card")).toHaveCount(3);
  await baseline(page, "projects-grouped");
});

test("projects lists", async ({ page }) => {
  const place = (id: string, name: string, dir: string) => ({ machine_id: id, machine_name: name, folder: dir });
  const found = [
    { name: "example/alpha", remote: "https://example.test/alpha", places: [place("build-box", "build-box", "/srv/example/alpha"), place("studio", "studio", "~/work/alpha"), place("laptop", "laptop", "~/work/alpha")], agents: [{ agent: "claude", sessions: 12 }, { agent: "codex", sessions: 3 }], last_activity: NOW - 2 * 86_400_000 },
    { name: "tools/echo", remote: "https://example.test/echo", places: [place("build-box", "build-box", "/srv/tools/echo"), place("studio", "studio", "~/work/echo")], agents: [{ agent: "claude", sessions: 4 }], last_activity: NOW - 7 * 86_400_000 },
    { name: "site/docs", remote: "https://example.test/docs", places: [place("laptop", "laptop", "~/work/docs")], agents: [], last_activity: NOW - 21 * 86_400_000 },
  ];
  await openDesktop(page, false, { center_home: home, found_projects: found, git_items: { items: gitItems, hosts: [] } });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await expect(page.locator("#sec-found .pl-row")).toHaveCount(3);
  await expect(page.locator("#sec-elsewhere .pl-row.repo")).toHaveCount(4);
  await expect(page.locator("#sec-elsewhere .it")).toHaveCount(3);
  await baseline(page, "projects-lists");
});

test("projects overview search", async ({ page }) => {
  await openDesktop(page, false, { center_home: home });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await page.getByRole("searchbox", { name: "Search projects" }).fill("gadget");
  await expect(page.locator(".pj-card")).toHaveCount(1);
  await baseline(page, "projects-search");
  await page.getByRole("searchbox", { name: "Search projects" }).fill("nothing");
  await expect(page.getByText("No projects match.")).toBeVisible();
  await baseline(page, "projects-search-none");
});

test("projects overview empty", async ({ page }) => {
  await openDesktop(page, false, { center_home: { ...home, projects: [], running: [] } });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await expect(page.locator(".cc-view[data-project]")).toBeVisible();
  await baseline(page, "projects-empty");
});

test("project page", async ({ page }) => {
  await openProject(page);
  await baseline(page, "project-page");
});

test("project with nothing on its board", async ({ page }) => {
  const hosts = [{ machine_id: machineId, machine_name: machineId, agents: ["codex"], workspace: null, home: null, capabilities: [] }];
  await openDesktop(page, false, { center_home: { ...home, running: [] }, ...projectPage, project_sessions: [], agent_hosts: hosts });
  // The headline is drawn by lot, as on the start screen.
  await page.evaluate(() => { Math.random = () => 0; });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name } }), projectName);
  await expect(page.locator(".cc-pjc.centered .agc-intro h3")).toBeVisible();
  await baseline(page, "project-blank");
});

test("project bar when its machine runs no coding agent yet", async ({ page }) => {
  const wslBox = { ...machine("wsl-box", "Ubuntu on desk-pc", true, 0), env: { os: "linux", os_version: "Ubuntu 24.04", arch: "x86_64", hostname: "desk-pc", cpus: 8, memory_mb: 16384, home: "/home/user", tools: [{ name: "claude", version: "2.1.0" }], logins: [], avds: [], gpus: [], wsl: [] }, adapters: [{ agent: "claude", failed: "needs Node.js: unpacking failed" }] };
  const hosts = [{ machine_id: machineId, machine_name: machineId, agents: ["codex"], workspace: null, home: null, capabilities: [] }];
  await openProject(page, { center_home: { ...home, projects: [project(projectName, undefined, "wsl-box")] }, list_machines: [wslBox], agent_hosts: hosts });
  await expect(page.getByText("No coding agent runs on Ubuntu on desk-pc yet")).toBeVisible();
  await baseline(page, "project-bar-blocked");
});

test("project sessions", async ({ page }) => {
  await openProject(page);
  await page.locator(".pj-strip button").filter({ hasText: "Sessions" }).click();
  await expect(page.locator(".pj-det .pv-row").filter({ hasText: "Past session 3" })).toBeVisible();
  await baseline(page, "project-sessions");
});

test("project git history", async ({ page }) => {
  await openProject(page);
  await page.locator(".pj-strip button").filter({ hasText: "branch" }).click();
  await expect(page.getByRole("heading", { name: "Keep the query string on redirect" })).toBeVisible();
  await baseline(page, "project-git");
});

test("project machines", async ({ page }) => {
  const box = { machine_id: machineId, machine_name: machineId, folder, status: "in_sync", files: 18, bytes: 1024, pending: 0, conflicts: ["notes (conflict 12345678).txt"], error: null, hint: null, paused: false, progress: null, waiting: [], can_resolve: true, can_start_over: false, git_only: false };
  const detail = { project: projectName, pinned: false, paused: false, machines: [box] };
  await openProject(page, { project_sync: [{ project: projectName, machine_id: machineId, status: "in_sync", files: 18, pending: 0, error: null }], project_sync_detail: detail });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name, tile: "machines" } }), projectName);
  await expect(page.locator(".fs-conflict")).toBeVisible();
  await baseline(page, "project-machines");
});

test("add to machine", async ({ page }) => {
  const machines = [
    { id: machineId, name: machineId, online: true, folder, checking: false, checked_at: 1 },
    { id: "laptop", name: "laptop", online: true, folder: null, checking: false, checked_at: 1, mesh: true },
  ];
  const at = (path: string, folders: string[], repos: string[]) => ({ path, parent: path.slice(0, path.lastIndexOf("/")), exists: true, folders, more: 0, empty: false, origin: null, repos });
  const machine_folders = { "~": at("/home/dev", ["projects"], []), "~/projects": at("/home/dev/projects", ["archive", "atlas", "harbor", "notes", "quill"], ["atlas", "quill"]) };
  await openProject(page, { project_machines: machines, machine_search: "~/projects", machine_folders });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name, tile: "machines" } }), projectName);
  await page.getByRole("article", { name: "laptop", exact: true }).getByRole("button", { name: "Add", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: /Add widget to laptop/ });
  await expect(sheet.getByRole("option", { name: "quill" })).toBeVisible();
  await baseline(page, "add-to-machine");
});

test("terminals", async ({ page }) => {
  const terminal = (id: string, name: string, state: string) => ({ id, project: "", machine_id: machineId, machine_name: machineId, name, command: "shell", cwd: folder, state, code: state === "exited" ? 0 : null, started_at: NOW - 600_000 });
  await openDesktop(page, false, { terminals_all: [terminal("t1", "dev server", "running"), terminal("t2", "tests", "running"), terminal("t3", "build", "exited")] });
  await page.evaluate(() => window.__navigation.open("terminals"));
  await expect(page.locator(".tl-list").getByRole("button", { name: /dev server/ })).toBeVisible();
  await baseline(page, "terminals");
});

const settingsResponses = { settings: { name: machineId, version: "0.9.0", autostart: false, agent_notices: false, developer: false }, list_machines: [], list_credentials: [], list_github_accounts: [], skill_sync_get: true, project_sync_get: true };
test("settings machines", async ({ page }) => {
  const tool = (name: string, version: string) => ({ name, version, path: `/usr/bin/${name}` });
  const env = (os_version: string, tools: string[]) => ({ os: "linux", os_version, arch: "x86_64", cpus: 16, memory_mb: 65536, tools: [tool("claude", "2.1.0"), tool("codex", "0.60.0"), ...tools.map(name => tool(name, "1.0.0"))], logins: [{ tool: "claude", signed_in: true, account: null }, { tool: "codex", signed_in: false, account: null }] });
  const machines = [{ ...machine("build-box", "build-box", true, 2), env: env("Ubuntu 26.04", ["git", "docker", "cargo", "node"]) }, { ...machine("test-box", "test-box", false, 0), version: "0.8.0", env: env("Debian 14", ["git"]) }, { ...machine("phone", "Phone", true, 0), can_host: false }];
  await openDesktop(page, false, { ...settingsResponses, list_machines: machines });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const nav = page.getByRole("dialog", { name: "Settings" }).getByRole("navigation", { name: "Settings pages" });
  await nav.getByRole("button", { name: /^Machines/ }).click();
  await expect(page.locator(".dmach").first()).toBeVisible();
  await baseline(page, "settings-machines");
});

test("settings machine with WSL", async ({ page }) => {
  const wsl = (name: string, agents: string[]) => ({ name, agents, node: null, machine_id: null, kept: false, problem: null, probed: true });
  const env = { os: "windows", os_version: "Windows", arch: "x86_64", hostname: "desk-pc", cpus: 16, memory_mb: 32768, home: "C:\\Users\\user", tools: [], logins: [], avds: [], gpus: [], wsl: [wsl("Ubuntu", ["claude", "codex"]), wsl("Debian", ["pi"])] };
  const machines = [{ ...machine("desk-pc", "desk-pc", true, 0), env }];
  const machine_mcp_servers = [
    { name: "build-tools", settings: { enabled: true, codex: null, claude_oauth: null, connection: { stdio: { command: "build-tools", args: [], env: {}, cwd: null } } } },
    { name: "docs", settings: { enabled: false, codex: null, claude_oauth: null, connection: { remote: {
      transport: "http", url: "https://tools.example.test/mcp", headers: {}, env_http_headers: {}, bearer_token_env_var: null,
    } } } },
  ];
  await openDesktop(page, false, { ...settingsResponses, list_machines: machines, machine_mcp_harnesses: ["codex", "claude"], machine_mcp_servers });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const nav = page.getByRole("dialog", { name: "Settings" }).getByRole("navigation", { name: "Settings pages" });
  await nav.getByRole("button", { name: /^Machines/ }).click();
  await page.locator("[data-machine='desk-pc']").click();
  await expect(page.getByText("Claude Code, Codex")).toBeVisible();
  await expect(page.getByRole("button", { name: /^build-tools/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Harness Codex" })).toBeVisible();
  await baseline(page, "settings-machine-wsl");
  await page.getByRole("button", { name: /^docs/ }).click();
  await expect(page.getByRole("dialog", { name: "Edit docs" })).toBeVisible();
  await baseline(page, "settings-machine-mcp-editor", ".am-sheet");
});

test("settings plugins", async ({ page }) => {
  const git = { kind: "git", url: "https://github.com/owner/idle-timer.git", ref: null, subfolder: null, commit: "4f0915a8b3c2d1e0f9a8b7c6d5e4f30211223344" };
  const plugin = (id: string, name: string, description: string, extra: Record<string, unknown>) => ({ id, name, version: "0.2.0", description, permissions: ["ui.add"], approved: ["ui.add"], source: { kind: "builtin" }, enabled: true, state: "running", usedMs: 0, pendingTimers: 0, ...extra });
  const plugins = [
    plugin("composer-cat", "Composer cat", "A cat sits on the message box.", { permissions: ["ui.add", "sessions.read"], approved: ["ui.add", "sessions.read"] }),
    plugin("idle-timer", "Idle timer", "Shows how long a session has been quiet.", { version: "1.0.3", source: git, enabled: false, state: "disabled" }),
  ];
  await openDesktop(page, false, { ...settingsResponses, plugins_status: plugins });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await dialog.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Plugins", exact: true }).click();
  await expect(dialog.getByRole("listitem")).toHaveCount(2);
  await baseline(page, "settings-plugins");
});

test("settings instructions", async ({ page }) => {
  await openDesktop(page, false, { ...settingsResponses, instructions_get: { version: 1, text: "# How I work\n\n- Write plainly.", note: "", saved_at: 0 } });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await dialog.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Instructions", exact: true }).click();
  await dialog.getByRole("button", { name: "Add", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Added", exact: true })).toBeDisabled();
  await baseline(page, "settings-instructions");
});

test("settings assistant", async ({ page }) => {
  await openDesktop(page, false, { ...settingsResponses, assistant_settings_get: { agent: null, model: null, effort: null, fast: null }, mac_control: { id: "build-box" }, agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["claude", "codex"], workspace: null, home: "/srv", capabilities: [], assistant: true }], agent_models: { pick: "listed", current: null, models: [{ id: "model-a", name: "Example Model A", description: null, provider: null }, { id: "model-b", name: "Example Model B", description: null, provider: null }] } });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Settings" });
  await dialog.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: /^Assistant/ }).click();
  await dialog.locator("#assistantSetting").getByRole("button").click();
  await expect(page.getByRole("dialog", { name: "Harness, model and thinking" }).getByText("Example Model B")).toBeVisible();
  await baseline(page, "settings-assistant");
});

test("settings claude code", async ({ page }) => {
  const claude = {
    agent: "claude", installed: true, runnable: true, version: "2.1.0", latest: null, program: "claude", home_var: "CLAUDE_CONFIG_DIR", home_default: "~/.claude",
    sign_in: { state: "signed_in", account: null, plan: null, last4: null }, install: null,
    settings: { enabled: true, program: "", home: "", args: [], env: [] },
  };
  await openDesktop(page, false, {
    ...settingsResponses,
    list_machines: [machine("build-box", "build-box", true, 0)],
    agent_harnesses: { machine_id: "build-box", machine_name: "build-box", checked_at: null, harnesses: [claude] },
    harness_settings_all: [], harness_compact: null, harness_model_compacts: [], harness_default_model: null, harness_driver: "direct", agent_models: null,
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const nav = settings.getByRole("navigation", { name: "Settings pages" });
  await nav.getByRole("button", { name: "Agent accounts", exact: true }).click();
  await nav.getByRole("button", { name: /^Claude/ }).click();
  await expect(settings.getByRole("button", { name: "Direct", exact: true })).toHaveAttribute("aria-pressed", "true");
  await baseline(page, "settings-claude-code");
});

test("settings flsh stream idle timeout", async ({ page }) => {
  await openDesktop(page, false, { ...settingsResponses, flsh_settings_get: EMPTY });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const nav = settings.getByRole("navigation", { name: "Settings pages" });
  await nav.getByRole("button", { name: "Agent accounts", exact: true }).click();
  await nav.getByRole("button", { name: "flsh", exact: true }).click();
  const timeout = settings.getByRole("textbox", { name: "Stream idle timeout (seconds)" });
  await expect(timeout).toHaveAttribute("placeholder", "300");
  await timeout.scrollIntoViewIfNeeded();
  await baseline(page, "settings-flsh");
});

test("settings codex", async ({ page }) => {
  const codex = {
    agent: "codex", installed: true, runnable: true, version: "0.169.0", latest: null, program: "codex-acp", home_var: "CODEX_HOME", home_default: "~/.codex",
    sign_in: { state: "signed_in", account: null, plan: null, last4: null }, install: null,
    settings: { enabled: true, program: "", home: "", args: [], env: [] },
  };
  await openDesktop(page, false, {
    ...settingsResponses,
    list_machines: [machine("build-box", "build-box", true, 0)],
    agent_harnesses: { machine_id: "build-box", machine_name: "build-box", checked_at: null, harnesses: [codex] },
    harness_settings_all: [], harness_compact: null, harness_model_compacts: [], harness_default_model: null, harness_driver: "direct", agent_models: null,
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  const nav = settings.getByRole("navigation", { name: "Settings pages" });
  await nav.getByRole("button", { name: "Agent accounts", exact: true }).click();
  await nav.getByRole("button", { name: /^Codex/ }).click();
  await expect(settings.getByRole("button", { name: "Direct", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(settings.getByText("Direct runs the codex CLI without the npm adapter. New sessions use it.")).toBeVisible();
  await baseline(page, "settings-codex");
});

const agentAccountResponses = {
  ...settingsResponses,
  agent_accounts: { claude: null, codex_key: null, machines: [
    { id: "build-box", name: "build-box", online: true, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null },
    { id: "test-box", name: "test-box", online: true, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null },
    { id: "offline-box", name: "offline-box", online: false, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null },
  ], named: [
    { agent: "claude", name: "Work", env_var: "CLAUDE_CODE_OAUTH_TOKEN", token: { last4: "4a2f", created_at: 0, unreadable: false } },
  ] },
  list_machines: [
    { ...machine("build-box", "build-box", true, 0), env: { logins: [{ tool: "claude", signed_in: true, account: "you@example.com" }, { tool: "agy", signed_in: true, account: null }] } },
    { ...machine("test-box", "test-box", true, 0), env: { logins: [{ tool: "claude", signed_in: false, account: null }, { tool: "agy", signed_in: false, account: null }] } },
  ],
};
for (const section of [null, "Agent accounts", "Appearance"] as const) {
  const name = `settings-${section === null ? "first" : section.toLowerCase().replace(/\s+/g, "-")}`;
  test(name, async ({ page }) => {
    await openDesktop(page, false, section === "Agent accounts" ? agentAccountResponses : settingsResponses);
    await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
    const nav = page.getByRole("dialog", { name: "Settings" }).getByRole("navigation", { name: "Settings pages" });
    await expect(nav).toBeVisible();
    if (section !== null) {
      await nav.getByRole("button", { name: section, exact: true }).click();
      await expect(nav.getByRole("button", { name: section, exact: true })).toHaveAttribute("aria-current", "true");
    }

    await baseline(page, name);
  });
}

// Agent session

test("narrow window folds the right pane to its tabs, which open it over the conversation", async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 832 });
  await openAgent(page);
  const strip = page.locator("#agpstrip-right"), pane = page.locator("#agpane-right");
  await expect(strip.getByRole("tab", { name: "Changes" })).toBeVisible();
  await expect(strip.getByRole("tab", { name: "Files" })).toBeVisible();
  await expect(pane).toBeHidden();
  await baseline(page, "narrow-session");

  await strip.getByRole("tab", { name: "Changes" }).click();
  await expect(page.locator('.dv-file[data-path="sample.ts"]')).toBeVisible();
  await expect(page.locator("#agshell")).toHaveClass(/zoomed/);
  await baseline(page, "narrow-session-changes");

  await pane.getByTitle("Hide the pane").click();
  await expect(pane).toBeHidden();
  await expect(strip).toBeVisible();

  // Wide again, the pane is back beside the conversation as it was.
  await page.setViewportSize({ width: 1600, height: 832 });
  await expect(pane).toBeVisible();
  await expect(page.locator("#agshell")).not.toHaveClass(/zoomed/);
});

test("agent session transcript", async ({ page }) => {
  const summary = "The redirect now preserves the query string.";
  const entries = [{ kind: "other", text: `[conversation compacted]\n${summary}` }, ...transcript];
  await openAgent(page, { agent_view: { ...agentView, usage: sessionUsage, entries } });
  const marker = page.getByText("Context summarized", { exact: true });
  await expect(marker).toBeVisible();
  await expect(page.getByText(summary, { exact: true })).toBeHidden();
  await baseline(page, "agent-transcript");
  await marker.click();
  await expect(page.getByText(summary, { exact: true })).toBeVisible();
});

const reportText = "Done: the redirect keeps the query string. Sign-in lands where it started.\n\n## Needs you\n1. **Old links:** keep `?next=` working for a release. The mobile app still sends it.\n\n## Changed\n- **Query string kept** (`src/login.ts:42`): it was dropped when `next` was read.\n- **Test for an empty `next`** (`src/login.test.ts:18`).\n\n## Checked\n- **`npm test`:** passes (48 tests).\n- **Old mobile build:** fails to sign in without `?next=`.\n- **Not checked:** Safari's private mode, and sign-in through the old single sign-on page.";
const reportHtml = "<p>Done: the redirect keeps the query string. Sign-in lands where it started.</p>\n<h2>Needs you</h2>\n<ol>\n<li><strong>Old links:</strong> keep <code>?next=</code> working for a release. The mobile app still sends it.</li>\n</ol>\n<h2>Changed</h2>\n<ul>\n<li><strong>Query string kept</strong> (<code>src/login.ts:42</code>): it was dropped when <code>next</code> was read.</li>\n<li><strong>Test for an empty <code>next</code></strong> (<code>src/login.test.ts:18</code>).</li>\n</ul>\n<h2>Checked</h2>\n<ul>\n<li><strong><code>npm test</code>:</strong> passes (48 tests).</li>\n<li><strong>Old mobile build:</strong> fails to sign in without <code>?next=</code>.</li>\n<li><strong>Not checked:</strong> Safari's private mode, and sign-in through the old single sign-on page.</li>\n</ul>";

test("agent report", async ({ page }) => {
  const entries = [...transcript, { kind: "user", text: "Report on the fix." }, { kind: "agent", text: reportText, html: reportHtml }];
  await openAgent(page, { agent_view: { ...agentView, session: { ...session, status: "idle", question: null }, entries } });
  const report = page.locator(".dcoderen.report");
  await expect(report.locator(".report-outcome")).toHaveText("Done");
  await expect(report.locator(".report-headline")).toHaveText("The redirect keeps the query string.");
  await expect(report.locator('li[data-check="fail"]')).toContainText("Old mobile build");
  await baseline(page, "agent-report");

  // Summarize asks the session's agent for a shorter version, naming the report.
  await report.getByRole("button", { name: "Summarize" }).click();
  await expect(page.getByText(/^Summarize your report that starts "Done: the redirect keeps the query string\./)).toBeVisible();
});

test("agent report after a line of the agent's own", async ({ page }) => {
  const lead = "One claim broken. Report:";
  const entries = [...transcript, { kind: "user", text: "Report on the fix." }, { kind: "agent", text: `${lead}\n\n${reportText}`, html: `<p>${lead}</p>\n${reportHtml}` }];
  await openAgent(page, { agent_view: { ...agentView, session: { ...session, status: "idle", question: null }, entries } });
  const report = page.locator(".dcoderen.report");
  await expect(report.locator(".report-outcome")).toHaveText("Done");
  await expect(report.locator(".report-lead")).toHaveText(lead);
  await expect(report.locator(".report-headline")).toHaveText("The redirect keeps the query string.");
  const order = await report.locator(".reply-body > :is(.report-lead, .report-headline)").evaluateAll(nodes => nodes.map(node => node.className));
  expect(order).toEqual(["report-lead", "report-headline"]);
});

test("agent report reviewed", async ({ page }) => {
  const entries = [...transcript, { kind: "user", text: "Report on the fix." }, { kind: "agent", text: reportText, html: reportHtml }];
  await openAgent(page, { agent_view: { ...agentView, session: { ...session, status: "idle", question: null }, entries } });
  const report = page.locator(".dcoderen.report");
  await expect(report).toBeVisible();

  // Selecting part of a report's item offers Ask and Change.
  await page.evaluate(() => {
    const item = [...document.querySelectorAll(".report-part li")].find(li => li.textContent?.includes("it was dropped"));
    const text = [...(item?.childNodes ?? [])].find(n => n.nodeType === Node.TEXT_NODE && n.textContent?.includes("it was dropped"));
    if (!text?.textContent) throw new Error("no item");
    const at = text.textContent.indexOf("it was dropped"), range = document.createRange();
    range.setStart(text, at);
    range.setEnd(text, at + "it was dropped when".length);
    getSelection()?.removeAllRanges();
    getSelection()?.addRange(range);
  });
  const pop = page.getByRole("toolbar", { name: "Comment on the selection" });
  await expect(pop).toBeVisible();
  await pop.getByRole("button", { name: /^Change/ }).click();

  // The comment is written in place of the composer, on the item it quotes.
  const field = page.getByRole("group", { name: "Change on the selection" });
  await expect(field).toContainText("on Changed 1");
  await expect(field.locator(".rv-quote")).toHaveText("it was dropped when");
  await expect(page.locator("#agfield")).toBeHidden();
  await field.getByRole("textbox").fill("Also keep the fragment.");
  await baseline(page, "agent-report-comment");
  await field.getByRole("textbox").press("Enter");

  // Saved, it waits under its item and over the composer.
  await expect(page.locator("#agfield")).toBeVisible();
  await expect(report.locator('.report-part[data-kind="changed"] li:first-child > .rv-note')).toContainText("Also keep the fragment.");
  await expect(page.locator(".rv-bar")).toContainText("1 change · goes with your next message");
  await page.locator(".rv-bar").getByRole("button", { name: "Show" }).click();
  await expect(page.getByRole("list", { name: "Review drafts" })).toContainText("Changed 1");
  await baseline(page, "agent-report-drafts");

  // It goes with the next message, and shows as its comments.
  await page.locator("#agmsg").fill("Then open a PR.");
  await page.locator("#agmsg").press("Enter");
  const sent = page.locator(".rv-sent");
  await expect(sent).toContainText("1 comment on “Done: the redirect keeps the query string.");
  await expect(sent.locator(".it")).toContainText("Change · Changed 1");
  await expect(sent.locator(".also")).toHaveText("Then open a PR.");
  await expect(page.locator(".rv-bar")).toHaveCount(0);
  await baseline(page, "agent-report-review-sent");
});

test("agent report draft on a whole item", async ({ page }) => {
  const entries = [...transcript, { kind: "user", text: "Report on the fix." }, { kind: "agent", text: reportText, html: reportHtml }];
  await openAgent(page, { agent_view: { ...agentView, session: { ...session, status: "idle", question: null }, entries } });
  const report = page.locator(".dcoderen.report");
  const marked = () => report.locator('.rv-mark[data-ask="question"]').allTextContents().then(parts => parts.join(""));

  // A whole item quoted stays marked once its card sits in the item.
  await page.evaluate(() => {
    const item = [...document.querySelectorAll(".report-part li")].find(li => li.textContent?.includes("it was dropped"));
    if (!item) throw new Error("no item");
    const range = document.createRange();
    range.selectNodeContents(item);
    getSelection()?.removeAllRanges();
    getSelection()?.addRange(range);
  });
  await page.getByRole("toolbar", { name: "Comment on the selection" }).getByRole("button", { name: /^Ask/ }).click();
  const field = page.getByRole("group", { name: "Question on the selection" });
  await field.getByRole("textbox").fill("Why here?");
  await field.getByRole("textbox").press("Enter");
  const note = report.locator(".rv-note");
  await expect(note).toContainText("Why here?");
  await expect.poll(marked).toBe("Query string kept (src/login.ts:42): it was dropped when next was read.");

  // × on the card removes the draft and its mark.
  await note.hover();
  await note.getByRole("button", { name: "Remove this comment" }).click();
  await expect(note).toHaveCount(0);
  await expect.poll(marked).toBe("");
  await expect(page.locator(".rv-bar")).toHaveCount(0);
});

const challengeHosts = [{ machine_id: machineId, machine_name: machineId, agents: ["codex", "claude"], workspace: folder, home: "/srv", capabilities: [] }];
const challengeModels = { pick: "listed", current: "alpha", models: [{ id: "alpha", name: "Alpha", description: null }, { id: "beta", name: "Beta", description: null }] };

test("agent report challenged", async ({ page }) => {
  const entries = [...transcript, { kind: "user", text: "Report on the fix." }, { kind: "agent", text: reportText, html: reportHtml }];
  const challenger = { ...agentRow("challenger-one", "Challenge the report", "working", 0), agent: "claude" };
  const work = `${folder}/.worktrees/beep-fix-login`;
  await openAgent(page, {
    agent_view: { ...agentView, session: { ...session, status: "idle", question: null }, entries }, agent_begin: { session: challenger, why: "" },
    agent_hosts: challengeHosts, agent_models: challengeModels,
    agent_worktrees: [{ path: folder, branch: "main", head: "abc1234", own: true, created: false }, { path: work, branch: "beep/fix-login", head: "def5678", own: false, created: true }],
  });
  const report = page.locator(".dcoderen.report");

  // Challenge asks for the harness first, then the model.
  await report.getByRole("button", { name: "Challenge" }).click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  await expect(picker.locator(".agc-stephead")).toHaveAttribute("data-agstep", "harness");
  await baseline(page, "agent-report-challenge-pick");
  await picker.locator('[data-agh="claude"]').click();
  await expect(picker.locator(".agc-stephead")).toHaveAttribute("data-agstep", "model");
  await picker.locator('[data-agmo="beta"]').click();
  await picker.locator('[data-age=""]').click();
  await expect(picker).toHaveCount(0);

  // It starts on that pick, in a copy of the worktree the session made, with the report.
  await expect(report.locator(".report-busy")).toContainText("Challenging…");
  const begun = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin").map(call => call.args));
  expect(begun).toEqual([expect.objectContaining({ agent: "claude", model: "beta", cwd: work, checkout: "copy_worktree" })]);
  expect(String(begun[0].prompt)).toContain("Beep's `challenge` skill");
  await baseline(page, "agent-report-challenging");

  // Finished, its findings wait as drafts on the claims they dispute, and its session is closed.
  // As a real challenger wrote it: a line of its own before the report, and blank lines in a finding.
  const found = "One claim broken. Report:\n\nPartly done: one claim broke.\n\n## Needs you\n\n1. **[confirmed] Changed 1**\n   > it was dropped when `next` was read.\n\n   It is still dropped after the old sign-on page.\n   Evidence: `npm test -- sso` failed 1 of 1.";
  await page.evaluate(([row, text]) => {
    window.__navigation.respond("agent_sessions", [{ ...row, status: "idle" }]);
    window.__navigation.respond("agent_views", { [row.id]: { session: { ...row, status: "idle" }, plan: [], files: [], entries: [{ kind: "agent", text }], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 1, usage: null } });
    window.__navigation.emit("sessions-changed", null);
  }, [challenger, found] as const);
  await expect(report.getByRole("button", { name: "1 finding" })).toBeVisible();
  await expect(report.locator(".report-note")).toHaveCount(0);
  const note = report.locator('.report-part[data-kind="changed"] li:first-child > .rv-note[data-ask="finding"]');
  await expect(note).toContainText("Confirmed: It is still dropped after the old sign-on page.");
  await expect(report.locator('.rv-mark[data-ask="finding"]')).toHaveText(["it was dropped when ", "next", " was read."]);
  await expect(page.locator(".rv-bar")).toContainText("1 finding · goes with your next message");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_steer").map(call => call.args))).toContainEqual(expect.objectContaining({ session: "challenger-one", steer: "close" }));
  await baseline(page, "agent-report-challenged");
});

test("agent report challenge picker shows whole model names", async ({ page }) => {
  const name = "openrouter/DeepSeek: DeepSeek V4 Flash";
  const models = { pick: "listed", current: "a", models: ["free", "nitro", "floor", "exacto"].map(tier => ({ id: `deepseek/deepseek-v4-flash:${tier}`, name, description: null, provider: "openrouter" })) };
  const entries = [...transcript, { kind: "user", text: "Report on the fix." }, { kind: "agent", text: reportText, html: reportHtml }];
  await openAgent(page, { agent_view: { ...agentView, session: { ...session, status: "idle", question: null }, entries }, agent_hosts: challengeHosts, agent_models: models });
  await page.locator(".dcoderen.report").getByRole("button", { name: "Challenge" }).click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  await picker.locator('[data-agh="codex"]').click();

  // Models that share a long name show it whole, each with the id that tells it apart.
  const rows = picker.locator("[data-agmo^='deepseek/']");
  await expect(rows).toHaveCount(4);
  for (const tier of ["free", "nitro", "floor", "exacto"]) await expect(picker.locator(`[data-agmo="deepseek/deepseek-v4-flash:${tier}"]`)).toContainText(`${name}openrouter · deepseek/deepseek-v4-flash:${tier}`);
  const cut = await rows.evaluateAll(list => list.filter(row => row.scrollWidth > row.clientWidth || [...row.children].some(child => child.scrollWidth > child.clientWidth)).length);
  expect(cut).toBe(0);
  await baseline(page, "agent-report-challenge-models");
});

test("agent report challenger stopped", async ({ page }) => {
  const entries = [...transcript, { kind: "user", text: "Report on the fix." }, { kind: "agent", text: reportText, html: reportHtml }];
  const challenger = agentRow("challenger-one", "Challenge the report", "working", 0);
  await openAgent(page, { agent_view: { ...agentView, session: { ...session, status: "idle", question: null }, entries }, agent_begin: { session: challenger, why: "" }, agent_hosts: challengeHosts, agent_models: challengeModels });
  const report = page.locator(".dcoderen.report");
  await report.getByRole("button", { name: "Challenge" }).click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  await picker.locator('[data-agh="codex"]').click();
  await picker.locator('[data-agmo="alpha"]').click();
  await picker.locator('[data-age=""]').click();

  // Stop, next to the spinner, closes the challenger and deletes its session; Challenge comes back.
  await report.locator(".report-busy").getByRole("button", { name: "Stop" }).click();
  await expect(report.getByRole("button", { name: "Challenge" })).toBeVisible();
  const calls = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_steer" || call.command === "agent_delete").map(call => [call.command, call.args]));
  expect(calls).toEqual([
    ["agent_steer", expect.objectContaining({ session: "challenger-one", steer: "close" })],
    ["agent_delete", { machineId, agent: "codex", session: "challenger-one" }],
  ]);
});

/// Opens the report and starts a challenger on Codex, as `challenger`.
async function startChallenge(page: Page, challenger: ReturnType<typeof agentRow>) {
  const entries = [...transcript, { kind: "user", text: "Report on the fix." }, { kind: "agent", text: reportText, html: reportHtml }];
  await openAgent(page, { agent_view: { ...agentView, session: { ...session, status: "idle", question: null }, entries }, agent_begin: { session: challenger, why: "" }, agent_hosts: challengeHosts, agent_models: challengeModels });
  const report = page.locator(".dcoderen.report");
  await report.getByRole("button", { name: "Challenge" }).click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  await picker.locator('[data-agh="codex"]').click();
  await picker.locator('[data-agmo="alpha"]').click();
  await picker.locator('[data-age=""]').click();
  await expect(report.locator(".report-busy")).toContainText("Challenging…");
  return report;
}

/// The challenger's session lists as `row` (none when null), its page as `page`.
async function challengerIs(page: Page, row: ReturnType<typeof agentRow> | null, view: { entries: unknown[]; error: string | null } | null = null) {
  await page.evaluate(([row, view]) => {
    window.__navigation.respond("agent_sessions", row ? [row] : []);
    if (row && view) window.__navigation.respond("agent_views", { [row.id]: { session: row, plan: [], files: [], earlier: 0, last_message: "", queued: 0, queued_messages: [], turns: 1, usage: null, ...view } });
    window.__navigation.emit("sessions-changed", null);
  }, [row, view] as const);
}

const closedChallenger = (page: Page) => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_steer").map(call => call.args));

test("agent report challenge that ends without answering", async ({ page }) => {
  const challenger = agentRow("challenger-one", "Challenge the report", "working", 0);
  const report = await startChallenge(page, challenger);

  // A turn that ended with no reply fails the challenge, says so, and closes the challenger.
  await challengerIs(page, { ...challenger, status: "idle" }, { entries: [{ kind: "user", text: "Challenge" }], error: null });
  const failed = report.locator('.report-busy[data-challenge="failed"]');
  await expect(failed.locator(".report-err")).toHaveText("Challenge failed");
  await expect(report.locator(".report-note")).toHaveText("It stopped without answering.");
  await expect(report.locator(".dspin")).toHaveCount(0);
  await expect(report.locator(".rv-note")).toHaveCount(0);
  await expect.poll(() => closedChallenger(page)).toContainEqual(expect.objectContaining({ session: "challenger-one", steer: "close" }));
  await baseline(page, "agent-report-challenge-failed");

  // Again picks a model for a new challenger.
  await failed.getByRole("button", { name: "Again" }).click();
  await expect(page.getByRole("dialog", { name: "Harness, model and thinking" })).toBeVisible();
});

test("agent report challenge that does not start", async ({ page }) => {
  const challenger = agentRow("start:one", "Challenge the report", "starting", 0);
  const report = await startChallenge(page, challenger);

  // A start its machine refused lists as idle with the reason on its page.
  await challengerIs(page, { ...challenger, status: "idle" }, { entries: [{ kind: "user", text: "Challenge" }], error: "Codex is not signed in on build-box." });
  await expect(report.locator(".report-note")).toHaveText("Codex is not signed in on build-box.");
});

test("agent report challenger that asks, then goes", async ({ page }) => {
  const challenger = agentRow("challenger-one", "Challenge the report", "working", 0);
  const report = await startChallenge(page, challenger);
  const busy = report.locator(".report-busy");

  // A question for the user shows in place of the spinner, with Open and Stop.
  await challengerIs(page, { ...challenger, status: "asking" });
  await expect(busy).toHaveAttribute("data-challenge", "asking");
  await expect(busy).toContainText("Challenger asks you");
  await expect(busy.locator(".dspin")).toHaveCount(0);
  await expect(busy.getByRole("button", { name: "Stop" })).toBeVisible();
  await baseline(page, "agent-report-challenger-asks");

  // Answered, it works again.
  await challengerIs(page, { ...challenger, status: "working" });
  await expect(busy).toHaveAttribute("data-challenge", "running");
  await expect(busy).toContainText("Challenging…");

  // A Stop the machine refuses says so, and the challenger stays.
  await page.evaluate(() => window.__navigation.rejectOnce("agent_steer", "build-box is offline."));
  await busy.getByRole("button", { name: "Stop" }).click();
  await expect(report.locator(".report-note")).toHaveText(/^Not stopped: .*build-box is offline\./);
  await expect(busy).toContainText("Challenging…");

  // A session deleted elsewhere fails the challenge.
  await challengerIs(page, null);
  await expect(report.locator(".report-note")).toHaveText("Its session is gone.");
});

test("a deleted session leaves the project's past sessions at once", async ({ page }) => {
  await openProject(page, { agent_deletable: true, agent_delete: null });
  await page.getByRole("button", { name: "Sessions 6 past" }).click();
  const row = (n: number) => page.locator(".pv-row").filter({ hasText: `Past session ${n}` });
  await expect(row(2)).toBeVisible();

  // Once deleted, the machine no longer lists it, and neither does the page.
  await row(2).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await page.evaluate(rows => window.__navigation.respond("project_sessions", rows), pastSessions.filter(row => row.id !== "past-1"));
  await page.getByRole("dialog").getByRole("button", { name: "Delete" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(row(2)).toHaveCount(0);
  await expect(row(1)).toBeVisible();
});

test("deleting a session its machine no longer has removes it without an error", async ({ page }) => {
  await openProject(page, { agent_deletable: true, agent_delete: null });
  await page.getByRole("button", { name: "Sessions 6 past" }).click();
  const row = (n: number) => page.locator(".pv-row").filter({ hasText: `Past session ${n}` });
  await row(2).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Delete" }).click();

  // Deleted elsewhere: the machine says it has no such session, and lists none.
  await page.evaluate(rows => {
    window.__navigation.respond("project_sessions", rows);
    window.__navigation.rejectOnce("agent_delete", "build-box: there is no codex session past-1 on this machine");
  }, pastSessions.filter(row => row.id !== "past-1"));
  await page.getByRole("dialog").getByRole("button", { name: "Delete" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(row(2)).toHaveCount(0);
});

test("deleting a session its machine still lists says why it failed", async ({ page }) => {
  await openProject(page, { agent_deletable: true, agent_delete: null, agent_sessions: pastSessions });
  await page.getByRole("button", { name: "Sessions 6 past" }).click();
  const row = (n: number) => page.locator(".pv-row").filter({ hasText: `Past session ${n}` });
  await row(2).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Delete" }).click();
  await page.evaluate(() => window.__navigation.rejectOnce("agent_delete", "build-box: session past-1 still runs here; close it first"));
  await page.getByRole("dialog").getByRole("button", { name: "Delete" }).click();
  await expect(page.getByRole("dialog")).toContainText("still runs here");
  await expect(row(2)).toBeVisible();
});

test("agent report summarized in place", async ({ page }) => {
  const summary = "Done: the redirect keeps the query string.\n\n- Keep `?next=` for one release: the mobile app sends it.";
  const summaryHtml = "<p>Done: the redirect keeps the query string.</p>\n<ul>\n<li>Keep <code>?next=</code> for one release: the mobile app sends it.</li>\n</ul>";
  const asked = { kind: "user", text: summaryPrompt(reportText) };
  const entries = [...transcript, { kind: "user", text: "Report on the fix." }, { kind: "agent", text: reportText, html: reportHtml }, asked, { kind: "agent", text: summary, html: summaryHtml }];
  await openAgent(page, { agent_view: { ...agentView, session: { ...session, status: "idle", question: null }, entries } });
  const report = page.locator(".dcoderen.report");
  // The summary takes the report's place; the message that asked for it is not drawn.
  await expect(report).toHaveCount(1);
  await expect(report.locator(".report-label")).toHaveText("Summary");
  await expect(report.getByText("Keep ?next= for one release", { exact: false })).toBeVisible();
  await expect(page.getByText(/^Summarize your report/)).toHaveCount(0);
  await baseline(page, "agent-report-summary");

  await report.getByRole("button", { name: "Original" }).click();
  await expect(report.locator(".report-label")).toHaveText("Report");
  await expect(report.locator('li[data-check="fail"]')).toContainText("Old mobile build");
  await report.getByRole("button", { name: "Summary" }).click();
  await expect(report.locator(".report-label")).toHaveText("Summary");
});

test("agent report being summarized", async ({ page }) => {
  const entries = [...transcript, { kind: "user", text: "Report on the fix." }, { kind: "agent", text: reportText, html: reportHtml }, { kind: "user", text: summaryPrompt(reportText) }];
  await openAgent(page, { agent_view: { ...agentView, session: { ...session, status: "working", question: null }, entries } });
  const report = page.locator(".dcoderen.report");
  await expect(report.locator(".report-busy")).toHaveText("Summarizing…");
  await expect(report.locator(".report-label")).toHaveText("Report");
  await expect(page.getByText(/^Summarize your report/)).toHaveCount(0);
});

test("agent composer with a draft", async ({ page }) => {
  await openAgent(page);
  await page.locator("#agmsg").fill("Also add a test for the query string");
  await baseline(page, "agent-composer");
});

test("agent files", async ({ page }) => {
  await openAgent(page);
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await expect(page.locator('[data-fpath="README.md"]')).toBeVisible();
  await baseline(page, "agent-files");
});

test("agent file being edited", async ({ page }) => {
  await openAgent(page);
  await page.evaluate(() => window.__navigation.respond("agent_tree_text", { size: 10000, text: Array.from({ length: 500 }, (_, i) => `const value${i} = ${i};`).join("\n"), cut: false, binary: false, editable: true }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="README.md"]').click();
  // Markdown opens in its preview; Source shows the text to edit.
  await page.locator("[data-preview]").click();
  await page.locator("[data-tedit]").click();
  await expect(page.locator(".agcm .cm-content")).toBeFocused();
  await page.keyboard.type("// note");

  // The editor fits its box and scrolls itself, drawing only the lines in view.
  const box = await page.locator(".agcm").evaluate(element => element.getBoundingClientRect().height);
  const editor = await page.locator(".agcm .cm-editor").evaluate(element => element.getBoundingClientRect().height);
  expect(Math.abs(box - editor)).toBeLessThan(1);
  expect(await page.locator(".agcm .cm-scroller").evaluate(element => element.scrollHeight > element.clientHeight)).toBe(true);
  await baseline(page, "agent-file-edit");
});

test("agent changes", async ({ page }) => {
  await openAgent(page);
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  await expect(page.locator('.dv-file[data-path="sample.ts"]')).toBeVisible();
  await baseline(page, "agent-changes");
});

test("the changed files list shows only with the pane full size, and folds", async ({ page }) => {
  await page.setViewportSize({ width: 2400, height: 900 });
  await openAgent(page);
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const tree = page.locator(".dv-tree"), shell = page.locator("#agshell");
  await expect(page.locator('.dv-file[data-path="sample.ts"]')).toBeVisible();
  await expect(tree).toBeHidden();

  await page.getByRole("button", { name: "Show files full size (Shift+Esc)" }).click();
  await expect(shell).toHaveClass(/zoomed/);
  await expect(tree).toBeVisible();

  await page.getByRole("button", { name: "Hide files" }).click();
  await expect(tree).toBeHidden();
  await page.getByRole("button", { name: "Show files", exact: true }).click();
  await expect(tree).toBeVisible();
});

test("another screen keeps its title after a session was open", async ({ page }) => {
  await openAgent(page, { center_home: home });
  await expect(page.locator("#title")).toBeHidden();
  await page.evaluate(() => window.__navigation.open("home"));
  await expect(page.locator("#title")).toBeVisible();
  await expect(page.locator("#title")).toHaveText("Home");
});

test("agent question", async ({ page }) => {
  const question = { id: 1, text: "Run cargo test?", detail: "cargo test --workspace", options: [{ id: "allow", name: "Allow", allow: true }, { id: "always", name: "Always allow", allow: true }, { id: "deny", name: "Deny", allow: false }] };
  await openAgent(page, { agent_view: { ...agentView, usage: sessionUsage, session: { ...session, status: "asking", question } } });
  await expect(page.locator(".dcoder .q")).toBeVisible();
  await baseline(page, "agent-question");
});

test("a choice question reads left to right, cancels, and holds its call as waiting", async ({ page }) => {
  const long = "Move every inline test module to its own tests file next to its module, with no tests directory at all";
  const question = { id: 2, text: "Where should the tests go?", detail: null, options: [{ id: "a", name: long, allow: false }, { id: "b", name: "Leave them", allow: false }] };
  const entries = [...transcript, { kind: "tool", text: "AskUserQuestion: Where should the tests go? [in_progress]" }];
  await openAgent(page, { agent_view: { ...agentView, entries, session: { ...session, status: "asking", question } }, agent_steer: [] });

  const answers = page.locator(".dcoder .q .dups .dbtn");
  await expect(answers).toHaveCount(3);
  await expect(page.locator(".dcoder .q .dbtn.warn")).toHaveCount(0);
  await expect(answers.first()).toHaveCSS("text-align", "start");
  const held = page.locator(".dcoderen.tool").filter({ hasText: "AskUserQuestion" });
  await expect(held.locator(".mk [title='Waiting for you']")).toBeVisible();
  await expect(held.locator(".rm")).toBeEmpty();

  await page.getByRole("button", { name: "Cancel ⌘." }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer").map(c => c.args))).toContainEqual(expect.objectContaining({ session: "notice-one", steer: "cancel" }));
});

test("agent plan with the subagents of a step", async ({ page }) => {
  const sub = (title: string, status: string, sent_at: number) => ({ kind: "tool", sent_at, text: `${title} [${status}]\n[steps] 4\n[agent] general-purpose\n[session] subagent:codex:${sent_at}\n[model] claude-sonnet-5-5\nDone` });
  const plan = [
    { content: "Add the flag to sessions", status: "completed", started_at: 100, ended_at: 200 },
    { content: "Answer the projects tool", status: "in_progress", started_at: 200, ended_at: null },
    { content: "Write the changelog", status: "pending", started_at: null, ended_at: null },
  ];
  const entries = [...transcript, sub("Build the flag", "completed", 110), sub("Review the flag", "completed", 150), sub("Build the tool", "in_progress", 210)];
  await openAgent(page, { agent_view: { ...agentView, usage: sessionUsage, session: { ...session, status: "working" }, plan, entries } });
  const line = page.locator(".ag-plan");
  await expect(line).toHaveText("Plan 1/3Answer the projects tool");
  await baseline(page, "agent-plan-folded");

  await line.click();
  await expect(page.locator(".ag-steps button.it")).toHaveText([/2 agents/, /1 agent/]);
  await page.locator(".ag-steps button.it").first().click();
  await expect(page.locator(".ag-steps .subs .dsubrow")).toHaveText([/Build the flag/, /Review the flag/]);
  await page.mouse.move(0, 0);
  await baseline(page, "agent-plan-subagents");
});

test("agent with commands running and messages queued", async ({ page }) => {
  const steers = ["Also log each retry", "Then run clippy"];
  const tasks = ["cargo test -p sync", "npm run dev"].map((name, i) => ({ id: `task-${i}`, name, description: name, state: "running", can_stop: true, output_file: null, summary: null, entry: null }));
  await openAgent(page, { agent_view: { ...agentView, usage: sessionUsage, queued: steers.length, queued_messages: steers }, agent_tasks: tasks, agent_steer: [] });
  await expect(page.locator(".ag-tasks")).toHaveText("2 commands running");
  await baseline(page, "agent-work");
});

test("agent compacting after its reply, with a command running", async ({ page }) => {
  const tasks = [{ id: "task-0", name: "npm run dev", description: "npm run dev", state: "running", can_stop: true, output_file: null, summary: null, entry: null }];
  await openAgent(page, { agent_view: { ...agentView, usage: sessionUsage, session: { ...session, doing: "Compacting the conversation" } }, agent_tasks: tasks });
  await expect(page.getByText("Compacting the conversation")).toBeVisible();
  await expect(page.getByText(/Waiting for/)).toHaveCount(0);
});

test("agent message of several lines stays inside its field", async ({ page }) => {
  await openAgent(page);
  const area = page.locator("#agfield textarea");
  await area.fill("the first line of a longer message that goes on\nsecond line\nthird line\nfourth line");
  const box = await page.locator("#agfield").boundingBox(), text = await area.boundingBox();
  if (!box || !text) throw new Error("no field");
  console.log(JSON.stringify({ box, text, sh: await area.evaluate(el => [el.scrollHeight, el.clientHeight, el.style.height]) }));
  expect(text.y).toBeGreaterThanOrEqual(box.y);
  expect(text.y + text.height).toBeLessThanOrEqual(box.y + box.height);
  expect(await area.evaluate(el => el.scrollHeight <= el.clientHeight)).toBe(true);
  await baseline(page, "agent-multiline");
});

test("agent running", async ({ page }) => {
  await openAgent(page);
  const process = (pid: number, command: string, parent = 0) => ({ pid, command, started_at: NOW - 600_000, ports: pid === 41 ? [5173] : [], left_group: false, parent, pages: pid === 41 ? [5173] : [] });
  const wrapped = "/usr/bin/zsh -c source /srv/home/.claude/shell-snapshots/snapshot-zsh-1.sh 2>/dev/null || true && eval 'npm run test:browser' < /dev/null";
  await page.evaluate(rows => window.__navigation.processes(rows), [process(41, "npm run dev"), process(42, "cargo watch -x check"), process(43, wrapped), process(44, "npm run test:browser", 43), process(45, "node /srv/project/node_modules/.bin/playwright test", 44), process(46, "/srv/cache/ms-playwright/chromium_headless_shell/chrome-headless-shell --headless", 45)]);
  await page.getByRole("tab", { name: /Running/ }).click();
  await expect(page.locator(".agrunningrow").filter({ hasText: "cargo watch" })).toBeVisible();
  await expect(page.locator(".agrunningrow")).toHaveCount(3);
  await page.locator(".agrunningrow").filter({ hasText: "npm run test:browser" }).locator(".t").click();
  await expect(page.locator(".agrunningrow .child")).toHaveCount(3);
  await expect(page.locator(".ag-page").getByRole("button", { name: "Open :5173" })).toBeVisible();
  await baseline(page, "agent-running");
});

test("agent artifacts", async ({ page }) => {
  const entries = [...transcript, { kind: "tool", text: "Artifact: /srv/project/report.html" }, { kind: "tool", text: "Artifact: /srv/project/chart.html" }];
  await openAgent(page, { agent_view: { ...agentView, usage: sessionUsage, entries }, agent_artifact: { kind: "html", size: 30, src: "data:text/html,%3Cp%3EPreview%3C%2Fp%3E" } });
  await page.getByRole("tab", { name: /Artifacts/ }).click();
  await expect(page.getByRole("listbox", { name: "Artifacts" }).getByRole("option", { name: /report.html/ })).toBeVisible();
  await expect(page.locator(".agartframe")).toBeVisible();
  await baseline(page, "agent-artifacts");
});

// Popovers

test("notices popover", async ({ page }) => {
  const session = (id: string, agent: string) => ({ agent: { machine_id: "m1", machine_name: machineId, id, agent, cwd: "/srv/widget", status: "idle", external: false } });
  const note = (id: string, kind: string, title: string, text: string, ago: number, target: object | null, html = "") => ({ id, at: NOW - ago, kind, title, text, html, source: machineId, read: false, done: false, target, allow_deny: true });
  await openDesktop(page, false, { notes_list: [
    note("first", "finished", "example/widget: Tidy the docs", "Rewrote the **README** example for `--compact`.", 60_000, session("s1", "codex"), "Rewrote the <strong>README</strong> example for <code>--compact</code>."),
    note("second", "asks", "example/widget: Fix the login", "Run cargo test -p widget?", 120_000, session("s2", "claude")),
    note("third", "offline", `${machineId} is offline`, "", 3_600_000, null),
  ] });
  await page.getByRole("button", { name: /Notifications/ }).click();
  await expect(page.locator('.nt-row[data-id="third"]')).toBeVisible();
  await baseline(page, "notices");
});

// Tray bar

const barRows = [
  { ...agentRow("asks", "Asks a question", "asking", 600_000), agent: "claude", question: { id: 1, text: "Run cargo test?", detail: "cargo test", options: [{ id: "allow", name: "Allow", allow: true }] } },
  agentRow("done", "Finished turn", "idle", 300_000),
  agentRow("runs", "Still running", "working", 30_000),
  agentRow("old", "Old idle work", "idle", 60_000),
];

async function openBar(page: Page) {
  const done = { id: "n1", kind: "finished", title: "Codex finished", text: "Done", source: machineId, at: NOW, read: false, done: false, target: { agent: barRows[1] } };
  await openDesktop(page, false, { agent_sessions: barRows, notes_list: [done], session_moves: [], agent_tree: listing, agent_view: { ...agentView, session: barRows[0], usage: sessionUsage } });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar")).toBeAttached();
  await page.evaluate(() => window.__navigation.emit("bar-shown", null));
  await expect(page.locator("#wtlist [role=option]")).toHaveCount(4);
}

test("bar waiting list", async ({ page }) => {
  await openBar(page);
  await expect(page.locator("#wtlist")).toBeFocused();
  await baseline(page, "bar-waiting");
});

test("bar session", async ({ page }) => {
  await openBar(page);
  await page.keyboard.press("Enter");
  await expect(page.locator("#agmsg")).toBeFocused();
  await expect(page.getByText("The query string now survives the redirect.")).toBeVisible();
  await baseline(page, "bar-session");
});

// Phone layout

test("phone home", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await openDesktop(page, false, { center_home: home });
  await expect(page.locator("body")).toHaveClass(/mobile/);
  await expect(page.locator("#sec-running .bd-card").first()).toBeVisible();
  await baseline(page, "phone-home");
});

test("phone assistant", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await openDesktop(page, false, { center_home: assistantHome, agent_sessions: assistantSessions, agent_hosts: [assistantHost], assistant_settings_get: { agent: "flsh", model: null, effort: "high", fast: null } });
  await expect(page.locator("body")).toHaveClass(/mobile/);
  await page.getByRole("button", { name: "New assistant" }).click();
  const list = page.locator(".asst-list");
  await expect(list.locator(".pj-li")).toHaveCount(2);
  await expect(page.locator("#agprompt")).toBeHidden();
  await baseline(page, "phone-assistant");
});

test("phone settings", async ({ page }) => {
  await page.addInitScript(() => { Reflect.set(window, "__BEEP_PHONE__", true); });
  await page.setViewportSize(PHONE);
  await openDesktop(page, false, { ...agentAccountResponses, center_home: home, mobile_paired: true });
  await page.locator("#main").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.getByRole("button", { name: "Report…" })).toBeVisible();
  await baseline(page, "phone-settings");
  await settings.getByRole("button", { name: "Agent accounts", exact: true }).click();
  await expect(settings.getByRole("button", { name: "Agent accounts", exact: true })).toHaveAttribute("aria-current", "true");
  await baseline(page, "phone-settings-agent-accounts");
  // A phone has no Esc: the sheet closes with its own button.
  await settings.getByRole("button", { name: "Close", exact: true }).click();
  await expect(settings).toBeHidden();
});

test("phone report", async ({ page }) => {
  await page.addInitScript(() => { Reflect.set(window, "__BEEP_PHONE__", true); });
  await page.setViewportSize(PHONE);
  await openDesktop(page, false, { ...settingsResponses, center_home: home, mobile_paired: true });
  await page.evaluate(() => window.__navigation.open("report"));
  const sheet = page.getByRole("dialog", { name: "Report a problem" });
  await expect(sheet.getByRole("button", { name: "Collect", exact: true })).toBeVisible();
  // Nothing is cut off: the sheet and its buttons stay inside the screen.
  expect(await sheet.evaluate(element => [element, ...element.querySelectorAll("button")].every(part => part.getBoundingClientRect().right <= document.documentElement.clientWidth))).toBe(true);
  await baseline(page, "phone-report");
});

test("unfolded foldable home", async ({ page }) => {
  await page.addInitScript(() => { Reflect.set(window, "__BEEP_PHONE__", true); });
  await page.setViewportSize({ width: 690, height: 830 });
  await openDesktop(page, false, { center_home: home, mobile_paired: true });
  await expect(page.locator("#sec-running .bd-card").first()).toBeVisible();
  await expect(page.locator("body")).not.toHaveClass(/mobile/);
  await expect(page.locator("#side kbd")).toHaveCount(0);
  await baseline(page, "foldable-home");
});

test("phone projects", async ({ page }) => {
  await page.setViewportSize(PHONE);
  await openDesktop(page, false, { center_home: home });
  await page.getByRole("navigation", { name: "Sections" }).getByRole("button", { name: "Projects", exact: true }).click();
  await expect(page.locator(".pj-card")).toHaveCount(3);
  await baseline(page, "phone-projects");
});

test("phone session with the command strip folded", async ({ page }) => {
  const steers = ["Also log each retry", "Then run clippy"];
  const commands = ["cargo test -p sync", "npm run dev", "tail -f node.log"];
  const tasks = commands.map((name, i) => ({ id: `task-${i}`, name, description: name, state: "running", can_stop: true, output_file: null, summary: null, entry: null }));
  await page.setViewportSize(PHONE);
  await openDesktop(page, false, { agent_view: { ...agentView, queued: steers.length, queued_messages: steers }, agent_tasks: tasks, agent_steer: [] });
  await page.evaluate(target => window.__navigation.open({ agent: target }), session);
  await expect(page.locator(".ag-workline")).toHaveText("2 queued");
  await expect(page.locator(".ag-tasks")).toHaveText("3 commands running");
  await baseline(page, "phone-session");
});

test("phone projects overflow", async ({ page }) => {
  const long = "a-very-long-repository-name-that-keeps-going-without-any-break-at-all";
  const boxes = Array.from({ length: 4 }, (_, i) => ({ id: `box-${i}`, name: `build-box-with-a-long-name-${i}` }));
  const rows = [
    ...boxes.map(box => ({ name: `example/${long}`, url: `https://example.test/example/${long}`, machine_id: box.id, machine_name: box.name, folder: `/srv/work/deeply/nested/folder/${long}`, cloned: true, imported_at: 1 })),
    { name: "plain-folder-without-an-org-and-a-long-name-too", url: "", machine_id: "box-1", machine_name: boxes[1].name, folder: "/srv/plain", cloned: false, imported_at: 1 },
  ];
  const longGroups = [{ id: "g", name: `A group with a long name ${long}`, paths: ["work/deeply/nested/folder"] }];
  const found = [{ name: `found-org/${long}`, remote: "https://example.test/found", places: boxes.map(box => ({ machine_id: box.id, machine_name: box.name, folder: `/srv/found/${long}` })), agents: [{ agent: "codex", sessions: 2 }], last_activity: NOW - 3_600_000 }];
  await page.setViewportSize(PHONE);
  await openDesktop(page, false, { center_home: { ...home, projects: rows, running: [] }, project_groups: longGroups, found_projects: found });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await expect(page.locator("#sec-found")).toBeVisible();
  await baseline(page, "phone-projects-overflow");
});


/// The first launch on a computer that is alone in its mesh.
async function openFirstRun(page: Page) {
  const env = { os: "macos", os_version: "macOS 26", arch: "aarch64", hostname: "build-box", cpus: 8, memory_mb: 16384, home: "/Users/me", avds: [], gpus: [], tools: [{ name: "git", version: "2.50.1" }, { name: "node", version: "25.9.0" }, { name: "claude", version: "2.1.0" }], logins: [] };
  await openDesktop(page, false, { center_home: home, list_machines: [{ ...machine(machineId, machineId, true, 0), env }] });
  await page.addInitScript(() => localStorage.removeItem("beep.firstRunDone"));
  await page.reload();
  await page.waitForFunction(() => window.__navigation.ready());
  await expect(page.getByRole("dialog", { name: "First run" })).toBeVisible();
}

test("first run", async ({ page }) => {
  await openFirstRun(page);
  await baseline(page, "first-run");
});

test("first run set up", async ({ page }) => {
  await openFirstRun(page);
  await page.getByRole("button", { name: /^Start here/ }).click();
  const setup = page.getByRole("dialog", { name: "Set up this computer" });
  await expect(setup.getByRole("button", { name: "Sign in: Claude Code" })).toBeVisible();
  await expect(setup.getByRole("button", { name: "Install: Codex" })).toBeVisible();
  await baseline(page, "first-run-setup");
});

test("home with nothing running", async ({ page }) => {
  await openDesktop(page, false, { center_home: { ...home, running: [], needs_you: [] }, agent_sessions: [], project_sessions: [], projects_sessions: [] });
  await expect(page.locator(".painted-empty")).toBeVisible();
  await baseline(page, "home-empty");
});

// The frozen clock is on a day of the sunrise. The paintings of the three days after it:
for (const [day, scene] of [[1, "haystacks"], [2, "lilies"], [3, "parliament"]] as const) {
  test(`home with nothing running, under the ${scene}`, async ({ page }) => {
    await page.clock.setFixedTime(NOW + day * 86_400_000);
    await openDesktop(page, false, { center_home: { ...home, running: [], needs_you: [] }, agent_sessions: [], project_sessions: [], projects_sessions: [] });
    await expect(page.locator(`.painted-empty > .painted-art.${scene}`)).toBeVisible();
    await baseline(page, `home-empty-${scene}`);
  });

  test(`start screen under the ${scene}`, async ({ page }) => {
    await page.clock.setFixedTime(NOW + day * 86_400_000);
    await openDesktop(page, false, { center_home: home });
    await page.evaluate(() => { Math.random = () => 0; });
    await openStartForm(page);
    await expect(page.locator(`.agc-intro > .painted-art.${scene}`)).toBeVisible();
    await baseline(page, `start-${scene}`);
  });
}

test("new project repository settings", async ({ page }) => {
  const forge = { host: "github", github: true, owners: ["example"], error: null };
  await openDesktop(page, false, { center_home: home, project_forges: [forge] });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog").getByRole("button", { name: "New project", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "New project" });
  const owner = sheet.getByRole("group", { name: "Account or organization" });
  const visibility = sheet.getByRole("group", { name: "Visibility" });
  await expect(owner.getByRole("button", { name: "example" })).toBeVisible();
  await expect(visibility.getByRole("button", { name: "Private" })).toHaveAttribute("aria-pressed", "true");
  const left = await owner.boundingBox();
  const right = await visibility.boundingBox();
  expect(left && right && left.x + left.width < right.x && left.y === right.y).toBe(true);
  await baseline(page, "new-project", ".np-sheet");
});

test("add a project", async ({ page }) => {
  const place = (folder: string) => ({ machine_id: machineId, machine_name: machineId, folder });
  const found = [
    { name: "example/atlas", remote: "https://example.test/example/atlas", places: [place("/srv/atlas")], agents: [{ agent: "codex", sessions: 4 }], last_activity: NOW - 3_600_000 },
    { name: "example/harbor", remote: "https://example.test/example/harbor", places: [place("/srv/harbor"), place("/srv/harbor-next")], agents: [], last_activity: 0 },
    { name: "tools/ledger", remote: "https://example.test/tools/ledger", places: [place("/srv/ledger")], agents: [{ agent: "claude", sessions: 12 }], last_activity: NOW - 86_400_000 },
  ];
  const hosts = [{ host: "example.test", github: false, error: null, repos: [{ name: "example/quill", url: "https://example.test/example/quill", updated_at: NOW - 7_200_000 }] }];
  await openDesktop(page, false, { center_home: home, found_projects: found, host_repos: hosts, found_machines: [] });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Add a project" });
  await expect(sheet.getByText("example/atlas")).toBeVisible();
  await sheet.getByText("Browse repositories on connected hosts").click();
  await baseline(page, "add-project");
});

test("add a folder", async ({ page }) => {
  const at = (path: string, folders: string[], repos: string[], origin: string | null = null) => ({ path, parent: path.slice(0, path.lastIndexOf("/")), exists: true, folders, more: 0, empty: false, origin, repos });
  const machine_folders = {
    "~": at("/home/dev", ["projects"], []),
    "/home/dev/projects": at("/home/dev/projects", ["atlas", "harbor", "notes", "quill"], ["atlas", "harbor", "quill"]),
    "/home/dev/projects/atlas": at("/home/dev/projects/atlas", ["docs", "src"], [], "https://example.test/example/atlas.git"),
  };
  const online = { ...home, machines: home.machines.map(m => ({ ...m, online: true })) };
  await openDesktop(page, false, { center_home: online, machine_search: "/home/dev/projects", machine_folders, syncthing_in_use: true, project_syncthing_files: { available: true, enabled: true, label: "Projects" } });
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "Choose folder" }).click();
  const sheet = page.getByRole("dialog", { name: "Add a folder" });
  await sheet.getByRole("option", { name: "atlas" }).click();
  await expect(sheet.getByRole("checkbox", { name: "Keep Syncthing file sync" })).toBeVisible();
  await baseline(page, "add-folder");
});

test("agent sign-in", async ({ page }) => {
  await openDesktop(page, false, {
    ...settingsResponses,
    agent_accounts: { claude: null, codex_key: null, machines: [{ id: "test-box", name: "test-box", online: true, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null }], named: [] },
    list_machines: [{ ...machine("test-box", "test-box", true, 0), env: { logins: [{ tool: "claude", signed_in: false, account: null }] } }],
    machine_sign_in: { state: "waiting", url: "https://example.test/oauth/authorize?code=true&client_id=0000&response_type=code&scope=profile+inference&state=abcdef", code: null, asks_code: true, reason: null },
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("button", { name: "Agent accounts", exact: true }).click();
  await settings.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Sign in to Claude Code on test-box", exact: true }).getByRole("button", { name: "Open in browser" })).toBeVisible();
  await baseline(page, "agent-sign-in");
});

test("settings codex fast thinking", async ({ page }) => {
  await openDesktop(page, false, { ...settingsResponses,
    assistant_settings_get: { agent: "codex", model: "model-a", effort: "ultra", fast: true },
    mac_control: { id: "build-box" },
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [], assistant: true }],
    agent_models: { pick: "listed", current: "model-a", models: [{ id: "model-a", name: "Example Model", description: null, provider: null, efforts: ["low", "medium", "high", "xhigh", "max", "ultra"], fast: true }] },
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: /^Assistant/ }).click();
  await settings.locator("#assistantSetting").getByRole("button").click();
  const picker = page.getByRole("dialog", { name: "Harness, model and thinking" });
  await expect(picker.getByRole("button", { name: "Ultra", exact: true })).toBeVisible();
  await expect(picker.getByRole("button", { name: "None", exact: true })).toHaveCount(0);
  const fast = picker.getByRole("switch", { name: "Fast mode", exact: true });
  await expect(fast).toHaveAttribute("aria-checked", "true");
  await baseline(page, "settings-codex-fast-thinking");
  await fast.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "assistant_settings_save").at(-1)?.args.settings)).toMatchObject({ agent: "codex", effort: "ultra", fast: false });
});

test("agent split session", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const widgetFolder = `/srv/${projectName}`;
  const first = { ...session, cwd: widgetFolder };
  const other = { ...agentRow("notice-two", "Update the changelog", "idle", 600_000, widgetFolder), agent: "claude", assistant: false, persona: null };
  const firstView = { ...agentView, session: { ...first, question: null }, usage: sessionUsage };
  const otherView = { ...agentView, session: { ...other, question: null }, entries: [{ kind: "agent", text: "Changelog updated for 0.9.0." }] };
  await openDesktop(page, false, {
    center_home: { ...home, running: [first, other] },
    agent_sessions: [first, other],
    project_sessions: [first, other],
    agent_views: { "notice-one": firstView, "notice-two": otherView },
    agent_tree: listing,
  });
  await page.evaluate(target => window.__navigation.open({ agent: target }), first);
  await expect(page.getByText("The query string now survives the redirect.")).toBeVisible();
  await page.locator("#rail").getByRole("button", { name: /Update the changelog/ }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Open beside" }).click();
  await expect(page.locator("#agpane-right .agsplit")).toContainText("Changelog updated for 0.9.0.");
  await baseline(page, "agent-split-session");
});
