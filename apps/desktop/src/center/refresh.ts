// Reading Home: on focus, on notices, every 10 seconds while the window
// shows, and now and then in the background.
import { backgroundTick } from "../ui/pagecache.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { onShownChange, pageShown } from "../ui/active.ts";
import { withoutDismissed } from "./updates.ts";
import type { HomeView } from "./home-state.ts";
import type { MachineTraffic } from "../machines/traffic-state.ts";
import { filterProjects, type ProjectGroup } from "../project/projects-state.ts";
import { preloadModels } from "../agents/agentpicker.ts";
import type { AgentHost } from "../agents/agents.ts";
import { ALL } from "../terminals/terminals.ts";
import { onlineMachines } from "./center.ts";
import { CONNECT_STUCK_MS, CONNECT_RETRY_MS, state, pages, pagesAccount, keepPagesFile } from "./state.ts";
import { cardKey, dismissedUpdates, problem, draw } from "./home.ts";
import { allTerms } from "../terminals/view.ts";
import { refreshGit, single } from "./openitems.ts";
import { shown, drawTop } from "./views.ts";
import { reloadGroup, reloadChats, projectRows, drawSide } from "./sidebar.ts";
import { loadOverview } from "../project/projects.ts";
import { splitOn } from "../project/split.ts";
import { terms, liveMode, tickRepo, projectsEl } from "../project/sections.ts";
import { loadProjectAgent, loadSessions as loadProjectSessions } from "../project/loads.ts";
import { drawProjects } from "../project/page.ts";
import type { MacControl } from "../machines/machine-state.ts";
import type { SyncRow } from "../project/filesync.ts";
import { readSync } from "../project/syncpage.ts";

/// The server answered for Home in this window; until then Home may be
/// the one the app kept from before.
export let fresh = false;

// ---------- Refresh ----------

export let loading = false, again = false, retryTimer: ReturnType<typeof setTimeout> | undefined;

/// Optional machine details refresh independently of Home and its retry loop.
const refreshDetails = single(async () => {
  const [mac, sync, traffic, top, projectGroups] = await Promise.all([
    invoke<MacControl>("mac_control").catch(e => { console.warn("this computer's state:", e); return state.mac; }),
    invoke<SyncRow[]>("project_sync").catch(e => { console.warn("project sync:", e); return state.sync; }),
    invoke<MachineTraffic[]>("machine_traffic").catch(e => { console.warn("machine traffic:", e); return state.traffic; }),
    invoke<string[]>("projects_on_top").catch(e => { console.warn("projects on top:", e); return state.top; }),
    invoke<ProjectGroup[] | null>("project_groups").catch(e => { console.warn("project groups:", e); return state.groups; }),
  ]);
  state.mac = mac;
  pagesAccount(mac);
  state.top = top || [];
  state.groups = projectGroups;
  state.sync = sync;
  state.traffic = traffic;
  await Promise.all(["projects", "pinned"].map(name => reloadGroup(name, false)));
  if (onScreen()) {
    draw();
    drawSide();
    readSync();
    if (shown === "projects") { drawProjects(projectsEl()); drawTop(); }
  }
});

/// Fetches Home. Never two calls at once: a request during one runs once more after it.
export async function refresh() {
  if (loading) { again = true; return; }
  clearTimeout(retryTimer);
  loading = true;
  void refreshDetails();
  try {
    const home = await invoke<HomeView>("center_home");
    if (home.updates) home.updates = withoutDismissed(home.updates, dismissedUpdates);
    const was = onlineMachines(state.home);
    state.home = home;
    fresh = true;
    // A machine came online: its harnesses' models are read ahead of the
    // picker.
    if (onlineMachines(home) !== was) invoke<AgentHost[]>("agent_hosts").then(preloadModels).catch(e => console.warn("agent hosts:", e));
    state.error = null;
    const live = new Set(state.home.needs_you.map(cardKey));
    for (const key of state.outcomes.keys()) if (!live.has(key)) state.outcomes.delete(key);
  } catch (e) {
    state.error = String(e);
  } finally {
    loading = false;
  }
  if (onScreen()) {
    draw();
    readSync();
  }
  // Every terminal's state refreshes with Home, while Home or the
  // Terminals page shows, or before the first list for the sidebar.
  if (state.home?.machines && (onScreen() && (shown === "home" || shown === "terminals") || allTerms.list(ALL) === undefined)) allTerms.load(ALL);
  // A project's agent refreshes with Home: on focus, on notices and every
  // 10 seconds while the window shows; hidden, it waits to show.
  if (shown === "projects" && onScreen()) {
    drawProjects(projectsEl());
    drawTop();
    if (state.picked) {
      loadProjectAgent(false);
      tickRepo(state.picked);
      // Sessions found before Home's scan stay current until they end.
      if (state.sessions.get(state.picked)?.some(s => s.status !== "ended")) void loadProjectSessions(state.picked);
      const p = filterProjects(projectRows() || [], state.picked)[0];
      if (p && splitOn(p) && onScreen()) terms.load(state.picked);
    } else {
      loadOverview();
    }
  }
  // The four groups load together and the sidebar draws once.
  await Promise.all(["projects", "pinned", "machines", "research"].map(name => reloadGroup(name, false)));
  if (onScreen()) drawSide();
  if (again) { again = false; refresh(); }
  // Until Home first loads, a failed try is made again soon, seen or not,
  // unless waiting will not fix it.
  else if (!fresh && !problem()) retryTimer = setTimeout(refresh, CONNECT_RETRY_MS);
}

// "Still connecting" takes over once the wait passes its limit.
setTimeout(() => { if (!state.home) draw(); }, CONNECT_STUCK_MS);

/// Closing the main window only hides it, and it reads now and then in the
/// background.
const BACKGROUND_MS = 4 * 60_000;
let lastBackground = Date.now();
export const onScreen = pageShown;
onShownChange(shown => {
  if (shown) return void refresh();
  lastBackground = Date.now();
  liveMode.follow(null);
});
setInterval(() => {
  const now = Date.now(), last = lastBackground;
  lastBackground = now;
  if (onScreen()) return;
  if (backgroundTick(last, now, BACKGROUND_MS, fresh && !state.error) === "run") refresh();
}, BACKGROUND_MS);

window.addEventListener("focus", refresh);
setInterval(() => { if (onScreen()) refresh(); }, 10000);
// Open PRs and issues: the server keeps them a few minutes, so once a minute
// on Home is plenty.
window.addEventListener("focus", () => { if (shown === "home") refreshGit(); });
setInterval(() => { if (onScreen() && shown === "home") refreshGit(); }, 60000);
// Signed out or removed: the kept project pages go with the account.
listen<string>("link-state", e => { if (e.payload !== "linked") { pages.setAccount(null); keepPagesFile(undefined); } });
for (const name of ["link-state", "sessions-changed", "agent-title-changed", "work-question", "work-resolved", "group-approval", "approval-answered", "reconnected", "home-changed"]) listen(name, () => { if (onScreen()) refresh(); });
for (const name of ["sessions-changed", "group-changed", "reconnected"]) listen(name, () => reloadChats());
