// A chat's turns: the prompt, the work before the answer, the answer and
// what tools sent under it.
import { turnState } from "./turn-state.ts";
import { conversation } from "./conversation.ts";
import { createChatTurn, discardChatNotice, discardChatSuggestions, renderChatPrompt, renderChatNotice, renderChatPictures, renderChatReply, renderChatSuggestions, renderChatWork, transferChatReply, type WorkStep, type WorkThought, type WorkView } from "./work-view.tsx";
import { askFiles } from "./attach.ts";
import type { AskFile } from "./attach.ts";
import { type Chart, chartRow } from "./charts.ts";
import { coderCard } from "./codercard.ts";
import { productRow, replyPic, sentFile } from "./media.ts";
import type { PicRef, ProductCard, SentFile, SentImage } from "./media.ts";
import { canPickModel, modelName } from "../settings/model.ts";
import { routeRow } from "./routes.ts";
import { doneLabel } from "./tools.ts";
import { attachLive, endLive, trackLive } from "../agents/thoughtlive.ts";
import { tickClocks, turnHeading } from "../agents/turn-heading-view.tsx";
import { linkified, plural } from "../ui/format.ts";
import type { ToolKind } from "../agents/agentturns.ts";
import { refreshStatus } from "../bar/views.ts";
import type { ChatBody, Step, Turn } from "../ui/turn.ts";
import { body } from "./chat.ts";
import { send } from "./runs.ts";

/// What a tool sent with its result, shown under the reply.
export interface ToolOutput {
  file: SentFile | null;
  /// A `send_image` result's pictures.
  images: SentImage[];
  /// A `show_products` result's cards.
  products: ProductCard[] | null;
  /// A `show_cards` result's cards, as HTML.
  cards: string | null;
  chart: Chart | null;
  /// A `code_agent` result's text, which names the session its card shows.
  code_agent?: string | null;
}

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
    box = document.createElement("div");
    box.className = "tools dcoderlog";
    box.dataset.key = String(++turnCount);
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
  const value: WorkView = {
    heading: turnHeading({ state, start: data.begun, ms, stats, turn: turnKey, opened }),
    thought, steps: shown.map(stepRow),
    live: live ? { thinking, start: data.begun } : null,
    toggleThought: open => {
      if (!data.thought) return;
      data.thought.touched = true;
      data.thought.open = open;
      drawWork(turn);
    },
    toggleSteps: () => { data.folded = !data.folded; drawWork(turn); },
  };
  const owner = turnBodies.get(turn) ?? body();
  if (!owner) return;
  turnBodies.set(turn, owner);
  renderChatWork(owner, turn, box, value, () => {
    attachLive(box, () => { if (data.thought) data.thought.touched = true; });
    tickClocks();
  });
}

/// The thought streaming into `turn` ended: something else came.
export function endThought(turn: Turn) {
  const data = turnState(turn);
  if (data.thought && data.thought.ms == null) data.thought.ms = Date.now() - data.thought.start;
}

export function reply(turn: Turn) {
  let el = turn.querySelector<HTMLElement>(ANSWER);

  if (!el) {
    el = document.createElement("div");
    el.className = "dcoderen agent md";
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
  if (x.products) productRow(turn, { kind: "products", cards: x.products });
  if (x.cards) productRow(turn, { kind: "html", html: x.cards });
  if (x.chart) chartRow(turn, x.chart);
  if (route) routeRow(turn, route);
  if (x.code_agent) coderCard(turn, x.code_agent);
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
