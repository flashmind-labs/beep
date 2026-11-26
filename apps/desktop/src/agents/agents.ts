import { AG, type AgentSession } from "./agents-state.ts";
import { onPhone, WINDOWS } from "../ui/platform.ts";
import { flushSync } from "react-dom";
import { createStore } from "zustand/vanilla";
import { type BarPageSlot, openBarPage } from "../bar/bar-page.ts";
import type { AgentListRow } from "./agents-list-view.tsx";
import { cap, formatAgo, plural } from "../ui/format.ts";
import { machineConnecting, machinesReachNow, onConnecting, type MachinesReach } from "../machines/machine-state.ts";
// agents/: coding agents in the bar: the start form, the picker, one
// session's log and messages, and every session. Entry: this file.
// Start a coding agent on one of the user's machines, follow it live and
// answer it: `agentnew` is the start form, `agent` one session, `agents`
// every session. They run in the bar and in the window's pane alike. This
// file holds their shared types and state, and the Every session list.
import { type Nested, nest, splitEnded } from "./nesting-state.ts";
import { chatOf } from "./threads-state.ts";
import { parentSession, ensureSessions, sessions, sessionsRead } from "./sessions.ts";
import { AMBIENT_MS, ambient } from "../ui/active.ts";
import { newTerminal, newTerminalLike, showPaneTab } from "../agentpane/media.ts";
import { leavePane } from "../agentpane/pane-tabs.ts";
import { draftEsc } from "./draft-state.ts";
import { bar, PANE, S, windowHooks, setView } from "../bar/core.ts";
import { hintModel, openAgentInWindow } from "../bar/hints.ts";
import { renderWaiting } from "../bar/waiting.ts";
import { openAgent, closeOpenSession, submitAgentMessage } from "./session.tsx";
import { openEditorKey } from "./session-head.tsx";
import { sessionMenu } from "./sessionmenu.ts";
import { toggleCommands, toggleLastCommand, toggleThoughts } from "./logkeys.ts";
import { jumpToStart, logJumpKey } from "./older.ts";
import { jumpToLatest } from "./logview.tsx";
import { pageDirection } from "./logkeys-state.ts";
import { closeModelPicker, toggleModelPicker } from "./modelpill.tsx";
import { interruptAgent, answerFirst } from "./messages.ts";
import { cycleMode } from "./modepill.tsx";
import { ICON } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";
import { onPlaces, placeOf } from "./place.ts";
import { agentKind, folderName } from "./agent-kind.ts";
import { moveLabel, onMoves } from "./move-store.ts";
import { onClosing, shownStatus, sessionVisible } from "./closing.ts";
import { placeSub } from "./place-state.ts";
import { agentMessage, draftMessage, sessionBody, shownMessage } from "./session-frame-state.ts";
import { closeMoveMenu } from "./move.ts";

export const AGENT_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>';

// ----- Every session -----

export async function renderAgents() {
  setView("agents");
  agentsList.setState({ selected: 0, endedOpen: false });
  drawAgents(true);

  try {
    await ensureSessions();
    AG.error = "";
  } catch (e) {
    AG.error = String(e);
  }

  if (S.view === "agents") drawAgents(false);
}

/// A row's sub line: the harness, repository and machine; in a linked
/// worktree, also the worktree name. The folder is the fallback.
function sessionSub(s: AgentSession, harness: string, folder: string) {
  const worktree = placeSub(placeOf(s), s.machine_name);
  const machine = moveLabel(s)?.sub ?? s.machine_name;
  return (worktree ? [harness, worktree.replace(s.machine_name, machine)] : [harness, folder, machine]).filter(Boolean).join(" · ");
}

// The list on screen follows the sessions as they arrive, drawn at most once a second.
const redrawAgents = ambient(() => drawAgents(false), AMBIENT_MS, () => S.view === "agents");
sessionsRead.add(() => redrawAgents());
onPlaces(() => { if (S.view === "agents") drawAgents(false); });
onMoves(() => { if (S.view === "agents") drawAgents(false); });
onClosing(() => { if (S.view === "agents") drawAgents(false); });
onConnecting(() => { if (S.view === "agents") drawAgents(false); });

function sessionRow({ row, depth }: { row: AgentSession; depth: number }): AgentListRow {
  const s = { ...row, status: shownStatus(row) };
  const k = agentKind(s.agent), folder = folderName(s.cwd), moving = moveLabel(s);
  const tail: AgentListRow["tail"] = s.reviewer ? { kind: "review" }
    : s.status !== "ended" && machineConnecting(s.machine_id) ? { kind: "connecting" }
    : moving ? { kind: "move", label: moving.chip, why: moving.why ?? null }
    : s.status === "asking" ? { kind: "asking" }
    : s.status === "working" || s.status === "running" || s.status === "starting" ? { kind: "running", doing: s.doing || "Working" }
    // The Ended group names the state; its rows say when they last ran.
    : s.status === "ended" ? { kind: "plain", label: s.last_activity ? formatAgo(s.last_activity, "since") : "Ended" }
    : { kind: "plain", label: s.external ? "In terminal" : cap(s.status) };
  return { key: `${s.machine_id}\0${s.agent}\0${s.id}`, depth, agent: s.agent, title: s.reviewer ? "Review" : s.title || folder || k.name, sub: sessionSub(s, k.name, folder), tail };
}

