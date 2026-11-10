import { drawHomeComposer } from "./home-composer.tsx";
// Home: its tabs, the Needs you, Updates, Running and Coming up sections, and the Connecting page before Home first loads.
import { sessionMenu } from "../agents/sessionmenu.ts";
import { harnessEl } from "../ui/harnessicon.ts";
import { dbtn as button } from "../ui/parts.ts";
import { invoke, Channel } from "../ui/tauri.ts";
import { connectProblem } from "./connect.ts";
import { pane } from "../bar/pane.ts";
import { S } from "../bar/core.ts";
import { homeForDisplay, type HomeView, type NeedsYou, type UpdateRow } from "./home-state.ts";
import { el as h } from "../ui/dom.ts";
import { type Outcome, $, CONNECT_STUCK_MS, OFFLINE_NOTE_MS, state } from "./state.ts";
import { failure, conflict, section, tag, line } from "../ui/pieces.ts";
import { useThisMac, showMachineMenu, readSyncDetails } from "../machines/table.ts";
import { refreshGit } from "./openitems.ts";
import { currentLocation, registerView, go, openResearch } from "./views.ts";
import { openAddMachine, projectRows } from "./sidebar.ts";
import { loading, refresh, fresh } from "./refresh.ts";
import { thisMacRow } from "../machines/machine-state.ts";
import { type RunningAgent, runningKey, runningLine, type RunningRow, runningWhen } from "../agents/running-state.ts";
import { onPlaces, runningLineAt } from "../agents/place.ts";
import { moveLabel, onMoves } from "../agents/move-store.ts";
import { closingNote, onClosing } from "../agents/closing.ts";
import { createElement } from "react";
import { showWindowParts } from "./window-root.ts";
import { ConnectingView } from "./connecting-view.tsx";
import { HomeShell, type HomeTab } from "./home-shell.tsx";
import { HomeOverview } from "./home-overview.tsx";
import { HomeGit } from "./openitems-view.tsx";
import { HomeMachines } from "../machines/home-machines-view.tsx";

export function runningStatus(r: RunningRow) {
  const moving = r.kind === "agent" ? moveLabel(r) : null;
  const closing = r.kind === "agent" ? closingNote(r) : null;
  const working = r.kind === "agent" && ["working", "running", "starting"].includes(r.status);
  const spin = working && !moving && !closing ? h("span", "dspin") : null;

  if (spin) {
    spin.setAttribute("role", "img");
    spin.setAttribute("aria-label", r.status === "starting" ? "Starting" : "Working");
  }

  const when = spin ? h("span", "cc-running", spin, runningWhen(r)) : runningWhen(r);
  return moving ? tag(moving.chip === "Moving" ? "run" : "bad", moving.chip) : closing || (r.status === "asking" ? tag("wait", "needs you") : when);
}

export function runningRow(r: RunningRow) {
  const projects = state.home?.projects || [];
  const { title, sub } = r.kind === "agent" ? runningLineAt(r, projects) : runningLine(r, projects);
  const moving = r.kind === "agent" ? moveLabel(r) : null;
  const right = [runningStatus(r)];
  const row = r.kind === "research"
    ? line(null, title, sub, right, () => openResearch(r.session))
    // The pane's agent view follows and answers it.
    : line(harnessEl(r.agent), title, sub, right, () => go("pane", { agent: r }));
  if (r.kind === "agent") row.addEventListener("contextmenu", e => void sessionMenu(e, r, s => go("pane", { agent: s })));
  row.dataset.key = runningKey(r);
  if (moving?.why) row.title = moving.why;
  return row;
}

// ---------- Needs you ----------

type ChatNeed = Extract<NeedsYou, { kind: "chat" }>;
type AgentNeed = Extract<NeedsYou, { kind: "agent" }>;

export function cardKey(n: NeedsYou) { return n.kind === "chat" ? `chat:${n.pending.query_id}` : `agent:${n.machine_id}:${n.session}:${n.question.id}`; }

function setOutcome(key: string, outcome: Outcome) { state.outcomes.set(key, outcome); draw(); }

export function answerChat(n: ChatNeed, decision: string, said: string) {
  const key = cardKey(n);
  setOutcome(key, { said });
  // The call returns when the run it resumes ends, so the outcome shows now.
  invoke("answer_approval", { session: n.session_id, queryId: n.pending.query_id, decision, onEvent: new Channel() })
    .then(() => refresh())
    .catch(e => setOutcome(key, { error: failure(e), final: conflict(e) }));
}

