import test from "node:test";
import assert from "node:assert/strict";
import {
  projectPaletteIndex,
  projectPromptEntries,
  rank,
  type Entry,
  type HitKind,
  type PaletteIndex,
  type ProjectPaletteScope,
} from "../src/shell/center/palette-rank.ts";
import { route, type WindowScope } from "../src/stores/project/projectwindow.ts";

const scope: WindowScope = {
  kind: "project",
  project: "org/site",
  folders: [{ id: "build-box", folder: "/srv/site" }],
};
const agent = (machine_id: string, cwd: string, main?: string) => ({
  kind: "agent",
  machine_id,
  machine_name: machine_id,
  id: "s1",
  agent: "claude",
  title: null as string | null,
  cwd,
  status: "idle",
  external: false,
  place: main ? { folder: cwd, main, linked: true, branch: "fix", origin: "" } : null,
});

test("a project window keeps its project and what opens from it", () => {
  assert.equal(route(scope, { view: "projects", arg: { project: "org/site" } }), "here");
  assert.equal(route(scope, { view: "pane", arg: { agent: agent("build-box", "/srv/site") } }), "here");
  assert.equal(route(scope, { view: "pane", arg: { agent: agent("build-box", "/srv/site/.worktrees/fix") } }), "here");
  assert.equal(route(scope, { view: "pane", arg: { agent: agent("build-box", "/srv/worktrees/site-fix", "/srv/site") } }), "here");
  assert.equal(route(scope, { view: "pane", arg: { agentnew: { prompt: "", assistant: false } } }), "here");
  assert.equal(route(scope, { view: "pane", arg: { draft: { project: "org/site", machine: null } } }), "here");
  assert.equal(route(scope, { view: "pane", arg: { draft: { project: "org/other", machine: null } } }), "main");
});

test("a project window sends what is not its project to the main window", () => {
  assert.equal(route(scope, { view: "home", arg: null }), "main");
  assert.equal(route(scope, { view: "projects", arg: { project: "org/other" } }), "main");
  assert.equal(route(scope, { view: "projects", arg: { project: null } }), "main");
  assert.equal(route(scope, { view: "pane", arg: "settings" }), "main");
  assert.equal(route(scope, { view: "pane", arg: { agent: agent("build-box", "/srv/site-old") } }), "main");
  assert.equal(route(scope, { view: "pane", arg: { agent: agent("gpu-box", "/srv/site") } }), "main");
  assert.equal(route(scope, { view: "pane", arg: { agent: agent("build-box", "") } }), "main");
});

type TestSession = ReturnType<typeof agent> & { last_activity?: number };

function entry(kind: HitKind, id: string, title: string, detail = "", keys = [title.toLowerCase()]): Entry<TestSession> {
  return { kind, id, title, detail, keys };
}

