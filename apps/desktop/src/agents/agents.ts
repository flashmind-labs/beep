import { WINDOWS } from "../ui/platform.ts";
import { flushSync } from "react-dom";
import { type BarPageSlot, openBarPage } from "../bar/bar-app.tsx";
import type { AgentListRow } from "./agents-list-view.tsx";
import type { ProviderCredits } from "../ui/credits.ts";
// agents/: coding agents in the bar: the start form, the picker, one
// session's log and messages, and every session. Entry: this file.
// Start a coding agent on one of the user's machines, follow it live and
// answer it: `agentnew` is the start form, `agent` one session, `agents`
// every session. They run in the bar and in the window's pane alike. This
// file holds their shared types and state, and the Every session list.
import { nest } from "./nesting-state.ts";
import { parentSession, ensureSessions, reviewers, sessions } from "./sessions.ts";
import { copies, copiesView, keepCopiesFresh } from "./copies.ts";
import { drawAgentNew, renderAgentNew, startAgent, togglePicker } from "./agentnew.ts";
import { preloadModels } from "./agentpicker.ts";
import type { CheckoutPick } from "./agentpick.ts";
import type { Attached, Waiting } from "./agentqueue.ts";
import { drawQueue, renderChat } from "../chat/chat.ts";
import { type Link, LINKED } from "./reconnect-state.ts";
import { leavePane, newTerminal, newTerminalLike, showPaneTab } from "../agentpane/media.ts";
import { draftEsc } from "./draft-state.ts";
import { type AgentReturn, bar, S, windowHooks } from "../bar/core.ts";
import { hintModel } from "../bar/hints.ts";
import { renderWaiting } from "../bar/waiting.ts";
import { openSession } from "../chat/history.ts";
import { cap } from "../chat/tools.ts";
import { openAgent, closeOpenSession, submitAgentMessage } from "./session.ts";
import { openEditorKey } from "./session-head.ts";
import { sessionMenu } from "./sessionmenu.ts";
import { TAKEOVER } from "./sessionmenu-state.ts";
import { toggleCommands, toggleLastCommand, toggleThoughts } from "./logkeys.ts";
import { jumpToStart, jumpToEnd, loadOlder, logJumpKey, nearTop } from "./older.ts";
import { pageDirection } from "./logkeys-state.ts";
import type { OlderStop } from "./older-state.ts";
import { closeModelPicker } from "./modelpill.ts";
import { interruptAgent, answerFirst } from "./messages.ts";
import { cycleMode } from "./modepill.ts";
import { money } from "../ui/format.ts";
import { ICON } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { Changes } from "../ui/pagecache.ts";
import { onPlaces, placeOf } from "./place.ts";
import { moveLabel, onMoves } from "./move-store.ts";
import { onClosing, shownStatus, sessionVisible } from "./closing.ts";
import { placeSub } from "./place-state.ts";
import { agentMessage, draftMessage, sessionBody, shownMessage } from "./session-frame.tsx";

/// A question a session waits on, such as whether to run a command.
export interface AgentQuestion {
  id: number;
  text: string;
  /// The command, path or other input it would act on, cut short.
  detail: string | null;
  options: { id: string; name: string; allow: boolean }[];
}

/// A session row as `agent_sessions` returns it.
export interface AgentSession {
  machine_id: string;
  machine_name: string;
  id: string;
  agent: string;
  title: string | null;
  cwd: string;
  /// asking, working, running, starting, waiting, idle or ended.
  status: string;
  doing: string | null;
  /// In milliseconds since the epoch.
  started_at: number;
  last_activity: number;
  /// Started outside Beep, which only follows it.
  external: boolean;
  question: AgentQuestion | null;
  /// The session whose agent started this one, when an agent did.
  parent?: AgentParent | null;
  /// A temporary reviewer of its parent's changes.
  reviewer?: boolean;
}

/// Where the session that started another runs and the ids it is listed
/// under: the agent's own, and the one its node gave it before that.
interface AgentParent {
  machine_id: string;
  id: string;
  key: string;
}

