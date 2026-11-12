// The window renders first, so modules that load after it find its elements.
import "./window-root.ts";
import { drawHomeComposer, initHomeComposer } from "./home-composer.tsx";
import { onPhone, WINDOWS } from "../ui/platform.ts";
import { initConversationSwitcher } from "../agents/conversation-switcher.ts";
import "../chat/diagrams.ts";
// center/: the command center window: sidebar, Home, palette, usage pill and
// the window's shared state (state.ts). Entry: this file (dist/center.js).
import "../ui/window-defaults.ts";
import "../ui/zoom.ts";
import { initMobile } from "./mobile.ts";
import { closeFocusedTab } from "../agentpane/media.ts";
import { isCloseKey, isZoomKey } from "../agentpane/media-state.ts";
import { isNewKey } from "../terminals/newkey-state.ts";
import { allTerms } from "../terminals/view.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { initPalette, openPalette } from "./palette.ts";
import { pane } from "../bar/pane.ts";
import { type BackTo, S } from "../bar/core.ts";
import { seed, type CachedViews } from "./cached.ts";
import type { HomeView } from "./home-state.ts";
import { startView, windowTitle, type CenterTarget } from "../project/projectwindow.ts";
import { newChatTarget } from "../project/agent.ts";
import { mountUsage, type UsageView } from "./usage.ts";
import { mountNotices } from "./notices.ts";
import { firstRun } from "./firstrun.ts";
import { pairing } from "./pairing.ts";
import { viewHidden } from "../settings/subscription.ts";
import { windowCommand } from "../ui/windowkeys.ts";
import { RESEARCHES } from "../research/researches.ts";
import { state, pagesAccount, keepPagesFile } from "./state.ts";
import { dismissedUpdates, draw, homeTab, jump } from "./home.ts";
import { currentLocation, windowProject, toMain, go, goHistory, hasHistory, setWindowProject, openResearch, goTo } from "./views.ts";
import { reloadGroup, paneOn, reloadChats, openProjects, openProjectWindow, addProject } from "./sidebar.ts";
import "../research/page.ts";
import "../research/open.ts";
import { escUp } from "./escup-state.ts";
import { setHeroCard, trackpadSwipe, withMove } from "../ui/motion.ts";
import { openTile } from "../project/split.ts";
import { pa } from "../project/sections.ts";
import { AG } from "../agents/agents.ts";
import { debugWithAgent, loadAgents, readPending, showPending } from "./pane.ts";
import { homeCached, refresh } from "./refresh.ts";
import { threadCard, threadChats } from "./threadcard.ts";
import { openSettings, embedView, closeSettings } from "../settings/sheet.ts";
import { followSkin } from "../ui/skin.ts";
import { openReport } from "../report/report.ts";
import { toggleShortcuts } from "../ui/shortcutsheet.tsx";
import { guardClicks } from "../ui/clickguard.ts";
import { reason } from "../ui/format.ts";
import { keepConnecting, type MacControl } from "../machines/machine-state.ts";
import { projectOf, repoOf, runningKey } from "../agents/running-state.ts";
import { placeOf } from "../agents/place.ts";
import { followToolUpdates } from "../machines/machines.ts";
import { dialogOpen } from "../ui/parts.ts";

followSkin();
guardClicks();
followToolUpdates();
initConversationSwitcher({
  current: () => currentLocation()?.view === "pane" && paneOn().view === "agent" && !AG.draft ? AG.page?.session ?? null : null,
  rows: () => (state.home?.running ?? []).filter(row => row.kind === "agent"),
  // The next conversation slides in, and Back from it returns to that session's card.
  open: agent => { setHeroCard(runningKey({ kind: "agent", ...agent })); withMove("next", () => { go("pane", { agent }); }); },
});

// ---------- Keys ----------

initMobile();
trackpadSwipe({ has: to => !dialogOpen() && hasHistory(to === "forward"), go: to => { goHistory(to === "forward"); } });
mountUsage(refresh => invoke<UsageView | null>("usage_get", { refresh }));
mountNotices();

initPalette({ go, jump, ask: askBeep, scope: () => windowProject, openChat: id => (RESEARCHES.has(id) ? openResearch(id) : go("pane", { chat: { session: id } })), research: id => RESEARCHES.has(id), settings: openSettings, providers: () => go("providers"), project: openProjects, addProject, branches: name => { if (openProjects(name)) openTile("branches"); } });

// Capture before the terminal handles the key, so Shift+Esc never reaches the shell.
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
    pane: currentLocation()?.view === "pane" ? pane.current().view : null,
    project: state.picked,
    projectWindow: windowProject !== null,
    subscribed: state.subscribed,
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
    case "blocked": return;
    case "palette": return openPalette();
    case "settings": return openSettings();
    case "shortcuts": return toggleShortcuts();
    case "research": return go("research");
    case "pane": return go("pane", command.view);
    case "new-agent":
      if (currentLocation()?.view === "terminals" && isNewKey(e) && allTerms.newLikeOpen()) return;
      return newAgentHere();
  }
});

