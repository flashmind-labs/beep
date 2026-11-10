import { turnState } from "./turn-state.ts";
// chat/: chats and groups in the bar: the open chat, the list, the composer,
// attachments and the cards under a reply. Entry: this file.
import { createElement } from "react";
import { flushSync } from "react-dom";
import { openBarPage } from "../bar/page-root.ts";
import { conversation } from "./conversation.ts";
import { ChatShell } from "./chat-shell.tsx";
import { updateChatNavigation } from "./navigation-view.tsx";
import { createChatBody, createChatTurn, disposeChatBody, discardChatApproval, discardChatNotice, discardChatSuggestions, renderChatApproval, renderChatPrompt, renderChatNotice, renderChatPictures, renderChatReply, renderChatSuggestions, renderChatWork, transferChatReply, type WorkStep, type WorkThought, type WorkView } from "./work-view.tsx";
import { AG } from "../agents/agents.ts";
import { askFiles, attach, attached, attachError, drawAttached, unattach } from "./attach.ts";
import type { AskFile } from "./attach.ts";
import { type Chart, chartRow } from "./charts.ts";
import { drawChats, loadChats, touch } from "./chats.ts";
import { coderCard, disposeCoderCards, resumeCoderCards } from "./codercard.ts";
import { composeField } from "./compose.ts";
import { $, $$, bar, changed, entry, here, live, newId, PANE, queued, S, stopping } from "../bar/core.ts";
import { renderAgentNew } from "../agents/agentnew.ts";
import { AnsweredIds } from "./approvals.ts";
import type { LiveEntry, PendingApproval } from "../bar/core.ts";
import { currentChatHints, refreshHints } from "../bar/hints.ts";
import { loadTurnAgent, sourceRow } from "./history.ts";
import type { WebSource } from "./history.ts";
import { heard, takeDraft } from "../bar/live.ts";
import { productRow, replyPic, sentFile } from "./media.ts";
import type { PicRef, SentFile, SentImage } from "./media.ts";
import { canPickModel, modelName, MODELS } from "../settings/model.ts";
import { replyActs, stoppedRow } from "./rerun.ts";
import { routeRow } from "./routes.ts";
import { DESCRIBED, doneLabel, QUIET_TOOLS, toolLabel } from "./tools.ts";
import { attachLive, endLive, trackLive } from "../agents/thoughtlive.ts";
import { followsAfterScroll, returnScroll } from "../agents/logscroll.ts";
import { followEnd, unfollowEnd } from "../ui/followend.ts";
import { answerHtml, tickTurns, toggleThought, turnHeadHtml } from "../agents/turnrows.ts";
import { linkified, plural } from "../ui/format.ts";
import type { ToolKind } from "../agents/agentturns.ts";
import { refreshStatus } from "../bar/views.ts";
import { Channel, invoke } from "../ui/tauri.ts";
import { h } from "../ui/dom.ts";
import type { ChatBody, Step, Turn } from "../ui/turn.ts";
import { dragHandle } from "../ui/drag.ts";

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

const retiringBodies = new WeakSet<ChatBody>();

function evictBody(body: ChatBody) {
  retiringBodies.delete(body);
  disposeCoderCards(body);
  unfollowEnd(body);
  disposeChatBody(body);
}

/// A cached live body may be detached; release it only when the entry goes.
export function forgetChat(id: string | null) {
  const entry = live.get(id), cached = entry?.body;
  live.delete(id);
  if (!cached) return;
  // A removed entry can still have an in-flight stream using its turn.
  if (entry?.run) retiringBodies.add(cached);
  else if (!cached.isConnected && ![...live.values()].some(entry => entry.body === cached)) evictBody(cached);
}

let chatShell: { host: HTMLElement; root: import("react-dom/client").Root; body: ChatBody; dock: import("../bar/live.ts").LiveView | null } | null = null;

