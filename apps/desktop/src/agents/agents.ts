// agents/: coding agents in the bar: the start form, the picker, one
// session's log and messages, and every session. Entry: this file.
// Start a coding agent on one of the user's machines, follow it live and
// answer it: `agentnew` is the start form, `agent` one session, `agents`
// every session. They run in the bar and in the window's pane alike. This
// file holds their shared types and state, and the Every session list.
import { nest, parentRow } from "./nesting-state.ts";
import { loadSessions, sessions } from "./sessions.ts";
import { drawAgentNew, renderAgentNew, startAgent, togglePicker } from "./agentnew.ts";
import { emptyPage } from "../lists/memories.ts";
import { preloadModels } from "./agentpicker.ts";
import { harnessTile } from "../ui/harnessicon.ts";
import type { CheckoutPick } from "./agentpick.ts";
import type { Attached, Waiting } from "./agentqueue.ts";
import { renderChat } from "../chat/chat.ts";
import { leavePane } from "../agentpane/media.ts";
import { $, type BackTo, bar, S, windowHooks } from "../bar/core.ts";
import { hints } from "../bar/hints.ts";
import { openSession } from "../chat/history.ts";
import { cap } from "../chat/tools.ts";
import { openAgent, turnRuns } from "./session.ts";
import { sessionMenu } from "./sessionmenu.ts";
import { closeChild } from "./logview.ts";
import { toggleCommands, toggleLastCommand, toggleThoughts } from "./logkeys.ts";
import { jumpToStart, jumpToEnd, logJumpKey } from "./older.ts";
import type { OlderStop } from "./older-state.ts";
import { closeModelPicker } from "./modelpill.ts";
import { queueMessage, steerMessage, interruptAgent, answerFirst, sendAgentMessage } from "./messages.ts";
import { cycleMode } from "./modepill.ts";
import { esc, money, tokenCount } from "../ui/format.ts";
import { h } from "../ui/dom.ts";
import { ICON } from "../ui/icons.ts";
import { invoke } from "../ui/tauri.ts";
import { onPlaces, placeOf } from "./place.ts";
import { moveLabel, onMoves } from "./move-store.ts";
import { placeSub } from "./place-state.ts";

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
}

/// Where the session that started another runs and the ids it is listed
/// under: the agent's own, and the one its node gave it before that.
export interface AgentParent {
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
  ms?: number | null;
}

