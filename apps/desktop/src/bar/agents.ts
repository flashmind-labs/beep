import { drawAgentNew, renderAgentNew, startAgent, togglePicker } from "./agentnew.ts";
import { closedPicker, drawPicker, type Picked, pillInner, preloadModels } from "./agentpicker.ts";
import { harnessTile } from "../harnessicon.ts";
import type { CheckoutPick } from "./agentpick.ts";
import { clock, commandDetail, commandOf, duration, heading, keptSteps, onlyNotes, span, splitTurns, stepLabel, stepOpen, summary, type Turn, type TurnState } from "../agentturns.ts";
import { fileBase64 } from "./attach.ts";
import { renderChat } from "./chat.ts";
import { $, $$, type BackTo, bar, type ChatBody, esc, h, ICON, invoke, listen, PANE, S, windowHooks } from "./core.ts";
import { hints } from "./hints.ts";
import { openSession } from "./history.ts";
import { GONE, openViewer } from "./media.ts";
import { cap } from "./tools.ts";

// ---------- Coding agents ----------

// Start a coding agent on one of the user's machines, follow it live and
// answer it: `agentnew` is the start form, `agent` one session, `agents`
// every session. They run in the bar and in the window's pane alike.

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

/// One line of a session's log. Only the agent's own have `html`.
export interface AgentEntry {
  kind: string;
  text: string;
  html?: string;
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
  /// The newest entries, and how many came before them.
  entries: AgentEntry[];
  earlier: number;
}

/// A session `agent_start` or `agent_resume` started, and why Auto picked its machine.
export interface Started {
  session: AgentSession;
  why: string;
}

/// An image waiting to go with the next message.
interface Attached {
  name: string;
  mime: string;
  size: number;
  /// Base64, and as a data URL for its chip.
  data: string;
  url: string;
}

/// The session the view follows, as the node knows it.
interface OpenAgent {
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
  /// In a checkout: Auto, the checkout, a new branch, a new worktree or an
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

/// What `agent_steer` sends: a message, stop the turn, close the session,
/// or switch its model or thinking from the next turn.
type Steer = { prompt: { text: string; images?: { name: string; mime: string; data: string }[] } } | { cancel: null } | { close: null }
  | { configure: { model: string | null; effort: string | null } };

interface AgentState {
  readError: string;
  hosts: AgentHost[] | null;
  sessions: AgentSession[];
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
  changed: number;
  paused: boolean;
  answering: number | null;
  barHidden: boolean;
  /// Images pasted or dropped into the session's field, the error from the
  /// last one and when it goes, and the session the server turned images
  /// down for.
  att: Attached[];
  attError: string;
  attTimer: ReturnType<typeof setTimeout> | undefined;
  noImages: string | null;
  /// The tool entries opened to their full output, by place in the session,
  /// the pasted blocks left open ("place:block"), and the module that reads
  /// tool entries, once loaded.
  opened: Set<number | string>;
  log: typeof import("../agentlog.ts") | null;
  /// The steps whose whole output shows, by place in the session.
  all: Set<number>;
  /// The subagent opened as its own conversation, by its call's place.
  child: number | null;
  /// Messages typed while a turn runs, sent when it stops or ends.
  queue: string[];
  flushing: boolean;
  /// Whether the model picker by the field is open.
  modelOpen: boolean;
  /// The picture open in the side panel, at fit or actual size, and the
  /// panel's tab.
  panel: { url: string; name: string; actual: boolean } | null;
  tab: "subagents" | "image";
}

export const AG: AgentState = {
  readError: "",
  hosts: null, sessions: [], error: "", busy: false,
  form: { prompt: "", machine: "", agent: null, cwd: "", chat: null },
  open: null, page: null, why: "", back: "chat", timer: undefined, inflight: null, note: "", sel: 0,
  sig: "", changed: 0, paused: false, answering: null, barHidden: false,
  att: [], attError: "", attTimer: undefined, noImages: null,
  opened: new Set(), log: null, all: new Set(), child: null, queue: [], flushing: false, modelOpen: false, panel: null, tab: "subagents",
};
export const agentLog = () => import("../agentlog.ts").then(m => (AG.log = m));
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
/// Kept while the app runs.
export const agentImages = new Map<string, Promise<string | null> | string | null>();
let pastedCount = 0;
/// How often the session view asks for its session while on screen.
export const AGENT_POLL_MS = 1500;
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
export const AGENT_ORDER = ["claude", "codex", "gemini", "opencode", "pi", "flsh"];
export const CAP_NAMES: Record<string, string> = { gpu: "GPU", cuda: "CUDA", blender: "Blender", unreal: "Unreal", xcode: "Xcode", android: "Android", docker: "Docker" };
export const AGENT_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>';

export const agentKind = (a: string): AgentKind => AGENT_KINDS[a] ?? { ini: String(a || "?").slice(0, 2).toUpperCase(), name: a || "Agent", cls: "" };
export const agentAvatar = (a: string) => harnessTile(a, "dapp dcoderav");
export const folderName = (p: string) => String(p || "").split("/").filter(Boolean).pop() || p || "";
export const byPreference = (a: string, b: string) => (AGENT_ORDER.indexOf(a) + 1 || 99) - (AGENT_ORDER.indexOf(b) + 1 || 99);
/// The session `o` names, as the node's commands take it.
const where = (o: OpenAgent) => ({ machineId: o.machine_id, agent: o.agent, session: o.session });

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

// ----- One session -----

/// Opens session `s` (a session row) and follows it while it is on screen.
export function openAgent(s: AgentRow, { why = "", back: from }: { why?: string; back?: string | BackTo } = {}) {
  if (from === undefined) from = S.view === "agents" ? "agents" : "chat";
  S.view = "agent";
  Object.assign(AG, {
    open: { machine_id: s.machine_id, agent: s.agent, session: s.id },
    page: { session: s, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, turns: 0 },
    why, back: from, note: "", sig: "", changed: Date.now(), paused: false, answering: null,
    att: [], attError: "", noImages: null, opened: new Set(), all: new Set(), child: null, queue: [], flushing: false, modelOpen: false, panel: null, tab: "subagents",
  });
  drawAgentFrame();
  pollAgent();
}

export const isOpen = (o: OpenAgent | null) => S.view === "agent" && AG.open && o === AG.open;

/// Whether the session view can be seen: the window is not hidden, and
/// the bar, when this is the bar, is up.
export const agentOnScreen = () => document.visibilityState === "visible" && !AG.barHidden;

/// How long until the open session is asked again, or null to stop: an
/// ended session, or a quiet one that has not changed for a while.
export function agentDelay() {
  const s = AG.page?.session;
  if (!s || s.status === "ended") return null;
  if (!s.external && AGENT_ACTIVE.has(s.status)) return AGENT_POLL_MS;
  const still = Date.now() - AG.changed;
  if (still >= AGENT_PAUSE_AFTER) return null;
  return still >= AGENT_SLOW_AFTER ? AGENT_SLOW_MS : AGENT_POLL_MS;
}

/// Asks for the open session, then again after `agentDelay()` while it
/// stays on screen. Never two calls at once for one session.
export async function pollAgent() {
  clearTimeout(AG.timer);
  const o = AG.open;
  if (!o || !isOpen(o) || AG.inflight === o || !agentOnScreen()) return;
  AG.inflight = o;
  try {
    const page = await invoke<AgentPage>("agent_view", { ...where(o), last: AGENT_LAST });
    if (isOpen(o)) {
      const sig = JSON.stringify(page);
      if (sig !== AG.sig) { AG.sig = sig; AG.changed = Date.now(); }
      if (page.session.question?.id !== AG.answering) AG.answering = null;
      AG.page = page;
      // What the last failed read said no longer holds.
      if (AG.note === "gone" || AG.note === AG.readError) AG.note = "";
      AG.readError = "";
      drawAgentBody();
      flushQueue();
    }
  } catch (e) {
    if (isOpen(o)) {
      // A dropped connection comes back by itself; the next read says so.
      AG.readError = /not found/i.test(String(e)) ? "" : /connection lost|closed by peer|timed out/i.test(String(e)) ? "Reconnecting to Beep…" : String(e);
      AG.note = AG.readError || "gone";
      drawAgentBody();
    }
  } finally {
    if (AG.inflight === o) AG.inflight = null;
  }
  // Another session opened while this one was asked: follow that one.
  if (AG.open !== o) { if (isOpen(AG.open) && !AG.inflight) pollAgent(); return; }
  if (!isOpen(o) || !agentOnScreen()) return;
  const delay = agentDelay();
  const paused = delay === null && AG.page?.session.status !== "ended";
  if (paused !== AG.paused) { AG.paused = paused; drawAgentBody(); }
  if (delay !== null) AG.timer = setTimeout(pollAgent, delay);
}

/// Follows the open session again after a pause or a message.
export function resumeAgent() {
  AG.changed = Date.now();
  AG.paused = false;
  pollAgent();
}

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") { if (S.view === "agent") pollAgent(); } else clearTimeout(AG.timer);
});
if (!PANE) {
  listen("bar-hidden", () => { AG.barHidden = true; clearTimeout(AG.timer); });
  listen("bar-shown", () => { AG.barHidden = false; if (S.view === "agent") pollAgent(); });
}