/// Opens the session an agent's question came from, where it is answered.
export function openAsking(n: AgentNeed) {
  const r = state.home?.running?.find((r): r is RunningAgent => r.kind === "agent" && r.machine_id === n.machine_id && r.id === n.session);
  go("pane", { agent: r || { kind: "agent", machine_id: n.machine_id, machine_name: n.machine_name, id: n.session, agent: n.agent, title: n.title, cwd: "", status: "asking", external: false } });
}

// ---------- Coming up ----------

export function whenOf(iso: string) {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return { time: "", day: "" };
  const time = at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  const days = Math.round((new Date(at).setHours(0, 0, 0, 0) - new Date().setHours(0, 0, 0, 0)) / 86400000);
  const day = days <= 0 ? "today" : days === 1 ? "tomorrow" : at.toLocaleDateString([], { weekday: "short" });
  return { time, day };
}
/// A routine's schedule in words. The row already shows when it runs next,
/// so a one-off is just "Once".
export function scheduleText(s: string) {
  if (/^\d{4}-\d\d-\d\dT/.test(s)) return "Once";
  return pane.when(s) ?? s;
}

// ---------- Updates ----------

/// When an update came: "09:41" today, then "Yesterday", "Thu" this week, "3 Oct".
export function updateTime(ms: number) {
  const at = new Date(ms);
  const days = Math.round((new Date().setHours(0, 0, 0, 0) - new Date(ms).setHours(0, 0, 0, 0)) / 86400000);
  if (days <= 0) return at.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  if (days === 1) return "Yesterday";
  if (days < 7) return at.toLocaleDateString([], { weekday: "short" });
  return at.toLocaleDateString([], { day: "numeric", month: "short" });
}

/// When a chat started, for its title line: "09:12" today, then as an
/// update's time reads.
export function startedWhen(ms: number) {
  const t = updateTime(ms);
  return t === "Yesterday" ? "yesterday" : t;
}

/// Updates dismissed here that a list may still carry, by id.
export const dismissedUpdates = new Set<number>();

/// Takes updates off Home at once and tells the server; a failed dismiss
/// brings them back on the next refresh.
export function dismissUpdates(ids: number[]) {
  if (!ids.length) return;
  for (const id of ids) dismissedUpdates.add(id);
  if (state.home?.updates) state.home.updates = state.home.updates.filter(u => !ids.includes(u.id));
  draw();
  invoke("center_dismiss_updates", { ids }).catch(e => {
    console.warn("dismissing updates:", e);
    for (const id of ids) dismissedUpdates.delete(id);
    refresh();
  });
}

/// Opens the update's chat, where Beep's first message tells it and the user
/// replies; the same chat each time.
export async function openUpdate(u: UpdateRow) {
  dismissUpdates([u.id]);

  try {
    const session = await invoke<string | null>("center_open_update_chat", { id: u.id });
    if (session) return go("pane", { chat: { session } });
    go("pane", { chat: { session: null } });
    await pane.draft(u.summary);
  } catch (e) {
    console.warn("opening an update:", e);
  }
}

// ---------- Home ----------

/// The Home tab on screen.
export let homeTab: HomeTab = "overview";
/// The tab each section Home can jump to sits on.
export const TAB_OF: Record<string, HomeTab> = {
  "sec-needs": "overview", "sec-running": "overview", "sec-updates": "overview", "sec-coming": "overview",
  "sec-git": "git", "sec-machines": "machines",
};
/// Rows an Overview box shows; its title opens the rest.
const BOX_ROWS = 5;

/// The phone's tab bar lights Machines for Home's Machines tab. An event, since the bar's
/// module imports this one.
export const HOME_TAB_EVENT = "beep:home-tab";
const homeTabChanged = () => document.dispatchEvent(new Event(HOME_TAB_EVENT));

function openHomeTab(tab: HomeTab) {
  homeTab = tab;
  homeTabChanged();
  draw();
  $("#view").scrollTop = 0;
}

