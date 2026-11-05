// chat/: chats and groups in the bar: the open chat, the list, the composer,
// attachments and the cards under a reply. Entry: this file.
import { drawAgentButton } from "../agents/agents.ts";
import { askFiles, attach, attached, attachError, drawAttached } from "./attach.ts";
import type { AskFile } from "./attach.ts";
import { type Chart, chartRow } from "./charts.ts";
import { drawChats, loadChats, touch } from "./chats.ts";
import { coderCard } from "./codercard.ts";
import { composeField } from "./compose.ts";
import { $, $$, bar, changed, entry, here, live, newId, queued, S, stopping } from "../bar/core.ts";
import { AnsweredIds } from "./approvals.ts";
import { appTile, fillAppTiles } from "./applogo.ts";
import type { LiveEntry, PendingApproval } from "../bar/core.ts";
import { chatHints, drawStop, refreshHints } from "../bar/hints.ts";
import { loadTurnAgent, sourceRow } from "./history.ts";
import type { WebSource } from "./history.ts";
import { heard, takeDraft } from "../bar/live.ts";
import { fillPics, picRow, productRow, replyPic, sentFile } from "./media.ts";
import type { PicRef, SentFile, SentImage } from "./media.ts";
import { canPickModel, modelName, MODELS } from "../settings/model.ts";
import { replyActs, stoppedRow } from "./rerun.ts";
import { routeRow } from "./routes.ts";
import { DESCRIBED, doneLabel, QUIET_TOOLS, toolLabel } from "./tools.ts";
import { attachLive } from "../agents/thoughtlive.ts";
import { answerHtml, errorCardHtml, queueRowHtml, liveLineHtml, stepMark, thoughtHtml, tickTurns, toggleThought, toolRowHtml, turnHeadHtml, userRowHtml } from "../agents/turnrows.ts";
import { esc, linkified, plural } from "../ui/format.ts";
import type { ToolKind } from "../agents/agentturns.ts";
import { refreshStatus } from "../bar/views.ts";
import { Channel, invoke } from "../ui/tauri.ts";
import { h } from "../ui/dom.ts";
import { ICON } from "../ui/icons.ts";
import type { ChatBody, Step, Turn } from "../ui/turn.ts";

// ---------- Chat ----------

/// One line of an approval card.
export interface ApprovalField {
  label: string;
  value: string;
}

/// An action waiting for the user's OK (`Asking`).
export interface Asking {
  query_id: string;
  app: string;
  action: string;
  fields: ApprovalField[];
}

/// The user's answer to an approval card.
export type Decision = "allow" | "session" | "always" | "deny";

/// What a tool sent with its result, shown under the reply.
export interface ToolOutput {
  file: SentFile | null;
  /// A `send_image` result's pictures.
  images: SentImage[];
  /// A `show_products` result's cards, as HTML.
  products: string | null;
  /// A `show_cards` result's cards, as HTML.
  cards: string | null;
  chart: Chart | null;
  /// A `code_agent` result's text, which names the session its card shows.
  code_agent?: string | null;
}

/// What a run streams into the bar (`BarEvent`).
export type BarEvent =
  | { kind: "started"; run_id: string; session_id: string }
  /// The reply so far, as sanitized HTML.
  | { kind: "text"; html: string }
  | { kind: "reasoning"; delta: string }
  | { kind: "tool_started"; call_id: string; name: string; description: string }
  | ({ kind: "tool_result"; call_id: string; name: string; success: boolean } & ToolOutput)
  | { kind: "sources"; sources: WebSource[] }
  | { kind: "replies"; replies: string[] }
  | ({ kind: "approval" } & Asking)
  /// Jev picked the model and thinking for this prompt.
  | { kind: "auto_picked"; model: string; thinking: string }
  | { kind: "transcript"; text: string }
  | { kind: "error"; message: string }
  | { kind: "finished"; status: string };

/// What this module reads of `MODELS.info` (`Models`).
interface ModelPick {
  auto: boolean;
  selected: string;
  thinking: string;
}

/// The right-edge map of prompts, which keeps the turns its ticks stand for.
interface PromptMap extends HTMLElement {
  turns?: HTMLElement[];
}

