import type { ProviderCredits } from "../ui/credits.ts";
// agents/: coding agents in the bar: the start form, the picker, one
// session's log and messages, and every session. Entry: this file.
// Start a coding agent on one of the user's machines, follow it live and
// answer it: `agentnew` is the start form, `agent` one session, `agents`
// every session. They run in the bar and in the window's pane alike. This
// file holds their shared types and state, and the Every session list.
import { preloadModels } from "./model-store.ts";
import type { CheckoutPick } from "./agentpick.ts";
import type { Attached, OutboxRow } from "./agentqueue.ts";
import { type Link, LINKED } from "./reconnect-state.ts";
import { type AgentReturn } from "../bar/core.ts";
import type { AgentQuestion } from "./running-state.ts";
import { AGENT_ACTIVE, TAKEOVER } from "./sessionmenu-state.ts";
import { transcriptKey, transcripts } from "./transcript-cache-state.ts";
import type { PlanStep } from "./plan-state.ts";
import type { OlderStop } from "./older-state.ts";
import { money } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import type { NamedAccount } from "../settings/agentaccounts.ts";
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { pushedList } from "../ui/snapshot-state.ts";
import { currentPage, setPage } from "./open-page.ts";
import { type WorkPlace } from "./place-state.ts";

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
/// Agents whose outside sessions a node can take over (the ones it reads sessions of).
export const canTakeOver = (a: string) => TAKEOVER.has(a);

/// The open session `row` is.
export { openOf, where } from "./conversation-state.ts";

/// Machines and accounts decide which machines run agents: both pushes carry the list.
subscribeEach([topic("machines"), topic("accounts")], event => {
  const pushed = pushedList<AgentHost>(event.data, "agent_hosts");
  if (!pushed) return;

  if ("error" in pushed) { console.warn("agent hosts:", pushed.error); return; }
  takeHosts(pushed.list);
});

const hostListeners = new Set<() => void>();

/// `redraw` runs when the agent hosts change.
export function onAgentHosts(redraw: () => void) {
  hostListeners.add(redraw);
}

function takeHosts(list: AgentHost[]) {
  AG.hosts = list;
  preloadModels(list);
  for (const redraw of hostListeners) redraw();
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