/// A session in the list and the row it shows as.
export interface ListedSession { session: AgentSession; row: AgentListRow }

/// The Every session list: sessions still going, the ended ones folded under
/// their own group, which row is picked and whether that group is open. The
/// list view reads it; the session list and the keys write it.
interface AgentsListState {
  live: ListedSession[];
  ended: ListedSession[];
  endedOpen: boolean;
  /// Index into `pickable`.
  selected: number;
  loading: boolean;
  error: string;
  summary: string;
  /// Whether a machine answered, for the empty list to tell none running from not connected yet.
  reach: MachinesReach;
}

export const agentsList = createStore<AgentsListState>(() => ({
  live: [], ended: [], endedOpen: false, selected: 0, loading: false, error: "", summary: "", reach: "connecting",
}));

/// The rows the arrows and Enter move over: the ended ones only while open.
const pickable = (s: AgentsListState) => s.endedOpen ? [...s.live, ...s.ended] : s.live;

/// The selected row, kept within the rows the list now has.
const keepSelected = (selected: number, s: AgentsListState) => Math.min(selected, Math.max(0, pickable(s).length - 1));

let listPage: BarPageSlot | null = null;

/// Reads the sessions into the list's store, and opens its page when another
/// replaced it.
export function drawAgents(loading: boolean) {
  const visible = sessions.filter(s => !s.parent && sessionVisible(s)), nested = nest(visible);
  const split = splitEnded(nested, row => !row.reviewer && shownStatus(row) === "ended");
  const listed = (n: Nested): ListedSession => ({ session: n.row, row: sessionRow(n) });
  const live = split.live.map(listed), ended = split.ended.map(listed);
  const going = split.live.filter(n => !n.row.reviewer).map(n => n.row), machines = new Set(going.map(s => s.machine_id)).size;
  const reach = machinesReachNow();
  const summary = loading && !nested.length ? "Looking at your machines…"
    : going.length ? `${going.length} live on ${plural(machines, "machine", "machines")}`
    // Until a machine answers, nothing is known to run.
    : reach === "reached" ? "Nothing running" : "";

  agentsList.setState(s => {
    const next = { ...s, live, ended, loading, summary, error: AG.error, reach };
    return { ...next, selected: keepSelected(s.selected, next) };
  });

  const page = listPage?.host.parentElement === bar ? listPage : listPage = openBarPage(() => { listPage = null; });
  flushSync(() => page.show({ view: "agents", props: { hints: hintModel([[ICON.ret, "open"], ["⌘⇧A", "new"], ["esc", "back"]]) } }));
}

/// Opens the folded group of ended sessions, or folds it; a pick inside it
/// moves to the last live row.
export function toggleEnded() {
  agentsList.setState(s => {
    const next = { ...s, endedOpen: !s.endedOpen };
    return { ...next, selected: keepSelected(s.selected, next) };
  });
}

export const openListed = (s: AgentSession) => openAgent(s, { back: "agents" });
export const listedMenu = (e: MouseEvent, s: AgentSession) => void sessionMenu(e, s, row => openAgent(row, { back: "agents" }));