export function renderChat() {
  // The chat needs the server: without it every way here lands on the agent's start form.
  if (S.link !== "linked") return renderAgentNew("", PANE ? "agents" : "chat");
  S.view = "chat";
  const kept = here()?.body;
  const chatBody = kept ?? createChatBody();
  let host: HTMLElement | null = null;
  const page = openBarPage(() => {
    chatShell = null;
    if (!retiringBodies.has(chatBody) && ![...live.values()].some(entry => entry.body === chatBody)) evictBody(chatBody);
    if (host?.parentElement !== bar) return;
    for (const child of [...bar.children]) if (child !== host) child.remove();
  });
  host = page.host;
  chatShell = { host: page.host, root: page.root, body: chatBody, dock: null };
  drawQueue();
  const b = body()!;
  // A chat put back goes where the reader left it if they had scrolled up, else to its end.
  if (kept) { b.scrollTop = returnScroll(b, b.top ?? 0); resumeCoderCards(b); }
  let top = b.scrollTop;
  b.onscroll = () => { b.stuck = followsAfterScroll(b.stuck !== false, top, b); top = b.top = b.scrollTop; drawLatest(); markMap(); };
  followEnd(b);
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
  dragHandle(grip, e => {
    grip.classList.add("on");
    const startY = e.clientY, startH = ask.offsetHeight;
    return {
      move: m => {
        // A click on the grip is not a drag: it would pin the prompt to one line.
        if (Math.abs(m.clientY - startY) < 4) return;
        askHeight = Math.max(26, Math.min(startH + m.clientY - startY, ASK_MAX));
        grow(ask);
      },
      end: () => {
        grip.classList.remove("on");
        try { if (askHeight) localStorage.setItem(ASK_HEIGHT, String(askHeight)); } catch {}
      },
    };
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
export function drawLatest() { const b = body(); if (b) updateChatNavigation(b); }

/// Prompt navigation reads layout at most once per frame.
export const drawMap = drawLatest;
export const markMap = drawLatest;

export function newTurn(question: string, images?: PicRef[], files?: AskFile[]) {
  const turn = makeTurn(question, images, files);
  conversation(body()!).append(turn);
  return turn;
}
/// A turn for `question`, not yet in any chat.
export function makeTurn(question: string, images?: PicRef[], files?: AskFile[], prepared: Turn | null = null) {
  const owner = body();
  if (!owner) throw new Error("Cannot create a chat turn without a body");
  const turn = prepared ?? createChatTurn(owner, { prompt: !!question, pictures: !!images?.length });
  turnState(turn).question = question;
  const prompt = turn.querySelector<HTMLElement>(".dcoderen.user");
  if (prompt) renderChatPrompt(owner, turn, prompt, linkified(question));
  const pictures = turn.querySelector<HTMLElement>(".dpics");
  if (pictures && images) renderChatPictures(owner, turn, pictures, images);
  if (files?.length) turn.append(askFiles(files, owner, turn));
  turnBodies.set(turn, owner);
  return turn;
}

/// The model and thinking a prompt ran on, under it.
export function ranOn(turn: Turn, model: string, thinking: string) {
  if (!model) return;
  const line = canPickModel()
    ? (thinking ? `${modelName(model)} · ${thinking}` : modelName(model))
    : thinking;
  if (!line) return;
  const old = turn.querySelector<HTMLElement>(".dranon");
  if (old) { discardChatNotice(turn, old); old.remove(); }
  const prompt = turn.querySelector(ASK), owner = owningChatBody(turn);
  if (!prompt) return;
  if (owner) {
    const box = document.createElement("div");
    box.className = "drerunof dranon";
    renderChatNotice(owner, turn, box, { kind: "label", message: line });
    prompt.after(box);
  }
}

/// The user's prompt in a turn, and the answer.
export const ASK = ":scope > .dcoderen.user", ANSWER = ":scope > .dcoderen.agent";
/// What the prompt of `turn` says.
export const askText = (turn: Turn) => turnState(turn).question ?? turn.querySelector(`${ASK} > span`)?.textContent ?? "";

/// The log's kind of call for a Beep tool, for its row's icon.
function stepKind(name: string): ToolKind {
  if (/^(web_search|browser|open_browser)$/.test(name)) return "web";
  if (/^(exec|node|desktop|code_agent)$/.test(name)) return "command";
  if (name === "attach") return "read";
  return name.startsWith("mcp") || (name && name === name.toUpperCase()) ? "mcp" : "other";
}

/// A tool's stable, typed row; React updates its status in place.
function stepRow(step: Step, index: number): WorkStep {
  const label = step.state === "ok" || step.state === "bad" ? doneLabel(step.label) : step.label;
  const [, verb = label, target = ""] = /^(\S+)\s+([\s\S]*)$/.exec(label) ?? [];
  return { key: step.id || `step:${index}`, kind: stepKind(step.name), verb, target, label, state: step.state };
}

/// Turns drawn so far, so each thought has its own key.
let turnCount = 0;
const turnBodies = new WeakMap<Turn, ChatBody>();
export const owningChatBody = (turn: Turn) => turnBodies.get(turn) ?? null;

/// Draws what the turn did before its answer, as a coding agent's turn shows
/// it: the heading with how long it worked, the thought, and the steps. A
/// running turn shows its newest step; a finished one folds them into the
/// heading until it is opened.
export function drawWork(turn: Turn) {
  const data = turnState(turn);
  let box = turn.querySelector<HTMLElement>(":scope > .tools");

  if (!box) {
    box = h('<div class="tools dcoderlog"></div>');
    box.dataset.key = String(++turnCount);
    box.addEventListener("click", e => {
      const btn = (e.target as Element).closest("[data-thought]");
      if (btn && data.thought) { data.thought.touched = true; data.thought.open = toggleThought(btn); return; }
      if (!(e.target as Element).closest("[data-turn]")) return;
      data.folded = !data.folded;
      drawWork(turn);
    });
    const after = turn.querySelector(`${ANSWER}, :scope > .dcard, :scope > .derrcard`);
    after ? turn.insertBefore(box, after) : turn.append(box);
  }

  const running = !data.done, steps = data.steps, th = data.thought, opened = !data.folded;
  const shown = opened ? steps : running ? steps.slice(-1) : [];
  let thought: WorkThought | null = null;

  if (th && (running || opened || !steps.length)) {
    const live = running && th.ms == null, folding = !live && !th.touched && !th.ended;
    if (!live) th.ended = true;
    const key = `chat:${box.dataset.key}`;
    if (live) trackLive(key); else if (folding) endLive(key);
    thought = { key, text: th.text, ms: th.ms ?? null, live, folding, open: live ? !th.touched || !!th.open : !!th.open || folding };
  }

  // Once the answer streams it says the turn is alive.
  const live = running && !turn.querySelector(ANSWER);
  const thinking = (th != null && th.ms == null) || !steps.some(t => t.state === "run");
  const ms = data.begun == null ? null : (data.ended ?? Date.now()) - data.begun;
  const turnKey = steps.length ? box.dataset.key ?? null : null;
  const state = running ? "working" : "done";
  const stats = steps.length ? plural(steps.length, "tool call", "tool calls") : "";
  const head = { state, start: data.begun, ms, stats, key: turnKey, opened } as const;
  const html = turnHeadHtml(head);
  const value: WorkView = {
    heading: html ? { kind: "heading", id: turnKey ?? "turn", html, state, start: data.begun ?? null, ms, stats, turn: turnKey, opened } : null,
    thought, steps: shown.map(stepRow),
    live: live ? { thinking, start: data.begun } : null,
  };
  const owner = turnBodies.get(turn) ?? body();
  if (!owner) return;
  turnBodies.set(turn, owner);
  renderChatWork(owner, turn, box, value, () => {
    attachLive(box, () => { if (data.thought) data.thought.touched = true; });
    tickTurns();
  });
}

/// The thought streaming into `turn` ended: something else came.
function endThought(turn: Turn) {
  const data = turnState(turn);
  if (data.thought && data.thought.ms == null) data.thought.ms = Date.now() - data.thought.start;
}

export function reply(turn: Turn) {
  let el = turn.querySelector<HTMLElement>(ANSWER);

  if (!el) {
    el = h('<div class="dcoderen agent md"></div>');
    const files = turn.querySelector(".files");
    files ? turn.insertBefore(el, files) : turn.append(el);
  }

  return el;
}

/// Sets reply HTML while keeping selected text stable.
export function setChatReply(turn: Turn, html: string) {
  const box = reply(turn), owner = owningChatBody(turn);
  turnState(turn).reply = html;
  if (owner) renderChatReply(owner, turn, box, html);
}
export function appendChatReply(turn: Turn, html: string) { setChatReply(turn, turnState(turn).reply + html); }
export function chatReplyHtml(turn: Turn) { return turnState(turn).reply; }
export function moveChatReply(from: Turn, to: Turn) {
  turnState(to).reply = chatReplyHtml(from); turnState(from).reply = "";
  transferChatReply(from, to);
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
  const card = document.createElement("div");
  card.className = "dcard";
  const owner = owningChatBody(turn);
  if (!owner) return;
  renderChatApproval(owner, turn, card, { app, action, fields, grant, answer: decision => void answer(decision) });
  turn.append(card);
  const e = entry(id);
  e.approval = { queryId: query_id, turn, card, grant };
  e.body ??= owningChatBody(turn);
  if (id === S.session) refreshHints();
}

/// Follow-ups under the reply; picking one sends it.
export function suggestions(turn: Turn, replies: string[]) {
  const owner = owningChatBody(turn);
  if (!owner) return;
  let row = turn.querySelector<HTMLElement>(".dreplies");
  if (!row) { row = document.createElement("div"); row.className = "dreplies"; turn.append(row); }
  renderChatSuggestions(owner, turn, row, replies, reply => void send(reply));
}

export function removeChatSuggestions(owner: ChatBody) {
  owner.querySelectorAll<HTMLElement>(".dreplies").forEach(row => {
    const turn = row.closest<Turn>(".turn");
    if (turn) discardChatSuggestions(turn);
    row.remove();
  });
}

export function failed(turn: Turn, message: string) {
  turnState(turn).failed = true;
  const owner = owningChatBody(turn);
  if (owner) {
    const box = document.createElement("div");
    box.className = "derrcard";
    renderChatNotice(owner, turn, box, { kind: "error", message });
    turn.append(box);
  }
  if (/removed|not linked/i.test(message)) refreshStatus();
}

export function finish(id: string | null, turn: Turn) {
  // A chat with no id yet is in no list.
  if (id) touch(id);
  turnState(turn).done = true;
  turnState(turn).ended = Date.now();
  endThought(turn);
  for (const s of turnState(turn).steps) if (s.state === "run") s.state = "stop";
  turnState(turn).folded = true;
  drawWork(turn);
  replyActs(turn);
  if (turnState(turn).user) loadTurnAgent(id, turn);
  const e = live.get(id);

  if (e?.run?.turn === turn) {
    e.run = null;
    if (!e.approval) forgetChat(id);
  }
  const completedBody = owningChatBody(turn);
  if (completedBody && retiringBodies.has(completedBody)) {
    retiringBodies.delete(completedBody);
    if (!completedBody.isConnected && ![...live.values()].some(entry => entry.body === completedBody)) evictBody(completedBody);
  }

  // The window's sidebar stops its spinner.
  changed();
  if (id === S.session) refreshHints();
  else if (S.view === "chats") S.chats.mode === "list" ? loadChats(S.chats.query) : drawChats();
  const q = queued.get(id);

  if (q) {
    const data = turnState(turn);
    q.held ||= !!(data.stopped || data.failed || (data.status && !["completed", "waiting"].includes(data.status)));

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

export function setChatDock(view: import("../bar/live.ts").LiveView | null) {
  if (!chatShell) return;
  chatShell.dock = view;
  drawQueue();
}

/// The open chat's queued prompts, between the reply and the hints. x puts
/// one back in the field.
export function drawQueue() {
  const shell = chatShell;
  if (!shell || shell.host.parentElement !== bar || S.view !== "chat") return;
  const q = queued.get(S.session);
  const update = (index: number, edit: boolean) => {
    const current = queued.get(S.session);
    if (!current) return;
    const [text] = current.list.splice(index, 1);
    if (!current.list.length) queued.delete(S.session);
    if (edit) setAsk([text, $$<HTMLTextAreaElement>("#ask").value].filter(value => value.trim()).join("\n\n"));
    drawQueue();
    refreshHints();
    focusAsk();
  };
  flushSync(() => shell.root.render(createElement(ChatShell, {
    body: shell.body, hints: currentChatHints(), running: !!here()?.run, dock: shell.dock, queue: q ?? null, attachments: attached,
    agent: AG.hosts?.length ? () => renderAgentNew($<HTMLTextAreaElement>("#ask")?.value.trim() ?? "") : null,
    unattach: (index: number) => { unattach(index); focusAsk(); },
    edit: (index: number) => update(index, true), remove: (index: number) => update(index, false), send: () => sendQueued(S.session),
  })));
}

/// A channel that draws one run's events into `turn`, which stays in its
/// chat's body while the bar shows another chat.
export function follow(id: string | null, turn: Turn) {
  const channel = new Channel<BarEvent>();
  // Text and thoughts arrive many times a frame: the newest answer and the
  // work box are drawn once per frame, and before any other event.
  let html: string | null = null, work = false, frame = 0;
  const flush = () => {
    cancelAnimationFrame(frame);
    frame = 0;
    if (html != null) { setChatReply(turn, answerHtml(html)); html = null; }
    if (work) { work = false; drawWork(turn); }
    if (turn.isConnected) keepDown();
  };
  const later = () => { frame ||= requestAnimationFrame(flush); };
  channel.onmessage = ev => {
    // A stopped turn only needs its run's id, to stop it.
    if (turnState(turn).stopped) return ev.kind === "started" && turnState(turn).started?.(ev.run_id);
    if (ev.kind !== "text" && ev.kind !== "reasoning" && frame) flush();

    switch (ev.kind) {
      case "started":
        entry(id).run = { id: ev.run_id, turn };
        if (id === S.session) refreshHints();
        break;
      case "text": {
        if (html == null && !turn.querySelector(ANSWER)) work = true;
        endThought(turn);
        html = ev.html;
        later();
        return;
      }
      case "reasoning": {
        const data = turnState(turn);
        if (!data.thought || data.thought.ms != null) data.thought = { text: data.thought?.text ?? "", start: Date.now() };
        data.thought.text += ev.delta;
        work = true;
        later();
        return;
      }
      case "tool_started":
        if (QUIET_TOOLS.has(ev.name)) break;
        endThought(turn);
        turnState(turn).steps.push({ id: ev.call_id, name: ev.name, label: (DESCRIBED.has(ev.name) && ev.description) || toolLabel(ev.name) || ev.description || "Using a tool", state: "run" });
        drawWork(turn);
        break;
      case "tool_result": {
        const step = turnState(turn).steps.find(s => s.id === ev.call_id);
        if (step) step.state = ev.success ? "ok" : "bad";
        drawWork(turn);
        toolOutput(turn, ev, ev.name === "get_route" && ev.success && ev.call_id ? ev.call_id : null);
        break;
      }
      case "sources": sourceRow(turn, ev.sources); break;
      case "replies": suggestions(turn, ev.replies); break;
      case "transcript": heard(turn, ev.text); break;
      case "auto_picked":
        turnState(turn).model = ev.model;
        ranOn(turn, ev.model, ev.thinking);
        break;
      case "approval":
        approvalCard(id, turn, ev);
        break;
      case "error": failed(turn, ev.message); break;
      case "finished":
        turnState(turn).status = ev.status;
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
  const current = owningChatBody(turn);
  if (current) conversation(current).invalidate();
  turnState(turn).begun = Date.now();
  turnState(turn).done = false;
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
    if (!turnState(turn).stopped) failed(turn, String(err));
  }

  turnState(turn).started?.(null);
  if (!turnState(turn).done) finish(id, turn);
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
  removeChatSuggestions(body()!);
  ask.value = "";
  ask.placeholder = "Ask a follow-up…";
  grow(ask);
  const turn = newTurn(message, images.map(url => ({ url })), files);
  turnState(turn).user = true;
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
    const last = conversation(body()!).turns.at(-1);
    const turn = last && !turnState(last).done && (turnState(last).user || last.querySelector(":scope > .dsystem")) ? last : newTurn("");
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
/// Replace a settled approval with a notice owned by its retained chat body.
export function approvalNotice(turn: Turn, card: HTMLElement, message: string) {
  discardChatApproval(turn);
  const owner = owningChatBody(turn);
  if (!owner) { card.remove(); return; }
  const box = document.createElement("div");
  box.className = "dmuted";
  renderChatNotice(owner, turn, box, { kind: "label", message });
  card.replaceWith(box);
}

export function settle(id: string | null, e: LiveEntry, pending: PendingApproval, decision: Decision) {
  e.approval = null;
  answeredTurns.mark(pending.queryId, id);
  const said = pending.grant
    ? { allow: "Allowed for this routine", deny: "Denied" }[decision as "allow" | "deny"]
    : { allow: "Allowed once", session: "Allowed for the rest of this chat", always: "Allowed, and always allowed from now on", deny: "Denied" }[decision];
  const turn = pending.turn;
  approvalNotice(turn, pending.card, String(said));
  turnState(turn).done = false;
  turnState(turn).ended = null;
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
  turnState(turn).stopped = true;
  // A run not started yet is stopped once it says its id.
  const runId = run.id ? Promise.resolve(run.id) : new Promise<string | null>(resolve => { turnState(turn).started = resolve; });
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
