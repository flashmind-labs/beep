import { build } from "esbuild";
import { readFile } from "node:fs/promises";
import { extname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { Tree } from "../../src/agentpane/worktree-state.ts";
import { expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import type { HomeView } from "../../src/center/home-state.ts";
import type { AgentUsage } from "../../src/agents/agents.ts";
import type { RunningAgent } from "../../src/agents/running-state.ts";
import type { ProjectRow } from "../../src/project/projects-state.ts";
import type { RunningProcess } from "../../src/agentpane/running-state.ts";
import type { MeshEvent } from "../../src/ui/mesh-events.ts";
import type { CenterTarget } from "../../src/project/projectwindow.ts";

/// Bundles isolated component fixtures with the app's asset loaders.
export async function bundleFixture(entry: string, production = false): Promise<string> {
  const bundle = await build({
    entryPoints: [entry], bundle: true, write: false, platform: "browser", format: "iife",
    loader: { ".svg": "text" },
    define: production ? { "process.env.NODE_ENV": '"production"' } : {},
  });
  const script = bundle.outputFiles?.[0];
  if (!script) throw new Error("Fixture bundle produced no script");
  return script.text;
}

interface BrowserSession extends RunningAgent { question: null }
interface Fixture { project: ProjectRow; sessions: [BrowserSession, BrowserSession]; home: HomeView; streaming: boolean }
interface NativeCall { command: string; args: Record<string, unknown> }
/// A project opened on one row of its list. The project page reads `tile` as the
/// window's own navigation passes it; no native target carries it.
type ProjectTileTarget = { projects: { project: string; tile: string } };

interface NavigationHarness {
  open(target: CenterTarget | ProjectTileTarget): void;
  ready(): boolean;
  processes(rows: RunningProcess[]): void;
  worktrees(rows: Tree[]): void;
  sessionUsage(id: string, usage: AgentUsage): void;
  calls(): NativeCall[];
  respond(command: string, value: unknown): void;
  notify(topic: string): void;
  /// One event on exactly `topic`, at `revision` or the next one.
  publish(topic: string, revision: number | null): void;
  emit(name: string, payload: unknown): void;
  hold(command: string): void;
  holdNext(command: string): void;
  release(command: string): void;
  /// Fails the next `command` with `error`, a plain test failure when none is given.
  rejectOnce(command: string, error?: string): void;
}
type NativeHandler = (event: { payload: unknown }) => void;
interface NativeBoundary {
  core: { invoke(command: string, args?: Record<string, unknown>): Promise<unknown>; Channel: new <T>() => { id: number; onmessage(message: T): void } };
  event: { listen(name: string, handler: NativeHandler): Promise<() => void> };
  webviewWindow: { getCurrentWebviewWindow(): { listen(name: string, handler: NativeHandler): Promise<() => void> } };
}

declare global { interface Window { __navigation: NavigationHarness } }

export const projectName = "example/widget";
const machine = "build-box";
const folder = "/srv/project";
const session = (id: string, title: string): BrowserSession => ({
  kind: "agent", machine_id: machine, machine_name: machine, id, agent: "codex", title, cwd: folder,
  status: "working", doing: null, started_at: 1, last_activity: 1, external: false, question: null,
});
const sessions: Fixture["sessions"] = [session("notice-one", "First notification"), session("notice-two", "Second notification")];
const project: ProjectRow = { display_name: null, name: projectName, url: "https://example.test/example/widget", machine_id: machine, machine_name: machine, folder, cloned: true, imported_at: 1 };
const fixture: Fixture = {
  project, sessions, streaming: false,
  home: { machines: [], projects: [project], running: sessions, needs_you: [] },
};

/// WebKit reports viewport keys it does not know as console errors; the
/// window keeps `interactive-widget` for Android keyboards.
const knownWarnings = [/^Viewport argument key "interactive-widget" not recognized/];

/// Opens the start form from the project's checkout on build-box, as its
/// "Start an agent here" does.
export async function openStartForm(page: Page): Promise<void> {
  await page.evaluate(name => window.__navigation.open({ projects: { project: name, tile: "machines" } }), projectName);
  await page.getByRole("article", { name: machine, exact: true }).locator(".pm-head").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Start an agent here" }).click();
  await expect(page.locator("#agprompt")).toBeVisible();
}

/// Collects the page's uncaught errors and console errors, without known browser warnings.
export function pageErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.stack ?? error.message));
  page.on("console", message => {
    const text = message.text();
    if (message.type() === "error" && !knownWarnings.some(known => known.test(text))) errors.push(text);
  });
  return errors;
}