/// One session in full, as `agent_view` returns it.
export interface AgentPage {
  session: AgentRow;
  plan: { content: string; status: string }[];
  files: string[];
  last_message: string;
  error: string | null;
  queued: number;
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

/// "62% context · 184K tokens · $1.42", leaving out what is not known.
export function usageLine(u: AgentUsage) {
  const parts: string[] = [];
  if (u.context?.size) parts.push(`${Math.round((u.context.used * 100) / u.context.size)}% context`);
  if (u.tokens) parts.push(`${tokenCount(u.tokens)} tokens`);
  if (u.cost_usd != null) parts.push(money(u.cost_usd));
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
export interface AgentForm {
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
  readError: string;
  hosts: AgentHost[] | null;
  error: string;
  busy: boolean;
  form: AgentForm;
  open: OpenAgent | null;
  page: AgentPage | null;
  why: string;
  /// Where Esc goes: "chat", "agents", or a window view.
  back: string | BackTo;
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
  paused: boolean;
  answering: number | null;
  barHidden: boolean;
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
  /// The subagent opened as its own conversation, by its call's place.
  child: number | null;
  /// Messages typed while a turn runs: steers, sent at once and shown until
  /// delivered, and queued ones, sent when it stops or ends.
  queue: Waiting[];
  flushing: boolean;
  /// Reads of the open session so far, which tell a read that left before
  /// a steer was sent from one after.
  polls: number;
  /// The session whose turn the user stopped, until it no longer runs.
  stopping: string | null;
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
  readError: "",
  hosts: null, error: "", busy: false,
  form: { prompt: "", machine: "", agent: null, cwd: "", chat: null },
  open: null, page: null, why: "", back: "chat", timer: undefined, inflight: null, note: "", sel: 0,
  sig: "", rest: "", changed: 0, paused: false, answering: null, barHidden: false,
  att: [], attError: "", attTimer: undefined, noImages: null,
  opened: new Set(), steps: new Map(), log: null, all: new Set(), child: null, queue: [], flushing: false, polls: 0, stopping: null, modelOpen: false, modes: null, tasks: [],
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
/// asked every `AGENT_SLOW_MS`, and after `AGENT_PAUSE_AFTER` no more.
export const AGENT_SLOW_AFTER = 2 * 60_000;
export const AGENT_SLOW_MS = 10_000;
export const AGENT_PAUSE_AFTER = 5 * 60_000;
export const AGENT_ACTIVE = new Set<string | undefined>(["asking", "working", "running", "starting"]);
export const AGENT_LAST = 60;
interface AgentKind {
  ini: string;
  name: string;
  cls: string;
}
export const AGENT_KINDS: Record<string, AgentKind> = {
  claude: { ini: "CC", name: "Claude Code", cls: "claude" },
  codex: { ini: "CX", name: "Codex", cls: "codex" },
  gemini: { ini: "GE", name: "Gemini CLI", cls: "gemini" },
  pi: { ini: "PI", name: "pi", cls: "pi" },
  flsh: { ini: "FL", name: "flsh", cls: "flsh" },
  opencode: { ini: "OC", name: "OpenCode", cls: "opencode" },
};
/// Agents whose outside sessions a node can take over (the ones it reads sessions of).
const TAKEOVER = new Set(["claude", "codex", "pi", "flsh", "opencode"]);
export const canTakeOver = (a: string) => TAKEOVER.has(a);
export const AGENT_ORDER = ["claude", "codex", "gemini", "opencode", "pi", "flsh"];
export const AGENT_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>';

export const agentKind = (a: string): AgentKind => AGENT_KINDS[a] ?? { ini: String(a || "?").slice(0, 2).toUpperCase(), name: a || "Agent", cls: "" };
export const agentAvatar = (a: string) => harnessTile(a, "dapp dcoderav");
export const folderName = (p: string) => String(p || "").split("/").filter(Boolean).pop() || p || "";
export const byPreference = (a: string, b: string) => (AGENT_ORDER.indexOf(a) + 1 || 99) - (AGENT_ORDER.indexOf(b) + 1 || 99);
/// The session `o` names, as the node's commands take it.
export const where = (o: OpenAgent) => ({ machineId: o.machine_id, agent: o.agent, session: o.session });

/// The machines that can run an agent; none for an account that may not use machines.
export async function loadAgentHosts() {
  if (S.link !== "linked") return;
  try {
    AG.hosts = await invoke<AgentHost[]>("agent_hosts");
    preloadModels(AG.hosts);
  } catch {
    AG.hosts = [];
  }
  drawAgentButton();
  if (S.view === "agentnew") drawAgentNew();
}

/// The composer's Agent button, once some machine offers an agent.
export function drawAgentButton() {
  const row = bar.querySelector(":scope > .dinput.dgrow");
  if (!row || S.view !== "chat" || $("#agentGo") || !AG.hosts?.length) return;
  const b = h(`<button class="dbtn quiet" id="agentGo" title="Start a coding agent on a machine (⌘⇧A)">${AGENT_ICON}Agent</button>`);
  b.onclick = () => renderAgentNew($<HTMLTextAreaElement>("#ask")?.value.trim() ?? "");
  row.insertBefore(b, $("#grip"));
}

export function stateTag(status: string) {
  if (status === "asking") return '<span class="dstate ask">Waiting for you</span>';
  if (status === "working" || status === "running" || status === "starting") return `<span class="dstate run">${status === "starting" ? "Starting" : "Working"}</span>`;
  if (status === "waiting") return '<span class="dstate ask">Waiting in terminal</span>';
  if (status === "ended") return '<span class="dstate">Ended</span>';
  return '<span class="dstate">Idle</span>';
}

// ----- Every session -----

export async function renderAgents() {
  S.view = "agents";
  AG.sel = 0;
  drawAgents(true);
  try {
    await loadSessions();
    AG.error = "";
  } catch (e) {
    AG.error = String(e);
  }
  if (S.view === "agents") drawAgents(false);
}

/// A row's sub line: the harness, and the folder and machine; in a linked
/// worktree the repository, the worktree and the machine.
function sessionSub(s: AgentSession, harness: string, folder: string) {
  const worktree = placeSub(placeOf(s), s.machine_name);
  const machine = moveLabel(s)?.sub ?? s.machine_name;
  return (worktree ? [harness, worktree.replace(s.machine_name, machine)] : [harness, folder, machine]).filter(Boolean).join(" · ");
}

onPlaces(() => { if (S.view === "agents") drawAgents(false); });
onMoves(() => { if (S.view === "agents") drawAgents(false); });

function sessionRow({ row: s, depth }: { row: AgentSession; depth: number }, i: number) {
  const k = agentKind(s.agent), folder = folderName(s.cwd);
  const moving = moveLabel(s);
  const tail = moving ? `<span class="dstate ${moving.chip === "Moving" ? "run" : "ask"}"${moving.why ? ` title="${esc(moving.why)}"` : ""}>${moving.chip}</span>`
    : s.status === "asking" ? '<span class="dstate ask">Needs you</span>'
    : s.status === "working" || s.status === "running" || s.status === "starting" ? `<span class="run" title="${esc(s.doing || "Working")}"><span class="dspin"></span><span class="tx">${esc(s.doing || "Working")}</span></span>`
    : `<span class="w">${s.external ? "In terminal" : esc(cap(s.status))}</span>`;
  const kid = depth ? ` kid" style="--depth:${depth}` : "";
  return `<button class="dchat${kid}" role="option" aria-selected="${i === AG.sel}" data-agi="${i}">
      <span class="t"><span>${esc(s.title || folder || k.name)}</span><small>${esc(sessionSub(s, k.name, folder))}</small></span>${tail}</button>`;
}

/// The sessions the list on screen shows, which its rows index. Another page
/// can replace the shared list without a redraw here.
let drawn: AgentSession[] = [];

export function drawAgents(loading: boolean) {
  const list = sessions, shown = nest(list);
  drawn = shown.map(n => n.row);
  const machines = new Set(list.map(s => s.machine_id)).size;
  const sum = loading && !list.length ? "Looking at your machines…" : list.length ? `${list.length} on ${machines} ${machines === 1 ? "machine" : "machines"}` : "Nothing running";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Coding agents</b><small>${esc(sum)}</small></span><button class="dbtn" id="agNew">${AGENT_ICON}New<kbd>⌘⇧A</kbd></button></div>
    ${AG.error ? `<div class="derr dpadx">${esc(AG.error)}</div>` : ""}
    ${list.length ? `<div class="dlist top" role="listbox">${shown.map(sessionRow).join("")}</div>`
      : loading ? "" : emptyPage(AGENT_ICON, "No sessions running.", "Start one here, ask Beep in a chat, or run Claude Code in a terminal on a linked machine.")}
    ${hints([[ICON.ret, "open"], ["⌘⇧A", "new"], ["esc", "back"]])}`;
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
    // ⌘T opens or folds every thought, ⌘Y every command, ⇧⌘Y the last one.
    // They work from the field: neither key edits text.
    if (cmd && !e.altKey && !e.ctrlKey && !e.isComposing) {
      const k = e.key.toLowerCase();
      if (k === "t" && !e.shiftKey) { e.preventDefault(); toggleThoughts(); return true; }
      if (k === "y") { e.preventDefault(); (e.shiftKey ? toggleLastCommand : toggleCommands)(); return true; }
    }
    // Home and ⌘↑ go to the first entry of the log, End and ⌘↓ to the last,
    // unless the field has text to move through.
    const jump = logJumpKey(e, cmd);
    if (jump) { e.preventDefault(); void (jump === "start" ? jumpToStart() : jumpToEnd()); return true; }
    if (e.key === "Tab" && e.shiftKey && (e.target as HTMLElement).id === "agmsg" && cycleMode()) { e.preventDefault(); return true; }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && (e.target as HTMLElement).id === "agmsg") {
      e.preventDefault();
      // While the turn runs, Enter steers it after its current tool call,
      // and Option-Enter queues for after the turn.
      if (turnRuns()) (e.altKey ? queueMessage : steerMessage)(e.target as HTMLTextAreaElement);
      else sendAgentMessage();
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
export function agentBack() {
  if (S.view === "agent" && (closeModelPicker() || closeChild())) return;
  // A session an agent started goes back to the session that started it.
  const up = S.view === "agent" && AG.open ? parentRow(sessions, { machine_id: AG.open.machine_id, id: AG.open.session }) : undefined;
  if (up) return openAgent(up, { back: AG.back });
  // In the window a session's parent is its project, wherever it was opened from.
  if (S.view === "agent" && windowHooks) {
    if (leavePane()) return;
    const project = AG.page && windowHooks.sessionProject?.(AG.page.session);
    if (project) return project.open();
  }
  if (typeof AG.back === "object" && AG.back.view && windowHooks) return windowHooks.goBack(AG.back);
  if ((S.view === "agent" || S.view === "agentnew") && AG.back === "agents") return renderAgents();
  if (S.link === "unlinked" && !windowHooks) return void invoke("hide_bar");
  return S.session ? openSession(S.session) : renderChat();
}