export function renderChat() {
  S.view = "chat";
  bar.innerHTML = `<div class="dinput dgrow"><textarea id="ask" rows="1" placeholder="Ask Beep…" autocomplete="off" spellcheck="false"></textarea><div class="dgrip" id="grip" role="separator" aria-orientation="horizontal" aria-label="Resize the prompt" title="Drag to resize, double-click to reset"></div></div>
    <div class="dbody" id="body"></div>${chatHints()}`;
  const kept = here()?.body;
  if (kept) $$("#body").replaceWith(kept);
  const b = body()!;
  b.onscroll = () => { b.stuck = b.scrollHeight - b.scrollTop - b.clientHeight < 40; drawLatest(); markMap(); };
  drawLatest();
  drawQueue();
  drawAttached();
  const ask = $$<HTMLTextAreaElement>("#ask");
  let multi = false, typed = false;
  ask.addEventListener("input", () => {
    grow(ask);
    if (multi !== ask.value.includes("\n")) { multi = !multi; refreshHints(); }
    if (typed !== !!ask.value.trim()) { typed = !typed; refreshHints(); }
  });
  dragGrip(ask);
  composeField(ask, { images: attach });
  takeDraft(ask);
  drawStop();
  drawAgentButton();
  focusAsk();
}
export const ASK_HEIGHT = "bar.askHeight", ASK_MAX = 420;
/// A height the user dragged the bar's prompt to; null lets it follow its lines.
export let askHeight: number | null = null;
try { askHeight = Number(localStorage.getItem(ASK_HEIGHT)) || null; } catch {}
/// The prompt grows with its lines, up to six, then scrolls. A height the
/// user dragged it to is the least it shows, so its lines still fit.
export function grow(ask: HTMLTextAreaElement) {
  ask.style.height = "auto";
  const lines = Math.min(ask.scrollHeight, 156);
  const dragged = askHeight && ask.id === "ask" ? Math.min(askHeight, ASK_MAX) : 0;
  ask.style.height = Math.max(lines, dragged) + "px";
}
/// Dragging the grip under the prompt sets its height, kept for next time.
export function dragGrip(ask: HTMLTextAreaElement) {
  const grip = $$("#grip");
  grip.addEventListener("pointerdown", e => {
    e.preventDefault();
    grip.setPointerCapture(e.pointerId);
    grip.classList.add("on");
    const startY = e.clientY, startH = ask.offsetHeight;
    const move = (m: PointerEvent) => {
      // A click on the grip is not a drag: it would pin the prompt to one line.
      if (Math.abs(m.clientY - startY) < 4) return;
      askHeight = Math.max(26, Math.min(startH + m.clientY - startY, ASK_MAX));
      grow(ask);
    };
    const up = () => {
      grip.classList.remove("on");
      grip.removeEventListener("pointermove", move);
      grip.removeEventListener("pointerup", up);
      try { if (askHeight) localStorage.setItem(ASK_HEIGHT, String(askHeight)); } catch {}
    };
    grip.addEventListener("pointermove", move);
    grip.addEventListener("pointerup", up);
  });
  grip.addEventListener("dblclick", () => {
    askHeight = null;
    try { localStorage.removeItem(ASK_HEIGHT); } catch {}
    grow(ask);
  });
  grow(ask);
}
export function focusAsk() { $("#ask")?.focus(); }
export function body() { return $<ChatBody>("#body"); }
export function scrollDown() { const b = body(); if (b) { b.stuck = true; b.scrollTop = b.scrollHeight; } }
/// Follows a streaming reply down, unless the user scrolled up to read.
export function keepDown() { if (body()?.stuck !== false) scrollDown(); }

/// While the user reads back, a button that jumps to the newest text.
export function drawLatest() {
  const want = S.view === "chat" && body()?.stuck === false;
  const btn = $("#latest");
  if (!want) return btn?.remove();
  if (btn) return;
  const el = h(`<button class="dbtn dlatest" id="latest"><span class="down">${ICON.chev}</span>Latest</button>`);
  el.addEventListener("click", () => { scrollDown(); drawLatest(); focusAsk(); });
  bar.append(el);
}

/// Past this many prompts the map hides until the chat is hovered.
export const MAP_QUIET_AFTER = 12;