/// Waits until an open session has its final layout. The session shows before the window has laid its
/// tools over the title row, and before the side pane has the size for the shell, so a box read
/// earlier is a box of a layout that is about to change.
export async function sessionSettled(page: Page): Promise<void> {
  await expect.poll(() => page.evaluate(() => {
    const main = document.querySelector<HTMLElement>(".cc-main");
    const top = main?.querySelector<HTMLElement>(":scope > .cc-top");
    const head = main?.querySelector<HTMLElement>(".ag-head");
    const agent = main?.querySelector(':scope > #pane-box:not([hidden]) > .dbar[data-view="agent"]');
    const toolsKept = Boolean(head && top && head.getBoundingClientRect().right <= top.getBoundingClientRect().left + 0.5);

    const pane = document.querySelector<HTMLElement>("#agpane-right");
    const shell = document.querySelector<HTMLElement>(".agshell");
    const wanted = shell && pane && !pane.hidden ? `${Math.round(shell.clientWidth * 0.42)}px` : null;
    const sized = wanted === null || pane?.style.flexBasis === `var(--drag-size, ${wanted})`;

    return Boolean(agent && toolsKept && sized);
  }), { message: "the session's layout settles" }).toBe(true);
}

export async function openDesktop(page: Page, streaming = false, responses: Record<string, unknown> = {}): Promise<void> {
  const dist = fileURLToPath(new URL("../../dist/", import.meta.url));
  const contentTypes = new Map([[".html", "text/html"], [".js", "text/javascript"], [".css", "text/css"], [".svg", "image/svg+xml"], [".woff2", "font/woff2"]]);
  await page.route("https://app.test/**", async route => {
    const path = resolve(dist, decodeURIComponent(new URL(route.request().url()).pathname).replace(/^\/+/, ""));
    const within = relative(dist, path);
    if (within.startsWith("..") || isAbsolute(within)) return route.fulfill({ status: 403 });

    try { await route.fulfill({ body: await readFile(path), contentType: contentTypes.get(extname(path)) ?? "application/octet-stream" }); }
    catch { await route.fulfill({ status: 404 }); }
  });
  // Most flows here look at Home's lanes: a test of the review picks it before it opens.
  await page.addInitScript(() => {
    try { if (localStorage.getItem("beep.home.grouping") === null) localStorage.setItem("beep.home.grouping", "lanes"); }
    catch { /* a blank page has no storage */ }
  });
  await page.addInitScript(installNativeBoundary, { ...fixture, streaming, responses });
  await page.goto("https://app.test/center.html");
  await page.waitForFunction(() => window.__navigation.ready());
}