/// Whether the server turned images down for the open session.
export const noAgentImages = () => AG.noImages != null && AG.noImages === AG.open?.session;

/// Whether the open session's turn runs, so Enter queues and Esc
/// interrupts, as flsh does.
const turnRuns = () => {
  const s = AG.page?.session;
  return !!s && !s.external && !s.question && AGENT_ACTIVE.has(s.status);
};

/// The session's keys on one small line.
export function agentHints() {
  const s = AG.page?.session, q = s?.question;
  const slim = (items: Parameters<typeof hints>[0]) => hints(items).replace('class="dhints"', 'class="dhints slim"');
  if (q) return slim([[`⌘${ICON.ret}`, "allow"], ["⌘D", "deny"], ["esc", "back"]]);
  if (AG.child != null) return slim([["esc", "parent session"]]);
  if (s?.external) return slim([["esc", "back"]]);
  if (s?.status === "ended") return slim([[ICON.ret, "continue"], ["esc", "back"]]);
  if (turnRuns()) return slim([["esc", AG.queue.length ? "interrupt and send queued" : "interrupt"], [ICON.ret, "queue"]]);
  // Once one is attached its chip says enough.
  const paste = !AG.att.length && !noAgentImages() ? [["⌘V", "paste an image"]] : [];
  return slim([...paste, [ICON.ret, "send"], ["esc", "back"]]);
}

/// Puts the session's key hints in place of those on screen.
function redrawAgentHints() {
  const hint = bar.querySelector(":scope > .dhints");
  if (hint) hint.replaceWith(h(agentHints()));
}

// ----- Images for the agent -----

/// A name for an image, unique among those waiting; a pasted one is
/// named "Pasted image 1.png" and so on rather than "image.png".
export function agentImageName(f: File) {
  const ext = ({ "image/png": "png", "image/jpeg": "jpg", "image/gif": "gif", "image/webp": "webp" } as Record<string, string>)[f.type] || "png";
  const base = f.name && f.name !== `image.${ext}` && f.name !== "image.png" ? f.name : `Pasted image ${++pastedCount}.${ext}`;
  const dot = base.lastIndexOf(".");
  let name = base;
  for (let n = 2; AG.att.some(a => a.name === name); n++) name = dot > 0 ? `${base.slice(0, dot)} ${n}${base.slice(dot)}` : `${base} ${n}`;
  return name;
}

export function agentAttachError(text: string) {
  AG.attError = text;
  drawAgentAttached();
  clearTimeout(AG.attTimer);
  AG.attTimer = setTimeout(() => { AG.attError = ""; drawAgentAttached(); }, 5000);
}

/// Adds pasted or dropped images to the session's next message, refusing
/// what the agent cannot take rather than shrinking or dropping it.
export async function agentAttach(files: Iterable<File>) {
  if (noAgentImages()) return agentAttachError("This Beep server does not take images yet.");
  for (const f of [...files]) {
    if (!AGENT_IMAGE_TYPES.includes(f.type)) { agentAttachError(`${f.name || "That file"} is not a PNG, JPEG, GIF or WebP image`); continue; }
    if (AG.att.length >= AGENT_MAX_IMAGES) { agentAttachError(`Up to ${AGENT_MAX_IMAGES} images in one message`); break; }
    if (f.size > AGENT_MAX_IMAGE) { agentAttachError(`${f.name || "That image"} is over ${AGENT_MAX_IMAGE >> 20} MB`); continue; }
    const used = AG.att.reduce((n, a) => n + a.size, 0);
    if (used + f.size > AGENT_MAX_IMAGES_TOTAL) { agentAttachError(`Images up to ${AGENT_MAX_IMAGES_TOTAL >> 20} MB in all in one message`); continue; }
    try {
      const data = await fileBase64(f);
      AG.att.push({ name: agentImageName(f), mime: f.type, size: f.size, data, url: `data:${f.type};base64,${data}` });
    } catch {
      agentAttachError(`${f.name || "That image"} could not be read`);
    }
  }
  drawAgentAttached();
}

/// The chips of waiting images above the field, as in the mock.
export function drawAgentAttached() {
  const field = $("#agfield");
  if (!field) return;
  const old = $("#agatt");
  const el = AG.att.length || AG.attError ? h(`<div class="ag-att" id="agatt">${AG.att.map((a, i) =>
    `<span class="chip"><img src="${esc(a.url)}" alt=""><span class="nm">${esc(a.name)}</span><button aria-label="Remove ${esc(a.name)}" data-agatt="${i}">${ICON.x}</button></span>`).join("")}${AG.attError ? `<span class="derr">${esc(AG.attError)}</span>` : ""}</div>`) : null;
  el?.querySelectorAll<HTMLElement>("[data-agatt]").forEach(b => b.addEventListener("click", () => {
    AG.att.splice(Number(b.dataset.agatt), 1);
    drawAgentAttached();
    $("#agmsg")?.focus();
  }));
  if (old) el ? old.replaceWith(el) : old.remove();
  else if (el) field.before(el);
  if (S.view === "agent") redrawAgentHints();
}

/// The picture `key` of session `o`, read from its machine the first time.
/// Only one the machine no longer has stays gone; anything else tries again.
export function loadAgentImage(o: OpenAgent, key: string) {
  let got = agentImages.get(key);
  if (got === undefined) {
    got = invoke<string>("agent_image", { machineId: o.machine_id, session: o.session, key }).then(url => {
      agentImages.set(key, url);
      return url;
    }, e => {
      if (String(e) === GONE) agentImages.set(key, null);
      else agentImages.delete(key);
      return null;
    });
    agentImages.set(key, got);
  }
  return Promise.resolve(got);
}

/// Fills the session pictures drawn before their bytes were here.
export function fillAgentPics(b: HTMLElement) {
  const o = AG.open;
  if (!o) return;
  for (const img of b.querySelectorAll<HTMLImageElement>("img.pic[data-key]:not([src])")) {
    loadAgentImage(o, img.dataset.key!).then(url => {
      if (url) img.src = url;
      else if (img.isConnected && agentImages.get(img.dataset.key!) === null) img.replaceWith(h(gonePic(img.alt)));
    });
  }
}