/// What the session view needs of a session to open it: a row from
/// `agent_sessions`, or one the window has, such as Home's running rows.
export type AgentRow = Pick<AgentSession, "machine_id" | "machine_name" | "id" | "agent" | "cwd" | "status" | "external"> & Partial<AgentSession>;

/// A machine that can run a coding agent.
export interface AgentHost {
  machine_id: string;
  machine_name: string;
  agents: string[];
  workspace: string | null;
  home: string | null;
  capabilities: string[];
}

/// One line of a session's log. Only the agent's own and its thoughts
/// have `html`; a thought that ended has `ms`.
export interface AgentEntry {
  kind: string;
  text: string;
  html?: string;
  /// Cleaned Markdown for child prompt parts, in source order.
  child_html?: string[];
  ms?: number | null;
  tool?: AgentToolCall | null;
}

/// Structured details from the harness, separate from its display title.
export interface AgentToolCall {
  name: string | null;
  input: string | null;
  input_truncated: boolean;
  result: string | null;
  result_truncated: boolean;
}

/// One session in full, as `agent_view` returns it.
export interface AgentPage {
  model?: string | null;
  session: AgentRow;
  plan: { content: string; status: string }[];
  files: string[];
  last_message: string;
  error: string | null;
  queued: number;
  queued_messages?: string[];
  turns: number;
  /// The entries held, newest last, and how many came before them. A read
  /// brings the newest; older ones are joined on as the reader goes up.
  entries: AgentEntry[];
  earlier: number;
}

/// What a session used, as `agent_usage` returns it.
export interface AgentUsage {
  context: { used: number; size: number } | null;
  tokens: number;
  cost_usd: number | null;
  credits?: ProviderCredits[];
}

/// The modes of a session, as `agent_modes` returns them. `plan` is the plan
/// waiting for approval, as HTML.
export interface AgentModes {
  current: string | null;
  modes: { id: string; name: string }[];
  plan: string | null;
}

/// A command the agent runs in the background, as `agent_tasks` returns it.
export interface AgentTask {
  id: string;
  name: string;
  description: string;
  /// `running`, `paused`, `completed`, `failed` or `stopped`.
  state: string;
  can_stop: boolean;
  output_file: string | null;
  summary: string | null;
  /// The log entry of the call that started it.
  entry: number | null;
}

const sessionTokens = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 });
const contextTokens = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 2 });

/// Context, session totals and provider balances in the session header.
export function usageLine(u: AgentUsage) {
  const parts: string[] = [];
  if (u.context?.size) parts.push(`${Math.round((u.context.used * 100) / u.context.size)}%/${contextTokens.format(u.context.size).replace("K", "k")}`);
  if (u.tokens) parts.push(`${sessionTokens.format(u.tokens).replace("K", "k")} tokens`);
  const credit = u.credits?.find(c => c.unit === "usd");
  const spent = u.cost_usd == null ? "?" : money(u.cost_usd);

  if (credit) {
    const remaining = credit.remaining == null ? NaN : Number(credit.remaining);
    parts.push(`${spent} / ${Number.isFinite(remaining) ? money(remaining) : "?"}`);
  } else if (u.cost_usd != null) parts.push(spent);

  return parts.join(" · ");
}

/// A session `agent_start` or `agent_resume` started, and why Auto picked its machine.
export interface Started {
  session: AgentSession;
  why: string;
}

/// The session the view follows, as the node knows it.
export interface OpenAgent {
  machine_id: string;
  agent: string;
  session: string;
}