/// A tick per prompt on the right edge once the chat scrolls: hover one to read
/// the prompt, click it to jump there.
export function drawMap() {
  const b = body();
  const turns = b && S.view === "chat" ? ([...b.children] as HTMLElement[]).filter(t => t.querySelector(ASK)) : [];
  let map = $<PromptMap>("#map");
  if (!b || turns.length < 2 || b.scrollHeight <= b.clientHeight + 4) return map?.remove();
  if (!map) map = bar.appendChild(h<PromptMap>('<div class="dmap" id="map" aria-label="Prompts in this chat"></div>'));
  // A long chat's ticks are a wall of noise: they show only on hover.
  map.classList.toggle("many", turns.length > MAP_QUIET_AFTER);
  if (map.turns?.length !== turns.length || map.turns.some((t, i) => t !== turns[i])) {
    map.turns = turns;
    map.replaceChildren(...turns.map(t => {
      const tk = h('<button class="tk"><i></i><span class="tip"></span></button>');
      const q = askText(t);
      tk.querySelector(".tip")!.textContent = q;
      tk.setAttribute("aria-label", q);
      tk.addEventListener("click", () => { b.scrollTo({ top: t.offsetTop - b.offsetTop - 6, behavior: "smooth" }); focusAsk(); });
      return tk;
    }));
  }
  map.style.top = b.offsetTop + "px";
  map.style.height = b.clientHeight + "px";
  turns.forEach((t, i) => {
    const at = (t.offsetTop - b.offsetTop) / b.scrollHeight * 100;
    (map.children[i] as HTMLElement).style.top = Math.min(97, Math.max(3, at)) + "%";
  });
  markMap();
}
/// Marks the prompt whose turn is at the top of the view.
export function markMap() {
  const b = body(), map = $<PromptMap>("#map");
  if (!b || !map?.turns) return;
  let cur = 0;
  map.turns.forEach((t, i) => { if (t.offsetTop - b.offsetTop <= b.scrollTop + b.clientHeight / 3) cur = i; });
  [...map.children].forEach((tk, i) => tk.classList.toggle("on", i === cur));
}

export function newTurn(question: string, images?: PicRef[], files?: AskFile[]) {
  const turn = makeTurn(question, images, files);
  body()!.append(turn);
  return turn;
}
/// A turn for `question`, not yet in any chat.
export function makeTurn(question: string, images?: PicRef[], files?: AskFile[]) {
  const turn = h<Turn>(`<div class="turn dcoderlog">${question ? userRowHtml(linkified(question)) : ""}${picRow(images)}</div>`);
  fillPics(turn);
  if (files?.length) turn.append(askFiles(files));
  turn.steps = [];
  turn.folded = true;
  return turn;
}

/// The model and thinking a prompt ran on, under it.
export function ranOn(turn: Turn, model: string, thinking: string) {
  if (!model) return;
  const line = canPickModel()
    ? (thinking ? `${modelName(model)} · ${thinking}` : modelName(model))
    : thinking;
  if (!line) return;
  turn.querySelector(".dranon")?.remove();
  turn.querySelector(ASK)?.after(h(`<div class="drerunof dranon">${esc(line)}</div>`));
}

/// The user's prompt in a turn, and the answer.
export const ASK = ":scope > .dcoderen.user", ANSWER = ":scope > .dcoderen.agent";
/// What the prompt of `turn` says.
export const askText = (turn: Element) => turn.querySelector(`${ASK} > span`)?.textContent ?? "";

/// The log's kind of call for a Beep tool, for its row's icon.
function stepKind(name: string): ToolKind {
  if (/^(web_search|browser|open_browser)$/.test(name)) return "web";
  if (/^(exec|node|desktop|code_agent)$/.test(name)) return "command";
  if (name === "attach") return "read";
  return name.startsWith("mcp") || (name && name === name.toUpperCase()) ? "mcp" : "other";
}

const STATUS: Record<Step["state"], string> = { run: "in_progress", ok: "completed", bad: "failed", stop: "pending" };

/// A step as the coding agent's log draws a call: its verb, then what it acts on.
function stepRow(t: Step) {
  const label = t.state === "ok" || t.state === "bad" ? doneLabel(t.label) : t.label;
  const [, verb = label, target = ""] = /^(\S+)\s+([\s\S]*)$/.exec(label) ?? [];
  return toolRowHtml({ kind: stepKind(t.name), verb, target, tip: label, detail: t.state === "bad" ? "failed" : "", bad: t.state === "bad", mark: stepMark(STATUS[t.state], t.state === "run") });
}

/// Turns drawn so far, so each thought has its own key.
let turnCount = 0;

