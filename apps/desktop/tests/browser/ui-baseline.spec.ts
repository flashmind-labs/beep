// UI baseline: a "no visual or DOM change" check for refactors.
//
// Each test opens one representative state through the browser harness with
// fixed fixture data and a frozen clock, then compares two things against the
// committed baselines in `ui-baseline.spec.ts-snapshots/`:
//   - a normalized DOM snapshot of the window (tags, sorted classes, ids,
//     roles, aria and other stable attributes, text, focus), and
//   - a screenshot with a small tolerance.
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

import { expect, test, type Page } from "@playwright/test";
import { openDesktop, openStartForm, projectName } from "./harness.ts";

/// The frozen wall clock every state renders at.
const NOW = Date.UTC(2026, 0, 15, 12, 0, 0);
const PHONE = { width: 375, height: 812 };

test.describe.configure({ mode: "parallel" });
test.use({ timezoneId: "UTC", locale: "en-US" });
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
const agentRow = (id: string, title: string, status: string, ago: number, cwd = folder) => ({ kind: "agent", machine_id: machineId, machine_name: machineId, id, agent: "codex", title, cwd, status, doing: status === "working" ? "Running tests" : null, started_at: NOW - 7_200_000, last_activity: NOW - ago, external: false, question: null });

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
const agentView = { session: { ...session, question: null }, plan: [], files: [], entries: transcript, earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 2 };
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

  expect(previous).toMatchSnapshot(`${name}.txt`);
  await expect(page).toHaveScreenshot(`${name}.png`, { maxDiffPixelRatio: 0.002, animations: "disabled", caret: "hide" });
}

async function openAgent(page: Page, responses: Record<string, unknown> = {}) {
  await openDesktop(page, false, { agent_view: agentView, agent_tree: listing, agent_usage: sessionUsage, ...responses });
  await page.evaluate(target => window.__navigation.open({ agent: target }), session);
  await expect(page.getByText("The query string now survives the redirect.")).toBeVisible();
}

async function openProject(page: Page, responses: Record<string, unknown> = {}) {
  await openDesktop(page, false, { center_home: home, ...projectPage, ...responses });
  await page.evaluate(name => window.__navigation.open({ projects: { project: name } }), projectName);
  await expect(page.locator(".pj-strip")).toBeVisible();
}

// Desktop window