/// The start form.
interface AgentForm {
  prompt: string;
  /// A machine id, or "" until one is picked.
  machine: string;
  agent: string | null;
  cwd: string;
  /// The project's checkout on each machine, when the form was opened
  /// from one, so picking another machine picks its folder.
  places?: { machine: string; cwd: string }[];
  /// The chat the form was opened from, which hears how the session goes.
  chat: string | null;
  /// The model picked or typed; empty for the agent's own.
  model?: string;
  /// The machine and agent `model` was picked for, as `machine/agent`.
  modelFor?: string;
  /// The thinking level picked; null for the agent's own.
  effort?: string | null;
  /// The project the form works on, picked or the one it was opened from.
  project?: string | null;
  /// In a checkout: the checkout, a new branch, a new worktree or an
  /// existing one.
  checkout?: CheckoutPick;
  /// The ref a new branch or worktree starts from; null for the project's
  /// default branch (the checkout's HEAD outside a project).
  base?: string | null;
  /// Whether the Base button is a field to type a ref in.
  baseTyping?: boolean;
  /// The open popover: the harness picker, or the Project, Machine,
  /// Folder or Base menu.
  open?: "pick" | "project" | "host" | "folder" | "base" | null;
  /// What the picker's search holds.
  query?: string;
  /// Whether the Folder button is a field to type a folder in.
  typing?: boolean;
}

type SentImage = { name: string; mime: string; data: string };
/// What `agent_steer` sends: a message, one for the running turn, take
/// those back, stop the turn, close the session, or switch its model or
/// thinking from the next turn.
export type Steer = { prompt: { text: string; images?: SentImage[] } } | { steer: { text: string; images?: SentImage[] } } | { drop_steers: null }
  | { cancel: null } | { close: null } | { configure: { model: string | null; effort: string | null } } | { mode: { mode: string } } | { stop_task: { task: string } };

interface AgentState {
  /// What the last failed read of the open session said, and how its link is.
  readError: string;
  link: Link;
  hosts: AgentHost[] | null;
  error: string;
  busy: boolean;
  form: AgentForm;
  open: OpenAgent | null;
  /// The project whose empty view shows (no session yet), else null.
  draft: string | null;
  page: AgentPage | null;
  why: string;
  /// Where Esc goes: "chat", "agents", or a window view.
  back: AgentReturn;
  timer: ReturnType<typeof setTimeout> | undefined;
  inflight: OpenAgent | null;
  /// "gone", or an error to show under the log.
  note: string;
  sel: number;
  /// Polling: what the last answer looked like, when it last changed,
  /// whether it stopped for quiet, and the question an answer is going to.
  sig: string;
  /// The rest of what the log was last drawn from; empty to draw again.
  rest: string;
  changed: number;
  answering: number | null;
  /// Images pasted or dropped into the session's field, the error from the last
  /// one and when it goes, and the session the server turned images down for.
  att: Attached[];
  attError: string;
  attTimer: ReturnType<typeof setTimeout> | undefined;
  noImages: string | null;
  /// The pasted blocks left open ("place:block"), and the module that reads
  /// tool entries, once loaded.
  opened: Set<number | string>;
  /// What the reader chose for a tool row (true open, false closed), by place
  /// in the session. A row they never touched has no entry.
  steps: Map<number, boolean>;
  log: typeof import("./agentlog.ts") | null;
  /// The steps whose whole output shows, by place in the session.
  all: Set<number>;
  /// Steers sent while a turn runs, shown until the node delivers them.
  queue: Waiting[];
  /// Messages and steers waiting for the session's machine to come back.
  held: number;
  /// Reads of the open session so far, which tell a read that left before
  /// a steer was sent from one after.
  polls: number;
  /// The session whose turn the user stopped, until it no longer runs.
  stopping: { id: string; turns: number } | null;
  /// Whether the model picker by the field is open.
  modelOpen: boolean;
  /// The open session's modes, when the server says.
  modes: AgentModes | null;
  /// The open session's background commands, when the server says.
  tasks: AgentTask[];
  /// Reading older entries: why no more come, whether a read is out, and
  /// whether the search for the oldest turn's prompt gave up.
  older: { stop: OlderStop; loading: boolean; gaveUp: boolean };
}