/// A session picture that is not here: its icon and name.
const gonePic = (label: string) => `<span class="pic none" title="${esc(label)}">${ICON.image}${esc(label)}</span>`;

/// The images a message sent from here carried, under the keys the log
/// will name them by, so they show without asking the machine.
function keepSentImages(keys: string[], images: { mime: string; data: string }[]) {
  keys.forEach((key, i) => { if (images[i]) agentImages.set(key, `data:${images[i].mime};base64,${images[i].data}`); });
}

/// Keeps the thumbnails of images just sent, for the transcript.
export function keepAgentPics(o: OpenAgent, images: Attached[]) {
  for (const a of images) {
    const key = `${o.session}\n${a.name}`;
    agentPics.delete(key);
    agentPics.set(key, a.url);
  }
  while (agentPics.size > AGENT_PICS_KEPT) agentPics.delete(agentPics.keys().next().value!);
}

/// A session's folder as the header shows it: the repository and its
/// worktree (`widget · fix-login`), else the folder's name.
export function shortCwd(cwd: string) {
  const m = /^(.*?)\/(?:\.claude\/worktrees|\.worktrees)\/(.+?)\/?$/.exec(cwd);
  if (m) return `${folderName(m[1])} · ${m[2]}`;
  return folderName(cwd) || cwd;
}

/// The harnesses that switch models in a running session.
const SWITCHES = new Set(["claude", "codex", "opencode"]);
/// The model and thinking switched to in each session from here, by session.
const sessionPicks = new Map<string, { model: string; effort: string | null }>();

/// The parts that stay while the session updates: the head, the body and the field.
export function drawAgentFrame() {
  const s = AG.page!.session, k = agentKind(s.agent);
  const field = s.external
    ? `<div class="dmuted" style="display:block;padding:10px 16px;border-top:1px solid var(--border)">Started outside Beep. Beep follows it here; to steer it, ask Beep in a chat to take it over once it stops.</div>`
    : `<div class="dinput small" id="agfield"><input id="agmsg" autocomplete="off" spellcheck="false" placeholder="Message ${esc(k.name)}"><span id="agmodel"></span><button class="dbtn" id="agstop" aria-label="Stop the turn" title="Stop the turn (esc)"><span style="width:9px;height:9px;border-radius:2px;background:currentColor"></span>Stop</button><button class="dbtn quiet" id="agclose">Close</button></div><div id="agpickm"></div>`;
  bar.innerHTML = `<div class="dhead">${agentAvatar(s.agent)}<span class="tt"><b>${esc(k.name)} on ${esc(s.machine_name)}</b><small style="font-family:var(--mono)" title="${esc(s.cwd)}">${esc(shortCwd(s.cwd))}</small></span><span id="agstate">${stateTag(s.status)}</span></div>
    <div class="dbody" id="agbody" style="padding-top:10px"></div>${field}${agentHints()}`;
  drawAgentAttached();
  drawModelPill();
  const b = $$<ChatBody>("#agbody");
  b.stuck = true;
  b.onscroll = () => { b.stuck = b.scrollHeight - b.scrollTop - b.clientHeight < 40; };
  drawAgentBody();
  $("#agmsg")?.focus();
  bar.onclick = agentClick;
}

function agentClick(e: MouseEvent) {
  if (S.view !== "agent") return;
  const target = e.target as HTMLElement;
  const opt = target.closest<HTMLElement>("[data-agopt]");
  if (opt) return answerAgentQuestion(Number(opt.dataset.agopt));
  if (target.closest("#agmodelb")) return toggleModelPicker();
  if (AG.modelOpen && !target.closest(".agc-pick")) closeModelPicker();
  const queued = target.closest<HTMLElement>("[data-agq]");
  if (queued) return editQueued(Number(queued.dataset.agq), queued.dataset.act === "edit");
  // A subagent opens as its own conversation; the crumb goes back.
  const sub = target.closest<HTMLElement>("#agbody [data-sub]");
  if (sub) { AG.child = Number(sub.dataset.sub); drawAgentBody(); $("#agbody")!.scrollTop = 0; return; }
  if (target.closest("#agparent")) return closeChild();
  // "Show all N lines" opens the whole output in a box that scrolls.
  const all = target.closest<HTMLElement>("#agbody [data-all]");
  if (all) {
    const at = Number(all.dataset.all);
    if (!AG.all.delete(at)) AG.all.add(at);
    return drawAgentBody();
  }
  if (target.closest("#agbody pre.out.all")) return;
  // A finished turn's heading or folded line opens its steps; the heading folds them again.
  const turn = target.closest<HTMLElement>("#agbody [data-turn]");
  if (turn && AG.open) {
    const opened = turnsOf(AG.open.session).opened, key = Number(turn.dataset.turn);
    if (!opened.delete(key)) opened.add(key);
    return drawAgentBody();
  }
  // The side panel: its tabs, closing it, fit or actual size, full size.
  const tab = target.closest<HTMLElement>("#agbody [data-tab]");
  if (tab) { AG.tab = tab.dataset.tab as AgentState["tab"]; return drawAgentBody(); }
  if (target.closest("#agbody [data-panelclose]")) { AG.panel = null; AG.tab = "subagents"; return drawAgentBody(); }
  const fit = target.closest<HTMLElement>("#agbody [data-fit]");
  if (fit && AG.panel) { AG.panel.actual = fit.dataset.fit === "1"; return drawAgentBody(); }
  if (target.closest("#agbody [data-full]") && AG.panel) {
    const shown = AG.panel;
    return openViewer([{ url: shown.url, name: shown.name, caption: "", saves: shown.url.startsWith("data:") }], 0);
  }
  if (target.closest("#agbody .dimgbox")) return;
  // A file the reply names opens its picture, as its thumbnail does.
  const link = target.closest<HTMLElement>("#agbody a.imgl[data-img]");
  const linked = link ? link.closest(".dcoderen")?.querySelectorAll<HTMLImageElement>(":scope > .pics img.pic")[Number(link.dataset.img)] : null;
  if (link && !linked) return;
  // The agent's pictures open in the side panel when there is room; any
  // other, or without room, full size. One still loading opens once it is here.
  const pic = linked ?? target.closest<HTMLImageElement>("#agbody img.pic");
  if (pic) {
    if (!pic.getAttribute("src")) return;
    const name = (link ? link.textContent : null) || pic.dataset.name || pic.alt;
    if (!pic.closest(".dcoderen.user") && $("#agbody")!.clientWidth >= PANEL_MIN) {
      AG.panel = { url: pic.src, name, actual: false };
      AG.tab = "image";
      return drawAgentBody();
    }
    const all = [...pic.parentElement!.querySelectorAll<HTMLImageElement>("img.pic[src]")];
    // Each opens by its data URL, which can be saved or copied.
    const shown = all.map(i => ({ url: i.src, name: i.dataset.name || i.alt, caption: "", saves: i.src.startsWith("data:") }));
    return openViewer(shown, all.indexOf(pic));
  }
  const tool = target.closest<HTMLElement>("#agbody .dcoderen.tool[data-at]");
  if (tool && !getSelection()!.toString()) {
    const at = Number(tool.dataset.at);
    if (!AG.opened.delete(at)) AG.opened.add(at);
    return drawAgentBody();
  }
  // A pasted block opens by itself; remember it so a redraw keeps it open.
  const paste = target.closest<HTMLElement>("#agbody .dcoderen.user[data-at] details.paste > summary");
  if (paste) {
    const key = `${paste.closest<HTMLElement>("[data-at]")!.dataset.at}:${paste.parentElement!.dataset.paste}`;
    if (!AG.opened.delete(key)) AG.opened.add(key);
    return;
  }
  if (target.closest("#agparent2")) return closeChild();
  if (target.closest("#agresume")) return resumeAgent();
  if (target.closest("#agstop")) return steerAgent({ cancel: null });
  if (target.closest("#agclose")) return steerAgent({ close: null });
}

