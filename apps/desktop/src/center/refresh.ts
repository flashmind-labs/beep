import { subscribeChanges as subscribeTopics, topic, type MeshEvent } from "../ui/mesh-events.ts";
import { snapshotView } from "../ui/snapshot-state.ts";
// Home reads changed domains; telemetry arrives from the native cache.
import { invoke, listen } from "../ui/tauri.ts";
import { AMBIENT_MS, SLOW_MS, ambient, onShownChange, pageShown } from "../ui/active.ts";
import { withoutDismissed } from "./updates.ts";
import { applyTelemetry, keepUnchanged, type HomeView, type Telemetry } from "./home-state.ts";
import { type ProjectGroup } from "../project/projects-state.ts";
import { preloadModels } from "../agents/agentpicker.ts";
import type { AgentHost } from "../agents/agents.ts";
import { onlineMachines } from "./center.ts";
import { CONNECT_STUCK_MS, CONNECT_RETRY_MS, state, pages, pagesAccount, keepPagesFile } from "./state.ts";
import { cardKey, dismissedUpdates, problem, draw } from "./home.ts";
import { readAllTerms } from "../terminals/view.ts";
import { refreshGit, single } from "./openitems.ts";
import { currentLocation, drawTop } from "./views.ts";
import { reloadGroup, reloadChats, drawSide } from "./sidebar.ts";
import { loadOverview } from "../project/projects.ts";
import { liveMode, projectsEl, runs } from "../project/sections.ts";
import { drawProjects } from "../project/page.ts";
import type { MacControl } from "../machines/machine-state.ts";
import { mergeSyncRows, type SyncRow } from "../project/filesync.ts";
import { followSync, readSync } from "../project/syncpage.ts";

/// The server answered for Home in this window; until then Home may be
/// the one the app kept from before.
export let fresh = false;

// ---------- Refresh ----------

export let loading = false, again = false, retryTimer: ReturnType<typeof setTimeout> | undefined;

let telemetry: Telemetry | undefined;
let homeRead: Promise<void> | undefined;
/// The last answer, before telemetry, and the Home it made: a Home put in
/// place elsewhere is compared whole.
let lastRead: HomeView | null = null;
let shown: HomeView | null = null;
let undrawn = false;

/// Reads membership, running sessions and Home data without refreshing unrelated panes.
/// Sections that did not change keep their objects; an unchanged Home draws nothing.
async function readHome(): Promise<void> {
  if (homeRead) { again = true; return homeRead; }
  loading = true;
  homeRead = (async () => {
    do {
      again = false;
      let before: HomeView | null = null, next: HomeView | null = null, failed = false;

      try {
        const home = await invoke<HomeView>("center_home");
        if (home.updates) home.updates = withoutDismissed(home.updates, dismissedUpdates);
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
        const was = onlineMachines(state.home);
        lastRead = next;
        state.home = shown = telemetry ? applyTelemetry(next, telemetry) : next;
        if (onlineMachines(next) !== was) invoke<AgentHost[]>("agent_hosts").then(preloadModels).catch(e => console.warn("agent hosts:", e));
        const live = new Set(next.needs_you.map(cardKey));
        for (const key of state.outcomes.keys()) if (!live.has(key)) state.outcomes.delete(key);
      }

      // A change read off screen draws with the next read on screen.
      undrawn ||= !!next || failed;
      if (!onScreen() || !undrawn) continue;
      undrawn = false;
      drawHome(false);

      // The groups load from Home without drawing; the sidebar draws once.
      const projects = !next || next.projects !== before?.projects || next.running !== before?.running;
      const groups = [...(!next || next.machines !== before?.machines ? ["machines"] : []), ...(projects ? ["projects", "pinned"] : [])];
      await Promise.all(groups.map(name => reloadGroup(name, false)));
      if (onScreen()) drawSide();
    } while (again && onScreen());
  })();

  try { await homeRead; }
  finally { homeRead = undefined; loading = false; if (onScreen() && (!state.home || state.error !== null)) draw(); }
}

/// Home, the projects page while it shows, and the sidebar unless it draws later.
function drawHome(side = true) {
  draw();
  if (currentLocation()?.view === "projects") { drawProjects(projectsEl()); drawTop(); }
  if (side) drawSide();
}