export const AG: AgentState = {
  readError: "", link: LINKED,
  hosts: null, error: "", busy: false,
  form: { prompt: "", machine: "", agent: null, cwd: "", chat: null },
  open: null, draft: null, page: null, why: "", back: "chat", timer: undefined, inflight: null, note: "", sel: 0,
  sig: "", rest: "", changed: 0, answering: null,
  att: [], attError: "", attTimer: undefined, noImages: null,
  opened: new Set(), steps: new Map(), log: null, all: new Set(), queue: [], held: 0, polls: 0, stopping: null, modelOpen: false, modes: null, tasks: [],
  older: { stop: "", loading: false, gaveUp: false },
};
export const agentLog = () => import("./agentlog.ts").then(m => (AG.log = m));
/// What a coding agent takes with a message, as the server checks it.
export const AGENT_IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
export const AGENT_MAX_IMAGES = 5;
export const AGENT_MAX_IMAGE = 5 * 1024 * 1024;
export const AGENT_MAX_IMAGES_TOTAL = 7 * 1024 * 1024;
/// Thumbnails of images sent from here, by session and name, so the
/// transcript's `[image: name]` lines show the picture.
export const agentPics = new Map<string, string>();
export const AGENT_PICS_KEPT = 60;
/// The pictures of `[image: image-<key>]` lines, read from the session's
/// machine, by key, as data URLs: `null` for one the machine no longer has.
export const agentImages = new Map<string, Promise<string | null> | string | null>();
/// The same pictures made small, which only the log draws: never saved or
/// shown in a viewer.
export const agentThumbs = new Map<string, Promise<string | null> | string | null>();
/// How often the session view asks for its session while on screen.
export const AGENT_POLL_MS = 1500;
/// How often while a thought streams, so its letters arrive nearly live.
export const AGENT_THINK_POLL_MS = 300;
/// How often while a stopped turn has not ended yet.
export const AGENT_STOP_POLL_MS = 400;
/// A quiet session (idle, or started outside Beep) unchanged this long is
/// asked every `AGENT_SLOW_MS` when its node cannot stream changes.
export const AGENT_SLOW_AFTER = 2 * 60_000;
export const AGENT_SLOW_MS = 10_000;
export const AGENT_ACTIVE = new Set<string | undefined>(["asking", "working", "running", "starting"]);
export const AGENT_LAST = 60;
/// How long the page waits on a read of the session, and on a prompt or a
/// resume to be taken, past the app's own limits (20 s and 30 s), so a call
/// that never returns shows an error instead of holding the view (#427).
export const AGENT_READ_WAIT_MS = 25_000;
export const AGENT_SEND_WAIT_MS = 40_000;
interface AgentKind {
  ini: string;
  name: string;
  cls: string;
}
const AGENT_KINDS: Record<string, AgentKind> = {
  claude: { ini: "CC", name: "Claude Code", cls: "claude" },
  codex: { ini: "CX", name: "Codex", cls: "codex" },
  gemini: { ini: "GE", name: "Gemini CLI", cls: "gemini" },
  pi: { ini: "PI", name: "pi", cls: "pi" },
  flsh: { ini: "FL", name: "flsh", cls: "flsh" },
  opencode: { ini: "OC", name: "OpenCode", cls: "opencode" },
  hermes: { ini: "HE", name: "Hermes", cls: "hermes" },
};
/// Agents whose outside sessions a node can take over (the ones it reads sessions of).
export const canTakeOver = (a: string) => TAKEOVER.has(a);
export const AGENT_ORDER = ["claude", "codex", "gemini", "opencode", "pi", "flsh", "hermes"];
export const AGENT_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>';

export const agentKind = (a: string): AgentKind => AGENT_KINDS[a] ?? { ini: String(a || "?").slice(0, 2).toUpperCase(), name: a || "Agent", cls: "" };
export const folderName = (p: string) => String(p || "").split("/").filter(Boolean).pop() || p || "";
export const byPreference = (a: string, b: string) => (AGENT_ORDER.indexOf(a) + 1 || 99) - (AGENT_ORDER.indexOf(b) + 1 || 99);
/// The session `o` names, as the node's commands take it.
export const where = (o: OpenAgent) => ({ machineId: o.machine_id, agent: o.agent, session: o.session });

/// Machines and accounts decide which machines run agents.
export const hostChanges = new Changes();
subscribeEach([topic("machines"), topic("accounts")], () => hostChanges.changed());