/// A user entry's text and the images it carried: the node writes each
/// as a line `[image: name]` after the text.
export function userImages(text: string) {
  const lines = String(text).split("\n"), names: string[] = [];
  while (lines.length) {
    const m = /^\[image: (.+)\]$/.exec(lines[lines.length - 1]);
    if (!m) break;
    names.unshift(m[1]);
    lines.pop();
  }
  return { text: lines.join("\n"), names };
}

/// A step's mark: done, failed, running or did not finish.
function stepMark(status: string) {
  const running = AGENT_ACTIVE.has(AG.page?.session.status);
  return status === "completed" ? `<span class="dok" title="Done">${ICON.check}</span>`
    : status === "failed" ? `<span class="dbad" title="Failed">${ICON.x}</span>`
    : status && running ? `<span class="dspin" title="Running"></span>`
    : status ? '<span class="ddot off" title="Did not finish"></span>' : "";
}

/// A tool entry as one line: its mark and a short label ("Ran cargo
/// test"). Opened, the label wraps to the whole command and the last lines
/// of its output follow, then "Show all N lines" for all of it in a box
/// that scrolls. A failed step is one line with its exit code like any
/// other; only the newest step of a running turn starts open when it
/// failed. `at` is its place in the session, which keeps it open across
/// redraws. `pics` are the pictures it showed, drawn last. A subagent's
/// call is a card.
export function toolRow(en: AgentEntry, at: number, pics = "") {
  const log = AG.log!;
  const { title, status, output } = log.parseTool(en.text);
  if (log.subagent(output)) return subCard([at]) + (pics ? `<div class="dcoderen tool">${pics}</div>` : "");
  const failed = status === "failed", open = stepOpen(failed, at === newestStep, AG.opened.has(at));
  const live = status === "in_progress" && AGENT_ACTIVE.has(AG.page?.session.status);
  const label = stepLabel(en.text, live, open);
  const exit = failed ? log.exitCode(output) : null;
  const long = title.includes("\n") || label.length > 80;
  const detail = commandDetail(title, label);
  const tail = log.tailLines(output), whole = AG.all.has(at);
  const body = !open ? ""
    : (detail ? `<pre class="cmd">${esc(detail)}</pre>` : "")
      + (output ? `<pre class="out${whole ? " all" : ""}">${esc(whole ? output : tail.shown)}</pre>` : "")
      + (tail.cut ? `<button class="more" data-all="${at}">${whole ? "Show less" : `Show all ${tail.total === 1 ? "of it" : `${tail.total} lines`}`}</button>` : "");
  const opens = !!output || long;
  return `<div class="dcoderen tool${failed ? " failed" : ""}${open ? " open" : ""}" data-row="${at}"${opens ? ` data-at="${at}"` : ""}>`
    + `<span class="mk">${stepMark(status)}</span><code class="tt" title="${esc(commandOf(title))}">${esc(label)}</code>`
    + (failed ? `<span class="exit">${exit != null ? `exit ${exit}` : "failed"}</span>` : "")
    + body + pics + "</div>";
}

const SUB_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V13a5 5 0 0 0 5 5h4.5"/></svg>';

/// What the window saw of each subagent: when it started and ended, by
/// session and the call's place. Calls carry no times.
const subTimes = new Map<string, { start: number; live: boolean; end?: number }>();

/// A subagent's call as its parts: name, how it stands, its steps.
function subOf(at: number) {
  const p = AG.page!, en = p.entries[at - p.earlier];
  const { title, status, output } = AG.log!.parseTool(AG.log!.imageLines(en.text).text);
  const sub = AG.log!.subagent(output) ?? { count: 0, steps: [], result: output };
  const working = (status === "in_progress" || status === "pending") && AGENT_ACTIVE.has(p.session.status);
  // Timed from when this window saw it working, if it did.
  const key = `${AG.open?.session}\n${at}`, now = Date.now();
  let seen = subTimes.get(key);
  if (!seen) subTimes.set(key, seen = { start: now, live: working });
  if (!working && seen.live && seen.end == null) seen.end = now;
  const ms = !seen.live ? null : working ? now - seen.start : (seen.end ?? now) - seen.start;
  return { title: title.split("\n")[0], status, working, failed: status === "failed", sub, ms };
}

/// One subagent as a row: its name, how it stands, how long and how many steps.
function subRow(at: number) {
  const x = subOf(at);
  const st = x.working ? "working" : x.failed ? "failed" : "done";
  const meta = [st, x.ms != null ? duration(x.ms) : "", x.sub.count === 1 ? "1 step" : `${x.sub.count} steps`].filter(Boolean).join(" · ");
  return `<button class="dsubrow" data-sub="${at}" title="Open ${esc(x.title)}"><span class="mk">${stepMark(x.working ? "in_progress" : x.status)}</span><span class="t"><b>${esc(x.title)}</b><small>${esc(meta)}</small></span></button>`;
}

/// Subagents started together as one card: "3 subagents · 2 working · 1 done".
function subCard(ats: number[]) {
  const all = ats.map(subOf), working = all.filter(x => x.working).length, failed = all.filter(x => x.failed).length;
  const done = all.length - working - failed;
  const counts = [working && `${working} working`, done && `${done} done`, failed && `${failed} failed`].filter(Boolean).join(" · ");
  const head = ats.length > 1 ? `<div class="h">${SUB_ICON}<b>${ats.length} subagents</b><span>${esc(counts)}</span></div>` : "";
  return `<div class="dsubgrp" data-row="${ats[0]}">${head}${ats.map(subRow).join("")}</div>`;
}

/// How wide the log must be for the side panel to show; narrower, a
/// picture opens full size and the panel hides.
const PANEL_MIN = 820;

/// The side panel: the session's subagents, and the picture opened from
/// the log, each a tab when both are there.
function sidePanel(subs: number[]) {
  const img = AG.panel;
  if (!subs.length && !img) return "";
  const tab = img && (AG.tab === "image" || !subs.length) ? "image" : "subagents";
  const tabs = `<div class="tabs">${subs.length ? `<button data-tab="subagents" class="${tab === "subagents" ? "on" : ""}">Subagents <small>${subs.length}</small></button>` : ""}`
    + (img ? `<button data-tab="image" class="${tab === "image" ? "on" : ""}">Image</button><span class="sp"></span><button data-panelclose="1" aria-label="Close the image">${ICON.x}</button>` : "") + "</div>";
  const body = tab === "image" && img
    ? `<div class="nm" title="${esc(img.name)}">${esc(img.name)}</div><div class="dimgbox${img.actual ? " actual" : ""}"><img src="${esc(img.url)}" alt="${esc(img.name)}"></div>
      <div class="acts"><button class="dbtn${img.actual ? "" : " on"}" data-fit="0">Fit</button><button class="dbtn${img.actual ? " on" : ""}" data-fit="1">Actual size</button><button class="dbtn" data-full="1" title="Full size, to save or copy">Open full size</button></div>`
    : subs.map(subRow).join("");
  return `<aside class="dsubs">${tabs}${body}</aside>`;
}

/// The places of the subagents' calls among the entries shown.
function subagentsShown(p: AgentPage) {
  if (!AG.log) return [];
  return p.entries.flatMap((en, i) => en.kind === "tool" && AG.log!.subagent(AG.log!.parseTool(en.text).output) ? [p.earlier + i] : []);
}

