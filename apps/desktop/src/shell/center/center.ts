// The window renders first, so modules that load after it find its elements.
import { ensureMac } from "../../stores/machines/machine-store.ts";
import "../bar/bar-app.tsx";
import "../../parts/center/window-root.tsx";
import { isMobile, onPhone, WINDOWS } from "../../ui/platform.ts";
import { initConversationSwitcher } from "../../parts/session/conversation-switcher.tsx";
// center/: the command center window: sidebar, Home, palette, usage pill and
// the window's shared state (state.ts). Entry: this file (dist/center.js).
import "../../ui/window-defaults.ts";
import "../../ui/zoom.ts";
import { initMobile } from "./mobile.tsx";
import { shell, toggleSide } from "../../parts/center/window-shell.tsx";
import { closeFocusedTab } from "../../parts/session/media.ts";
import { isCloseKey, isZoomKey } from "../../stores/agentpane/media-state.ts";
import { isNewKey } from "../../stores/terminals/newkey-state.ts";
import { allTerms } from "../../parts/center/allterms.ts";
import { invoke } from "../../ui/tauri.ts";
import { initPalette, openPalette } from "./palette.tsx";
import type { ProjectPaletteScope } from "./palette-rank.ts";
import { windowRequests } from "../../stores/bar/core.ts";
import { seed, type CachedViews } from "../../stores/center/cached.ts";
import { savedWindowSession, windowTitle, type Alone, type CenterTarget } from "../../stores/project/projectwindow.ts";
import { ensureSessions } from "../../stores/agents/sessions.ts";
import { sessionVisible } from "../../stores/agents/closing.ts";
import { mountUsage } from "../../screens/center/usage.tsx";
import type { UsageView } from "../../stores/center/usage-state.ts";
import { mountNotices } from "../../screens/center/notices.tsx";
import { pairing } from "../../screens/center/pairing.tsx";
import { windowCommand } from "../../ui/windowkeys.ts";
import { state, pagesAccount, keepPagesFile, listsChanged, sessionsChanged, sessionsDue, sessionsOf } from "../../stores/center/state.ts";
import { draw, homeTab, jump } from "../../screens/center/home.tsx";
import { openDetachedWindow } from "./views.tsx";
import { currentLocation, windowAlone, aloneProject, go, goHistory, hasHistory } from "../../parts/center/nav.ts";
import { drawSide, paneOn, projectHere } from "./sidebar.tsx";
import { openProjects, addProject, openAddMachine } from "../../parts/project/addflow.ts";
import { openProjectWindow } from "../../parts/project/projectops.ts";
import { projectRows } from "../../stores/project/projectrows.ts";
import { projectName } from "../../stores/project/projects-state.ts";
import { projectRef } from "../../stores/project/localid.ts";
import { shortName } from "../../stores/project/agent.ts";
import { loadSessions } from "../../parts/project/sessionsload.ts";
import { terms, thr } from "../../parts/project/stores.ts";
import { sortedPrs } from "../../parts/project/threads.ts";
import { teamNote } from "../../parts/project/team.ts";
import { openProjectHarnesses } from "../../parts/project/project-harnesses.tsx";
import { escUp } from "../../stores/center/escup-state.ts";
import { runningKey } from "../../stores/agents/running-state.ts";
import { itemScroll, setHeroCard, trackpadSwipe, withMove } from "../../ui/motion.ts";
import { pa, projectChanged } from "../../stores/project/barstate.ts";
import { newThreadHere } from "../../parts/session/agents.ts";
import { AG, type AgentRow } from "../../stores/agents/agents-state.ts";
import { debugWithAgent, initPaneView, readPending, showPending } from "./pane.tsx";
import { homeCached, refresh } from "../../parts/center/refresh.ts";
import { openSettings, embedView, unembedView, closeSettings, openMachineSettings } from "../settings/sheet.ts";
import { openChatSkills, openProjectSkills } from "../../screens/project/project-skills.tsx";
import { followSkin, frontLayout } from "../../ui/skin.ts";
import { guardClicks } from "../../ui/clickguard.ts";
import { keepConnecting } from "../../parts/machines/machine-state.ts";
import { followToolUpdates } from "../../screens/machines/machines.tsx";
import { dialogOpen } from "../../ui/parts.tsx";
import { startupDrawn, startupStage } from "../../ui/perf.ts";
import { dropKeptFrame, followHomeFrame } from "../../screens/center/home-frame.ts";