/// Draws what the turn did before its answer, as a coding agent's turn shows
/// it: the heading with how long it worked, the thought, and the steps. A
/// running turn shows its newest step; a finished one folds them into the
/// heading until it is opened.
export function drawWork(turn: Turn) {
  let box = turn.querySelector<HTMLElement>(":scope > .tools");
  if (!box) {
    box = h('<div class="tools dcoderlog"></div>');
    box.dataset.key = String(++turnCount);
    box.addEventListener("click", e => {
      const btn = (e.target as Element).closest("[data-thought]");
      if (btn && turn.thought) { turn.thought.touched = true; turn.thought.open = toggleThought(btn); return; }
      if (!(e.target as Element).closest("[data-turn]")) return;
      turn.folded = !turn.folded;
      drawWork(turn);
    });
    const after = turn.querySelector(`${ANSWER}, :scope > .dcard, :scope > .derrcard`);
    after ? turn.insertBefore(box, after) : turn.append(box);
  }
  const running = !turn.done, steps = turn.steps, th = turn.thought, opened = !turn.folded;
  const shown = opened ? steps : running ? steps.slice(-1) : [];
  let thought = "";
  if (th && (running || opened || !steps.length)) {
    const live = running && th.ms == null, folding = !live && !th.touched && !th.ended;
    if (!live) th.ended = true;
    thought = thoughtHtml({ key: `chat:${box.dataset.key}`, text: th.text, ms: th.ms, live, folding, open: live ? !th.touched || !!th.open : !!th.open || folding });
  }
  // Once the answer streams it says the turn is alive.
  const live = running && !turn.querySelector(ANSWER);
  const thinking = (th != null && th.ms == null) || !steps.some(t => t.state === "run");
  const ms = turn.begun == null ? null : (turn.ended ?? Date.now()) - turn.begun;
  box.innerHTML = turnHeadHtml({ state: running ? "working" : "done", start: turn.begun, ms, stats: steps.length ? plural(steps.length, "tool call", "tool calls") : "", key: steps.length ? box.dataset.key! : null, opened })
    + thought + shown.map(stepRow).join("") + (live ? liveLineHtml({ state: "working", thinking, start: turn.begun }) : "");
  attachLive(box, () => { if (turn.thought) turn.thought.touched = true; });
  tickTurns();
}

/// The thought streaming into `turn` ended: something else came.
function endThought(turn: Turn) {
  if (turn.thought && turn.thought.ms == null) turn.thought.ms = Date.now() - turn.thought.start;
}

export function reply(turn: Turn) {
  let el = turn.querySelector(ANSWER);
  if (!el) {
    el = h('<div class="dcoderen agent md"></div>');
    const files = turn.querySelector(".files");
    files ? turn.insertBefore(el, files) : turn.append(el);
  }
  return el;
}

/// What a tool sent, under the reply. `route` is a `get_route` call's id,
/// whose transit card is fetched.
export function toolOutput(turn: Turn, x: ToolOutput, route: string | null) {
  if (x.file) sentFile(turn, x.file);
  for (const p of x.images ?? []) replyPic(turn, p);
  if (x.products) productRow(turn, x.products);
  if (x.cards) productRow(turn, x.cards);
  if (x.chart) chartRow(turn, x.chart);
  if (route) routeRow(turn, route);
  if (x.code_agent) coderCard(turn, x.code_agent);
}

/// A routine asking for a grant (`request_grant`). A grant always ends, so
/// its card has no Always, and the Always key allows it.
export const forRoutine = (action: string | null | undefined) => /for a routine$/.test(action ?? "");

/// Turn approvals answered in this window.
export const answeredTurns = new AnsweredIds();

