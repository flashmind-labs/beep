import { onPhone, WINDOWS } from "../ui/platform.ts";
import { flushSync } from "react-dom";
import { createStore } from "zustand/vanilla";
import { type BarPageSlot, openBarPage } from "../bar/bar-app.tsx";
import type { AgentListRow } from "./agents-list-view.tsx";
import { cap, formatAgo, plural } from "../ui/format.ts";
import { machineConnecting, machinesReachNow, onConnecting, type MachinesReach } from "../machines/machine-state.ts";
import type { ProviderCredits } from "../ui/credits.ts";
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
import { drawAgentNew, startAgent, togglePicker } from "./agentnew.tsx";
import { preloadModels } from "./model-store.ts";
import type { CheckoutPick } from "./agentpick.ts";
import type { Attached, OutboxRow } from "./agentqueue.ts";
import { mainTarget } from "../project/projectwindow.ts";
import { type Link, LINKED } from "./reconnect-state.ts";
import { leavePane, newTerminal, newTerminalLike, showPaneTab } from "../agentpane/media.ts";
import { draftEsc } from "./draft-state.ts";
import { type AgentReturn, bar, PANE, S, toWindow, windowHooks, setView } from "../bar/core.ts";
import { hintModel } from "../bar/hints.ts";
import { renderWaiting } from "../bar/waiting.ts";
import { openAgent, closeOpenSession, submitAgentMessage } from "./session.tsx";
import { openEditorKey } from "./session-head.tsx";
import { sessionMenu } from "./sessionmenu.ts";
import type { AgentQuestion } from "./running-state.ts";
import { AGENT_ACTIVE, TAKEOVER } from "./sessionmenu-state.ts";
import { transcriptKey, transcripts } from "./transcript-cache-state.ts";
import { toggleCommands, toggleLastCommand, toggleThoughts } from "./logkeys.ts";
import { jumpToStart, logJumpKey } from "./older.ts";
import { jumpToLatest } from "./logview.tsx";
import type { PlanStep } from "./plan-state.ts";
import { pageDirection } from "./logkeys-state.ts";
import type { OlderStop } from "./older-state.ts";
import { closeModelPicker, toggleModelPicker } from "./modelpill.tsx";
import { interruptAgent, answerFirst } from "./messages.ts";
import { cycleMode } from "./modepill.tsx";
import { money } from "../ui/format.ts";
import { ICON } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";
import type { NamedAccount } from "../settings/agentaccounts.ts";
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { pushedList } from "../ui/snapshot-state.ts";
import { currentPage, setPage } from "./open-page.ts";
import { onPlaces, placeOf } from "./place.ts";
import { moveLabel, onMoves } from "./move-store.ts";
import { onClosing, shownStatus, sessionVisible } from "./closing.ts";
import { type WorkPlace, placeSub } from "./place-state.ts";
import { agentMessage, draftMessage, sessionBody, shownMessage } from "./session-frame.tsx";
import { closeMoveMenu } from "./move.ts";

export type { AgentQuestion };

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
  /// Its subagents that still run.
  subagents?: number;
  /// The session whose agent started this one, when an agent did.
  parent?: AgentParent | null;
  /// A temporary reviewer of its parent's changes.
  reviewer?: boolean;
  /// The model it runs on, when its node says.
  model?: string | null;
  /// The Home assistant.
  assistant: boolean;
  /// The first line of its last reply, as inline HTML.
  reply_html?: string | null;
  /// Where its node says it works, for a Beep session.
  place?: WorkPlace | null;
  /// Started in the background by its parent, which alone shows it.
  background?: boolean;
  /// The custom agent it runs as, which its parent started it as.
  persona: Persona | null;
  /// The session it carries on, on the same machine, when it was started to
  /// continue a chat under another harness.
  continues?: string | null;
}

/// A custom agent a session runs as: its name, and the colour its file gives.
export interface Persona {
  name: string;
  color: string | null;
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
  /// Its node runs the home assistant: this computer's, or one new enough.
  assistant: boolean;
  /// Its node takes part in moving a chat.
  moves_chats: boolean;
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
  sent_at?: number | null;
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
  plan: PlanStep[];
  error: string | null;
  queued: number;
  queued_messages?: string[];
  turns: number;
  usage: AgentUsage | null;
  /// Why the last turn ended, as the node's `StopReason` names it; null before the first.
  last_stop?: string | null;
  /// The entries held, newest last, and how many came before them. A read
  /// brings the newest; older ones are joined on as the reader goes up.
  entries: AgentEntry[];
  earlier: number;
}

/// What a session used, as reported in its latest page.
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

/// What the session header shows of a session's usage: how full the context
/// is, the tokens it spent, and its spend with the credit left when known.
export interface SessionUsage {
  context: { percent: number; size: string } | null;
  tokens: string | null;
  spend: string | null;
}

