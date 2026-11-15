// Home: its tabs and the board.
import { invoke } from "../ui/tauri.ts";
import { isMobile } from "../ui/platform.ts";
import { Button } from "../ui/kit.tsx";
import { Icon } from "../ui/icons.tsx";
import { homeForDisplay, type HomeView, type NeedsYou } from "./home-state.ts";
import { type CenterView, CONNECT_STUCK_MS, OFFLINE_NOTE_MS, state } from "./state.ts";
import { useThisMac, showMachineMenu, readSyncDetails } from "../machines/table.ts";
import { refreshGit } from "./openitems.ts";
import { currentLocation, go } from "./views.tsx";
import { openAddMachine, projectRows } from "./sidebar.tsx";
import { ensureHome, refresh } from "./refresh.ts";
import { ConnectingView } from "./connecting-view.tsx";
import { UNWATCHED_MS } from "../ui/pagecache.ts";
import { ensureAllTerms } from "../terminals/view.tsx";
import { thisMacRow } from "../machines/machine-state.ts";
import { onPlaces } from "../agents/place.ts";
import { ensureSessions, sessionsChanged, sessionsRead } from "../agents/sessions.ts";
import type { SessionRow } from "../project/projects-state.ts";
import { ambient } from "../ui/active.ts";
import { frontLayout, layoutChanged } from "../ui/skin.ts";
import { onMoves } from "../agents/move-store.ts";
import { onClosing } from "../agents/closing.ts";
import { placeWindowParts } from "./window-root.tsx";
import { windowHost } from "./window-shell.tsx";
import { HomeShell, type HomeTab } from "./home-shell.tsx";
import { HomeOverview } from "./home-overview.tsx";
import { HomeGit } from "./openitems-view.tsx";
import { HomeMachines } from "../machines/home-machines-view.tsx";

// ---------- Needs you ----------

/// Opens the session an agent's question came from, where it is answered.
export function openAsking(n: NeedsYou) {
  const r = state.home?.running?.find(r => r.machine_id === n.machine_id && r.id === n.session);
  go("pane", { agent: r || { kind: "agent", machine_id: n.machine_id, machine_name: n.machine_name, id: n.session, agent: n.agent, title: n.title, cwd: "", status: "asking", external: false } });
}

// ---------- Home ----------

/// The Home tab on screen.
export let homeTab: HomeTab = "overview";
/// The tab each section Home can jump to sits on.
const TAB_OF: Record<string, HomeTab> = {
  "sec-needs": "overview", "sec-running": "overview",
  "sec-git": "git", "sec-machines": "machines",
};

/// The phone's tab bar lights Machines for Home's Machines tab. An event, since the bar's
/// module imports this one.
export const HOME_TAB_EVENT = "beep:home-tab";
const homeTabChanged = () => document.dispatchEvent(new Event(HOME_TAB_EVENT));

function openHomeTab(tab: HomeTab) {
  homeTab = tab;
  homeTabChanged();
  draw();
  windowHost("view").scrollTop = 0;
}

/// Home: the machines strip, then one tab at a time. A tab with nothing in
/// it is left out; Overview and Machines stay.
function page(home: HomeView) {
  const { machines, running } = home;
  const projects = projectRows() || [], git = state.git;
  const tabs: { key: HomeTab; label: string; count: number | null; on: boolean }[] = [
    { key: "overview", label: frontLayout("home") === "list" ? "Overview" : "Board", count: null, on: true },
    { key: "git", label: "PRs and issues", count: git?.items.length ?? 0, on: !!git?.hosts?.length || projects.length > 0 || !!state.gitError },
    { key: "machines", label: "Machines", count: null, on: !!machines },
  ];
  const shownTabs = tabs.filter(t => t.on);
  if (homeTab === "machines" && machines) readSyncDetails();
  const tab = shownTabs.find(t => t.key === homeTab) || shownTabs[0];
  const offline = machines?.filter(m => m.can_host !== false && !m.online && !m.connecting && Date.now() - m.seen_at < OFFLINE_NOTE_MS).map(m => m.name) ?? [];

  return { machines, mac: machines ? thisMacRow(state.mac, machines) : null, jump,
    menu: (machine: NonNullable<typeof machines>[number], event: MouseEvent) => showMachineMenu(machine, () => [], event),
    addMachine: openAddMachine, useMac: () => { void useThisMac(); }, error: state.error, unavailable: !machines && !running, offline, content: tab.key === "overview" ? <HomeOverview home={home} /> : tab.key === "git" ? <HomeGit view={git || { items: [], hosts: [] }} /> : machines ? <HomeMachines machines={machines} /> : null,
    tabs: shownTabs, selected: tab.key, open: openHomeTab };
}

