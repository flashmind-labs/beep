import { subscribeChanges as subscribeTopics, topic } from "../ui/mesh-events.ts";
import { snapshotView } from "../ui/snapshot-state.ts";
// Home reads changed domains; telemetry arrives from the native cache.
import { invoke, listen } from "../ui/tauri.ts";
import { onShownChange, pageShown } from "../ui/active.ts";
import { withoutDismissed } from "./updates.ts";
import { applyTelemetry, type HomeView, type Telemetry } from "./home-state.ts";
import { type ProjectGroup } from "../project/projects-state.ts";
import { preloadModels } from "../agents/agentpicker.ts";
import type { AgentHost } from "../agents/agents.ts";
import { ALL } from "../terminals/terminals.ts";
import { onlineMachines } from "./center.ts";
import { CONNECT_STUCK_MS, CONNECT_RETRY_MS, state, pages, pagesAccount, keepPagesFile } from "./state.ts";
import { cardKey, dismissedUpdates, problem, draw } from "./home.ts";
import { allTerms } from "../terminals/view.ts";
import { refreshGit, single } from "./openitems.ts";
import { shown, drawTop } from "./views.ts";
import { reloadGroup, reloadChats, drawSide } from "./sidebar.ts";
import { loadOverview } from "../project/projects.ts";
import { liveMode, projectsEl, runs } from "../project/sections.ts";
import { drawProjects } from "../project/page.ts";
import type { MacControl } from "../machines/machine-state.ts";
import type { SyncRow } from "../project/filesync.ts";
import { readSync } from "../project/syncpage.ts";

/// The server answered for Home in this window; until then Home may be
/// the one the app kept from before.
export let fresh = false;

// ---------- Refresh ----------

export let loading = false, again = false, retryTimer: ReturnType<typeof setTimeout> | undefined;

let telemetry: Telemetry | undefined;
let homeRead: Promise<void> | undefined;

/// Reads membership, running sessions and Home data without refreshing unrelated panes.
async function readHome(): Promise<void> {
  if (homeRead) { again = true; return homeRead; }
  loading = true;
  homeRead = (async () => {
    do {
      again = false;
      try {
        const home = await invoke<HomeView>("center_home");
        if (home.updates) home.updates = withoutDismissed(home.updates, dismissedUpdates);
        const was = onlineMachines(state.home);
        state.home = telemetry ? applyTelemetry(home, telemetry) : home;
        fresh = true;
        if (onlineMachines(home) !== was) invoke<AgentHost[]>("agent_hosts").then(preloadModels).catch(e => console.warn("agent hosts:", e));
        state.error = null;
        const live = new Set(home.needs_you.map(cardKey));
        for (const key of state.outcomes.keys()) if (!live.has(key)) state.outcomes.delete(key);
      } catch (e) {
        state.error = String(e);
        throw e;
      } finally {
        if (onScreen()) {
          draw();
          if (shown === "projects") { drawProjects(projectsEl()); drawTop(); }
          drawSide();
        }
      }
    } while (again && onScreen());
  })();
  try { await homeRead; }
  finally { homeRead = undefined; loading = false; }
}

const refreshSync = single(async () => {
  state.sync = await invoke<SyncRow[]>("project_sync");
  if (onScreen()) { draw(); drawSide(); await readSync(true); }
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
    if (shown === "projects") { drawProjects(projectsEl()); drawTop(); }
  }
});

const refreshMac = single(async () => {
  state.mac = await invoke<MacControl>("mac_control");
  pagesAccount(state.mac);
});

/// Explicit refresh and first load reconcile all Home domains.
export async function refresh() {
  clearTimeout(retryTimer);
  try {
    await Promise.all([readHome(), refreshMac(), refreshSync(), refreshProjects(),
      ...["machines", "research"].map(name => reloadGroup(name, false))]);
    if (onScreen() && (shown === "home" || shown === "terminals")) await allTerms.load(ALL);
    if (shown === "projects" && !state.picked && onScreen()) loadOverview();
  } catch (error) {
    console.warn("Home:", error);
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
subscribeTopics(topic("home"), readHome);
subscribeTopics(topic("machines"), async () => { await refreshMac(); await reloadGroup("machines"); });
subscribeTopics(topic("projects"), refreshProjects);
subscribeTopics(topic("sync"), refreshSync);
subscribeTopics(topic("terminals"), () => allTerms.load(ALL));
subscribeTopics(topic("forge"), async () => {
  if (!snapshotView("git_items")) return;
  await refreshGit();
  if (state.gitError) throw new Error(state.gitError);
}, null, () => shown === "home");
subscribeTopics(topic("telemetry"), event => {
  if (!event.data) return;
  telemetry = event.data as Telemetry;
  state.traffic = telemetry.traffic;
  if (state.home) state.home = applyTelemetry(state.home, telemetry);
  if (shown === "home") draw();
}, null, () => shown === "home");
// Signed out or removed: the kept project pages go with the account.
listen<string>("link-state", e => { if (e.payload !== "linked") { pages.setAccount(null); keepPagesFile(undefined); } });
for (const name of ["sessions-changed", "group-changed", "reconnected"]) listen(name, () => reloadChats());
