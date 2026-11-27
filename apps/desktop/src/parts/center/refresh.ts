import { subscribeChanges, topic } from "../../ui/mesh-events.ts";
import { pushedView, type Pushed } from "../../ui/snapshot-state.ts";
// Home reads changed domains; telemetry arrives from the native cache.
import { invoke } from "../../ui/tauri.ts";
import { ambient, AMBIENT_MS, onShownChange, pageShown, single, SLOW_MS } from "../../ui/active.ts";
import { applyTelemetry, keepUnchanged, type HomeView, type Telemetry } from "../../stores/center/home-state.ts";
import { type ProjectGroup } from "../../stores/project/projects-state.ts";
import { CONNECT_STUCK_MS, state, heldHome, pagesAccount, homeDataChanged, listsChanged, projectLoaded, sessionsChanged } from "../../stores/center/state.ts";
import { readAllTerms } from "./allterms.ts";
import { takeGit } from "./openitems.ts";
import type { GitItemsView } from "../../stores/center/openitems-state.ts";
import { currentLocation } from "./nav.ts";
import { liveMode, runs } from "../project/sections.ts";
import { ensureMac, MACHINES, machinesChanged, takeTraffic } from "../../stores/machines/machine-store.ts";
import { keepConnecting } from "../machines/machine-state.ts";
import { mergeSyncRows, type SyncRow } from "../project/filesync.ts";
import { readSync } from "../project/syncpage.ts";
import { read } from "../../stores/center/homeedit.ts";
import { perfFresh } from "../../ui/perf.ts";

/// The mesh answered for Home in this window; until then Home may be
/// the one the app kept from before.
let fresh = false;

// ---------- Refresh ----------

let again = false;

let telemetry: Telemetry | undefined;
/// The newest Home the `home` push carried and no read took yet.
let pushedHome: Pushed<HomeView> | null = null;
let homeRead: Promise<void> | undefined;
let undrawn = false;
/// Times Home from its kept draw to the first fresh one.
let homeFresh: (() => void) | null = null;

/// Home drew what the app kept; called before any fresh read drew.
export function homeCached() {
  if (!fresh) homeFresh = perfFresh("home");
}

/// The Home held for what it read, with telemetry. `state.home` names the cards from the machine
/// list and drops a machine removed here whenever it is read.
function shownHome(got: HomeView): HomeView {
  return telemetry ? applyTelemetry(got, telemetry) : got;
}

// Home's cards take their names from the machine list as they draw.
machinesChanged.add(() => {
  pagesAccount(MACHINES.mac);
  if (onScreen()) { homeDataChanged.notify(); listsChanged.notify(); }
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
        before = heldHome() === read.shown ? read.last : null;
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
        read.last = next;
        state.home = read.shown = shownHome(next);
        keepConnecting(next.machines);
      }

      // A change read off screen draws with the next read on screen.
      undrawn ||= !!next || failed;
      if (!onScreen() || !undrawn) continue;
      undrawn = false;
      drawHome(false);
      homeFresh?.();
      homeFresh = null;

      if (onScreen()) listsChanged.notify();
    } while (again && onScreen());
  })();

  try { await homeRead; }
  finally { homeRead = undefined; if (onScreen() && (!state.home || state.error !== null)) homeDataChanged.notify(); }
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
  homeDataChanged.notify();
  projectLoaded.notify();
  if (side) listsChanged.notify();
}

/// Whether a pushed Home asks something new of the user, or no longer asks it.
function needsChanged(home: HomeView): boolean {
  return JSON.stringify(home.needs_you) !== JSON.stringify(read.last?.needs_you ?? []);
}

/// Whether the set of running session ids changed: starts, handoffs and closes
/// show at once rather than waiting behind turn-progress throttling.
function runningIdsChanged(home: HomeView): boolean {
  const ids = (list: HomeView["running"]) => (list ?? []).map(r => `${r.machine_id} ${r.id}`).sort().join("\n");
  return ids(home.running) !== ids(read.last?.running ?? null);
}

/// Agents, processes and moves change Home many times a second during turns.
/// Home and the sidebar follow at most once a second; what waits on the
/// user shows at once.
const homeChanged = ambient(readHome, AMBIENT_MS);