/// A subagent opened as its own conversation: the way back, what it was
/// asked, its steps and what it answered.
function childHtml(p: AgentPage, at: number) {
  const x = subOf(at), s = p.session, k = agentKind(s.agent);
  const steps = x.sub.steps.map(st => `<div class="dcoderen tool"><span class="mk">${stepMark(st.status)}</span><code class="tt" title="${esc(st.title)}">${esc(stepLabel(`${st.title} [${st.status}]`, st.status === "in_progress" && x.working))}</code></div>`).join("");
  const earlier = x.sub.count > x.sub.steps.length ? `<div class="dmuted">${x.sub.count - x.sub.steps.length} earlier steps not shown</div>` : "";
  const none = !x.sub.count && !x.working ? `<div class="dcoderen other">Steps not reported by ${esc(k.name)}.</div>` : "";
  const result = x.sub.result ? `<div class="dcoderen agent">${esc(x.sub.result)}</div>` : x.working ? '<div class="dmuted"><span class="dspin"></span> Working…</div>' : "";
  return `<div class="dcoderlog"><div class="dcrumb"><button id="agparent">${esc(s.title || "Session")}</button><span>›</span><b>${esc(x.title)}</b><span class="sp"></span><button class="dbtn" id="agparent2" data-parent="1">Open parent<kbd>esc</kbd></button></div>
    <div class="dcoderen user"><b>Task</b><span>${esc(x.title)}</span></div>
    <div class="dcoderturn"><span class="hd">${esc(x.working ? "Working" : x.ms != null ? `Worked for ${duration(x.ms)}` : "Worked")}</span></div>
    ${earlier}${steps}${none}${result}</div>`;
}

/// Back from a subagent to its session.
export function closeChild() {
  if (AG.child == null) return false;
  AG.child = null;
  drawAgentBody();
  return true;
}

/// An image a user entry carried: the one sent from here, the one read
/// from the session's machine by its key (an empty tile while it loads),
/// or its name when neither can be.
function entryPic(n: string) {
  const key = /^image-([0-9a-f]{16})$/.exec(n)?.[1], label = key ? "Image" : n;
  const got = key ? agentImages.get(key) : undefined;
  const url = (AG.open && agentPics.get(`${AG.open.session}\n${n}`)) || (typeof got === "string" ? got : "");
  const named = ` data-name="${esc(n)}" alt="${esc(label)}" title="${esc(label)}"`;
  if (url) return `<img class="pic" src="${esc(url)}"${named}>`;
  if (key && got !== null) return `<img class="pic" data-key="${key}"${named}>`;
  return gonePic(label);
}

export function entryRow(en: AgentEntry, at: number) {
  const text = esc(en.text);
  // Codex echoes answered questions as a message: show them as answered.
  const answered = en.kind === "user" && AG.log ? AG.log.answeredQuestions(en.text) : null;
  if (answered) return `<div class="dcoderen qa" data-row="${at}">${answered.map(x => `${x.question ? `<span class="q">${esc(x.question)}</span>` : ""}<span class="a">${esc(x.answer)}</span>`).join("")}</div>`;
  if (en.kind === "user") {
    const { text: said, names } = userImages(en.text);
    const user = (s: string) => AG.log ? AG.log.userHtml(s, esc, i => AG.opened.has(`${at}:${i}`)) : esc(s);
    if (!names.length) return `<div class="dcoderen user" data-at="${at}"><b>You</b><span>${user(en.text)}</span></div>`;
    return `<div class="dcoderen user" data-at="${at}"><b>You</b>${said ? `<span>${user(said)}</span>` : ""}<span class="pics">${names.map(entryPic).join("")}</span></div>`;
  }
  // The pictures the agent or a tool showed go after the text, as a
  // user's do. The HTML is already without their lines.
  const { text: said, names } = AG.log ? AG.log.imageLines(en.text) : { text: en.text, names: [] };
  const pics = names.length ? `<span class="pics">${names.map(entryPic).join("")}</span>` : "";
  // The HTML is the desktop's own Markdown render, cleaned of anything that runs.
  // Image files the reply names link to its pictures, shown in a strip.
  const linked = (html: string) => AG.log ? AG.log.linkImagePaths(html).html : html;
  if (en.kind === "agent") return en.html ? `<div class="dcoderen agent md" data-row="${at}">${linked(en.html)}${pics}</div>` : `<div class="dcoderen agent" data-row="${at}">${linked(esc(said))}${pics}</div>`;
  if (en.kind === "tool") return AG.log ? toolRow({ ...en, text: said }, at, pics) : `<div class="dcoderen tool">${text}</div>`;
  return `<div class="dcoderen other" data-row="${at}">${text}</div>`;
}

/// What this window knows of a session's turns: the ones it has seen,
/// when it saw a turn start or end (entries carry no times), the turn
/// running at the last draw, and the finished turns opened to their steps.
/// Kept in memory only, by session.
interface TurnMemory {
  known: Set<number>;
  starts: Map<number, number>;
  ends: Map<number, number>;
  running: number | null;
  opened: Set<number>;
}

const turnMemory = new Map<string, TurnMemory>();

function turnsOf(session: string) {
  let m = turnMemory.get(session);
  if (!m) turnMemory.set(session, m = { known: new Set(), starts: new Map(), ends: new Map(), running: null, opened: new Set() });
  return m;
}

/// Notes the turns that started or ended since the last draw. The turns
/// there at the first draw started before this window looked.
function noteTurns(m: TurnMemory, turns: Turn[], running: number | null) {
  const now = Date.now(), first = !m.known.size;
  for (const t of turns) {
    if (!m.known.has(t.key) && !first) m.starts.set(t.key, now);
    m.known.add(t.key);
  }
  if (m.running != null && m.running !== running && !m.ends.has(m.running)) m.ends.set(m.running, now);
  m.running = running;
}

const FOLD_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 6h16M4 12h16M4 18h10"/></svg>';

/// The newest step of the running turn, which starts open when it failed.
let newestStep: number | null = null;

/// Rows for `ats` in order, subagents started one after another as one card.
function rowsOf(p: AgentPage, ats: number[]) {
  const out: string[] = [];
  let run: number[] = [];
  const isSub = (at: number) => {
    const en = p.entries[at - p.earlier];
    return en.kind === "tool" && !!AG.log?.subagent(AG.log.parseTool(en.text).output);
  };
  const flush = () => { if (run.length) out.push(subCard(run)); run = []; };
  for (const at of ats) {
    if (isSub(at)) { run.push(at); continue; }
    flush();
    out.push(entryRow(p.entries[at - p.earlier], at));
  }
  flush();
  return out.join("");
}