test("a detached project palette keeps only that project's items and actions", () => {
  const siteSession: TestSession = { ...agent("build-box", "/srv/site"), id: "s-site", title: "Update CI workflow", last_activity: 20 };
  const namedSession: TestSession = { ...agent("build-box", "/srv/site"), id: "s-named", title: "Polish site footer", last_activity: 30 };
  const nestedSession: TestSession = { ...agent("build-box", "/srv/site/nested"), id: "s-nested", title: "Nested repo work", last_activity: 40 };
  const wrongMachineSession: TestSession = { ...agent("gpu-box", "/tmp/site"), id: "s-wrong-box", title: "Unimported copy", last_activity: 50 };
  const otherSession: TestSession = { ...agent("build-box", "/srv/other"), id: "s-other", title: "Other repo fix", last_activity: 60 };
  const olderPast: TestSession = { ...agent("build-box", "/srv/site"), id: "s-past", title: "Migrate auth tokens", last_activity: 10 };

  const global: PaletteIndex<TestSession> = {
    actions: [entry("action", "home", "Home"), entry("action", "settings", "Settings")],
    settings: [entry("setting", "creds", "GitHub token", "Settings · Credentials")],
    machines: [entry("machine", "build-box", "build-box", "online")],
    projects: [
      entry("project", "org/site", "org/site", "build-box"),
      entry("project", "org/other", "org/other", "build-box"),
      entry("folder", "scratch", "scratch", "Not imported"),
    ],
    code: [
      entry("branch", "org/site fix-auth", "org/site · fix-auth", "Branch · 2 ahead · Refresh auth cookie", ["fix-auth", "org/site"]),
      entry("branch", "org/other fix-auth", "org/other · fix-auth", "Branch · 1 ahead · Other auth", ["fix-auth", "org/other"]),
    ],
    sessions: [
      { ...entry("agent_session", "build-box s-site", "Update CI workflow", "org/site", ["update ci workflow", "org/site", "claude", "build-box"]), project: "org/site", session: siteSession },
      { ...entry("agent_session", "build-box s-named", "Polish site footer", "org/site", ["polish site footer", "org/site", "claude", "build-box"]), project: "org/site", session: namedSession },
      { ...entry("agent_session", "build-box s-nested", "Nested repo work", "org/nested", ["nested repo work", "org/nested", "claude", "build-box"]), project: "org/nested", session: nestedSession },
      { ...entry("agent_session", "gpu-box s-wrong-box", "Unimported copy", "org/site", ["unimported copy", "org/site", "claude", "gpu-box"]), project: "org/site", session: wrongMachineSession },
      { ...entry("agent_session", "build-box s-other", "Other repo fix", "org/other", ["other repo fix", "org/other", "claude", "build-box"]), project: "org/other", session: otherSession },
    ],
  };

  const paletteScope: ProjectPaletteScope<TestSession> = {
    project: "org/site",
    label: "org/site",
    imported: true,
    board: true,
    hasTeam: false,
    hasCheckout: true,
    folders: [{ id: "build-box", folder: "/srv/site" }],
    prs: [{ number: 42, title: "Fix auth redirect", author: "alice", mine: false, draft: true, labels: ["security"] }],
    issues: [{ number: 19, title: "Auth cookie expires early", author: "bob", labels: ["bug"], assignees: ["carol"] }],
    terminals: [{ id: "t1", name: "dev server", command: "npm run dev", machine_name: "build-box", state: "running" }],
    sessions: [siteSession, olderPast],
  };

  const scoped = projectPaletteIndex(global, paletteScope);
  assert.deepEqual(scoped.projects, []);
  assert.deepEqual(scoped.machines, []);
  assert.deepEqual(scoped.settings, []);

  // Empty query lists only in-project actions, never global Home or Settings.
  const emptyTitles = rank("", scoped).map(hit => hit.title);
  assert.ok(emptyTitles.includes("New session"));
  assert.ok(emptyTitles.includes("New terminal"));
  assert.ok(emptyTitles.includes("Board"));
  assert.ok(emptyTitles.includes("Pull requests"));
  assert.ok(emptyTitles.includes("Skills"));
  assert.ok(!emptyTitles.includes("Home"));
  assert.ok(!emptyTitles.includes("Settings"));

  // Sessions exclude other repos, nested child repos and checkouts outside this window's folders, and include project_sessions history once.
  assert.deepEqual(scoped.sessions.map(s => s.title), [
    "Polish site footer",
    "Update CI workflow",
    "Migrate auth tokens",
  ]);
  assert.equal(scoped.sessions[0].detail, "build-box");

  // Searching the repo name only matches items whose own text holds it, not every session or branch via a synthetic repo key.
  assert.deepEqual(rank("org/site", scoped).map(h => h.title), []);
  assert.deepEqual(rank("site", scoped).map(h => h.title), ["Polish site footer"]);

  // PRs, issues, branches and terminals inside the project rank by number, title, label and command.
  const authHits = rank("auth", scoped);
  assert.equal(authHits[0]?.top, true);
  assert.deepEqual(authHits.map(h => [h.kind, h.title]), [
    ["issue", "#19 Auth cookie expires early"],
    ["agent_session", "Migrate auth tokens"],
    ["pr", "#42 Fix auth redirect"],
    ["branch", "fix-auth"],
  ]);
  assert.equal(rank("#42", scoped)[0]?.kind, "pr");
  assert.equal(rank("carol", scoped)[0]?.kind, "issue");
  assert.equal(rank("npm run dev", scoped)[0]?.kind, "terminal");

  // Prompt search results also filter to this project's checkouts and rewrite their detail.
  const prompts = projectPromptEntries([
    { ...global.sessions[0], snippet: "retry CI step" },
    { ...global.sessions[3], snippet: "retry wrong machine" },
    { ...global.sessions[4], snippet: "retry other repo" },
  ], paletteScope);
  assert.equal(prompts.length, 1);
  assert.equal(prompts[0].id, "build-box s-site");
  assert.equal(prompts[0].detail, "build-box");
});