/// Keys of the agent views; true when one was taken.
export function agentKeys(e: KeyboardEvent, cmd: boolean) {
  if (S.view === "agentnew") return false;

  if (S.view === "agent") {
    if (cmd && e.key === "Enter" && answerFirst(true)) { e.preventDefault(); return true; }
    if (cmd && e.key.toLowerCase() === "d" && answerFirst(false)) { e.preventDefault(); return true; }
    if (cmd && e.key === ".") { e.preventDefault(); interruptAgent(); return true; }

    // ⌘T a terminal in the pane, ⌘N one like the terminal on screen, ⌘1 focuses chat, ⌘2 to ⌘9 the pane's tabs
    // in strip order, ⌘O the session in the window from the bar, else the folder in the editor. ⌘U opens or folds every thought, ⌘Y
    // every command, ⇧⌘Y the last one, ⇧⌘M the model picker. They work from the field and from a terminal: none edits text.
    if (cmd && !e.altKey && (WINDOWS || !e.ctrlKey) && !e.isComposing) {
      const k = e.key.toLowerCase();
      if (k === "w" && e.shiftKey && !AG.draft && AG.open) { e.preventDefault(); if (!e.repeat) void closeOpenSession(); return true; }
      if (k === "t" && !e.shiftKey) { e.preventDefault(); newTerminal(undefined, !!AG.draft); return true; }
      if (k === "n" && !e.shiftKey && newTerminalLike()) { e.preventDefault(); return true; }
      if (k === "n" && e.shiftKey && chatWindowHere()) { e.preventDefault(); return true; }
      if (k === "u" && !e.shiftKey) { e.preventDefault(); toggleThoughts(); return true; }
      if (k === "m" && e.shiftKey) { e.preventDefault(); toggleModelPicker(); return true; }
      if (k === "y") { e.preventDefault(); (e.shiftKey ? toggleLastCommand : toggleCommands)(); return true; }

      if (!e.shiftKey && k === "1") {
        e.preventDefault();
        leavePane();
        const body = sessionBody(), chat = shownMessage() ?? body;
        if (chat && chat === body && !chat.hasAttribute("tabindex")) chat.setAttribute("tabindex", "-1");
        chat?.focus();
        return true;
      }

      if (!e.shiftKey && k >= "2" && k <= "9" && k.length === 1) { e.preventDefault(); showPaneTab(Number(k) - 2); return true; }
      if (k === "o" && !e.shiftKey) {
        e.preventDefault();
        if (!PANE && AG.page && !AG.draft) void openAgentInWindow(AG.page.session); else openEditorKey();
        return true;
      }
    }

    const target = e.target, field = agentMessage.current, inField = !!field && target === field;
    const body = sessionBody();
    const page = pageDirection(e, !!body && ((inField && !field.value) || target === body || target === document.body));

    if (page && body) {
      e.preventDefault();
      body.stuck = false;
      body.scrollTop += page * body.clientHeight * 0.9;
      return true;
    }

    // Home and ⌘↑ go to the first entry of the log, End and ⌘↓ to the last,
    // unless the field has text to move through.
    const jump = logJumpKey(e, cmd);
    if (jump) { e.preventDefault(); void (jump === "start" ? jumpToStart() : jumpToLatest()); return true; }
    if (e.key === "Tab" && e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey && inField && cycleMode()) { e.preventDefault(); return true; }

    // On a phone Return starts a new line; the Send button sends.
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && inField && !document.body?.classList.contains("mobile")) {
      e.preventDefault();
      submitAgentMessage(field);
      return true;
    }

    return false;
  }

  if (S.view === "agents") {
    const list = agentsList.getState(), rows = pickable(list), n = rows.length;
    if (!n) return false;

    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      agentsList.setState({ selected: (list.selected + (e.key === "ArrowDown" ? 1 : n - 1)) % n });
      return true;
    }

    if (e.key === "Enter") { e.preventDefault(); openListed(rows[list.selected].session); return true; }
  }

  return false;
}

/// Esc from an agent view: closes the model picker, leaves a subagent for
/// its session, else goes back.
export function agentBack(closed = false) {
  // The empty view: the pane hands the keys back to the input, then Esc goes to the
  // project, unless the input holds a prompt.
  if (S.view === "agent" && AG.draft) {
    if (leavePane()) return;
    const typed = draftMessage.current?.value ?? "";
    if (draftEsc(typed) === "project") windowHooks?.goBack({ view: "projects", arg: { project: AG.draft } }, true);
    return;
  }

  if (!closed && S.view === "agent" && (closeModelPicker() || closeMoveMenu())) return;
  // A closed session opens the project's next running one, else the project; one without a
  // return target goes to its project.
  const project = S.view === "agent" && windowHooks && AG.page ? windowHooks.sessionProject?.(AG.page.session) : null;
  if (closed && project && AG.page) return project.afterClose(AG.page.session);

  // Esc from a child opens its parent; otherwise it returns to the previous page.
  const up = !closed && S.view === "agent" && AG.open ? AG.page && parentSession(AG.page.session) : undefined;
  if (up) return openAgent(up, { back: AG.back });
  if (!closed && S.view === "agent" && leavePane()) return;
  if (!closed && (S.view === "agent" || S.view === "agentnew") && windowHooks?.stepOut?.()) return;
  if (typeof AG.back === "object" && AG.back.view && windowHooks) return windowHooks.goBack(AG.back, !closed);
  if ((S.view === "agent" || S.view === "agentnew") && AG.back === "agents") return renderAgents();
  if ((S.view === "agent" || S.view === "agentnew") && AG.back === "waiting") return renderWaiting();
  if (project) return project.open();

  if (windowHooks) return windowHooks.goBack({ view: "home", arg: null }, !closed);
  void invoke("hide_bar");
}

/// ⇧⌘N on an assistant chat or one of its threads: the chat with its threads in a window of
/// its own.
function chatWindowHere() {
  const s = AG.draft ? null : AG.page?.session, chat = s ? chatOf(sessions, s) : null;
  if (!chat?.assistant || onPhone()) return false;
  void import("../center/sidebar.tsx").then(({ openChatWindow }) => openChatWindow(chat));
  return true;
}

/// ⌘N beside a chat's threads: a new thread of the chat, asked for in the chat's field. False
/// elsewhere, and for a chat that ended.
export function newThreadHere() {
  const s = AG.draft ? null : AG.page?.session, chat = s ? chatOf(sessions, s) : null;
  if (!chat?.assistant || chat.status === "ended") return false;
  void import("./assistant-page.tsx").then(({ newThread }) => newThread(chat));
  return true;
}