/// Home: the machines strip, then one tab at a time. A tab with nothing in
/// it is left out; Overview and Machines stay.
function page(home: HomeView) {
  const { machines, running } = home;
  const projects = projectRows() || [], git = state.git;
  const tabs: { key: HomeTab; label: string; count: number | null; on: boolean }[] = [
    { key: "overview", label: "Overview", count: null, on: true },
    { key: "git", label: "PRs and issues", count: git?.items.length ?? 0, on: !!git?.hosts?.length || projects.length > 0 || !!state.gitError },
    { key: "machines", label: "Machines", count: null, on: !!machines },
  ];
  const shownTabs = tabs.filter(t => t.on);
  if (homeTab === "machines" && machines) readSyncDetails();
  const tab = shownTabs.find(t => t.key === homeTab) || shownTabs[0];
  const offline = machines?.filter(m => m.can_host !== false && !m.online && Date.now() - m.seen_at < OFFLINE_NOTE_MS).map(m => m.name) ?? [];

  return { machines, mac: machines ? thisMacRow(state.mac, machines) : null, jump,
    menu: (machine: NonNullable<typeof machines>[number], event: MouseEvent) => showMachineMenu(machine, () => [], event),
    addMachine: openAddMachine, useMac: () => { void useThisMac(); }, error: state.error, unavailable: !machines && !running, offline, content: tab.key === "overview" ? createElement(HomeOverview, { home }) : tab.key === "git" ? createElement(HomeGit, { view: git || { items: [], hosts: [] } }) : machines ? createElement(HomeMachines, { machines }) : null,
    tabs: shownTabs, selected: tab.key, open: openHomeTab };
}

/// An Overview box: its title opens the full list, then its first rows.
export function homeBox(id: string, title: string, count: number | string, open: () => void, rows: Node[], max = BOX_ROWS) {
  return section(id, button(title, "lk", open), count, null, h("div", "cc-panel", ...rows.slice(0, max)));
}

// ---------- Connecting ----------

/// True once the first Home has been waited for too long.
export function stuck() { return !state.home && Date.now() - state.since >= CONNECT_STUCK_MS; }

/// Before Home first loads, a failure waiting will not fix, or null.
export function problem() {
  const p = fresh ? null : connectProblem(state.error);
  // With no server, its errors are not shown: Home reads the mesh.
  return p && !S.server && p.kind !== "revoked" && p.kind !== "refused" ? null : p;
}

/// What "Still connecting" says, with how long the server has been silent.
function waitText() {
  const secs = Math.round((Date.now() - state.since) / 1000);
  return `No answer from ${S.server ? "the server" : "your machines"} for ${secs} seconds. Still trying.`;
}

/// What Home shows before the server first answers: pulsing dots, never a blank
/// page.
function drawConnecting(view: HTMLElement) {
  const p = problem();
  showWindowParts([{ part: "home", host: view, content: createElement(ConnectingView, { problem: p, stuck: stuck(), text: waitText(), loading,
    open: () => { if (p?.view) go("pane", p.view); }, retry: () => { void refresh(); drawConnecting(view); },
    log: () => { void invoke("open_log").catch(e => console.warn("opening the log:", e)); } }) }]);
}

export function draw() {
  // A hidden Home draws when it is shown: its mount refreshes.
  if (currentLocation()?.view && currentLocation()?.view !== "home") return;
  const home = homeForDisplay(state.home, S.server), view = $("#view");
  if (!home || S.server && problem()) return drawConnecting(view);
  const top = view.scrollTop;
  showWindowParts([{ part: "home", host: view, content: createElement(HomeShell, page(home)) }]);
  view.scrollTop = top;
  applyScroll();
  scrollTarget = null;
  drawHomeComposer();
}

onPlaces(() => draw());
onMoves(() => draw());
onClosing(() => draw());

registerView("home", {
  el: $("#home-box"),
  title: () => ({ title: "Home", sub: new Date().toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" }) }),
  mount() { draw(); refresh(); refreshGit(); },
});

/// Element Home scrolls to once it has drawn.
export let scrollTarget: string | null = null;

export function jump(id?: string) {
  if (!go("home")) return;
  const tab = !id ? "overview" : id.startsWith("m-") ? "machines" : TAB_OF[id];

  if (tab && tab !== homeTab) {
    homeTab = tab;
    homeTabChanged();
    if (state.home) draw();
  }

  const view = $("#view");
  if (!id) return view.scrollTo({ top: 0 });
  scrollTarget = id;
  // Home redraws when it mounts, so the target is also applied after that draw.
  applyScroll();
}

export function applyScroll() {
  if (!scrollTarget) return;
  const el = document.getElementById(scrollTarget);
  if (el) { el.scrollIntoView({ block: "start" }); scrollTarget = null; }
}