test("home overview", async ({ page }) => {
  await openDesktop(page, false, { center_home: home });
  await expect(page.locator("#sec-needs .bd-card").first()).toBeVisible();
  await baseline(page, "home-overview");
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
  const prompt = { ...hit("agent_session", "s2", "Tidy the example docs", "Claude Code · example/widget · build-box"), keys: [], snippet: "Prompt: …rewrite the example in the README so it matches the new widget options and drop the old flags…" };
  await openDesktop(page, false, { center_home: home, center_palette_index: index, center_find_prompts: [prompt] });
  await page.getByRole("button", { name: /^Search/ }).click();
  await page.getByRole("dialog", { name: "Search" }).getByRole("combobox").fill("example");
  await page.locator(".cc-pal .it", { hasText: "Tidy the example docs" }).scrollIntoViewIfNeeded();
  await baseline(page, "palette");
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
  const item = (repo: string, number: number, kind: string, title: string, hours: number, extra = {}) => ({ host: "example.test", repo, number, kind, title, url: `https://example.test/${repo}/${number}`, updated_at: NOW - hours * 3_600_000, authored: true, assigned: false, review_requested: false, draft: false, review: "none", checks_failing: false, comments: 0, labels: [], ...extra });
  const items = [
    item("agent/plugins", 412, "pull_request", "feat: add background and foreground task switch", 3, { labels: ["plugins"] }),
    item("agent/plugins", 398, "pull_request", "fix: keep a session's model when it resumes", 24, { checks_failing: true }),
    item("agent/plugins", 377, "issue", "Tool calls should be shown in one place", 96),
    item("docs/handbook", 88, "pull_request", "docs: a shorter README and a manual", 30),
    item("docs/handbook", 85, "pull_request", "docs: sync, explained in one page", 50),
    item("cli/runner", 31, "pull_request", "fix: timeouts other than the default are honoured", 48),
    item("cli/runner", 14, "issue", "Exit code is lost in a pipe", 60),
    item("lib/parser", 12, "issue", "Tables break on nested pipes", 168),
  ];
  await openDesktop(page, false, { center_home: home, found_projects: found, git_items: { items, hosts: [] } });
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

test("terminals", async ({ page }) => {
  const terminal = (id: string, name: string, state: string) => ({ id, project: "", machine_id: machineId, machine_name: machineId, name, command: "shell", cwd: folder, state, code: state === "exited" ? 0 : null, started_at: NOW - 600_000 });
  await openDesktop(page, false, { terminals_all: [terminal("t1", "dev server", "running"), terminal("t2", "tests", "running"), terminal("t3", "build", "exited")] });
  await page.evaluate(() => window.__navigation.open("terminals"));
  await expect(page.locator(".tl-list").getByRole("button", { name: /dev server/ })).toBeVisible();
  await baseline(page, "terminals");
});

const settingsResponses = { settings: { name: machineId, version: "0.9.0", autostart: false, agent_notices: false }, list_machines: [], list_credentials: [], list_github_accounts: [], skill_sync_get: true, project_sync_get: true, beep_sync_over_syncthing_get: false };
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
  const wsl = (name: string, agents: string[], machineId: string | null) => ({ name, agents, node: machineId ? "0.9.0" : null, machine_id: machineId, kept: !!machineId, problem: null, probed: true });
  const env = { os: "windows", os_version: "Windows", arch: "x86_64", hostname: "desk-pc", cpus: 16, memory_mb: 32768, home: "C:\\Users\\user", tools: [], logins: [], avds: [], gpus: [], wsl: [wsl("Ubuntu", ["claude", "codex"], "wsl-box"), wsl("Debian", ["gemini"], null)] };
  const machines = [{ ...machine("desk-pc", "desk-pc", true, 0), env }, machine("wsl-box", "Ubuntu on desk-pc", true, 0)];
  await openDesktop(page, false, { ...settingsResponses, list_machines: machines });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const nav = page.getByRole("dialog", { name: "Settings" }).getByRole("navigation", { name: "Settings pages" });
  await nav.getByRole("button", { name: /^Machines/ }).click();
  await page.locator("[data-machine='desk-pc']").click();
  await expect(page.getByRole("button", { name: "Add Debian as a machine" })).toBeVisible();
  await baseline(page, "settings-machine-wsl");
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

for (const section of [null, "Agent accounts", "Appearance"] as const) {
  const name = `settings-${section === null ? "first" : section.toLowerCase().replace(/\s+/g, "-")}`;
  test(name, async ({ page }) => {
    await openDesktop(page, false, section === "Agent accounts" ? {
      ...settingsResponses,
      agent_accounts: { claude: null, codex_key: null, machines: [
        { id: "build-box", name: "build-box", online: true, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null },
        { id: "test-box", name: "test-box", online: true, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null },
        { id: "offline-box", name: "offline-box", online: false, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null },
      ] },
      list_machines: [
        { ...machine("build-box", "build-box", true, 0), env: { logins: [{ tool: "claude", signed_in: true, account: "you@example.com" }] } },
        { ...machine("test-box", "test-box", true, 0), env: { logins: [{ tool: "claude", signed_in: false, account: null }] } },
      ],
    } : settingsResponses);
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

test("unfolded foldable session with the sidebar collapsed", async ({ page }) => {
  await page.addInitScript(() => { Reflect.set(window, "__BEEP_PHONE__", true); });
  await page.setViewportSize({ width: 750, height: 832 });
  await openAgent(page, { mobile_paired: true });
  await page.getByRole("button", { name: "Collapse sidebar" }).click();
  // A phone docks Files under the conversation, never beside it.
  await expect(page.locator("#agshell")).toHaveClass(/narrow/);
  expect((await page.locator("#agtitle").boundingBox())?.width ?? 0).toBeGreaterThan(150);
  await baseline(page, "foldable-session");

  // Folded, the dock keeps its tabs in a row; a tab opens it again.
  await page.locator("#agpane-bottom").getByTitle("Hide the pane").click();
  const strip = page.locator("#agpstrip-bottom");
  await expect(strip.getByRole("tab", { name: "Changes" })).toBeVisible();
  await baseline(page, "foldable-session-folded");
  await strip.getByRole("tab", { name: "Changes" }).click();
  await expect(page.locator("#agpane-bottom")).toBeVisible();
  await expect(page.locator("#agpane-bottom .agtab.on")).toHaveAttribute("data-mtab", /changes/i);
});

test("agent session transcript", async ({ page }) => {
  await openAgent(page);
  await baseline(page, "agent-transcript");
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

test("agent changes", async ({ page }) => {
  await openAgent(page);
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  await expect(page.locator('.dv-file[data-path="sample.ts"]')).toBeVisible();
  await baseline(page, "agent-changes");
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
  await openAgent(page, { agent_view: { ...agentView, session: { ...session, status: "asking", question } } });
  await expect(page.locator(".dcoder .q")).toBeVisible();
  await baseline(page, "agent-question");
});

test("agent with commands running and messages queued", async ({ page }) => {
  const steers = ["Also log each retry", "Then run clippy"];
  const tasks = ["cargo test -p sync", "npm run dev"].map((name, i) => ({ id: `task-${i}`, name, description: name, state: "running", can_stop: true, output_file: null, summary: null, entry: null }));
  await openAgent(page, { agent_view: { ...agentView, queued: steers.length, queued_messages: steers }, agent_tasks: tasks, agent_steer: [] });
  await expect(page.locator(".ag-tasks")).toHaveText("2 commands running");
  await baseline(page, "agent-work");
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
  const process = (pid: number, command: string, parent = 0) => ({ pid, command, started_at: NOW - 600_000, ports: pid === 41 ? [5173] : [], left_group: false, parent });
  const wrapped = "/usr/bin/zsh -c source /srv/home/.claude/shell-snapshots/snapshot-zsh-1.sh 2>/dev/null || true && eval 'npm run test:browser' < /dev/null";
  await page.evaluate(rows => window.__navigation.processes(rows), [process(41, "npm run dev"), process(42, "cargo watch -x check"), process(43, wrapped), process(44, "npm run test:browser", 43), process(45, "node /srv/project/node_modules/.bin/playwright test", 44), process(46, "/srv/cache/ms-playwright/chromium_headless_shell/chrome-headless-shell --headless", 45)]);
  await page.getByRole("tab", { name: /Running/ }).click();
  await expect(page.locator(".agrunningrow").filter({ hasText: "cargo watch" })).toBeVisible();
  await expect(page.locator(".agrunningrow")).toHaveCount(3);
  await page.locator(".agrunningrow").filter({ hasText: "npm run test:browser" }).locator(".t").click();
  await expect(page.locator(".agrunningrow .child")).toHaveCount(3);
  await baseline(page, "agent-running");
});

test("agent artifacts", async ({ page }) => {
  const entries = [...transcript, { kind: "tool", text: "Artifact: /srv/project/report.html" }, { kind: "tool", text: "Artifact: /srv/project/chart.html" }];
  await openAgent(page, { agent_view: { ...agentView, entries }, agent_artifact: { kind: "html", size: 30, src: "data:text/html,%3Cp%3EPreview%3C%2Fp%3E" } });
  await page.getByRole("tab", { name: /Artifacts/ }).click();
  await expect(page.getByRole("listbox", { name: "Artifacts" }).getByRole("option", { name: /report.html/ })).toBeVisible();
  await expect(page.locator(".agartframe")).toBeVisible();
  await baseline(page, "agent-artifacts");
});

// Popovers

test("notices popover", async ({ page }) => {
  const note = (id: string, kind: string, title: string, ago: number) => ({ id, at: NOW - ago, kind, title, text: "", source: machineId, read: false, done: false, target: null });
  await openDesktop(page, false, { notes_list: [note("first", "finished", "Codex finished", 60_000), note("second", "asks", "Permission required", 120_000), note("third", "finished", "Claude Code finished", 3_600_000)] });
  await page.getByRole("button", { name: /Notifications/ }).click();
  await expect(page.locator('.nt-row[data-id="third"]')).toBeVisible();
  await baseline(page, "notices");
});

const usage = (() => {
  const spend = (name: string, tokens: number) => ({ project: name, cost_usd: 0, unpriced: [], metered_tokens: 0, subscription: [{ name: "claude", tokens }] });
  const period = {
    total: { project: "", cost_usd: 0, unpriced: [], metered_tokens: 0, subscription: [] }, tokens: 39_300_000,
    projects: [spend("example/widget", 38_000_000), spend("example/gadget", 1_300_000)],
    harnesses: [{ agent: "claude", subscription: true, cost_usd: null, tokens: 39_300_000, models: [{ name: "claude-sonnet", tokens: 39_300_000 }] }],
  };
  const window = (minutes: number, used: number) => ({ minutes, used_percent: used, resets_at: NOW + 3_600_000, scope: null });
  return {
    limits: [
      { agent: "claude", plan: "max", machine_name: machineId, as_of: NOW, windows: [window(300, 38), window(10_080, 76)], credits: [] },
      { agent: "codex", plan: "pro", machine_name: machineId, as_of: NOW, windows: [window(10_080, 20)], credits: [] },
    ],
    today: period, week: period, month: period,
  };
})();

test("usage popover", async ({ page }) => {
  await openDesktop(page, false, { usage_get: usage });
  await page.getByRole("button", { name: /^Plan usage/ }).click();
  await expect(page.getByRole("dialog", { name: "Plan usage and cost" })).toBeVisible();
  await baseline(page, "usage");
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
  await openDesktop(page, false, { agent_sessions: barRows, notes_list: [done], session_moves: [], agent_tree: listing, agent_view: { ...agentView, session: barRows[0] }, agent_usage: sessionUsage });
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
  await expect(page.locator("#sec-needs .bd-card").first()).toBeVisible();
  await baseline(page, "phone-home");
});

test("unfolded foldable home", async ({ page }) => {
  await page.addInitScript(() => { Reflect.set(window, "__BEEP_PHONE__", true); });
  await page.setViewportSize({ width: 690, height: 830 });
  await openDesktop(page, false, { center_home: home, mobile_paired: true });
  await expect(page.locator("#sec-needs .bd-card").first()).toBeVisible();
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

test("agent sign-in", async ({ page }) => {
  await openDesktop(page, false, {
    ...settingsResponses,
    agent_accounts: { claude: null, codex_key: null, machines: [{ id: "test-box", name: "test-box", online: true, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null }] },
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