/// The log in turns: each message, then a heading with how long the agent
/// worked, then its work and its answer. Done, the work folds into one
/// summary line, failures counted in it; notes, subagents and pictures
/// stay in view. While it runs, only the newest step shows, with the
/// thought's line.
function turnsHtml(p: AgentPage) {
  const s = p.session, active = AGENT_ACTIVE.has(s.status);
  newestStep = null;
  const turns = splitTurns(p.entries, p.earlier), lastTurn = turns.at(-1);
  const m = turnsOf(AG.open?.session ?? s.id);
  noteTurns(m, turns, active && lastTurn ? lastTurn.key : null);
  const row = (at: number) => entryRow(p.entries[at - p.earlier], at);
  const entry = (at: number) => p.entries[at - p.earlier];
  return turns.map(t => {
    // Notes before the first message are not a turn of work.
    if (onlyNotes(p.entries, t, p.earlier)) return t.steps.map(row).join("");
    const last = t === lastTurn, running = active && last;
    const user = t.user == null ? "" : row(t.user);
    if (!running && !t.steps.length && !t.answer.length) return user;
    const { start, end } = span({
      first: p.earlier === 0 && t.key === 0, last, running,
      seenStart: m.starts.get(t.key), seenEnd: m.ends.get(t.key),
      sessionStart: s.started_at, lastActivity: s.last_activity,
    });
    const state: TurnState = !running ? "done" : s.status === "asking" ? "waiting" : "working";
    const ms = start == null ? null : running ? Date.now() - start : end == null ? null : end - start;
    const opened = m.opened.has(t.key);
    const label = `<span${running && start != null ? ` data-since="${start}" data-state="${state}"` : ""}>${esc(heading(state, ms))}</span>`;
    const head = `<div class="dcoderturn">`
      + (!t.steps.length ? `<span class="hd">${label}</span>`
        : `<button class="hd${opened ? " open" : ""}" data-turn="${t.key}" aria-expanded="${opened}">${label}${ICON.chev}</button>`)
      + (start != null ? `<span class="when">${clock(start)}</span>` : "") + "</div>";
    let work: string;
    // The newest step of a running turn, shown as it goes.
    const newest = running ? [...t.steps].reverse().find(at => entry(at).kind === "tool") : undefined;
    if (newest != null) newestStep = newest;
    if (opened) work = rowsOf(p, t.steps);
    else {
      const kept = new Set(keptSteps(p.entries, t, p.earlier));
      if (newest != null) kept.add(newest);
      const folding = t.steps.filter(at => !kept.has(at));
      // Pictures the folded steps looked at show small under the line.
      const seen = folding.flatMap(at => entry(at).kind === "tool" && AG.log ? AG.log.imageLines(entry(at).text).names : []);
      const fold = folding.some(at => entry(at).kind === "tool")
        ? `<button class="dcoderfold" data-turn="${t.key}"><span class="mk">${FOLD_ICON}</span><span>${esc(summary(p.entries, t, p.earlier, folding))}</span><span class="go">${ICON.chev}</span></button>`
          + (seen.length ? `<div class="dcoderen dfoldpics" data-row="${t.key}"><span class="pics">${seen.map(entryPic).join("")}</span></div>` : "") : "";
      work = fold + rowsOf(p, t.steps.filter(at => kept.has(at)));
    }
    // What the agent thinks or does now, on one dim line, unless the
    // newest step says it already.
    const doing = s.doing?.split("\n")[0].trim() ?? "";
    const shownTitle = newest != null ? AG.log?.parseTool(entry(newest).text).title.trim() : "";
    const thought = running && state === "working" && doing && doing !== shownTitle
      ? `<div class="dcoderthought" title="${esc(s.doing ?? "")}">${esc(doing === "Thinking" ? "Thinking…" : doing)}</div>` : "";
    return user + head + work + thought + t.answer.map(row).join("");
  }).join("");
}

let turnTick: ReturnType<typeof setInterval> | undefined;

/// Moves the running turn's clock on between polls, and stops once no
/// turn runs on screen.
function tickTurns() {
  const tick = () => {
    const els = S.view === "agent" ? bar.querySelectorAll<HTMLElement>("#agbody [data-since]") : null;
    if (!els?.length) { clearInterval(turnTick); turnTick = undefined; return; }
    for (const el of els) el.textContent = heading(el.dataset.state as TurnState, Date.now() - Number(el.dataset.since));
  };
  if (!turnTick && bar.querySelector("#agbody [data-since]")) turnTick = setInterval(tick, 1000);
}

/// The question's answers; the first that allows and the first that does
/// not take ⌘Return and ⌘D.
function questionCard(q: AgentQuestion) {
  const allow = q.options.findIndex(x => x.allow), deny = q.options.findIndex(x => !x.allow);
  const buttons = q.options.map((o, i) => `<button class="dbtn${i === allow ? " pri" : i === deny ? " warn" : ""}" data-agopt="${i}"${q.id === AG.answering ? " disabled" : ""}>${esc(o.name)}${i === allow ? `<kbd>⌘${ICON.ret}</kbd>` : i === deny ? "<kbd>⌘D</kbd>" : ""}</button>`);
  return `<div class="q"><b style="font-weight:600">${esc(q.text)}</b>${q.detail ? `<code style="font-family:var(--mono);font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere">${esc(q.detail)}</code>` : ""}
      <div class="dups">${buttons.join("")}</div></div>`;
}

/// Where the reader is in the log: the first row in view and how far it
/// sits from the top, so a redraw puts it back there.
function scrollAnchor(b: HTMLElement) {
  const top = b.getBoundingClientRect().top;
  for (const el of b.querySelectorAll<HTMLElement>("[data-row]")) {
    const r = el.getBoundingClientRect();
    if (r.bottom > top) return { row: el.dataset.row!, off: r.top - top, scroll: b.scrollTop };
  }
  return { row: null, off: 0, scroll: b.scrollTop };
}

function restoreScroll(b: HTMLElement, a: ReturnType<typeof scrollAnchor>) {
  const el = a.row != null ? b.querySelector<HTMLElement>(`[data-row="${a.row}"]`) : null;
  if (el) b.scrollTop += el.getBoundingClientRect().top - b.getBoundingClientRect().top - a.off;
  else b.scrollTop = a.scroll;
}

export function drawAgentBody() {
  const b = $<ChatBody>("#agbody"), p = AG.page;
  if (!b || !p) return;
  const s = p.session, q = s.question;
  const st = $("#agstate");
  if (st) st.innerHTML = stateTag(s.status);
  if (!AG.log) agentLog().then(() => { if (S.view === "agent") drawAgentBody(); }).catch(console.warn);
  // Read where the reader is before the log is drawn again: only a reader
  // at the bottom follows new entries; anyone else stays where they are.
  const anchor = b.stuck === false ? scrollAnchor(b) : null;
  const child = AG.child != null && AG.log && p.entries[AG.child - p.earlier] ? AG.child : null;
  if (child == null) AG.child = null;
  const mark = (x: string) => x === "completed" ? `<span class="dok">${ICON.check}</span>` : x === "in_progress" ? (s.status === "asking" ? '<span class="ddot" style="background:var(--coral)"></span>' : '<span class="dspin"></span>') : '<span class="ddot off"></span>';
  const question = q ? questionCard(q) : "";
  const plan = p.plan.length ? `<div class="sec">Plan</div><div class="plan">${p.plan.map(x => `<div class="row"><span style="width:16px;display:grid;place-items:center">${mark(x.status)}</span><span style="${x.status === "pending" ? "color:var(--muted)" : ""}">${esc(x.content)}</span></div>`).join("")}</div>` : "";
  const files = p.files.length ? `<div class="sec">Files changed</div><div class="files">${p.files.slice(-12).map(f => `<span><b>${esc(f)}</b></span>`).join("")}${p.files.length > 12 ? `<span style="color:var(--muted)">and ${p.files.length - 12} more</span>` : ""}</div>` : "";
  const queued = p.queued ? `<div class="row" style="padding:8px 12px 0;color:var(--muted)">${p.queued === 1 ? "1 message waits" : `${p.queued} messages wait`} for the turn to end</div>` : "";
  // The question, plan and files sit under the log, by the field.
  const card = question || plan || files || queued
    ? `<div class="dcard dcoder">${question}${plan}${files}${queued}<div style="height:12px"></div></div>` : "";
  const why = AG.why ? `<div class="dmuted">Auto picked ${esc(s.machine_name)}. ${esc(AG.why)}</div>` : "";
  const earlier = p.earlier ? `<div class="dmuted">${p.earlier} earlier ${p.earlier === 1 ? "entry" : "entries"} not shown</div>` : "";
  const entries = child != null ? childHtml(p, child) : p.entries.length && AG.log ? `<div class="dcoderlog">${earlier}${turnsHtml(p)}</div>`
    : s.status === "starting" ? '<div class="dmuted"><span class="dspin"></span> Starting the agent…</div>' : "";
  const err = p.error ? `<div class="derr">${esc(p.error)}</div>` : "";
  const note = AG.note === "gone" ? '<div class="dmuted">This session is no longer on the machine, or the machine went offline.</div>'
    : AG.note && AG.note === AG.readError && AG.note.startsWith("Reconnecting") ? `<div class="dmuted">${esc(AG.note)}</div>`
    : AG.note ? `<div class="derr">${esc(AG.note)}</div>` : "";
  const paused = AG.paused ? '<button class="dbtn quiet" id="agresume">Paused, click to refresh</button>' : "";
  const subs = child == null ? subagentsShown(p) : [];
  const aside = sidePanel(subs);
  b.innerHTML = `<div class="dagsplit${aside ? " has" : ""}"><div class="dagmain">${why}${entries}${child == null ? card : ""}${err}${note}${paused}</div>${aside}</div>`;
  fillAgentPics(b);
  drawModelPill();
  drawQueue();
  tickTurns();
  redrawAgentHints();
  // An ended session goes on from the field: the next message resumes it.
  const msg = $<HTMLInputElement>("#agmsg"), k = agentKind(s.agent);
  if (msg) msg.placeholder = s.status === "ended" ? `Message ${k.name} to continue` : `Message ${k.name}`;
  const close = $("#agclose");
  if (close) close.style.display = s.status === "ended" ? "none" : "";
  // Only a turn that runs can be stopped.
  const stop = $("#agstop");
  if (stop) stop.style.display = AGENT_ACTIVE.has(s.status) ? "" : "none";
  if (b.stuck !== false) b.scrollTop = b.scrollHeight;
  else if (anchor) restoreScroll(b, anchor);
}