const showShortcuts = () => void import("../../ui/shortcutsheet.tsx").then(m => m.toggleShortcuts());
const showReport = () => void import("../../screens/report/report.tsx").then(m => m.openReport(document.body, debugWithAgent));

let diagramsLoaded = false;
const ensureDiagrams = () => {
  if (diagramsLoaded) return;
  diagramsLoaded = true;
  void import("../../parts/session/diagrams.tsx");
};
document.addEventListener("animationstart", e => { if (e.animationName === "diagram-found") ensureDiagrams(); });
if (document.getElementsByClassName("language-mermaid").length) ensureDiagrams();

startupStage("scripts run");
initPaneView();
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
itemScroll();
// A mouse's back (3) and forward (4) side buttons step like ⌘[ and ⌘]; the webview does not navigate itself.
for (const type of ["mousedown", "mouseup"] as const) document.addEventListener(type, e => {
  if (e.button !== 3 && e.button !== 4) return;
  e.preventDefault();
  if (type === "mouseup" && !dialogOpen()) goHistory(e.button === 4);
}, true);
mountUsage(refresh => invoke<UsageView | null>("usage_get", { refresh }));

function paletteScope(): ProjectPaletteScope<AgentRow> | null {
  const project = aloneProject();
  if (!project) return null;
  const row = projectRows()?.find(r => r.name === project) ?? null;
  const pid = projectRef(project).project_id;
  const prs = thr.prs(pid);
  return {
    project,
    label: row ? projectName(row) : shortName(project),
    imported: row ? row.imported : true,
    board: frontLayout("project") === "board",
    hasTeam: !isMobile() && teamNote(project) !== null,
    hasCheckout: !!row?.machines.length,
    folders: row?.machines ?? [],
    prs: prs ? sortedPrs(prs) : null,
    issues: thr.issues(pid) ?? null,
    terminals: terms.list(project) ?? null,
    sessions: sessionsOf(project)?.filter(sessionVisible) ?? null,
  };
}

function preparePaletteScope(project: string) {
  const row = projectRows()?.find(r => r.name === project) ?? null;
  if (row && !row.imported) return;
  const pid = projectRef(project).project_id;
  thr.ensure(pid);
  if (terms.list(project) === undefined) terms.load(project);
  if (sessionsOf(project) === undefined || sessionsDue.has(project)) void loadSessions(project);
}

function openProjectTerminal() {
  const to = projectHere();
  const project = to?.project ?? aloneProject();
  if (!project) return;
  const machine = to?.machine ?? state.home?.projects?.find(p => p.name === project)?.machine_id ?? null;
  go("pane", { draft: { project, machine, terminal: true } });
}

initPalette({
  go, jump, scope: paletteScope, prepareScope: preparePaletteScope,
  followScope: redraw => {
    projectChanged.follow(redraw);
    listsChanged.follow(redraw);
    sessionsChanged.follow(project => { if (project === null || project === aloneProject()) redraw(); });
  },
  settings: openSettings, providers: () => go("providers"),
  project: openProjects, projects: () => void openProjects(null), terminals: () => void go("terminals"),
  newSession: newAgentHere, newTerminal: openProjectTerminal,
  addProject, joinTeam: onPhone() ? undefined : () => void import("../../screens/project/teamjoin.tsx").then(t => t.openTeamJoin(null)),
  addMachine: openAddMachine, shortcuts: showShortcuts, report: showReport,
  branches: name => { void go("projects", { project: name, tile: "branches" }); },
  openTile: (project, tile) => { void go("projects", { project, tile }); },
  openTerminal: (project, id) => {
    const row = terms.list(project)?.find(t => t.id === id);
    if (row) go("pane", { draft: { project, machine: row.machine_id, terminal: row } });
  },
  projectSkills: openProjectSkills,
  projectHarnesses: openProjectHarnesses,
});

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

