// The window renders first, so modules that load after it find its elements.
import { ensureMac } from "../machines/machine-store.ts";
import "../bar/bar-app.tsx";
import "./window-root.tsx";
import { isMobile, onPhone, WINDOWS } from "../ui/platform.ts";
import { initConversationSwitcher } from "../agents/conversation-switcher.tsx";
// center/: the command center window: sidebar, Home, palette, usage pill and
// the window's shared state (state.ts). Entry: this file (dist/center.js).
import "../ui/window-defaults.ts";
import "../ui/zoom.ts";
import { initMobile } from "./mobile.tsx";
import { shell, stepOutOfRail, toggleSide } from "./window-shell.tsx";
import { closeFocusedTab } from "../agentpane/media.ts";
import { isCloseKey, isZoomKey } from "../agentpane/media-state.ts";
import { isNewKey } from "../terminals/newkey-state.ts";
import { allTerms } from "./allterms.ts";
import { invoke } from "../ui/tauri.ts";
import { initPalette, openPalette } from "./palette.tsx";
import { type BackTo, S } from "../bar/core.ts";
import { seed, type CachedViews } from "./cached.ts";
import { savedWindowSession, windowTitle, type Alone, type CenterTarget } from "../project/projectwindow.ts";
import { ensureSessions } from "../agents/sessions.ts";
import { mountUsage } from "./usage.tsx";
import type { UsageView } from "./usage-state.ts";
import { mountNotices } from "./notices.tsx";
import { pairing } from "./pairing.tsx";
import { windowCommand } from "../ui/windowkeys.ts";
import { state, pagesAccount, keepPagesFile } from "./state.ts";
import { draw, homeTab, jump } from "./home.tsx";
import { openDetachedWindow } from "./views.tsx";
import { asBack, currentLocation, windowAlone, aloneProject, go, goHistory, hasHistory, goTo } from "./nav.ts";
import { drawSide, paneOn, projectHere } from "./sidebar.tsx";
import { openProjects, addProject, openAddMachine } from "../project/addflow.ts";
import { projectWindowProps, openProjectWindow } from "../project/projectops.ts";
import { escUp } from "./escup-state.ts";
import { nextRunning } from "../project/loads.tsx";
import { itemScroll, setHeroCard, trackpadSwipe, withMove } from "../ui/motion.ts";
import { pa } from "../project/barstate.ts";
import { newThreadHere } from "../agents/agents.ts";
import { AG } from "../agents/agents-state.ts";
import { ASSISTANT_CHATS } from "../agents/assistant-page.tsx";
import { pane } from "../bar/pane.ts";
import { debugWithAgent, initPaneView, readPending, showPending } from "./pane.tsx";
import { homeCached, refresh } from "./refresh.ts";
import { openSettings, embedView, closeSettings } from "../settings/sheet.ts";
import { followSkin } from "../ui/skin.ts";
import { guardClicks } from "../ui/clickguard.ts";
import { keepConnecting } from "../machines/machine-state.ts";
import { projectOf, repoOf, runningKey } from "../agents/running-state.ts";
import { followToolUpdates } from "../machines/machines.tsx";
import { dialogOpen } from "../ui/parts.tsx";
import { startupDrawn, startupStage } from "../ui/perf.ts";
import { dropKeptFrame, followHomeFrame } from "./home-frame.ts";

const showShortcuts = () => void import("../ui/shortcutsheet.tsx").then(m => m.toggleShortcuts());
const showReport = () => void import("../report/report.tsx").then(m => m.openReport(document.body, debugWithAgent));

let diagramsLoaded = false;
const ensureDiagrams = () => {
  if (diagramsLoaded) return;
  diagramsLoaded = true;
  void import("../chat/diagrams.tsx");
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

initPalette({
  go, jump, scope: aloneProject, settings: openSettings, providers: () => go("providers"),
  project: openProjects, projects: () => void openProjects(null), terminals: () => void go("terminals"),
  newSession: newAgentHere, addProject, joinTeam: onPhone() ? undefined : () => void import("../project/teamjoin.tsx").then(t => t.openTeamJoin(null)),
  addMachine: openAddMachine, shortcuts: showShortcuts, report: showReport,
  branches: name => { if (openProjects(name)) void import("../project/split.tsx").then(m => m.openTile("branches")); },
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
      if (up.to === "tile") return void import("../project/split.tsx").then(m => m.openTile(null));
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
pane.connect({
  openSettings,
  openShortcuts() {
    closeSettings();
    showShortcuts();
  },
  openReport() {
    closeSettings();
    showReport();
  },
  embedView,
  newAgent: newAgentHere,
  stepOut() {
    if (S.view === "agent" && stepOutOfRail()) return true;

    const { assistant } = shell.getState();
    if (!assistant) return false;
    if (windowAlone?.kind === "chat") return true;

    if (isMobile()) {
      asBack(() => { goTo(assistant.picked ? ASSISTANT_CHATS : { view: "home", arg: null }); });
      return true;
    }

    goBack({ view: "home", arg: null }, S.view === "agent");

    return true;
  },
  goBack,
  openStarted(session) {
    if (currentLocation()?.view === "pane") go("pane", { agent: session, adopt: true });
  },
  sessionDestinations(s) {
    const projects = state.home?.projects ?? [];
    const current = projectOf(s, projects);
    return projects
      .filter(p => p.machine_id === s.machine_id && p.name !== current?.name && p.folder !== s.cwd)
      .map(p => ({ name: p.name, folder: p.folder }));
  },
  projectWindowProps,
  sessionProject(s) {
    const projects = state.home?.projects ?? [], p = projectOf(s, projects);
    if (!p) return null;
    const open = () => void openProjects(p.name);
    // A closed session hands over to the project's next running one, else to the project.
    const afterClose = (closed: { machine_id: string; id: string; started_at?: number }) => {
      const next = nextRunning(p.name, closed);
      if (next) go("pane", { agent: next }); else open();
    };
    return { label: repoOf(s, projects) || p.name, name: p.name, open, afterClose };
  },
});

// The session shrinks back into its card on the board.
function goBack(from: BackTo, move: boolean) {
  if (!move) return void goTo(from);

  const session = AG.page?.session;
  if (session) setHeroCard(runningKey(session));
  withMove("back", () => { goTo(from); });
}

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
  if (!onPhone()) void import("./firstrun.tsx").then(m => m.firstRun());
  start(open);
  startupDrawn();
}).catch(e => {
  dropKeptFrame();
  throw e;
});