// ----- Model and thinking -----

/// The pill by the field: the harness, the model and thinking switched to.
/// A harness that cannot switch in a running session shows it disabled.
function drawModelPill() {
  const el = $("#agmodel"), s = AG.page?.session;
  if (!el || !s) return;
  const can = SWITCHES.has(s.agent) && !s.external && s.status !== "ended";
  const p = sessionPicks.get(s.id) ?? { model: "", effort: null };
  const why = can ? "Switch the model or thinking; applies from the next turn" : `${agentKind(s.agent).name} cannot switch models in a running session`;
  el.innerHTML = `<button class="agc-pill agm" id="agmodelb" aria-expanded="${AG.modelOpen}" title="${esc(why)}"${can ? "" : " disabled"}>${pillInner(s.agent, p.model, p.effort, s.machine_id)}</button>`;
}

function toggleModelPicker() {
  AG.modelOpen = !AG.modelOpen;
  drawModelPicker();
  drawModelPill();
}

/// Closes the model picker; true when it was open, so Esc goes no further.
export function closeModelPicker() {
  if (!AG.modelOpen) return false;
  AG.modelOpen = false;
  drawModelPicker();
  drawModelPill();
  $("#agmsg")?.focus();
  return true;
}

/// The picker by the pill, for the session's own harness: a model or a
/// thinking level picked goes to the session for its next turn.
function drawModelPicker() {
  const el = $("#agpickm"), s = AG.page?.session;
  if (!el) return;
  if (!AG.modelOpen || !s) { el.innerHTML = ""; closedPicker(el); return; }
  const now = sessionPicks.get(s.id) ?? { model: "", effort: null };
  const picked: Picked = { agent: s.agent, model: now.model, effort: now.effort, query: "" };
  drawPicker(el, {
    picked,
    offered: [s.agent],
    everywhere: [s.agent],
    machine: { id: s.machine_id, name: s.machine_name },
    noMachine: "",
    anchor: () => $("#agmodelb"),
    bounds: () => bar,
    changed: what => {
      if (what === "models") return drawModelPill();
      if (what === "agent") return;
      const model = what === "model" ? picked.model || null : null, effort = what === "effort" ? picked.effort : null;
      sessionPicks.set(s.id, { model: picked.model, effort: picked.effort });
      if (what === "model") AG.modelOpen = false;
      drawModelPicker();
      drawModelPill();
      if (model || effort) steerAgent({ configure: { model, effort } });
    },
  });
}

// ----- Messages typed while a turn runs -----

/// The queued messages above the field, each to edit or take out.
function drawQueue() {
  const field = $("#agfield");
  if (!field) return;
  const old = $("#agqueue");
  const el = AG.queue.length ? h(`<div class="ag-queue" id="agqueue">${AG.queue.map((q, i) =>
    `<div class="it"><span class="lbl">Queued</span><span class="t" title="${esc(q)}">${esc(q)}</span><button data-agq="${i}" data-act="edit">Edit</button><button data-agq="${i}" aria-label="Remove">${ICON.x}</button></div>`).join("")}</div>`) : null;
  if (old) el ? old.replaceWith(el) : old.remove();
  else if (el) ($("#agatt") ?? field).before(el);
}

/// Takes queued message `i` out; to edit, it goes back in the field.
function editQueued(i: number, edit: boolean) {
  const [text] = AG.queue.splice(i, 1);
  const input = $<HTMLInputElement>("#agmsg");
  if (edit && input && text != null) input.value = input.value ? `${text}\n${input.value}` : text;
  drawQueue();
  redrawAgentHints();
  input?.focus();
}

/// Queues a message typed while the turn runs, as flsh does.
function queueMessage(input: HTMLInputElement) {
  const text = input.value.trim();
  if (!text) return;
  AG.queue.push(text);
  input.value = "";
  drawQueue();
  redrawAgentHints();
}

/// Sends what was queued once the turn stopped or ended, as one message.
/// An ended session takes it back into the field, to continue with.
function flushQueue() {
  const o = AG.open, page = AG.page, s = page?.session;
  if (!AG.queue.length || !o || !page || !s || AG.flushing || AGENT_ACTIVE.has(s.status)) return;
  const text = AG.queue.join("\n\n");
  AG.queue = [];
  drawQueue();
  redrawAgentHints();
  if (s.status === "ended" || s.external) {
    const input = $<HTMLInputElement>("#agmsg");
    if (input && !input.value) input.value = text;
    return;
  }
  AG.flushing = true;
  page.entries.push({ kind: "user", text });
  drawAgentBody();
  steerAgent({ prompt: { text } }).then(ok => {
    AG.flushing = false;
    if (!ok && isOpen(o)) { AG.queue.unshift(text); drawQueue(); redrawAgentHints(); }
  });
}

/// Esc while the turn runs: stop it and keep the field, so a correction
/// can be typed and sent; what is queued goes once the stop lands.
export function interruptAgent() {
  if (!turnRuns()) return false;
  steerAgent({ cancel: null });
  $("#agmsg")?.focus();
  return true;
}