/// The machines that can run an agent; none for an account that may not use machines.
export async function loadAgentHosts() {
  hostChanges.read("");

  try {
    AG.hosts = await invoke<AgentHost[]>("agent_hosts");
    preloadModels(AG.hosts);
  } catch {
    AG.hosts = [];
    hostChanges.changed();
  }

  drawQueue();
  if (S.view === "agentnew") drawAgentNew();
}

// ----- Every session -----

export async function renderAgents() {
  S.view = "agents";
  AG.sel = 0;
  keepCopiesFresh(() => S.view === "agents");
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

onPlaces(() => { if (S.view === "agents") drawAgents(false); });
onMoves(() => { if (S.view === "agents") drawAgents(false); });
copies.subscribe(() => { if (S.view === "agents") drawAgents(false); });
onClosing(() => { if (S.view === "agents") drawAgents(false); });

function sessionRow({ row, depth }: { row: AgentSession; depth: number }): AgentListRow {
  const s = { ...row, status: shownStatus(row) };
  const k = agentKind(s.agent), folder = folderName(s.cwd), moving = moveLabel(s);
  const tail: AgentListRow["tail"] = s.reviewer ? { kind: "review" }
    : moving ? { kind: "move", label: moving.chip, why: moving.why ?? null }
    : s.status === "asking" ? { kind: "asking" }
    : s.status === "working" || s.status === "running" || s.status === "starting" ? { kind: "running", doing: s.doing || "Working" }
    : { kind: "plain", label: s.external && s.status !== "ended" ? "In terminal" : cap(s.status) };
  return { key: `${s.machine_id}\0${s.agent}\0${s.id}`, depth, title: s.reviewer ? "Review" : s.title || folder || k.name, sub: sessionSub(s, k.name, folder), tail };
}

let listPage: BarPageSlot | null = null;
/// The sessions the list on screen shows, which its rows index. Another page
/// can replace the shared list without a redraw here.
let drawn: AgentSession[] = [];

export function drawAgents(loading: boolean) {
  const list = sessions.filter(sessionVisible), shown = nest([...list, ...reviewers]);
  drawn = shown.map(n => n.row);
  AG.sel = Math.min(AG.sel, Math.max(0, drawn.length - 1));
  const machines = new Set(list.map(s => s.machine_id)).size;
  const copyRows = copiesView(new Set(list.map(s => s.id)));
  const sum = loading && !list.length ? "Looking at your machines…" : list.length ? `${list.length} on ${machines} ${machines === 1 ? "machine" : "machines"}` : "Nothing running";
  const page = listPage?.host.parentElement === bar ? listPage : listPage = openBarPage(() => { listPage = null; });
  flushSync(() => page.show({ view: "agents", props: {
    rows: shown.map(sessionRow), showSessions: !!list.length, selected: AG.sel, summary: sum, error: AG.error, copies: copyRows, loading,
    hints: hintModel([[ICON.ret, "open"], ["⌘⇧A", "new"], ["esc", "back"]]),
  } }));
  bar.onclick = e => {
    if (S.view !== "agents") return;
    const target = e.target as HTMLElement;
    const r = target.closest<HTMLElement>("[data-agi]");
    if (r) return openAgent(drawn[Number(r.dataset.agi)], { back: "agents" });
    if (target.closest("#agNew")) renderAgentNew("", "agents");
  };
  bar.oncontextmenu = e => {
    const r = S.view === "agents" ? (e.target as HTMLElement).closest<HTMLElement>("[data-agi]") : null;
    if (r) void sessionMenu(e, drawn[Number(r.dataset.agi)], s => openAgent(s, { back: "agents" }));
  };
}

/// Keys of the agent views; true when one was taken.
export function agentKeys(e: KeyboardEvent, cmd: boolean) {
  if (S.view === "agentnew") {
    if (cmd && e.key === "Enter") { e.preventDefault(); startAgent(); return true; }
    if (cmd && e.key.toLowerCase() === "m") { e.preventDefault(); togglePicker(); return true; }
    return false;
  }

  if (S.view === "agent") {
    if (cmd && e.key === "Enter" && answerFirst(true)) { e.preventDefault(); return true; }
    if (cmd && e.key.toLowerCase() === "d" && answerFirst(false)) { e.preventDefault(); return true; }
    if (cmd && e.key === ".") { e.preventDefault(); interruptAgent(); return true; }

    // ⌘T a terminal in the pane, ⌘N one like the terminal on screen, ⌘1 focuses chat, ⌘2 ⌘3 ⌘4 the pane's Files,
    // Changes and Artifacts, ⌘O the folder in the editor. ⌘U opens or folds every thought, ⌘Y
    // every command, ⇧⌘Y the last one. They work from the field and from a terminal: none edits text.
    if (cmd && !e.altKey && (WINDOWS || !e.ctrlKey) && !e.isComposing) {
      const k = e.key.toLowerCase();
      if (k === "w" && e.shiftKey && !AG.draft && AG.open) { e.preventDefault(); if (!e.repeat) void closeOpenSession(); return true; }
      if (k === "t" && !e.shiftKey) { e.preventDefault(); newTerminal(undefined, !!AG.draft); return true; }
      if (k === "n" && !e.shiftKey && newTerminalLike()) { e.preventDefault(); return true; }
      if (k === "u" && !e.shiftKey) { e.preventDefault(); toggleThoughts(); return true; }
      if (k === "y") { e.preventDefault(); (e.shiftKey ? toggleLastCommand : toggleCommands)(); return true; }

      if (!e.shiftKey && k === "1") {
        e.preventDefault();
        leavePane();
        const body = sessionBody(), chat = shownMessage() ?? body;
        if (chat && chat === body && !chat.hasAttribute("tabindex")) chat.setAttribute("tabindex", "-1");
        chat?.focus();
        return true;
      }

      if (!e.shiftKey && (k === "2" || k === "3" || k === "4")) { e.preventDefault(); showPaneTab(Number(k) - 2); return true; }
      if (k === "o" && !e.shiftKey) { e.preventDefault(); openEditorKey(); return true; }
    }

    const target = e.target, field = agentMessage.current, inField = !!field && target === field;
    const body = sessionBody();
    const page = pageDirection(e, !!body && ((inField && !field.value) || target === body || target === document.body));

    if (page && body) {
      e.preventDefault();
      body.stuck = false;
      body.scrollTop += page * body.clientHeight * 0.9;
      if (nearTop(body)) void loadOlder("near");
      return true;
    }

    // Home and ⌘↑ go to the first entry of the log, End and ⌘↓ to the last,
    // unless the field has text to move through.
    const jump = logJumpKey(e, cmd);
    if (jump) { e.preventDefault(); void (jump === "start" ? jumpToStart() : jumpToEnd()); return true; }
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
    const n = drawn.length;
    if (!n) return false;

    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      AG.sel = (AG.sel + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
      drawAgents(false);
      return true;
    }

    if (e.key === "Enter") { e.preventDefault(); openAgent(drawn[AG.sel], { back: "agents" }); return true; }
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
    if (draftEsc(typed) === "project") windowHooks?.goBack({ view: "projects", arg: { project: AG.draft } });
    return;
  }

  if (!closed && S.view === "agent" && closeModelPicker()) return;
  // Esc from a child opens its parent; closing returns to the previous page.
  const up = !closed && S.view === "agent" && AG.open ? AG.page && parentSession(AG.page.session) : undefined;
  if (up) return openAgent(up, { back: AG.back });
  if (S.view === "agent" && windowHooks && !closed && leavePane()) return;
  if (typeof AG.back === "object" && AG.back.view && windowHooks) return windowHooks.goBack(AG.back);
  if ((S.view === "agent" || S.view === "agentnew") && AG.back === "agents") return renderAgents();
  if ((S.view === "agent" || S.view === "agentnew") && AG.back === "waiting") return renderWaiting();

  // A session without a return target goes to its project.
  if (S.view === "agent" && windowHooks) {
    const project = AG.page && windowHooks.sessionProject?.(AG.page.session);
    if (project) return project.open();
  }

  if (S.link === "unlinked" && !windowHooks) return void invoke("hide_bar");
  return S.session ? openSession(S.session) : renderChat();
}