function installNativeBoundary(data: Fixture & { responses: Record<string, unknown> }): void {
  // The first-run screen was answered, so it stays out of every test's page.
  try { localStorage.setItem("beep.firstRunDone", "1"); } catch { /* the page has no storage */ }
  interface EventChannel { onmessage(event: unknown): void }
  const channel = (value: unknown): value is EventChannel => value !== null && typeof value === "object" && "onmessage" in value && typeof value.onmessage === "function";
  const rejected = new Map<string, string>();
  const held = new Set<string>();
  const heldOnce = new Set<string>();
  const releases = new Map<string, () => void>();
  const readers = new Map<string, { channel: EventChannel; topics: { machine: string | null; topic: string }[] }>();
  const follows = new Map<string, EventChannel>();
  let processes: RunningProcess[] = [];
  let revision = 0;
  const agentPage = (id: unknown) => ({ session: data.sessions.find(row => row.id === id) ?? data.sessions[0], plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 0, usage: null });
  const empty = { start: 0, next: 0, fingerprint: "empty" };
  const interests = (value: unknown): { machine: string | null; topic: string }[] => Array.isArray(value) ? value.filter((v: unknown): v is { machine: string | null; topic: string } => v !== null && typeof v === "object" && "machine" in v && (v.machine === null || typeof v.machine === "string") && "topic" in v && typeof v.topic === "string") : [];
  const calls: NativeCall[] = [];
  const handlers = new Map<string, Set<NativeHandler>>();
  let pending: CenterTarget | ProjectTileTarget | null = null;
  let pendingReads = 0;
  const answers: Record<string, unknown> = {
    build_capabilities: { local_host: true, metrics_recording: true, resource_profiling: true, autostart: true, window_state: true, image_clipboard: true, desktop_control: true, rich_text: true },
    mac_control: { id: data.project.machine_id, name: data.project.machine_name, enabled: true, status: "connected", error: null, permissions: {} },
    agent_tree_text: { size: 10000, text: Array.from({ length: 500 }, (_, i) => `const value${i} = ${i};`).join("\n"), cut: false, binary: false },
    center_home: data.home,
    project_rename_repositories: [{ url: data.project.url, host: "GitHub", name: "widget" }],
    rename_project: [],
    project_git_state: [{ machine_id: data.project.machine_id, machine_name: data.project.machine_name, branch: "main", upstream: null, ahead: 0, behind: 0, error: null }],
    project_git_history: { machine_id: data.project.machine_id, machine_name: data.project.machine_name, root: data.project.folder, refs: [], worktrees: [], commits: [], next: null },
    project_machines: [{ id: data.project.machine_id, name: data.project.machine_name, online: true, folder: data.project.folder, checking: false, checked_at: 1 }],
    agent_sessions: data.sessions, project_sessions: data.sessions, projects_sessions: data.sessions,
    center_project_runs: { runs: [], checks: [], synced_at: null, error: null, rate_limited_until: null, hosts: [] },
  };
  const emptyLists = ["list_machines", "agent_hosts", "agent_reviews", "agent_tasks", "agent_artifacts", "agent_running", "agent_editors", "editors", "project_groups", "projects_on_top", "project_sync", "found_projects", "project_syncthing", "machine_traffic", "tool_updates", "updatable_machines", "notes_list", "terminal_list", "terminals_list", "terminals_all", "outbox_list"];
  for (const command of emptyLists) answers[command] = [];
  Object.assign(answers, data.responses);
  const boundary: NativeBoundary = {
    core: {
      async invoke(command, args = {}) {
        calls.push({ command, args });
        if (held.has(command) || heldOnce.delete(command)) await new Promise<void>(resolve => { releases.set(command, resolve); });
        const failure = rejected.get(command);
        if (rejected.delete(command)) throw new Error(failure);
        if (command === "center_pending") { pendingReads++; const target = pending; pending = null; return target; }

        if (command === "mesh_watch") {
          if (typeof args.id === "string" && channel(args.onEvent)) readers.set(args.id, { channel: args.onEvent, topics: interests(args.topics) });
          return new Promise<never>(() => {});
        }

        if (command === "mesh_topics" && typeof args.id === "string") {
          const reader = readers.get(args.id);
          if (reader) reader.topics = interests(args.topics);
          return null;
        }

        if (command === "agent_follow" && data.streaming && typeof args.session === "string" && channel(args.onEvent)) {
          follows.set(args.session, args.onEvent);
          args.onEvent.onmessage({ kind: "snapshot", resumed: false, cursor: empty, page: agentPage(args.session), modes: null, tasks: [] });
          return new Promise<never>(() => {});
        }

        if (command === "project_sync_pause") {
          const detail = answers.project_sync_detail;
          if (detail && typeof detail === "object" && "paused" in detail && typeof args.paused === "boolean") {
            answers.project_sync_detail = { ...detail, paused: args.paused };
            return structuredClone(answers.project_sync_detail);
          }
        }

        if (command === "plugins_asset") {
          // The bytes of a plugin image, as the desktop sends them: `plugins_assets` maps a hash to base64.
          const table = answers.plugins_assets;
          const text = table !== null && typeof table === "object" && typeof args.hash === "string" ? Reflect.get(table, args.hash) : null;
          if (typeof text !== "string") throw new Error("no plugin image");
          return Uint8Array.from(atob(text), char => char.charCodeAt(0)).buffer;
        }

        if (command === "list_machine_folder" || command === "list_machine_tree") {
          // A folder by the path asked for, as `machine_folders` maps them; any other gets `list_machine_folder`.
          // A tree is that folder alone.
          const table = answers.machine_folders;
          const listing = (table !== null && typeof table === "object" && typeof args.path === "string" ? Reflect.get(table, args.path) : null) ?? answers.list_machine_folder ?? null;
          return structuredClone(command === "list_machine_tree" ? (listing ? [listing] : []) : listing);
        }

        if (command === "agent_processes") { return { processes: structuredClone(processes) }; }
        if (command === "machine_forward_port") { return args.port; }

        if (command === "work_diff") {
          const context = typeof args.context === "number" ? args.context : 3;
          const start = Math.max(1, 20 - context), end = Math.min(100, 20 + context);
          return { root: data.project.folder, base: "main", merge_base: "abc1234", working_tree: true, additions: 1, deletions: 0, more: 0, files: [{ path: "sample.ts", old_path: null, status: "modified", additions: 1, deletions: 0, left_out: null, hunks: [{ old_start: start, new_start: start, old_lines: end - start + 1, new_lines: end - start + 1, section: "", lines: Array.from({ length: end - start + 1 }, (_, i) => ({ k: " ", t: `const value${start + i} = ${start + i};` })) }] }] };
        }

        if (command === "agent_view") {
          const bySession = answers.agent_views && typeof answers.agent_views === "object" && typeof args.session === "string"
            ? Reflect.get(answers.agent_views, args.session) : undefined;
          return structuredClone(bySession ?? answers.agent_view ?? agentPage(args.session));
        }

        return structuredClone(answers[command] ?? null);
      },
      Channel: class<T> { id = 0; onmessage(_message: T): void {} },
    },
    event: { async listen(name, handler) {
      let registered = handlers.get(name);
      if (!registered) { registered = new Set(); handlers.set(name, registered); }
      registered.add(handler);
      return () => { registered.delete(handler); };
    } },
    webviewWindow: { getCurrentWebviewWindow: () => ({ listen: (name, handler) => boundary.event.listen(name, handler) }) },
  };
  Object.defineProperty(window, "__TAURI__", { value: boundary });
  // The callbacks Rust's channel messages call; nothing here sends any.
  let nextCallback = 1;
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: { transformCallback: () => nextCallback++, unregisterCallback() {} } });
  const harness: NavigationHarness = {
    open(target) { pending = target; for (const handler of handlers.get("center-open") ?? []) handler({ payload: target }); },
    processes(rows) {
      processes = rows;
      const event: MeshEvent = { machine_id: data.project.machine_id, topic: "processes.notice-one", revision: ++revision, initial: false };
      for (const reader of readers.values()) if (reader.topics.some(interest => interest.topic === event.topic || interest.topic === "processes.*")) reader.channel.onmessage(event);
    },
    worktrees(rows) { answers.agent_worktrees = rows; },
    sessionUsage(id, usage) {
      follows.get(id)?.onmessage({ kind: "delta", resumed: false, cursor: empty, page: { ...agentPage(id), usage }, modes: null, tasks: [] });
    },
    emit(name, payload) { for (const handler of handlers.get(name) ?? []) handler({ payload }); },
    rejectOnce(command, error = "Requested test failure") { rejected.set(command, error); },
    hold(command) { held.add(command); },
    holdNext(command) { heldOnce.add(command); },
    release(command) { held.delete(command); releases.get(command)?.(); releases.delete(command); },
    calls: () => calls,
    respond(command, value) { answers[command] = value; },
    notify(topic) {
      const event: MeshEvent = { machine_id: data.project.machine_id, topic: `${topic}.*`, revision: ++revision, initial: false };
      for (const reader of readers.values()) if (reader.topics.some(interest => interest.topic === event.topic)) reader.channel.onmessage(event);
    },
    publish(topic, at) {
      const event: MeshEvent = { machine_id: data.project.machine_id, topic, revision: at ?? ++revision, initial: false };
      for (const reader of readers.values()) if (reader.topics.some(interest => interest.topic === topic)) reader.channel.onmessage(event);
    },
    ready: () => (pendingReads > 0 || (typeof answers.center_window_alone === "object" && answers.center_window_alone !== null)) && handlers.has("center-open"),
  };
  Object.defineProperty(window, "__navigation", { value: harness });
}

/// The app limits text selection to chat. A test that checks a selection surviving a
/// re-render opts the page in, so it protects the render and not the selection policy.
export async function allowSelection(page: Page): Promise<void> {
  await page.addStyleTag({ content: ":root:root:root * { -webkit-user-select: text !important; user-select: text !important }" });
}