export function sessionUsage(u: AgentUsage): SessionUsage {
  const context = u.context?.size ? { percent: Math.round((u.context.used * 100) / u.context.size), size: contextTokens.format(u.context.size).replace("K", "k") } : null;
  const tokens = u.tokens ? sessionTokens.format(u.tokens).replace("K", "k") : null;
  const credit = u.credits?.find(c => c.unit === "usd");
  const spent = u.cost_usd == null ? "?" : money(u.cost_usd);

  if (credit) {
    const remaining = credit.remaining == null ? NaN : Number(credit.remaining);
    return { context, tokens, spend: `${spent} / ${Number.isFinite(remaining) ? money(remaining) : "?"}` };
  }

  return { context, tokens, spend: u.cost_usd == null ? null : spent };
}

/// The same in one line of words.
export function usageLine(u: AgentUsage) {
  const { context, tokens, spend } = sessionUsage(u);
  return [context && `${context.percent}%/${context.size}`, tokens && `${tokens} tokens`, spend].filter(Boolean).join(" · ");
}

/// A session `agent_start` or `agent_resume` started, or the one `agent_begin` keeps a start as, and
/// why Auto picked its machine.
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
  /// Starts the Home assistant: no project and no folder.
  assistant: boolean;
  /// A machine id, or "" until one is picked.
  machine: string;
  /// The user picked `machine` in the Machine menu, rather than the form.
  machinePicked: boolean;
  agent: string | null;
  cwd: string;
  /// The project's checkout on each machine, when the form was opened
  /// from one, so picking another machine picks its folder.
  places?: { machine: string; cwd: string }[];
  /// The model picked or typed; empty for the agent's own.
  model?: string;
  /// The machine and agent `model` was picked for, as `machine/agent`.
  modelFor?: string;
  /// The thinking level picked; null for the agent's own.
  effort?: string | null;
  fast?: boolean | null;
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
  /// Folder, Base or Account menu.
  open?: "pick" | "host" | "folder" | "base" | "account" | null;
  /// What the picker's search holds.
  query?: string;
  /// Whether the Folder button is a field to type a folder in.
  typing?: boolean;
}

export type SentImage = { name: string; mime: string; data: string };
/// An image as `agent_start`, `agent_resume` and `agent_steer` take it, without what only the field shows.
export const sentImage = ({ name, mime, data }: SentImage): SentImage => ({ name, mime, data });
/// What `agent_steer` sends: a message, one for the running turn, take
/// those back, stop the turn, close the session, or switch its model or
/// thinking from the next turn.
export type Steer = { prompt: { text: string; images?: SentImage[] } } | { steer: { text: string; images?: SentImage[] } } | { drop_steers: null }
  | { cancel: null } | { stop_all: null } | { close: null } | { configure: { model: string | null; effort: string | null; fast?: boolean | null } } | { mode: { mode: string } } | { stop_task: { task: string } };

interface AgentState {
  /// What the last failed read of the open session said, and how its link is.
  readError: string;
  link: Link;
  hosts: AgentHost[] | null;
  error: string;
  form: AgentForm;
  open: OpenAgent | null;
  /// The project whose empty view shows (no session yet), else null.
  draft: string | null;
  /// The open session's page, kept by `open-page.ts`.
  page: AgentPage | null;
  why: string;
  /// Where Esc goes: "agents", "waiting", "hide", or a window view.
  back: AgentReturn;
  timer: ReturnType<typeof setTimeout> | undefined;
  inflight: OpenAgent | null;
  /// "gone", or an error to show under the log.
  note: string;
  /// Polling: what the last answer looked like, when it last changed,
  /// whether it stopped for quiet, and the question an answer is going to.
  sig: string;
  /// The rest of what the log was last drawn from; empty to draw again.
  rest: string;
  changed: number;
  answering: number | null;
  /// Images pasted or dropped into the session's field, the error from the last
  /// one and when it goes.
  att: Attached[];
  attError: string;
  attTimer: ReturnType<typeof setTimeout> | undefined;
  /// The pasted blocks left open ("place:block").
  opened: Set<number | string>;
  /// What the reader chose for a tool row (true open, false closed), by place
  /// in the session. A row they never touched has no entry.
  steps: Map<number, boolean>;
  /// The steps whose whole output shows, by place in the session.
  all: Set<number>;
  /// Messages waiting on this device for the session's machine.
  outbox: OutboxRow[];
  /// The revision of `outbox`: an older set never replaces a newer one.
  outboxRevision: number;
  /// Reads of the open session so far, which tell one update from the next.
  polls: number;
  /// The session whose turn the user stopped, until it no longer runs.
  stopping: { id: string; turns: number } | null;
  /// Whether the model picker by the field is open.
  modelOpen: boolean;
  /// The open conversation's history read and why it stopped early.
  older: { stop: OlderStop; reading: Promise<void> | null };
}