/// Takes the new sync summary; only projects whose rows changed are drawn.
async function takeSync(next: SyncRow[]) {
  const { rows, changed } = mergeSyncRows(state.sync, next);
  state.sync = rows;
  if (!onScreen()) return;

  if (changed.size) { homeDataChanged.notify(); listsChanged.notify(); }
}

const refreshSync = single(async () => takeSync(await invoke<SyncRow[]>("project_sync")));

async function takeProjects(top: string[], groups: ProjectGroup[] | null) {
  state.top = top || [];
  state.groups = groups;

  if (onScreen()) {
    listsChanged.notify();
    projectLoaded.notify();
  }
}

const refreshProjects = single(async () => takeProjects(
  ...await Promise.all([invoke<string[]>("projects_on_top"), invoke<ProjectGroup[] | null>("project_groups")]),
));

/// Whether a first load ran or runs in this window.
let started = false;
/// Whether a load runs now, and whether Home was pushed while it did.
let loading = false;
let pushedDuring = false;
let retry: ReturnType<typeof setTimeout> | undefined;
/// How long a failed first load waits before it reads again, while the window shows.
const RETRY_MS = 5000;

/// Pages call this as they mount. The first load reads every Home domain;
/// after it the topic subscriptions keep them current, so a page draws what
/// the window holds and reads nothing.
export function ensureHome() {
  if (!started) void refresh();
}

/// Explicit refresh and first load reconcile all Home domains.
export async function refresh() {
  started = true;
  loading = true;
  pushedDuring = false;
  clearTimeout(retry);
  let failed = false;

  try {
    // At load no page shows yet, and Home is the usual first.
    const view = currentLocation()?.view ?? "home";
    const terminals = onScreen() && (view === "home" || view === "terminals");
    // This computer's row in the machines group comes from `mac_control`.
    await Promise.all([readHome(), ensureMac(), refreshSync().then(() => onScreen() ? readSync() : undefined), refreshProjects(),
      terminals ? readAllTerms() : undefined]);
  } catch (error) {
    console.warn("Home:", error);
    failed = true;
  }

  loading = false;
  if (!failed) return;

  // A push that came meanwhile found a load running and read nothing: load again now. Else
  // the next push loads everything again, or this does while the window shows.
  started = false;
  if (pushedDuring) void refresh();
  else retry = setTimeout(() => { if (!started && onScreen()) void refresh(); }, RETRY_MS);
}

// "Still connecting" takes over once the wait passes its limit.
setTimeout(() => { if (!state.home) homeDataChanged.notify(); }, CONNECT_STUCK_MS);

/// Hidden windows rejoin their subscriptions when shown.
export const onScreen = pageShown;
/// A shown projects page draws again so its live mode and Actions polling start over, since the
/// reader resumes with only the topics that changed.
onShownChange(visible => {
  if (!visible) { runs.stop(); liveMode.follow(null); return; }

  if (state.home) { projectLoaded.notify(); sessionsChanged.notify(null); }
});

// These are state changes. Agent tokens and terminal output have their own topics.
subscribeChanges(topic("home"), { deliver: "each", receive: event => {
  const pushed = pushedView<HomeView>(event.data, "center_home");
  if (!pushed) return;

  pushedHome = pushed;
  if (!started) { void refresh(); return; }
  if (loading) pushedDuring = true;
  homeChanged("value" in pushed && (needsChanged(pushed.value) || runningIdsChanged(pushed.value)));
} });
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
subscribeChanges(topic("forge"), { deliver: "each", receive: event => {
  const pushed = pushedView<GitItemsView | null>(event.data, "git_items");
  if (!pushed) return;
  if ("error" in pushed) console.warn("open PRs and issues:", pushed.error);

  takeGit(pushed);
} }, { when: () => currentLocation()?.view === "home" });
subscribeChanges(topic("telemetry"), { deliver: "each", receive: event => {
  if (!event.data) return;
  telemetry = event.data as Telemetry;
  takeTraffic(telemetry);
  const held = heldHome();
  if (held) { state.home = applyTelemetry(held, telemetry); if (held === read.shown) read.shown = heldHome(); }
  homeDataChanged.notify();
} }, { when: () => currentLocation()?.view === "home", wait: SLOW_MS });