/// Answers the open question once: its buttons stay disabled, and a poll
/// that still shows it does not bring them back, until the node moves on.
export async function answerAgentQuestion(i: number) {
  const o = AG.open!, q = AG.page?.session.question, opt = q?.options[i];
  if (!q || !opt || AG.answering === q.id) return;
  AG.answering = q.id;
  drawAgentBody();
  try {
    await invoke("center_answer_agent", { ...where(o), question: q.id, reply: opt.id });
    if (isOpen(o) && AG.page!.session.question?.id === q.id) {
      AG.page!.session.question = null;
      AG.page!.session.status = "working";
    }
  } catch (e) {
    // A conflict means it was answered already, so it stays answered.
    const conflict = /^conflict/i.test(String(e));
    if (!conflict && AG.answering === q.id) AG.answering = null;
    if (isOpen(o)) AG.note = conflict ? "" : String(e);
  }
  if (isOpen(o)) { drawAgentBody(); resumeAgent(); }
}
/// The first option of the open question that allows, or that does not.
export function answerFirst(allow: boolean) {
  const q = AG.page?.session.question;
  const i = q ? q.options.findIndex(o => o.allow === allow) : -1;
  if (i >= 0) answerAgentQuestion(i);
  return i >= 0;
}

/// Sends `steer` to the open session; true when the node took it.
export async function steerAgent(steer: Steer) {
  const o = AG.open;
  if (!o || AG.page?.session.external) return false;
  let ok = false;
  try {
    const keys = await invoke<string[]>("agent_steer", { ...where(o), steer: "prompt" in steer || "configure" in steer ? steer : Object.keys(steer)[0] });
    if ("prompt" in steer && steer.prompt.images) keepSentImages(keys, steer.prompt.images);
    AG.note = "";
    ok = true;
  } catch (e) {
    AG.note = String(e).replace(/^bad request: /, "");
  }
  if (isOpen(o)) { drawAgentBody(); resumeAgent(); }
  return ok;
}

export async function sendAgentMessage() {
  const input = $<HTMLInputElement>("#agmsg"), text = input?.value.trim() ?? "", images = AG.att;
  if (!text && !images.length) return;
  const o = AG.open!, page = AG.page!;
  if (page.session.status === "ended") return continueAgent(input!, text, images);
  input!.value = "";
  if (!images.length) {
    page.entries.push({ kind: "user", text });
    drawAgentBody();
    return steerAgent({ prompt: { text } });
  }
  AG.att = [];
  drawAgentAttached();
  keepAgentPics(o, images);
  const mine = { kind: "user", text: [text, ...images.map(a => `[image: ${a.name}]`)].filter(Boolean).join("\n") };
  page.entries.push(mine);
  drawAgentBody();
  const sent = await steerAgent({ prompt: { text, images: images.map(({ name, mime, data }) => ({ name, mime, data })) } });
  if (sent || !isOpen(o)) return;
  // Nothing went: the message and its images wait in the field again.
  if (/does not take images yet/.test(AG.note)) AG.noImages = o.session;
  // A poll may have drawn more since: take out this message, not the last.
  const at = page.entries.indexOf(mine);
  if (at >= 0) page.entries.splice(at, 1);
  const field = $<HTMLInputElement>("#agmsg");
  if (field && !field.value) field.value = text;
  AG.att = images;
  drawAgentBody();
  drawAgentAttached();
}

/// Resumes the ended session with `text`, then follows the session that
/// goes on from it. Images go once it runs again.
export async function continueAgent(input: HTMLInputElement, text: string, images: Attached[]) {
  const o = AG.open!;
  if (images.length && !text) { AG.note = "Say something to continue the session, then send the images."; return drawAgentBody(); }
  input.value = "";
  input.disabled = true;
  AG.note = "";
  AG.page!.entries.push({ kind: "user", text });
  drawAgentBody();
  try {
    const started = await invoke<Started>("agent_resume", { ...where(o), prompt: text });
    if (!isOpen(o)) return;
    openAgent(started.session, { back: AG.back });
    if (images.length) { AG.att = images; drawAgentAttached(); }
  } catch (e) {
    if (!isOpen(o)) return;
    AG.page!.entries.pop();
    input.disabled = false;
    input.value = text;
    AG.note = String(e).replace(/^(bad request|error):\s*/i, "");
    drawAgentBody();
  }
}

// ----- Every session -----

export async function renderAgents() {
  S.view = "agents";
  AG.sel = 0;
  drawAgents(true);
  try {
    AG.sessions = await invoke<AgentSession[]>("agent_sessions");
    AG.error = "";
  } catch (e) {
    AG.error = String(e);
  }
  if (S.view === "agents") drawAgents(false);
}

function sessionRow(s: AgentSession, i: number) {
  const k = agentKind(s.agent), folder = folderName(s.cwd);
  const tail = s.status === "asking" ? '<span class="dstate ask">Needs you</span>'
    : s.status === "working" || s.status === "running" || s.status === "starting" ? `<span class="run"><span class="dspin"></span>${esc(s.doing || "Working")}</span>`
    : `<span class="w">${s.external ? "In terminal" : esc(cap(s.status))}</span>`;
  return `<button class="dchat" role="option" aria-selected="${i === AG.sel}" data-agi="${i}">
      <span class="t"><span>${esc(s.title || folder || k.name)}</span><small>${esc([k.name, folder, s.machine_name].filter(Boolean).join(" · "))}</small></span>${tail}</button>`;
}

export function drawAgents(loading: boolean) {
  const list = AG.sessions;
  const machines = new Set(list.map(s => s.machine_id)).size;
  const sum = loading && !list.length ? "Looking at your machines…" : list.length ? `${list.length} on ${machines} ${machines === 1 ? "machine" : "machines"}` : "Nothing running";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Coding agents</b><small>${esc(sum)}</small></span><button class="dbtn" id="agNew">${AGENT_ICON}New<kbd>⌘⇧A</kbd></button></div>
    ${AG.error ? `<div class="derr" style="padding:8px 18px 0">${esc(AG.error)}</div>` : ""}
    ${list.length ? `<div class="dlist" role="listbox" style="padding-top:6px">${list.map(sessionRow).join("")}</div>`
      : loading ? "" : `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${AGENT_ICON}</span><b style="font-size:14px">No sessions running.</b>
        <span class="dmuted" style="display:block">Start one here, ask Beep in a chat, or run Claude Code in a terminal on a linked machine.</span></div>`}
    ${hints([[ICON.ret, "open"], ["⌘⇧A", "new"], ["esc", "back"]])}`;
  bar.onclick = e => {
    if (S.view !== "agents") return;
    const target = e.target as HTMLElement;
    const r = target.closest<HTMLElement>("[data-agi]");
    if (r) return openAgent(AG.sessions[Number(r.dataset.agi)], { back: "agents" });
    if (target.closest("#agNew")) renderAgentNew("", "agents");
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
    if (cmd && e.key === ".") { e.preventDefault(); steerAgent({ cancel: null }); return true; }
    if (e.key === "Enter" && !e.isComposing && (e.target as HTMLElement).id === "agmsg") {
      e.preventDefault();
      // While the turn runs, Enter queues; the queue goes when it stops.
      if (turnRuns()) queueMessage(e.target as HTMLInputElement);
      else sendAgentMessage();
      return true;
    }
    return false;
  }
  if (S.view === "agents") {
    const n = AG.sessions.length;
    if (!n) return false;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      AG.sel = (AG.sel + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
      drawAgents(false);
      return true;
    }
    if (e.key === "Enter") { e.preventDefault(); openAgent(AG.sessions[AG.sel], { back: "agents" }); return true; }
  }
  return false;
}

/// Esc from an agent view: closes the model picker, leaves a subagent for
/// its session, interrupts a running turn, else goes back.
export function agentBack() {
  if (S.view === "agent" && (closeModelPicker() || closeChild() || interruptAgent())) return;
  if (typeof AG.back === "object" && AG.back.view && windowHooks) return windowHooks.goBack(AG.back);
  if ((S.view === "agent" || S.view === "agentnew") && AG.back === "agents") return renderAgents();
  return S.session ? openSession(S.session) : renderChat();
}
