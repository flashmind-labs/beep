import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { pushedView, snapshotView, type Pushed } from "../ui/snapshot-state.ts";
// Home reads changed domains; telemetry arrives from the native cache.
import { invoke } from "../ui/tauri.ts";
import { ambient, AMBIENT_MS, onShownChange, pageShown, single, SLOW_MS } from "../ui/active.ts";
import { applyTelemetry, keepUnchanged, withListedMachines, type HomeView, type Telemetry } from "./home-state.ts";
import { type ProjectGroup } from "../project/projects-state.ts";
import { CONNECT_STUCK_MS, CONNECT_RETRY_MS, state, pagesAccount } from "./state.ts";
import { draw } from "./home.tsx";
import { readAllTerms } from "../terminals/view.tsx";
import { refreshGit } from "./openitems.ts";
import { currentLocation } from "./views.tsx";
import { reloadGroup, drawRail, drawSide } from "./sidebar.tsx";
import { liveMode, redrawProjectsPage, runs } from "../project/sections.ts";
import { MACHINES, machinesChanged, takeTraffic } from "../machines/machine-store.ts";
import { keepConnecting, type MacControl } from "../machines/machine-state.ts";
import { mergeSyncRows, type SyncRow } from "../project/filesync.ts";
import { followSync, readSync } from "../project/syncpage.ts";
import { perfFresh } from "../ui/perf.ts";

/// The mesh answered for Home in this window; until then Home may be
/// the one the app kept from before.
let fresh = false;

// ---------- Refresh ----------

let again = false, retryTimer: ReturnType<typeof setTimeout> | undefined;

let telemetry: Telemetry | undefined;
/// The newest Home the `home` push carried and no read took yet.
let pushedHome: Pushed<HomeView> | null = null;
let homeRead: Promise<void> | undefined;
/// The last answer, before telemetry, and the Home it made: a Home put in
/// place elsewhere is compared whole.
let lastRead: HomeView | null = null;
let shown: HomeView | null = null;
let undrawn = false;
/// Times Home from its kept draw to the first fresh one.
let homeFresh: (() => void) | null = null;

/// Home drew what the app kept; called before any fresh read drew.
export function homeCached() {
  if (!fresh) homeFresh = perfFresh("home");
}

/// Every write to the window's Home, pins, groups, `where` and Mac comes
/// through here. A local edit goes to `lastRead` too, so the next read still
/// compares against what Home shows and an unchanged Home draws nothing.
function editHome(edit: (home: HomeView) => HomeView) {
  if (!state.home) return;
  const kept = state.home === shown;
  state.home = edit(state.home);

  if (kept) {
    shown = state.home;
    if (lastRead) lastRead = edit(lastRead);
  }
}

/// A project the user removed: the topics push the new lists.
export function forgetProject(name: string) {
  state.top = state.top.filter(n => n !== name);
  editHome(home => home.projects ? { ...home, projects: home.projects.filter(p => p.name !== name) } : home);
  state.where.delete(name);
}

/// Forgets what `where` read for project `name`, or for every project.
export function dropWhere(name?: string) {
  if (name === undefined) state.where.clear();
  else state.where.delete(name);
}

// Home's cards take their names, and drop a removed machine, from the machine list.
machinesChanged.add(() => {
  editHome(home => withListedMachines(home, MACHINES.list));
  if (onScreen()) draw();
});

/// Takes the newest pushed Home, or reads it when none came yet: membership, running sessions
/// and Home data without refreshing unrelated panes.
/// Sections that did not change keep their objects; an unchanged Home draws nothing.
async function readHome(): Promise<void> {
  if (homeRead) { again = true; return homeRead; }
  homeRead = (async () => {
    do {
      again = false;
      let before: HomeView | null = null, next: HomeView | null = null, failed = false;

      try {
        const home = await takeHome();
        before = state.home === shown ? lastRead : null;
        next = keepUnchanged(before, home);
        fresh = true;
        failed = state.error !== null;
        state.error = null;
      } catch (e) {
        state.error = String(e);
        if (onScreen()) drawHome();
        throw e;
      }

      if (next) {
        lastRead = next;
        state.home = shown = withListedMachines(telemetry ? applyTelemetry(next, telemetry) : next, MACHINES.list);
        keepConnecting(next.machines);
      }

      // A change read off screen draws with the next read on screen.
      undrawn ||= !!next || failed;
      if (!onScreen() || !undrawn) continue;
      undrawn = false;
      drawHome(false);
      homeFresh?.();
      homeFresh = null;

      // The groups load from Home without drawing; the sidebar draws once.
      const projects = !next || next.projects !== before?.projects || next.running !== before?.running;
      const groups = [...(!next || next.machines !== before?.machines ? ["machines"] : []), ...(projects ? ["projects", "pinned"] : [])];
      await Promise.all(groups.map(name => reloadGroup(name, false)));
      if (onScreen()) drawSide();
    } while (again && onScreen());
  })();

  try { await homeRead; }
  finally { homeRead = undefined; if (onScreen() && (!state.home || state.error !== null)) draw(); }
}

async function takeHome(): Promise<HomeView> {
  const pushed = pushedHome;
  pushedHome = null;
  if (!pushed) return invoke<HomeView>("center_home");
  if ("error" in pushed) throw pushed.error;

  return pushed.value;
}

/// Home, the projects page while it shows, and the sidebar unless it draws later.
function drawHome(side = true) {
  draw();
  redrawProjectsPage();
  if (side) drawSide();
}