/// The machines online in `home`, as a key that changes when one connects.
export function onlineMachines(home: HomeView | null) {
  return (home?.machines || []).filter(m => m.online).map(m => m.id).sort().join(" ");
}

/// The project on screen, its page, a session or chat of it, and the machine
/// its start input opens on; null elsewhere.
function projectHere() {
  const on = paneOn();
  if (on.view === "agent" && AG.draft) return { project: AG.draft, machine: null };
  // The session on screen, which the pane may have moved to since the window opened it.
  const page = currentLocation();
  const target = page?.view === "pane" ? page.arg : null;
  const open = AG.page?.session ?? (target && typeof target === "object" && "agent" in target ? target.agent : null);
  const agent = on.view === "agent" && open ? { machine_id: open.machine_id, cwd: open.cwd, main: placeOf(open)?.main } : null;
  return newChatTarget({ shown: currentLocation()?.view ?? null, picked: state.picked, view: on.view, session: on.session, agent, mac: state.mac?.id }, state.agents, state.home?.projects);
}

/// ⌘N: in a project (its page, or one of its sessions or chats) the agent
/// view with an empty conversation and the project's start input, focused;
/// elsewhere the start form.
function newAgentHere() {
  const to = projectHere();
  if (!to) return void go("pane", { agentnew: { prompt: "" } });
  go("pane", { draft: { project: to.project, machine: to.machine } });
}
pane.connect({
  openSettings,
  openInstructions() {
    closeSettings();
    go("instructions");
  },
  openProviders() {
    closeSettings();
    go("providers");
  },
  openShortcuts() {
    closeSettings();
    toggleShortcuts();
  },
  openReport() {
    closeSettings();
    openReport(document.body, debugWithAgent);
  },
  settingsAway(view) {
    closeSettings();
    if (view) go("pane", view);
  },
  embedView,
  newAgent: newAgentHere,
  // The session shrinks back into its card on the board.
  goBack(from: BackTo, move: boolean) {
    if (!move) return void goTo(from);
    const session = AG.page?.session;
    if (session) setHeroCard(runningKey({ kind: "agent", ...session }));
    withMove("back", () => { goTo(from); });
  },
  chatTop(session) {
    const t = threadChats?.get(session);
    return t ? threadCard(t) : null;
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

initHomeComposer({
  async send(prompt) {
    go("pane", { chat: { session: null } });
    try {
      await pane.compose(prompt);
      return { kind: "sent" };
    } catch (error) {
      go("home");
      return { kind: "failed", error: reason(error) };
    } finally {
      reloadChats();
    }
  },
  agent: prompt => { go("pane", { agentnew: { prompt } }); },
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
  document.title = windowTitle(project);
  const first = startView(project);
  if (!first) return showPending(pending, "home");
  // A project window: no sidebar, and a button back to the project.
  const name = first.arg.project;
  setWindowProject(name);
  document.body.classList.add("pw");
  go(first.view, first.arg);
}

/// A new chat that starts with `prompt`, from the palette's ⌘↵ or another
/// window; a project window asks the main window.
export function askBeep(prompt: string) {
  if (windowProject) return toMain({ ask: { prompt } });
  go("pane", { chat: { session: null } });
  pane.compose(prompt).catch(e => console.warn("asking Beep:", e));
}

/// Shows the views the app kept from its last answers, so the window
/// opens on Home, its projects and sidebar before the mesh answers.
async function showCached() {
  const [cached, mac, link] = await Promise.all([
    invoke<CachedViews | null>("center_cached").catch(e => { console.warn("kept views:", e); return null; }),
    invoke<MacControl>("mac_control").catch(() => null),
    invoke<{ link: string; signed_out: boolean; server: boolean }>("status").catch(() => null),
  ]);
  state.subscribed = link?.link === "linked";
  S.signedOut = link?.signed_out ?? false;
  S.server = link?.server ?? false;
  drawHomeComposer();
  const home = seed(state, cached, dismissedUpdates);
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
}

// Signing in or out changes what the window shows.
listen<string>("link-state", e => {
  const now = e.payload === "linked";
  if (now === state.subscribed) return;
  state.subscribed = now;
  for (const name of ["pinned", "chats", "views", "research"]) reloadGroup(name);

  if (now) loadAgents();
  else {
    state.agents = null;
    // Signed out on a page that is only there with a subscription: Home.
    const on = paneOn();
    const arg = on.group ? { group: { id: on.group } } : on.session ? { chat: { session: on.session } } : on.view;
    const page = currentLocation();
    if (page && viewHidden(page.view, arg, false, S.server)) go("home");
  }

  draw();
});

// A reload starts with no chat open, as the window just opened does.
// Home's reads start with the window; the first view waits only for the kept views.
const opening = readOpening();
void pairing().then(async () => {
  void refresh();
  const [, , open] = await Promise.all([invoke("new_chat").catch(() => {}), showCached(), opening]);
  if (!onPhone()) void firstRun(state.subscribed, S.signedOut);
  start(open);
  if (state.subscribed) loadAgents();
});