function onWindowKey(e: KeyboardEvent) {
  // Keys typed into a terminal are the terminal's, Esc and Tab too, but not ⌘W or ⌘N.
  if (WINDOWS && e.ctrlKey && e.target instanceof Element && e.target.closest(".tm-term")) return;
  if (e.target instanceof Element && e.target.closest(".tm-term") && !isCloseKey(e) && !isNewKey(e)) return;

  // Esc goes up one page: a project's open tile closes, then the Projects page, then Home, then its Overview tab.
  if (e.key === "Escape" && !(currentLocation()?.view === "terminals" && isZoomKey(e)) && !e.defaultPrevented) {
    const up = escUp({ page: currentLocation()?.view ?? null, picked: state.picked, tile: !!pa.tile, alone: !!windowAlone, homeTab: homeTab !== "overview" });

    if (up) {
      e.preventDefault();
      if (up.to === "overview") return jump();
      if (up.to === "tile") return void import("../../screens/project/tiles.tsx").then(m => m.openTile(null));
      return up.to === "home" ? go("home") : void openProjects();
    }
  }

  const command = windowCommand(e, {
    page: currentLocation()?.view ?? null,
    project: state.picked,
    alone: windowAlone !== null,
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
    case "shortcuts": return showShortcuts();
    // A project's or chat's window has its sessions for a sidebar: there is nothing to swap them for.
    case "sidebar": return windowAlone ? undefined : toggleSide();
    case "assistant": return void go("pane", { assistant: "new" });
    case "new-agent":
      if (currentLocation()?.view === "terminals" && isNewKey(e) && allTerms.newLikeOpen()) return;
      return newAgentHere();
  }
}

/// ⌘N: beside a chat's threads (its window, or its rail in place of the sidebar) a new thread;
/// in a project (its page, or one of its sessions) the agent view with an empty conversation
/// and the project's start input, focused; on Home and the Assistant page beside its chats the
/// assistant's start form; elsewhere Projects, to pick the project to start in.
function newAgentHere() {
  const { side, assistant } = shell.getState();
  if (side.rail?.kind === "chat" && !side.railHidden && newThreadHere()) return;
  if (assistant) return void go("pane", { assistant: "new" });
  const to = projectHere();
  if (to) return void go("pane", { draft: { project: to.project, machine: to.machine } });
  if (currentLocation()?.view === "home") return void go("pane", { assistant: "new" });
  go("projects", { project: null });
}
windowRequests.follow(request => {
  switch (request.kind) {
    case "settings": return openSettings();
    case "shortcuts":
      closeSettings();
      return showShortcuts();
    case "report":
      closeSettings();
      return showReport();
    case "new-agent": return newAgentHere();
    case "embed": return embedView(request.name, request.root, request.arg, request.token);
    case "unembed": return unembedView(request.token);
    case "machine-settings": return openMachineSettings(request.id);
    case "chat-skills": return openChatSkills(request.folder, request.title);
    case "open-agent": return;
    case "open-session": return void go("pane", { agent: request.agent });
    case "open-session-link": return;
  }
});

/// What the window opens on: its project or chat, with the chat's row as far as the sessions
/// know it, or for the main window what the bar or tray asked for. Asked at load, beside the
/// kept views and Home's reads.
type Opening = { alone: null; pending: CenterTarget | null }
  | { alone: Alone; saved: ReturnType<typeof savedWindowSession> };

async function readOpening(): Promise<Opening> {
  const alone = await invoke<Alone | null>("center_window_alone").catch(() => null);
  if (!alone) mountNotices();
  document.addEventListener("keydown", onWindowKey);

  if (!alone) return { alone, pending: await readPending() };
  const saved = savedWindowSession();
  if (alone.kind === "chat" || saved) await ensureSessions().catch(() => {});
  return { alone, saved };
}

function start(opening: Opening) {
  dropKeptFrame();
  document.title = windowTitle(opening.alone);
  if (!opening.alone) return showPending(opening.pending, "home");
  openDetachedWindow(opening.alone, opening.saved);
}

/// Shows the views the app kept from its last answers, so the window
/// opens on Home, its projects and sidebar before the mesh answers.
async function showCached() {
  const [cached, mac] = await Promise.all([
    invoke<CachedViews | null>("center_cached").catch(e => { console.warn("kept views:", e); return null; }),
    ensureMac().catch(() => null),
  ]);
  startupStage("kept views read");
  const home = seed(state, cached);
  keepPagesFile(cached?.pages);
  pagesAccount(mac);
  if (!home) return;
  keepConnecting(state.home?.machines ?? null);
  draw();
  homeCached();
  drawSide();
  startupStage("home drawn");
}

// Home's reads start with the window; the first view waits only for the kept views.
const opening = readOpening();
void pairing().then(async () => {
  void refresh();
  const [, open] = await Promise.all([showCached(), opening]);
  if (!onPhone()) void import("../../screens/center/firstrun.tsx").then(m => m.firstRun());
  start(open);
  startupDrawn();
}).catch(e => {
  dropKeptFrame();
  throw e;
});
