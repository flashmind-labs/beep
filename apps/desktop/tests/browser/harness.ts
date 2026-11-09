import { readFile } from "node:fs/promises";
import { extname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { Tree } from "../../src/agentpane/worktree-state.ts";
import type { Page } from "@playwright/test";
import type { HomeView } from "../../src/center/home-state.ts";
import type { RunningAgent } from "../../src/agents/running-state.ts";
import type { ProjectRow } from "../../src/project/projects-state.ts";
import type { RunningProcess } from "../../src/agentpane/running-state.ts";
import type { MeshEvent } from "../../src/ui/mesh-events.ts";
import type { CenterTarget } from "../../src/project/projectwindow.ts";

interface BrowserSession extends RunningAgent { question: null }
interface Fixture { project: ProjectRow; sessions: [BrowserSession, BrowserSession]; home: HomeView; streaming: boolean }
interface NavigationHarness { open(target: CenterTarget): void; ready(): boolean; processes(rows: RunningProcess[]): void; worktrees(rows: Tree[]): void }
type NativeHandler = (event: { payload: unknown }) => void;
interface NativeBoundary {
  core: { invoke(command: string, args?: Record<string, unknown>): Promise<unknown>; Channel: new <T>() => { onmessage(message: T): void } };
  event: { listen(name: string, handler: NativeHandler): Promise<() => void> };
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
const project: ProjectRow = { name: projectName, url: "https://example.test/example/widget", machine_id: machine, machine_name: machine, folder, cloned: true, imported_at: 1 };
const fixture: Fixture = {
  project, sessions, streaming: false,
  home: { machines: [], projects: [project], running: sessions, needs_you: [], coming_up: null, updates: null },
};

export async function openDesktop(page: Page, streaming = false): Promise<void> {
  const dist = fileURLToPath(new URL("../../dist/", import.meta.url));
  const contentTypes = new Map([[".html", "text/html"], [".js", "text/javascript"], [".css", "text/css"], [".svg", "image/svg+xml"], [".woff2", "font/woff2"]]);
  await page.route("https://app.test/**", async route => {
    const path = resolve(dist, decodeURIComponent(new URL(route.request().url()).pathname).replace(/^\/+/, ""));
    const within = relative(dist, path);
    if (within.startsWith("..") || isAbsolute(within)) return route.fulfill({ status: 403 });

    try { await route.fulfill({ body: await readFile(path), contentType: contentTypes.get(extname(path)) ?? "application/octet-stream" }); }
    catch { await route.fulfill({ status: 404 }); }
  });
  await page.addInitScript(installNativeBoundary, { ...fixture, streaming });
  await page.goto("https://app.test/center.html");
  await page.waitForFunction(() => window.__navigation.ready());
}

function installNativeBoundary(data: Fixture): void {
  interface EventChannel { onmessage(event: unknown): void }
  const channel = (value: unknown): value is EventChannel => value !== null && typeof value === "object" && "onmessage" in value && typeof value.onmessage === "function";
  const readers = new Map<string, { channel: EventChannel; topics: { machine: string | null; topic: string }[] }>();
  let processes: RunningProcess[] = [];
  let revision = 0;
  const agentPage = (id: unknown) => ({ session: data.sessions.find(row => row.id === id) ?? data.sessions[0], plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 0 });
  const interests = (value: unknown): { machine: string | null; topic: string }[] => Array.isArray(value) ? value.filter((v: unknown): v is { machine: string | null; topic: string } => v !== null && typeof v === "object" && "machine" in v && (v.machine === null || typeof v.machine === "string") && "topic" in v && typeof v.topic === "string") : [];
  const handlers = new Map<string, Set<NativeHandler>>();
  let pending: CenterTarget | null = null;
  let pendingReads = 0;
  const answers: Record<string, unknown> = {
    status: { link: "unlinked", signed_out: true, server: false, live: false, session: null },
    mac_control: { id: data.project.machine_id, name: data.project.machine_name, enabled: true, status: "connected", error: null, permissions: {} },
    center_home: data.home,
    project_git_state: [{ machine_id: data.project.machine_id, machine_name: data.project.machine_name, branch: "main", upstream: null, ahead: 0, behind: 0, error: null }],
    project_git_history: { machine_id: data.project.machine_id, machine_name: data.project.machine_name, root: data.project.folder, refs: [], worktrees: [], commits: [], next: null },
    project_machines: [{ id: data.project.machine_id, name: data.project.machine_name, online: true, folder: data.project.folder, checking: false, checked_at: 1 }],
    agent_sessions: data.sessions, project_sessions: data.sessions,
  };
  const emptyLists = ["list_machines", "list_sessions", "list_groups", "agent_hosts", "agent_reviews", "agent_tasks", "agent_artifacts", "agent_running", "agent_editors", "editors", "project_overview", "project_groups", "projects_on_top", "project_sync", "found_projects", "project_syncthing", "machine_traffic", "tool_updates", "updatable_machines", "notes_list", "list_research", "terminal_list"];
  for (const command of emptyLists) answers[command] = [];
  const boundary: NativeBoundary = {
    core: {
      async invoke(command, args = {}) {
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

        if (command === "agent_follow" && data.streaming && channel(args.onEvent)) {
          args.onEvent.onmessage({ kind: "snapshot", resumed: false, cursor: { start: 0, next: 0, fingerprint: "empty" }, page: agentPage(args.session), modes: null, tasks: [] });
          return new Promise<never>(() => {});
        }

        if (command === "agent_processes") { return { processes: structuredClone(processes), local: false }; }

        if (command === "agent_view") {
          return agentPage(args.session);
        }

        return structuredClone(answers[command] ?? null);
      },
      Channel: class<T> { onmessage(_message: T): void {} },
    },
    event: { async listen(name, handler) {
      let registered = handlers.get(name);
      if (!registered) { registered = new Set(); handlers.set(name, registered); }
      registered.add(handler);
      return () => { registered.delete(handler); };
    } },
  };
  Object.defineProperty(window, "__TAURI__", { value: boundary });
  const harness: NavigationHarness = {
    open(target) { pending = target; for (const handler of handlers.get("center-open") ?? []) handler({ payload: target }); },
    processes(rows) {
      processes = rows;
      const event: MeshEvent = { machine_id: data.project.machine_id, topic: "processes.notice-one", revision: ++revision, initial: false };
      for (const reader of readers.values()) if (reader.topics.some(interest => interest.topic === event.topic || interest.topic === "processes.*")) reader.channel.onmessage(event);
    },
    worktrees(rows) { answers.agent_worktrees = rows; },
    ready: () => pendingReads > 0 && handlers.has("center-open"),
  };
  Object.defineProperty(window, "__navigation", { value: harness });
}
