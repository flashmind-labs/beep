import { readFile } from "node:fs/promises";
import { extname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import type { Page } from "@playwright/test";
import type { HomeView } from "../../src/center/home-state.ts";
import type { RunningAgent } from "../../src/agents/running-state.ts";
import type { ProjectRow } from "../../src/project/projects-state.ts";
import type { CenterTarget } from "../../src/project/projectwindow.ts";

interface BrowserSession extends RunningAgent { question: null }
interface Fixture { project: ProjectRow; sessions: [BrowserSession, BrowserSession]; home: HomeView }
interface NavigationHarness { open(target: CenterTarget): void; ready(): boolean }
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
  project, sessions,
  home: { machines: [], projects: [project], running: sessions, needs_you: [], coming_up: null, updates: null },
};

export async function openDesktop(page: Page): Promise<void> {
  const dist = fileURLToPath(new URL("../../dist/", import.meta.url));
  const contentTypes = new Map([[".html", "text/html"], [".js", "text/javascript"], [".css", "text/css"], [".svg", "image/svg+xml"], [".woff2", "font/woff2"]]);
  await page.route("https://app.test/**", async route => {
    const path = resolve(dist, decodeURIComponent(new URL(route.request().url()).pathname).replace(/^\/+/, ""));
    const within = relative(dist, path);
    if (within.startsWith("..") || isAbsolute(within)) return route.fulfill({ status: 403 });
    try { await route.fulfill({ body: await readFile(path), contentType: contentTypes.get(extname(path)) ?? "application/octet-stream" }); }
    catch { await route.fulfill({ status: 404 }); }
  });
  await page.addInitScript(installNativeBoundary, fixture);
  await page.goto("https://app.test/center.html");
  await page.waitForFunction(() => window.__navigation.ready());
}

function installNativeBoundary(data: Fixture): void {
  const handlers = new Map<string, Set<NativeHandler>>();
  let pending: CenterTarget | null = null;
  let pendingReads = 0;
  const answers: Record<string, unknown> = {
    status: { link: "unlinked", signed_out: true, server: false, live: false, session: null },
    mac_control: { id: data.project.machine_id, name: data.project.machine_name, enabled: true, status: "connected", error: null, permissions: {} },
    center_home: data.home,
    project_machines: [{ id: data.project.machine_id, name: data.project.machine_name, online: true, folder: data.project.folder, checking: false, checked_at: 1 }],
    agent_sessions: data.sessions, project_sessions: data.sessions,
  };
  const emptyLists = ["list_machines", "list_sessions", "list_groups", "agent_hosts", "agent_tasks", "agent_artifacts", "agent_running", "agent_editors", "editors", "project_overview", "project_groups", "projects_on_top", "project_sync", "found_projects", "project_syncthing", "project_git_state", "machine_traffic", "tool_updates", "updatable_machines", "notes_list", "list_research", "terminal_list"];
  for (const command of emptyLists) answers[command] = [];
  const boundary: NativeBoundary = {
    core: {
      async invoke(command, args = {}) {
        if (command === "center_pending") { pendingReads++; const target = pending; pending = null; return target; }
        if (command === "mesh_watch") return new Promise<never>(() => {});
        if (command === "agent_view") {
          const shown = data.sessions.find(row => row.id === args.session) ?? data.sessions[0];
          return { session: shown, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, queued_messages: [], turns: 0 };
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
    ready: () => pendingReads > 0 && handlers.has("center-open"),
  };
  Object.defineProperty(window, "__navigation", { value: harness });
}
