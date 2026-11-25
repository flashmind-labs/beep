import { build } from "esbuild";
import { readFile } from "node:fs/promises";
import { extname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { Tree } from "../../src/agentpane/worktree-state.ts";
import { expect } from "@playwright/test";
import type { Page } from "@playwright/test";
import type { HomeView } from "../../src/center/home-state.ts";
import type { AgentUsage } from "../../src/agents/agents-state.ts";
import type { RunningAgent } from "../../src/agents/running-state.ts";
import type { ProjectRow } from "../../src/project/projects-state.ts";
import type { RunningContainer, RunningProcess } from "../../src/agentpane/running-state.ts";
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
  /// The node pushes a session's whole process list; a view that subscribes later is sent the last one, as native code does.
  processes(rows: RunningProcess[], session?: string, containers?: RunningContainer[]): void;
  /// While quiet, the node sends no snapshot to a topic taken anew: only what native code still retains arrives.
  quietNode(on: boolean): void;
  worktrees(rows: Tree[]): void;
  sessionUsage(id: string, usage: AgentUsage): void;
  calls(): NativeCall[];
  respond(command: string, value: unknown): void;
  /// An update on `topic`, carrying `views` the way a topic push carries its values.
  notify(topic: string, views?: { command: string; args?: Record<string, unknown>; value: unknown }[]): void;
  /// One event on exactly `topic`, at `revision` or the next one, carrying `data` from `machine` when given.
  publish(topic: string, revision: number | null, data?: unknown, machine?: string): void;
  emit(name: string, payload: unknown): void;
  hold(command: string): void;
  holdNext(command: string): void;
  release(command: string): void;
  /// Fails the next `command` with `error`, a plain test failure when none is given.
  rejectOnce(command: string, error?: string): void;
  /// Every later call of `command` fails.
  rejectAll(command: string): void;
  /// Ends `rejectAll`: the next call of `command` answers again.
  allow(command: string): void;
}
type NativeHandler = (event: { payload: unknown }) => void;
interface NativeBoundary {
  core: { invoke(command: string, args?: Record<string, unknown>): Promise<unknown>; Channel: new <T>() => { id: number; onmessage(message: T): void } };
  event: { listen(name: string, handler: NativeHandler): Promise<() => void> };
  webviewWindow: { getCurrentWebviewWindow(): { label: string; listen(name: string, handler: NativeHandler): Promise<() => void> } };
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
const project: ProjectRow = { display_name: null, name: projectName, url: "https://example.test/example/widget", machine_id: machine, machine_name: machine, folder, cloned: true };
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

/// Pushes Home (the fixture's when none is given) with one more project, and waits until the sidebar
/// draws it. Home follows a push once a second at most, so what a spec checks after this has seen that refresh.
export async function pushHome(page: Page, home: { projects: unknown[] } = { ...fixture.home, projects: [project] }, sentinel = "team/sentinel"): Promise<void> {
  const next = { ...home, projects: [...home.projects, { ...project, name: sentinel, folder: "/srv/sentinel" }] };
  await page.evaluate(value => { window.__navigation.respond("center_home", value); window.__navigation.notify("home"); }, next);
  await expect(page.locator("#side .cc-grp.projects").getByRole("button", { name: sentinel, exact: true })).toBeVisible();
}

/// Pushes the projects pinned to the top and waits until the sidebar's Pinned group lists exactly those.
export async function pushPinned(page: Page, names: string[]): Promise<void> {
  await page.evaluate(value => { window.__navigation.respond("projects_on_top", value); window.__navigation.notify("projects"); }, names);
  const group = page.locator("#side .cc-grp.pinned");
  if (!names.length) { await expect(group).toHaveCount(0); return; }
  for (const name of names) await expect(group.getByRole("button", { name })).toBeVisible();
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

export async function openDesktop(page: Page, streaming = false, responses: Record<string, unknown> = {}, windowLabel = "center"): Promise<void> {
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
  await page.addInitScript(installNativeBoundary, { ...fixture, streaming, responses, windowLabel });
  await page.goto("https://app.test/center.html");
  await page.waitForFunction(() => window.__navigation.ready());
}

function installNativeBoundary(data: Fixture & { responses: Record<string, unknown>; windowLabel: string }): void {
  // The first-run screen was answered, so it stays out of every test's page.
  try { localStorage.setItem("beep.firstRunDone", "1"); } catch { /* the page has no storage */ }
  interface EventChannel { onmessage(event: unknown): void }
  const channel = (value: unknown): value is EventChannel => value !== null && typeof value === "object" && "onmessage" in value && typeof value.onmessage === "function";
  const rejected = new Map<string, string>();
  const failing = new Set<string>();
  const held = new Set<string>();
  const heldOnce = new Set<string>();
  const releases = new Map<string, () => void>();
  const readers = new Map<string, { channel: EventChannel; topics: { machine: string | null; topic: string }[] }>();
  const follows = new Map<string, EventChannel>();
  /// The node's current list per session topic, and the part of it native code retains for its windows.
  const pushed = new Map<string, MeshEvent>();
  const retained = new Map<string, MeshEvent>();
  let quiet = false;
  let revision = 0;
  const agentPage = (id: unknown) => ({ session: data.sessions.find(row => row.id === id) ?? data.sessions[0], plan: [], entries: [], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 0, usage: null });
  const empty = { start: 0, next: 0, fingerprint: "empty" };
  const interests = (value: unknown): { machine: string | null; topic: string }[] => Array.isArray(value) ? value.filter((v: unknown): v is { machine: string | null; topic: string } => v !== null && typeof v === "object" && "machine" in v && (v.machine === null || typeof v.machine === "string") && "topic" in v && typeof v.topic === "string") : [];
  const calls: NativeCall[] = [];
  const covers = (topics: { topic: string }[], event: MeshEvent) => topics.some(interest => interest.topic === event.topic || interest.topic === "processes.*");
  /// Native code drops a retained update once no window wants its topic; the node sends a fresh one later.
  const forget = () => {
    for (const [topic, event] of retained) if (![...readers.values()].some(reader => covers(reader.topics, event))) retained.delete(topic);
  };
  /// What a window newly interested in a topic is sent from what native code retained.
  const prime = (reader: { channel: EventChannel; topics: { machine: string | null; topic: string }[] }, before: { topic: string }[]) => {
    for (const event of pushed.values()) {
      if (!covers(reader.topics, event) || covers(before, event) || (quiet && !retained.has(event.topic))) continue;
      retained.set(event.topic, event);
      reader.channel.onmessage(structuredClone(event));
    }
  };
  /// The views each topic's push carries (fleet/event_views.rs).
  const pushedViews: Record<string, string[]> = {
    home: ["center_home", "agent_sessions"],
    machines: ["list_machines", "mac_control", "updatable_machines", "agent_hosts", "device_hosts", "machine_clocks"],
    tools: ["tool_updates"], terminals: ["terminals_all"],
    projects: ["projects_on_top", "project_groups"],
    sync: ["project_sync"], usage: ["usage_get"], accounts: ["agent_accounts"], forge: ["git_items"],
  };
  const handlers = new Map<string, Set<NativeHandler>>();
  let pending: CenterTarget | ProjectTileTarget | null = null;
  let pendingReads = 0;
  const answers: Record<string, unknown> = {
    build_capabilities: { local_host: true, metrics_recording: true, autostart: true, image_clipboard: true, desktop_control: true },
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
  const emptyLists = ["list_machines", "agent_hosts", "agent_reviews", "agent_tasks", "agent_artifacts", "agent_running", "agent_editors", "editors", "project_groups", "projects_on_top", "project_sync", "found_projects", "project_syncthing", "notes_list", "terminal_list", "terminals_list", "terminals_all"];
  for (const command of emptyLists) answers[command] = [];
  Object.assign(answers, data.responses);
  // `failing` in the responses names commands that fail from the first call, until `allow`.
  if (Array.isArray(answers.failing)) for (const command of answers.failing) if (typeof command === "string") failing.add(command);
  const boundary: NativeBoundary = {
    core: {
      async invoke(command, args = {}) {
        calls.push({ command, args });
        if (held.has(command) || heldOnce.delete(command)) await new Promise<void>(resolve => { releases.set(command, resolve); });
        const failure = rejected.get(command);
        if (rejected.delete(command)) throw new Error(failure);
        if (failing.has(command)) throw new Error("Requested test failure");
        if (command === "center_pending") { pendingReads++; const target = pending; pending = null; return target; }

        if (command === "mesh_watch") {
          if (typeof args.id === "string" && channel(args.onEvent)) {
            const reader = { channel: args.onEvent, topics: interests(args.topics) };
            readers.set(args.id, reader);
            prime(reader, []);
            forget();
          }
          return new Promise<never>(() => {});
        }

        if (command === "mesh_topics" && typeof args.id === "string") {
          const reader = readers.get(args.id);
          if (reader) {
            const before = reader.topics;
            reader.topics = interests(args.topics);
            prime(reader, before);
            forget();
          }
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

        if (command === "machine_forward_port") { return args.port; }

        if (command === "work_diff") {
          const context = typeof args.context === "number" ? args.context : 3;
          const start = Math.max(1, 20 - context), end = Math.min(100, 20 + context);
          return { root: data.project.folder, base: "main", merge_base: "abc1234", working_tree: true, additions: 1, deletions: 0, more: 0, files: [{ path: "sample.ts", old_path: null, status: "modified", additions: 1, deletions: 0, left_out: null, hunks: [{ old_start: start, new_start: start, old_lines: end - start + 1, new_lines: end - start + 1, section: "", lines: Array.from({ length: end - start + 1 }, (_, i) => ({ k: " ", t: `const value${start + i} = ${start + i};` })) }] }] };
        }

        if (command === "outbox_list") return { revision: 0, session: args.session, rows: [] };

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
    webviewWindow: { getCurrentWebviewWindow: () => ({ label: data.windowLabel, listen: (name, handler) => boundary.event.listen(name, handler) }) },
  };
  Object.defineProperty(window, "__TAURI__", { value: boundary });
  // The callbacks Rust's channel messages call; nothing here sends any.
  let nextCallback = 1;
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: { transformCallback: () => nextCallback++, unregisterCallback() {} } });
  const harness: NavigationHarness = {
    open(target) { pending = target; for (const handler of handlers.get("center-open") ?? []) handler({ payload: target }); },
    processes(rows, session = "notice-one", containers = []) {
      const event: MeshEvent = { machine_id: data.project.machine_id, topic: `processes.${session}`, revision: ++revision, initial: false, data: { Sessions: { Processes: { processes: rows, containers } } } };
      pushed.set(event.topic, event);
      const wanted = [...readers.values()].filter(reader => covers(reader.topics, event));
      if (wanted.length) retained.set(event.topic, event);
      for (const reader of wanted) reader.channel.onmessage(structuredClone(event));
    },
    quietNode(on) { quiet = on; },
    worktrees(rows) { answers.agent_worktrees = rows; },
    sessionUsage(id, usage) {
      follows.get(id)?.onmessage({ kind: "delta", resumed: false, cursor: empty, page: { ...agentPage(id), usage }, modes: null, tasks: [] });
    },
    emit(name, payload) { for (const handler of handlers.get(name) ?? []) handler({ payload }); },
    rejectOnce(command, error = "Requested test failure") { rejected.set(command, error); },
    rejectAll(command) { failing.add(command); },
    allow(command) { failing.delete(command); },
    hold(command) { held.add(command); },
    holdNext(command) { heldOnce.add(command); },
    release(command) { held.delete(command); releases.get(command)?.(); releases.delete(command); },
    calls: () => calls,
    respond(command, value) { answers[command] = value; },
    notify(topic, views) {
      // Native code always sends the views of a topic, as the commands answer now.
      const sent = views ?? (pushedViews[topic] ?? []).filter(command => command in answers).map(command => ({ command, args: {}, value: structuredClone(answers[command]) }));
      const event: MeshEvent = { machine_id: data.project.machine_id, topic: `${topic}.*`, revision: ++revision, initial: false, ...(sent.length ? { data: { views: sent } } : {}) };
      for (const reader of readers.values()) if (reader.topics.some(interest => interest.topic === event.topic)) reader.channel.onmessage(event);
    },
    publish(topic, at, payload, machine) {
      const event: MeshEvent = { machine_id: machine ?? data.project.machine_id, topic, revision: at ?? ++revision, initial: false, ...(payload === undefined ? {} : { data: payload }) };
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