/// True once the first Home has been waited for too long.
export function stuck() { return !state.home && Date.now() - state.since >= CONNECT_STUCK_MS; }

/// Before Home first loads: the dots, then Retry and Show log once the wait is long.
function drawConnecting() {
  const secs = Math.round((Date.now() - state.since) / 1000);
  placeWindowParts({ home: <ConnectingView stuck={stuck()} text={`No answer from your machines for ${secs} seconds. Still trying.`}
    retry={() => { void refresh(); drawConnecting(); }}
    log={() => { void invoke("open_log").catch(e => console.warn("opening the log:", e)); }} /> });
}

export function draw() {
  // A hidden Home draws when it is shown: its mount refreshes.
  if (currentLocation()?.view && currentLocation()?.view !== "home") return;
  if (!state.home) return drawConnecting();
  const home = homeForDisplay(state.home), view = windowHost("view");
  const top = view.scrollTop;
  placeWindowParts({ home: <HomeShell {...page(home)} /> });
  view.scrollTop = top;
  applyScroll();
  scrollTarget = null;
}

// The Done today lane: the sessions of every project, ended ones included, which the
// running list drops; and every session, for the Assistant list. Read when they changed,
// while Home shows, seldom: they are not live.
const DONE_REREAD_MS = 5000;
let projectSessions: SessionRow[] = [];
let sessionsStale = true;
export const homeSessions = () => projectSessions;
const readSessions = ambient(() => {
  sessionsStale = false;
  ensureSessions().catch(e => console.warn("sessions:", e));
  return invoke<SessionRow[]>("projects_sessions").then(rows => { projectSessions = rows; draw(); }, e => { sessionsStale = true; console.warn("sessions:", e); });
}, DONE_REREAD_MS, () => currentLocation()?.view === "home");
sessionsChanged.add(() => { sessionsStale = true; readSessions(); });
sessionsRead.add(() => draw());
layoutChanged.add(() => draw());
onPlaces(() => draw());
onMoves(() => draw());
onClosing(() => draw());

/// When Home last asked for the open items; null before the first ask.
let gitReadAt: number | null = null;

export const homeView: CenterView<"home"> = {
  title: () => ({ title: "Home", sub: new Date().toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" }),
    // A phone has no ⌘N: the assistant starts from here.
    reactTools: isMobile() ? <Button small className="pj-tool fab" aria-label="New assistant" onClick={() => go("pane", { agentnew: { at: null, assistant: true } })}><span style={{ display: "contents" }}><Icon name="plus" /></span> New assistant</Button> : null }),
  mount() {
    draw();
    ensureHome();
    ensureAllTerms();
    if (sessionsStale) readSessions(true);

    // Forge changes reach Home only while it shows, so the open items read
    // again on opening once they are old or failed.
    if (gitReadAt === null || state.gitError !== null || Date.now() - gitReadAt >= UNWATCHED_MS) {
      gitReadAt = Date.now();
      void refreshGit();
    }
  },
};


/// Element Home scrolls to once it has drawn.
let scrollTarget: string | null = null;

export function jump(id?: string) {
  if (!go("home")) return;
  const tab = !id ? "overview" : id.startsWith("m-") ? "machines" : TAB_OF[id];

  if (tab && tab !== homeTab) {
    homeTab = tab;
    homeTabChanged();
    if (state.home) draw();
  }

  const view = windowHost("view");
  if (!id) return view.scrollTo({ top: 0 });
  scrollTarget = id;
  // Home redraws when it mounts, so the target is also applied after that draw.
  applyScroll();
}

function applyScroll() {
  if (!scrollTarget) return;
  const el = document.getElementById(scrollTarget);
  if (el) { el.scrollIntoView({ block: "start" }); scrollTarget = null; }
}