/// The modes and tasks of the open session, held by `transcripts`.
export const openModes = () => AG.open ? transcripts.modes(transcriptKey(AG.open)) : null;
export const openTasks = () => AG.open ? transcripts.tasks(transcriptKey(AG.open)) : [];

export const AG: AgentState = {
  readError: "", link: LINKED,
  hosts: null, error: "",
  form: { prompt: "", assistant: false, machine: "", machinePicked: false, agent: null, cwd: "" },
  open: null, draft: null, get page() { return currentPage(); }, set page(next) { setPage(next); }, why: "", back: "hide", timer: undefined, inflight: null, note: "",
  sig: "", rest: "", changed: 0, answering: null,
  att: [], attError: "", attTimer: undefined,
  opened: new Set(), steps: new Map(), all: new Set(), outbox: [], outboxRevision: 0, polls: 0, stopping: null, modelOpen: false,
  older: { stop: "", reading: null },
};
/// What a coding agent takes with a message, as the server checks it.
export const AGENT_MAX_IMAGES = 5;
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
export { AGENT_ACTIVE };
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
  agy: { ini: "AG", name: "Antigravity", cls: "agy" },
  pi: { ini: "PI", name: "pi", cls: "pi" },
  flsh: { ini: "FL", name: "flsh", cls: "flsh" },
  opencode: { ini: "OC", name: "OpenCode", cls: "opencode" },
  hermes: { ini: "HE", name: "Hermes", cls: "hermes" },
};
/// Agents whose outside sessions a node can take over (the ones it reads sessions of).
export const canTakeOver = (a: string) => TAKEOVER.has(a);
export const AGENT_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>';

export const agentKind = (a: string): AgentKind => AGENT_KINDS[a] ?? { ini: String(a || "?").slice(0, 2).toUpperCase(), name: a || "Agent", cls: "" };
export const folderName = (p: string) => String(p || "").split("/").filter(Boolean).pop() || p || "";
/// The open session `row` is.
export { openOf, where } from "./conversation-state.ts";

/// Machines and accounts decide which machines run agents: both pushes carry the list.
subscribeEach([topic("machines"), topic("accounts")], event => {
  const pushed = pushedList<AgentHost>(event.data, "agent_hosts");
  if (!pushed) return;

  if ("error" in pushed) { console.warn("agent hosts:", pushed.error); return; }
  takeHosts(pushed.list);
});

function takeHosts(list: AgentHost[]) {
  AG.hosts = list;
  preloadModels(list);
  if (S.view === "agentnew") drawAgentNew();
}

/// The coding agents' named accounts, as Settings last saved them.
let namedAccounts: readonly NamedAccount[] = [];

/// Reads the named accounts again, from this device alone. One that cannot
/// be read keeps the last list.
export async function loadAccountNames(): Promise<readonly NamedAccount[]> {
  try {
    const named = await invoke<NamedAccount[]>("agent_account_names");
    namedAccounts = Array.isArray(named) ? named : [];
  } catch (e) { console.warn("account names:", e); }

  return namedAccounts;
}

/// The names of `agent`'s named accounts.
export function accountsOf(agent: string | null): string[] {
  return namedAccounts.filter(account => account.agent === agent).map(account => account.name);
}

let hostsRead: Promise<AgentHost[]> | null = null;

/// The machines that can run an agent; none for an account that may not use machines. Held from
/// the pushes; before the first one arrives, one read answers, and a push that comes while it is
/// out wins over its answer. One that fails is not kept: `AG.hosts` stays null, the caller gets an
/// empty list for now, and the next call reads again.
export function ensureAgentHosts(): Promise<AgentHost[]> {
  if (AG.hosts) return Promise.resolve(AG.hosts);

  hostsRead ??= invoke<AgentHost[]>("agent_hosts")
    .then(list => { if (!AG.hosts) takeHosts(list); }, e => console.warn("agent hosts:", e))
    .then(() => AG.hosts ?? [])
    .finally(() => { hostsRead = null; });

  return hostsRead;
}

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
  if (S.view === "agentnew") {
    if (cmd && e.key === "Enter") { e.preventDefault(); startAgent(); return true; }
    if (cmd && e.shiftKey && e.key.toLowerCase() === "m") { e.preventDefault(); togglePicker(); return true; }
    return false;
  }

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

/// Opens agent session `session` in the window.
export const openAgentInWindow = (session: AgentRow) => toWindow(mainTarget({ view: "pane", arg: { agent: session } }));

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