/// Whether a Home snapshot that came with a change asks something new of the
/// user, or no longer asks it.
function needsChanged(event: MeshEvent): boolean {
  const data = event.data;
  if (!data || typeof data !== "object" || !("views" in data) || !Array.isArray(data.views)) return false;
  const view: unknown = data.views.find((v: unknown) => v !== null && typeof v === "object" && "command" in v && v.command === "center_home");
  if (!view || typeof view !== "object" || !("value" in view) || !view.value || typeof view.value !== "object" || !("needs_you" in view.value)) return false;
  return JSON.stringify(view.value.needs_you) !== JSON.stringify(lastRead?.needs_you ?? []);
}

/// Agents, processes and moves change Home many times a second during turns.
/// Home and the sidebar follow at most once a second; what waits on the
/// user shows at once.
const homeChanged = ambient(readHome, AMBIENT_MS);

/// Takes the new sync summary; only projects whose rows changed are drawn and
/// read again.
const refreshSync = single(async () => {
  const { rows, changed } = mergeSyncRows(state.sync, await invoke<SyncRow[]>("project_sync"));
  state.sync = rows;
  if (!onScreen()) return;

  if (changed.size) { draw(); drawSide(); }
  await followSync(changed);
});

const refreshProjects = single(async () => {
  const [top, groups] = await Promise.all([
    invoke<string[]>("projects_on_top"),
    invoke<ProjectGroup[] | null>("project_groups"),
  ]);
  state.top = top || [];
  state.groups = groups;
  await Promise.all(["projects", "pinned"].map(name => reloadGroup(name, false)));

  if (onScreen()) {
    drawSide();
    if (currentLocation()?.view === "projects") { drawProjects(projectsEl()); drawTop(); }
  }
});

const refreshMac = single(async () => {
  state.mac = await invoke<MacControl>("mac_control");
  pagesAccount(state.mac);
});

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
    const terminals = onScreen() && (currentLocation()?.view === "home" || currentLocation()?.view === "terminals");
    await Promise.all([readHome(), refreshMac(), refreshSync().then(() => onScreen() ? readSync() : undefined), refreshProjects(),
      ...["machines", "research"].map(name => reloadGroup(name, false)), terminals ? readAllTerms() : undefined]);
    if (currentLocation()?.view === "projects" && !state.picked && onScreen()) loadOverview();
  } catch (error) {
    console.warn("Home:", error);
    started = false;
    if (!fresh && !problem() && onScreen()) retryTimer = setTimeout(refresh, CONNECT_RETRY_MS);
  }
}

// "Still connecting" takes over once the wait passes its limit.
setTimeout(() => { if (!state.home) draw(); }, CONNECT_STUCK_MS);

/// Hidden windows rejoin their subscriptions when shown.
export const onScreen = pageShown;
onShownChange(visible => {
  if (!visible) { clearTimeout(retryTimer); runs.stop(); liveMode.follow(null); }
});

// These are state changes. Agent tokens and terminal output have their own topics.
subscribeTopics(topic("home"), event => homeChanged(needsChanged(event)));
for (const name of ["work-question", "work-resolved", "group-approval", "approval-answered"]) listen(name, () => homeChanged(true));
subscribeTopics(topic("machines"), async () => { await refreshMac(); await reloadGroup("machines"); }, null, undefined, AMBIENT_MS);
subscribeTopics(topic("projects"), refreshProjects, null, undefined, AMBIENT_MS);
subscribeTopics(topic("sync"), refreshSync);
subscribeTopics(topic("terminals"), readAllTerms, null, undefined, AMBIENT_MS);
subscribeTopics(topic("forge"), async () => {
  if (!snapshotView("git_items")) return;
  await refreshGit();
  if (state.gitError) throw new Error(state.gitError);
}, null, () => currentLocation()?.view === "home");
subscribeTopics(topic("telemetry"), event => {
  if (!event.data) return;
  telemetry = event.data as Telemetry;
  state.traffic = telemetry.traffic;
  const kept = state.home === shown;
  if (state.home) state.home = applyTelemetry(state.home, telemetry);
  if (kept) shown = state.home;
  draw();
}, null, () => currentLocation()?.view === "home", SLOW_MS);
// Signed out or removed: the kept project pages go with the account.
listen<string>("link-state", e => { if (e.payload !== "linked") { pages.setAccount(null); keepPagesFile(undefined); } });
// The chat rows follow other devices at most once a second.
const chatsChanged = ambient(reloadChats, AMBIENT_MS);
for (const name of ["sessions-changed", "group-changed", "reconnected"]) listen(name, () => chatsChanged());
