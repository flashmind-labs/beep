// The window renders first, so modules that load after it find its elements.
import "./window-root.tsx";
import { onPhone, WINDOWS } from "../ui/platform.ts";
import { initConversationSwitcher } from "../agents/conversation-switcher.tsx";
import "../chat/diagrams.tsx";
// center/: the command center window: sidebar, Home, palette, usage pill and
// the window's shared state (state.ts). Entry: this file (dist/center.js).
import "../ui/window-defaults.ts";
import "../ui/zoom.ts";
import { initMobile } from "./mobile.tsx";
import { closeFocusedTab } from "../agentpane/media.ts";
import { isCloseKey, isZoomKey } from "../agentpane/media-state.ts";
import { isNewKey } from "../terminals/newkey-state.ts";
import { allTerms } from "../terminals/view.tsx";
import { invoke } from "../ui/tauri.ts";
import { initPalette, openPalette } from "./palette.tsx";
import { pane } from "../bar/pane.ts";
import type { BackTo } from "../bar/core.ts";
import { seed, type CachedViews } from "./cached.ts";
import { startView, windowTitle, type CenterTarget } from "../project/projectwindow.ts";
import { newChatTarget } from "../project/agent.ts";
import { mountUsage, type UsageView } from "./usage.tsx";
import { mountNotices } from "./notices.tsx";
import { firstRun } from "./firstrun.tsx";
import { pairing } from "./pairing.tsx";
import { windowCommand } from "../ui/windowkeys.ts";
import { state, pagesAccount, keepPagesFile } from "./state.ts";
import { draw, homeTab, jump } from "./home.tsx";
import { currentLocation, windowProject, go, goHistory, hasHistory, setWindowProject, goTo } from "./views.tsx";
import { reloadGroup, paneOn, openProjects, openProjectWindow, addProject } from "./sidebar.tsx";
import { escUp } from "./escup-state.ts";
import { setHeroCard, trackpadSwipe, withMove } from "../ui/motion.ts";
import { openTile } from "../project/split.tsx";
import { pa } from "../project/sections.ts";
import { AG } from "../agents/agents.ts";
import { debugWithAgent, readPending, showPending } from "./pane.tsx";
import { homeCached, refresh } from "./refresh.ts";
import { openSettings, embedView, closeSettings } from "../settings/sheet.ts";
import { followSkin } from "../ui/skin.ts";
import { openReport } from "../report/report.tsx";
import { toggleShortcuts } from "../ui/shortcutsheet.tsx";
import { guardClicks } from "../ui/clickguard.ts";
import { keepConnecting, type MacControl } from "../machines/machine-state.ts";
import { projectOf, repoOf, runningKey } from "../agents/running-state.ts";
import { placeOf } from "../agents/place.ts";
import { followToolUpdates } from "../machines/machines.tsx";
import { dialogOpen } from "../ui/parts.tsx";
import { startupDrawn, startupStage } from "../ui/perf.ts";
import { dropKeptFrame, followHomeFrame } from "./home-frame.ts";

startupStage("scripts run");
followSkin();
followHomeFrame();
guardClicks();
followToolUpdates();
initConversationSwitcher({
  current: () => currentLocation()?.view === "pane" && paneOn().view === "agent" && !AG.draft ? AG.page?.session ?? null : null,
  rows: () => (state.home?.running ?? []).filter(row => row.kind === "agent"),
  // The next conversation slides in, and Back from it returns to that session's card.
  open: agent => { setHeroCard(runningKey(agent)); withMove("next", () => { go("pane", { agent }); }); },
});

// ---------- Keys ----------

initMobile();
trackpadSwipe({ has: to => !dialogOpen() && hasHistory(to === "forward"), go: to => { goHistory(to === "forward"); } });
mountUsage(refresh => invoke<UsageView | null>("usage_get", { refresh }));
mountNotices();

initPalette({ go, jump, scope: () => windowProject, settings: openSettings, providers: () => go("providers"), project: openProjects, addProject, branches: name => { if (openProjects(name)) openTile("branches"); } });

// Capture before the terminal handles the key, so Shift+Esc never reaches the shell.
// A development build reloads the window on Cmd+R, as a browser does.
if (process.env.NODE_ENV === "development") document.addEventListener("keydown", e => {
  if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key === "r") { e.preventDefault(); location.reload(); }
});

document.addEventListener("keydown", e => {
  if (currentLocation()?.view !== "terminals" || !isZoomKey(e) || e.isComposing || e.defaultPrevented || dialogOpen()) return;
  if (!allTerms.focusOpen()) return;
  e.preventDefault();
  e.stopPropagation();
}, true);