export function approvalCard(id: string | null, turn: Turn, { query_id, app, action, fields }: Asking) {
  if (answeredTurns.has(query_id)) return;
  const grant = forRoutine(action);
  const card = h(`<div class="dcard">
    <div class="head">${appTile(app)}<div class="dcol"><b>${esc(action)}?</b><small>${esc(app)} asks first</small></div></div>
    ${fields.length ? `<div class="kv">${fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-d="allow">Allow<kbd>⌘${ICON.ret}</kbd></button>${grant ? "" : `<button class="dbtn" data-d="session" title="Allow this action without asking until this chat ends">Allow for this session<kbd>⇧⌘${ICON.ret}</kbd></button><button class="dbtn" data-d="always">Always<kbd>⌥⌘${ICON.ret}</kbd></button>`}<button class="dbtn warn" data-d="deny">Deny<kbd>⌘D</kbd></button></div>
  </div>`);
  card.querySelectorAll<HTMLElement>("[data-d]").forEach(b => b.addEventListener("click", () => answer(b.dataset.d as Decision)));
  fillAppTiles(card);
  turn.append(card);
  const e = entry(id);
  e.approval = { queryId: query_id, turn, card, grant };
  e.body ??= turn.parentElement;
  if (id === S.session) refreshHints();
}

/// Follow-ups under the reply; picking one sends it.
export function suggestions(turn: Turn, replies: string[]) {
  turn.querySelector(".dreplies")?.remove();
  const row = h(`<div class="dreplies">${replies.map(r => `<button class="dbtn">${esc(r)}</button>`).join("")}</div>`);
  row.querySelectorAll("button").forEach((b, i) => b.addEventListener("click", () => send(replies[i])));
  turn.append(row);
}

export function failed(turn: Turn, message: string) {
  turn.failed = true;
  turn.append(h(errorCardHtml(message)));
  if (/removed|not linked/i.test(message)) refreshStatus();
}

export function finish(id: string | null, turn: Turn) {
  // A chat with no id yet is in no list.
  if (id) touch(id);
  turn.done = true;
  turn.ended = Date.now();
  endThought(turn);
  for (const s of turn.steps) if (s.state === "run") s.state = "stop";
  turn.folded = true;
  drawWork(turn);
  replyActs(turn);
  if (turn.user) loadTurnAgent(id, turn);
  const e = live.get(id);
  if (e) {
    e.run = null;
    if (!e.approval) live.delete(id);
  }
  // The window's sidebar stops its spinner.
  changed();
  if (id === S.session) refreshHints();
  else if (S.view === "chats") S.chats.mode === "list" ? loadChats(S.chats.query) : drawChats();
  const q = queued.get(id);
  if (q) {
    q.held ||= !!(turn.stopped || turn.failed || (turn.status && !["completed", "waiting"].includes(turn.status)));
    // An approval still waiting keeps the queue until it is answered.
    if (!q.held && !live.get(id)?.approval && id === S.session && S.view === "chat") setTimeout(() => sendQueued(id));
    else if (id === S.session) { drawQueue(); refreshHints(); }
  }
}

/// Empties the prompt, or sets it to `text`, and sizes it to match.
function setAsk(text: string) {
  const ask = $$<HTMLTextAreaElement>("#ask");
  ask.value = text;
  grow(ask);
}

/// Holds a prompt typed while the open chat's reply runs.
export function enqueue(text: string) {
  const id = S.session;
  if (!id) return;
  const q = queued.get(id) ?? { list: [], held: false };
  q.list.push(text);
  queued.set(id, q);
  setAsk("");
  drawQueue();
  refreshHints();
}

/// Sends the chat's first queued prompt; the rest follow as each reply finishes.
export function sendQueued(id: string | null) {
  const q = queued.get(id);
  if (!q?.list.length || id !== S.session || S.view !== "chat" || live.get(id)?.run || live.get(id)?.approval) return;
  const text = q.list.shift()!;
  q.held = false;
  if (!q.list.length) queued.delete(id);
  drawQueue();
  send(text);
}

/// Takes the open chat's last queued prompt back into the empty field to
/// edit. False when nothing is queued.
export function popQueued() {
  const q = queued.get(S.session);
  const text = q?.list.pop();
  if (text == null) return false;
  if (!q!.list.length) queued.delete(S.session);
  setAsk(text);
  drawQueue();
  refreshHints();
  return true;
}

/// The open chat's queued prompts, between the reply and the hints. x puts
/// one back in the field.
export function drawQueue() {
  const old = $("#queue"), q = queued.get(S.session);
  if (S.view !== "chat" || !q?.list.length) return old?.remove();
  const el = h(`<div class="ag-queue" id="queue">${q.list.map((t, i) => queueRowHtml({
    label: "Queued", cls: "", text: t, tip: t, chips: [], when: q.held ? "not sent" : "after the reply",
    buttons: `${q.held && !i ? '<button data-q="send">Send</button>' : ""}<button data-q="${i}" data-act="edit">Edit</button><button data-q="${i}" aria-label="Remove">${ICON.x}</button>`,
  })).join("")}</div>`);
  el.querySelectorAll<HTMLElement>("[data-q]").forEach(b => b.addEventListener("click", () => {
    if (b.dataset.q === "send") return sendQueued(S.session);
    const [text] = q.list.splice(Number(b.dataset.q), 1);
    if (!q.list.length) queued.delete(S.session);
    if (b.dataset.act === "edit") setAsk([text, $$<HTMLTextAreaElement>("#ask").value].filter(s => s.trim()).join("\n\n"));
    drawQueue();
    refreshHints();
    focusAsk();
  }));
  if (old) old.replaceWith(el);
  else $$(".dhints").before(el);
}

/// A channel that draws one run's events into `turn`, which stays in its
/// chat's body while the bar shows another chat.
export function follow(id: string | null, turn: Turn) {
  const channel = new Channel<BarEvent>();
  channel.onmessage = ev => {
    // A stopped turn only needs its run's id, to stop it.
    if (turn.stopped) return ev.kind === "started" && turn.started?.(ev.run_id);
    switch (ev.kind) {
      case "started":
        entry(id).run = { id: ev.run_id, turn };
        if (id === S.session) refreshHints();
        break;
      case "text": {
        const first = !turn.querySelector(ANSWER);
        endThought(turn);
        reply(turn).innerHTML = answerHtml(ev.html);
        if (first) drawWork(turn);
        break;
      }
      case "reasoning":
        if (!turn.thought || turn.thought.ms != null) turn.thought = { text: turn.thought?.text ?? "", start: Date.now() };
        turn.thought.text += ev.delta;
        drawWork(turn);
        break;
      case "tool_started":
        if (QUIET_TOOLS.has(ev.name)) break;
        endThought(turn);
        turn.steps.push({ id: ev.call_id, name: ev.name, label: (DESCRIBED.has(ev.name) && ev.description) || toolLabel(ev.name) || ev.description || "Using a tool", state: "run" });
        drawWork(turn);
        break;
      case "tool_result": {
        const step = turn.steps.find(s => s.id === ev.call_id);
        if (step) step.state = ev.success ? "ok" : "bad";
        drawWork(turn);
        toolOutput(turn, ev, ev.name === "get_route" && ev.success && ev.call_id ? ev.call_id : null);
        break;
      }
      case "sources": sourceRow(turn, ev.sources); break;
      case "replies": suggestions(turn, ev.replies); break;
      case "transcript": heard(turn, ev.text); break;
      case "auto_picked":
        turn.model = ev.model;
        ranOn(turn, ev.model, ev.thinking);
        break;
      case "approval":
        approvalCard(id, turn, ev);
        break;
      case "error": failed(turn, ev.message); break;
      case "finished":
        turn.status = ev.status;
        if (ev.status === "cancelled") stoppedRow(turn);
        if (ev.status === "interrupted") stoppedRow(turn, "Cut off by a server restart");
        finish(id, turn);
        break;
    }
    if (turn.isConnected) keepDown();
  };
  return channel;
}

/// Makes `turn` chat `id`'s running reply, thinking until something comes.
export function startRun(id: string | null, turn: Turn, runId: string | null) {
  turn.begun = Date.now();
  turn.done = false;
  drawWork(turn);
  const e = entry(id);
  e.body = body();
  e.run = { id: runId, turn };
  // The window's sidebar shows the chat answering.
  changed();
}

/// Waits out the command streaming `turn`'s run, then finishes the turn.
/// A failure shows unless the turn was stopped.
export async function runTurn(id: string | null, turn: Turn, run: Promise<unknown>) {
  try {
    await run;
  } catch (err) {
    if (!turn.stopped) failed(turn, String(err));
  }
  turn.started?.(null);
  if (!turn.done) finish(id, turn);
}

export async function send(message: string) {
  const images = attached.pics.slice(), files = attached.files.slice();
  const n = images.length + files.length;
  if (!message.trim() && !n) return;
  const busy = here();
  if (n && (busy?.run || busy?.approval)) return attachError(`${files.length ? "Files" : "Images"} send once this reply finishes`);
  if (busy?.run || busy?.approval) return enqueue(message.trim());
  message = message.trim() || (n > 1 ? "What are these?" : "What is this?");
  attached.pics = [];
  attached.files = [];
  drawAttached();
  const id = S.session ??= newId();
  const ask = $$<HTMLTextAreaElement>("#ask");
  body()!.querySelectorAll(".dreplies").forEach(r => r.remove());
  ask.value = "";
  ask.placeholder = "Ask a follow-up…";
  grow(ask);
  const turn = newTurn(message, images.map(url => ({ url })), files);
  turn.user = true;
  // Without Auto every prompt runs on the chosen model; with it, Jev's pick
  // arrives as an event.
  const m = MODELS.info as ModelPick | null;
  if (m && !m.auto && canPickModel()) ranOn(turn, m.selected, m.thinking);
  startRun(id, turn, null);
  touch(id, message);
  refreshHints();
  scrollDown();
  await stopping.get(id);
  const uploads = files.map(({ name, mime, data }) => ({ name, mime, data }));
  await runTurn(id, turn, invoke("send_prompt", { session: id, message, images, files: uploads, onEvent: follow(id, turn) }));
}

// Chats whose run, started on another device, the bar is looking up or following.
export const watching = new Set<string>();

/// Follows the open chat's run when another device started it, drawing it
/// as it streams like one sent from here.
export async function watchRun(id: string | null) {
  if (!id || live.has(id) || watching.has(id)) return;
  watching.add(id);
  try {
    const runId = await invoke<string | null>("active_run", { session: id }).catch(() => null);
    if (!runId || live.has(id) || S.session !== id || S.view !== "chat") return;
    // The prompt or a turn's line is already drawn from the server's copy, waiting for its reply.
    const last = [...body()!.querySelectorAll<Turn>(".turn")].at(-1);
    const turn = last && !last.done && (last.user || last.querySelector(":scope > .dsystem")) ? last : newTurn("");
    turn.steps ??= [];
    startRun(id, turn, runId);
    refreshHints();
    scrollDown();
    await followAnswered(id, turn, runId);
  } finally {
    watching.delete(id);
  }
}

/// Takes chat `id`'s approval card away, saying how it was answered, and
/// readies its turn for the run the answer resumes.
export function settle(id: string | null, e: LiveEntry, pending: PendingApproval, decision: Decision) {
  e.approval = null;
  answeredTurns.mark(pending.queryId, id);
  const said = pending.grant
    ? { allow: "Allowed for this routine", deny: "Denied" }[decision as "allow" | "deny"]
    : { allow: "Allowed once", session: "Allowed for the rest of this chat", always: "Allowed, and always allowed from now on", deny: "Denied" }[decision];
  pending.card.replaceWith(h(`<div class="dmuted">${said}</div>`));
  const turn = pending.turn;
  turn.done = false;
  turn.ended = undefined;
  e.run = { id: null, turn };
  if (id === S.session) refreshHints();
  return turn;
}

export async function answer(decision: Decision) {
  const id = S.session, e = live.get(id), pending = e?.approval;
  if (!e || !pending) return;
  if (pending.grant && (decision === "always" || decision === "session")) decision = "allow";
  const turn = settle(id, e, pending, decision);
  await stopping.get(id);
  const answered = invoke("answer_approval", { session: id, queryId: pending.queryId, decision, onEvent: follow(id, turn) })
    // Not answered after all: the card may come back with the chat.
    .catch(err => { answeredTurns.forget(pending.queryId); throw err; });
  await runTurn(id, turn, answered);
}

/// Follows run `runId` into `turn`: one an answer given in the orb resumed,
/// or one another device started.
export function followAnswered(id: string | null, turn: Turn, runId: string) {
  return runTurn(id, turn, invoke("follow_run", { runId, onEvent: follow(id, turn) }));
}

/// Stops the run at once: the chat lets go of it and takes the next prompt
/// while the server is told to stop it.
export function stop() {
  const id = S.session, run = here()?.run;
  if (!run) return;
  const turn = run.turn;
  turn.stopped = true;
  // A run not started yet is stopped once it says its id.
  const runId = run.id ? Promise.resolve(run.id) : new Promise<string | null>(resolve => { turn.started = resolve; });
  const stopped: Promise<unknown> = runId
    .then(runId => runId && invoke("cancel_run", { runId }))
    .catch(() => {})
    .finally(() => { if (stopping.get(id) === stopped) stopping.delete(id); });
  stopping.set(id, stopped);
  stoppedRow(turn);
  finish(id, turn);
}

export async function newChat() {
  S.session = null;
  renderChat();
  await invoke("new_chat");
}

/// Hands a view to the command center window, which hides the bar.
export function toWindow(target: unknown) {
  return invoke("center_open", { target }).catch(() => {});
}
/// Opens chat `id` (null for a new one) in the window.
export const openInWindow = (id: string | null | undefined) => toWindow({ chat: { session: id ?? null } });
