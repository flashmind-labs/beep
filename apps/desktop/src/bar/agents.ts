import { drawAgentNew, renderAgentNew, startAgent, togglePicker } from "./agentnew.ts";
import { preloadModels } from "./agentpicker.ts";
import { harnessTile } from "../harnessicon.ts";
import type { CheckoutPick } from "./agentpick.ts";
import { clock, folded, heading, span, splitTurns, summary, type Turn, type TurnState } from "../agentturns.ts";
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

/// What `agent_steer` sends: a message, or stop the turn, or close the session.
type Steer = { prompt: { text: string; images?: { name: string; mime: string; data: string }[] } } | { cancel: null } | { close: null };

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
}

export const AG: AgentState = {
  readError: "",
  hosts: null, sessions: [], error: "", busy: false,
  form: { prompt: "", machine: "", agent: null, cwd: "", chat: null },
  open: null, page: null, why: "", back: "chat", timer: undefined, inflight: null, note: "", sel: 0,
  sig: "", changed: 0, paused: false, answering: null, barHidden: false,
  att: [], attError: "", attTimer: undefined, noImages: null,
  opened: new Set(), log: null,
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
    att: [], attError: "", noImages: null, opened: new Set(),
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

export function agentHints() {
  const s = AG.page?.session, q = s?.question;
  if (q) return hints([[`⌘${ICON.ret}`, "allow"], ["⌘D", "deny"], ["esc", "back"]]);
  if (s?.external) return hints([["esc", "back"]]);
  if (s?.status === "ended") return hints([[ICON.ret, "continue"], ["esc", "back"]]);
  // Once one is attached its chip says enough.
  const paste = !AG.att.length && !noAgentImages() ? [["⌘V", "paste an image"]] : [];
  const stop = AGENT_ACTIVE.has(s?.status) ? [["⌘.", "stop the turn"]] : [];
  return hints([...paste, [ICON.ret, "send"], ...stop, ["esc", "back"]]);
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

/// The parts that stay while the session updates: the head, the body and the field.
export function drawAgentFrame() {
  const s = AG.page!.session, k = agentKind(s.agent);
  const field = s.external
    ? `<div class="dmuted" style="display:block;padding:10px 16px;border-top:1px solid var(--border)">Started outside Beep. Beep follows it here; to steer it, ask Beep in a chat to take it over once it stops.</div>`
    : `<div class="dinput small" id="agfield"><input id="agmsg" autocomplete="off" spellcheck="false" placeholder="Message ${esc(k.name)}"><button class="dbtn" id="agstop" aria-label="Stop the turn"><span style="width:9px;height:9px;border-radius:2px;background:currentColor"></span>Stop<kbd>⌘.</kbd></button><button class="dbtn quiet" id="agclose">Close session</button></div>`;
  bar.innerHTML = `<div class="dhead">${agentAvatar(s.agent)}<span class="tt"><b>${esc(k.name)} on ${esc(s.machine_name)}</b><small style="font-family:var(--mono)">${esc(s.cwd)}</small></span><span id="agstate">${stateTag(s.status)}</span></div>
    <div class="dbody" id="agbody" style="padding-top:10px"></div>${field}${agentHints()}`;
  drawAgentAttached();
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
  // A finished turn's heading or folded line opens its steps; the heading folds them again.
  const turn = target.closest<HTMLElement>("#agbody [data-turn]");
  if (turn && AG.open) {
    const opened = turnsOf(AG.open.session).opened, key = Number(turn.dataset.turn);
    if (!opened.delete(key)) opened.add(key);
    return drawAgentBody();
  }
  // A picture opens full size, also one inside a tool row. One still
  // loading opens once it is here.
  const pic = target.closest<HTMLImageElement>("#agbody img.pic");
  if (pic) {
    if (!pic.getAttribute("src")) return;
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

/// A tool entry as a row: a mark for how it went, its title in mono and
/// the first lines of its output. `at` is its place in the session, which
/// keeps it open across redraws. `pics` are the pictures it showed, drawn
/// last.
export function toolRow(en: AgentEntry, at: number, pics = "") {
  const log = AG.log!;
  const { title, status, output } = log.parseTool(en.text);
  const open = AG.opened.has(at), { shown, more } = log.toolLines(output, open);
  const running = AGENT_ACTIVE.has(AG.page?.session.status);
  const mark = status === "completed" ? `<span class="dok" title="Done">${ICON.check}</span>`
    : status === "failed" ? `<span class="dbad" title="Failed">${ICON.x}</span>`
    : status && running ? `<span class="dspin" title="Running"></span>`
    : status ? '<span class="ddot off" title="Did not finish"></span>' : "";
  const long = title.length > 140 || title.includes("\n");
  const opens = more > 0 || open || long;
  const toggle = more > 0 ? `<span class="more">${more === 1 ? "1 more line" : `${more} more lines`}</span>`
    : open && (output.includes("\n") || long) ? '<span class="more">Show less</span>' : "";
  return `<div class="dcoderen tool${status === "failed" ? " failed" : ""}${open ? " open" : ""}"${opens ? ` data-at="${at}"` : ""}>`
    + `<span class="mk">${mark}</span><code class="tt">${esc(title)}</code>`
    + (shown ? `<pre class="out">${esc(shown)}</pre>` : "") + toggle + pics + "</div>";
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
  if (en.kind === "agent") return en.html ? `<div class="dcoderen agent md">${en.html}${pics}</div>` : `<div class="dcoderen agent">${esc(said)}${pics}</div>`;
  if (en.kind === "tool") return AG.log ? toolRow({ ...en, text: said }, at, pics) : `<div class="dcoderen tool">${text}</div>`;
  return `<div class="dcoderen other">${text}</div>`;
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

/// The log in turns: each message, then a heading with how long the agent
/// worked, then its steps (one line once the turn is done, unless opened)
/// and its answer.
function turnsHtml(p: AgentPage) {
  const s = p.session, active = AGENT_ACTIVE.has(s.status);
  const turns = splitTurns(p.entries, p.earlier), lastTurn = turns.at(-1);
  const m = turnsOf(AG.open?.session ?? s.id);
  noteTurns(m, turns, active && lastTurn ? lastTurn.key : null);
  const row = (at: number) => entryRow(p.entries[at - p.earlier], at);
  return turns.map(t => {
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
    const fold = folded(t, running, m.opened.has(t.key));
    const label = `<span${running && start != null ? ` data-since="${start}" data-state="${state}"` : ""}>${esc(heading(state, ms))}</span>`;
    const head = `<div class="dcoderturn">`
      + (running || !t.steps.length ? `<span class="hd">${label}</span>`
        : `<button class="hd${fold ? "" : " open"}" data-turn="${t.key}" aria-expanded="${!fold}">${label}${ICON.chev}</button>`)
      + (start != null ? `<span class="when">${clock(start)}</span>` : "") + "</div>";
    const steps = fold
      ? `<button class="dcoderfold" data-turn="${t.key}"><span class="mk">${FOLD_ICON}</span><span>${esc(summary(p.entries, t, p.earlier))}</span><span class="go">${ICON.chev}</span></button>`
      : t.steps.map(row).join("");
    return user + head + steps + t.answer.map(row).join("");
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

export function drawAgentBody() {
  const b = $<ChatBody>("#agbody"), p = AG.page;
  if (!b || !p) return;
  const s = p.session, q = s.question;
  const st = $("#agstate");
  if (st) st.innerHTML = stateTag(s.status);
  const mark = (x: string) => x === "completed" ? `<span class="dok">${ICON.check}</span>` : x === "in_progress" ? (s.status === "asking" ? '<span class="ddot" style="background:var(--coral)"></span>' : '<span class="dspin"></span>') : '<span class="ddot off"></span>';
  // What the agent says it is doing now, on one line, unless the log's
  // last row already shows it (an agent that names its running command).
  const last = p.entries.at(-1);
  const shown = last?.kind === "tool" && AG.log?.parseTool(last.text).title.trim() === s.doing?.trim();
  const doing = s.doing && s.status !== "ended" && !shown
    ? `<div class="row" style="padding:8px 12px 0;color:var(--muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis" title="${esc(s.doing)}">${esc(s.doing.split("\n")[0])}</div>` : "";
  const question = q ? questionCard(q) : "";
  const plan = p.plan.length ? `<div class="sec">Plan</div><div class="plan">${p.plan.map(x => `<div class="row"><span style="width:16px;display:grid;place-items:center">${mark(x.status)}</span><span style="${x.status === "pending" ? "color:var(--muted)" : ""}">${esc(x.content)}</span></div>`).join("")}</div>` : "";
  const files = p.files.length ? `<div class="sec">Files changed</div><div class="files">${p.files.slice(-12).map(f => `<span><b>${esc(f)}</b></span>`).join("")}${p.files.length > 12 ? `<span style="color:var(--muted)">and ${p.files.length - 12} more</span>` : ""}</div>` : "";
  const queued = p.queued ? `<div class="row" style="padding:8px 12px 0;color:var(--muted)">${p.queued === 1 ? "1 message waits" : `${p.queued} messages wait`} for the turn to end</div>` : "";
  const card = doing || question || plan || files || queued
    ? `<div class="dcard dcoder">${doing}${question}${plan}${files}${queued}<div style="height:12px"></div></div>` : "";
  const why = AG.why ? `<div class="dmuted">Auto picked ${esc(s.machine_name)}. ${esc(AG.why)}</div>` : "";
  const earlier = p.earlier ? `<div class="dmuted">${p.earlier} earlier ${p.earlier === 1 ? "entry" : "entries"} not shown</div>` : "";
  if (!AG.log) agentLog().then(() => { if (S.view === "agent") drawAgentBody(); }).catch(console.warn);
  const entries = p.entries.length ? `<div class="dcoderlog">${earlier}${turnsHtml(p)}</div>`
    : s.status === "starting" ? '<div class="dmuted"><span class="dspin"></span> Starting the agent…</div>' : "";
  const err = p.error ? `<div class="derr">${esc(p.error)}</div>` : "";
  const note = AG.note === "gone" ? '<div class="dmuted">This session is no longer on the machine, or the machine went offline.</div>'
    : AG.note && AG.note === AG.readError && AG.note.startsWith("Reconnecting") ? `<div class="dmuted">${esc(AG.note)}</div>`
    : AG.note ? `<div class="derr">${esc(AG.note)}</div>` : "";
  const paused = AG.paused ? '<button class="dbtn quiet" id="agresume">Paused, click to refresh</button>' : "";
  b.innerHTML = why + card + entries + err + note + paused;
  fillAgentPics(b);
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
    const keys = await invoke<string[]>("agent_steer", { ...where(o), steer: "prompt" in steer ? steer : Object.keys(steer)[0] });
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
    if (e.key === "Enter" && !e.isComposing && (e.target as HTMLElement).id === "agmsg") { e.preventDefault(); sendAgentMessage(); return true; }
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

/// Esc from an agent view.
export function agentBack() {
  if (typeof AG.back === "object" && AG.back.view && windowHooks) return windowHooks.goBack(AG.back);
  if ((S.view === "agent" || S.view === "agentnew") && AG.back === "agents") return renderAgents();
  return S.session ? openSession(S.session) : renderChat();
}