document.addEventListener("keydown", e => {
  // Keys typed into a terminal are the terminal's, Esc and Tab too, but not ⌘W or ⌘N.
  if (WINDOWS && e.ctrlKey && e.target instanceof Element && e.target.closest(".tm-term")) return;
  if (e.target instanceof Element && e.target.closest(".tm-term") && !isCloseKey(e) && !isNewKey(e)) return;

  // Esc goes up one page: a project's open tile closes, then the Projects page, then Home, then its Overview tab.
  if (e.key === "Escape" && !(currentLocation()?.view === "terminals" && isZoomKey(e)) && !e.defaultPrevented) {
    const up = escUp({ page: currentLocation()?.view ?? null, picked: state.picked, tile: !!pa.tile, alone: !!windowProject, homeTab: homeTab !== "overview" });

    if (up) {
      e.preventDefault();
      if (up.to === "overview") return jump();
      if (up.to === "tile") return openTile(null);
      return up.to === "home" ? go("home") : void openProjects();
    }
  }

  const command = windowCommand(e, {
    page: currentLocation()?.view ?? null,
    project: state.picked,
    projectWindow: windowProject !== null,
  });
  if (!command) return;

  // A project terminal only takes the key when a project can provide its folder.
  if (command.kind === "project-terminal") {
    const to = projectHere();
    if (to) { e.preventDefault(); go("pane", { draft: { project: to.project, machine: to.machine, terminal: true } }); }
    return;
  }

  e.preventDefault();

  switch (command.kind) {
    case "history": return goHistory(command.forward);
    case "close":
      if (!closeFocusedTab()) invoke("center_close_window").catch(() => window.close());
      return;
    case "project-window": return openProjectWindow(command.project);
    case "palette": return openPalette();
    case "settings": return openSettings();
    case "shortcuts": return toggleShortcuts();
    case "new-agent":
      if (currentLocation()?.view === "terminals" && isNewKey(e) && allTerms.newLikeOpen()) return;
      return newAgentHere();
  }
});

/// The project on screen, its page or a session of it, and the machine
/// its start input opens on; null elsewhere.
function projectHere() {
  const on = paneOn();
  if (on.view === "agent" && AG.draft) return { project: AG.draft, machine: null };
  // The session on screen, which the pane may have moved to since the window opened it.
  const page = currentLocation();
  const target = page?.view === "pane" ? page.arg : null;
  const open = AG.page?.session ?? (target && typeof target === "object" && "agent" in target ? target.agent : null);
  const agent = on.view === "agent" && open ? { machine_id: open.machine_id, cwd: open.cwd, main: placeOf(open)?.main } : null;
  return newChatTarget({ shown: currentLocation()?.view ?? null, picked: state.picked, view: on.view, agent, mac: state.mac?.id }, state.home?.projects);
}

/// ⌘N: in a project (its page, or one of its sessions) the agent
/// view with an empty conversation and the project's start input, focused;
/// on Home the assistant's start form; elsewhere Projects, to pick the
/// project to start in.
function newAgentHere() {
  const to = projectHere();
  if (to) return void go("pane", { draft: { project: to.project, machine: to.machine } });
  if (currentLocation()?.view === "home") return void go("pane", { agentnew: { at: null, assistant: true } });
  go("projects", { project: null });
}
pane.connect({
  openSettings,
  openShortcuts() {
    closeSettings();
    toggleShortcuts();
  },
  openReport() {
    closeSettings();
    openReport(document.body, debugWithAgent);
  },
  embedView,
  newAgent: newAgentHere,
  // The session shrinks back into its card on the board.
  goBack(from: BackTo, move: boolean) {
    if (!move) return void goTo(from);
    const session = AG.page?.session;
    if (session) setHeroCard(runningKey(session));
    withMove("back", () => { goTo(from); });
  },
  sessionDestinations(s) {
    const projects = state.home?.projects ?? [];
    const current = projectOf(s, projects);
    return projects
      .filter(p => p.machine_id === s.machine_id && p.name !== current?.name && p.folder !== s.cwd)
      .map(p => ({ name: p.name, folder: p.folder }));
  },
  sessionProject(s) {
    const projects = state.home?.projects ?? [], p = projectOf(s, projects);
    return p ? { label: repoOf(s, projects) || p.name, name: p.name, open: () => void openProjects(p.name) } : null;
  },
});

/// What the window opens on: its project, or for the main window what the bar or tray asked
/// for. Asked at load, beside the kept views and Home's reads.
interface Opening {
  project: string | null;
  pending: CenterTarget | null;
}

async function readOpening(): Promise<Opening> {
  const project = await invoke<string | null>("center_window_project").catch(() => null);
  return { project, pending: project ? null : await readPending() };
}

/// The first view.
function start({ project, pending }: Opening) {
  dropKeptFrame();
  document.title = windowTitle(project);
  const first = startView(project);
  if (!first) return showPending(pending, "home");
  // A project window: no sidebar, and a button back to the project.
  const name = first.arg.project;
  setWindowProject(name);
  document.body.classList.add("pw");
  go(first.view, first.arg);
}

/// Shows the views the app kept from its last answers, so the window
/// opens on Home, its projects and sidebar before the mesh answers.
async function showCached() {
  const [cached, mac] = await Promise.all([
    invoke<CachedViews | null>("center_cached").catch(e => { console.warn("kept views:", e); return null; }),
    invoke<MacControl>("mac_control").catch(() => null),
  ]);
  startupStage("kept views read");
  const home = seed(state, cached);
  state.mac ??= mac;
  keepPagesFile(cached?.pages);
  pagesAccount(mac);
  if (!home) return;
  keepConnecting(state.home?.machines ?? null);
  draw();
  homeCached();
  reloadGroup("projects");
  reloadGroup("pinned");
  reloadGroup("machines");
  startupStage("home drawn");
}

// Home's reads start with the window; the first view waits only for the kept views.
const opening = readOpening();
void pairing().then(async () => {
  void refresh();
  const [, open] = await Promise.all([showCached(), opening]);
  if (!onPhone()) void firstRun();
  start(open);
  startupDrawn();
}).catch(e => {
  dropKeptFrame();
  throw e;
});