/// Whether a pushed Home asks something new of the user, or no longer asks it.
function needsChanged(home: HomeView): boolean {
  return JSON.stringify(home.needs_you) !== JSON.stringify(lastRead?.needs_you ?? []);
}

/// Agents, processes and moves change Home many times a second during turns.
/// Home and the sidebar follow at most once a second; what waits on the
/// user shows at once.
const homeChanged = ambient(readHome, AMBIENT_MS);

/// Takes the new sync summary; only projects whose rows changed are drawn and
/// read again.
async function takeSync(next: SyncRow[]) {
  const { rows, changed } = mergeSyncRows(state.sync, next);
  state.sync = rows;
  if (!onScreen()) return;

  if (changed.size) { draw(); drawSide(); }
  await followSync(changed);
}

const refreshSync = single(async () => takeSync(await invoke<SyncRow[]>("project_sync")));

async function takeProjects(top: string[], groups: ProjectGroup[] | null) {
  state.top = top || [];
  state.groups = groups;
  await Promise.all(["projects", "pinned"].map(name => reloadGroup(name, false)));

  if (onScreen()) {
    drawSide();
    redrawProjectsPage();
  }
}

const refreshProjects = single(async () => takeProjects(
  ...await Promise.all([invoke<string[]>("projects_on_top"), invoke<ProjectGroup[] | null>("project_groups")]),
));

function takeMac(mac: MacControl) {
  state.mac = mac;
  pagesAccount(mac);
}

/// Whether a first load ran or runs in this window.
let started = false;

/// Pages call this as they mount. The first load reads every Home domain;
/// after it the topic subscriptions keep them current, so a page draws what
/// the window holds and reads nothing.
export function ensureHome() {
  if (!started) void refresh();
}

/// Explicit refresh and first load reconcile all Home domains.
export async function refresh() {
  clearTimeout(retryTimer);
  started = true;

  try {
    // At load no page shows yet, and Home is the usual first.
    const view = currentLocation()?.view ?? "home";
    const terminals = onScreen() && (view === "home" || view === "terminals");
    // This computer's row in the machines group comes from `mac_control`.
    await Promise.all([readHome(), invoke<MacControl>("mac_control").then(takeMac).then(() => reloadGroup("machines", false)), refreshSync().then(() => onScreen() ? readSync() : undefined), refreshProjects(),
      terminals ? readAllTerms() : undefined]);
  } catch (error) {
    console.warn("Home:", error);
    started = false;
    if (!fresh && onScreen()) retryTimer = setTimeout(refresh, CONNECT_RETRY_MS);
  }
}

// "Still connecting" takes over once the wait passes its limit.
setTimeout(() => { if (!state.home) draw(); }, CONNECT_STUCK_MS);

/// Hidden windows rejoin their subscriptions when shown.
export const onScreen = pageShown;
/// A shown projects page draws again so its live mode and Actions polling start over, since the
/// reader resumes with only the topics that changed.
onShownChange(visible => {
  if (!visible) { clearTimeout(retryTimer); runs.stop(); liveMode.follow(null); return; }

  if (state.home) { redrawProjectsPage(); drawRail(); }
});

// These are state changes. Agent tokens and terminal output have their own topics.
subscribeChanges(topic("home"), { deliver: "each", receive: event => {
  const pushed = pushedView<HomeView>(event.data, "center_home");
  if (!pushed) return;

  pushedHome = pushed;
  homeChanged("value" in pushed && needsChanged(pushed.value));
} });
subscribeChanges(topic("machines"), { deliver: "each", receive: async event => {
  const pushed = pushedView<MacControl>(event.data, "mac_control");
  if (!pushed) return;
  if ("error" in pushed) { console.warn("this computer's state:", pushed.error); return; }

  takeMac(pushed.value);
  await reloadGroup("machines");
} }, { wait: AMBIENT_MS });
subscribeChanges(topic("projects"), { deliver: "each", receive: async event => {
  const top = pushedView<string[]>(event.data, "projects_on_top"), groups = pushedView<ProjectGroup[] | null>(event.data, "project_groups");
  if (!top || !groups) return;
  if ("error" in top) { console.warn("projects on top:", top.error); return; }
  if ("error" in groups) { console.warn("project groups:", groups.error); return; }

  await takeProjects(top.value, groups.value);
} }, { wait: AMBIENT_MS });
// Sync progress changes many times a second during a transfer; the rows follow once a second.
subscribeChanges(topic("sync"), { deliver: "each", receive: async event => {
  const pushed = pushedView<SyncRow[]>(event.data, "project_sync");
  if (!pushed) return;
  if ("error" in pushed) { console.warn("file sync:", pushed.error); return; }

  await takeSync(pushed.value);
} }, { wait: AMBIENT_MS });
subscribeChanges(topic("terminals"), { deliver: "latest", receive: readAllTerms }, { wait: AMBIENT_MS });
subscribeChanges(topic("forge"), { deliver: "latest", receive: async () => {
  if (!snapshotView("git_items")) return;
  await refreshGit();
  if (state.gitError) throw new Error(state.gitError);
} }, { when: () => currentLocation()?.view === "home" });
subscribeChanges(topic("telemetry"), { deliver: "each", receive: event => {
  if (!event.data) return;
  telemetry = event.data as Telemetry;
  takeTraffic(telemetry);
  const kept = state.home === shown;
  if (state.home) state.home = applyTelemetry(state.home, telemetry);
  if (kept) shown = state.home;
  draw();
} }, { when: () => currentLocation()?.view === "home", wait: SLOW_MS });
