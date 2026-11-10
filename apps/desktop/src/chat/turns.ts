// A chat's turns: the prompt, the work before the answer, the answer and
// what tools sent under it.
import { turnState } from "./turn-state.ts";
import { appendTurn, createTurn, setView, streamReply, turnsOf, viewOf, type Sent, type SentItem, type Turn, type WorkStep, type WorkThought, type WorkView } from "./transcript.ts";
import type { AskFile } from "./attach.ts";
import { type Chart } from "./charts.ts";
import { cardKey, parseAgentReport } from "./agentcard.ts";
import { picMime, type PicRef, type ProductCard, type SentFile, type SentImage } from "./media.ts";
import { canPickModel, modelName } from "../settings/model.ts";
import { doneLabel } from "./tools.ts";
import { endLive, trackLive } from "../agents/thoughtlive.ts";
import { turnHeading } from "../agents/turn-heading-view.tsx";
import { linkified, plural } from "../ui/format.ts";
import type { ToolKind } from "../agents/agentturns.ts";
import { refreshStatus } from "../bar/views.ts";
import type { ChatBody, Step } from "../ui/turn.ts";
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

/// A turn for `question` at the end of the open chat.
export function newTurn(question: string, images?: PicRef[], files?: AskFile[]) {
  const owner = body();
  if (!owner) throw new Error("Cannot create a chat turn without a body");
  const turn = makeTurn(owner, question, images, files);
  appendTurn(turn);
  return turn;
}

/// A turn of `owner` for `question`, not yet in its chat.
export function makeTurn(owner: ChatBody, question: string, images?: PicRef[], files?: AskFile[]) {
  const turn = createTurn(owner, {
    prompt: question ? linkified(question) : null,
    pictures: images?.length ? images : null,
    files: files?.length ? files : null,
  });
  turnState(turn).question = question;
  return turn;
}

/// Shows `text` as what the user said in `turn`.
export function setPrompt(turn: Turn, text: string, html: string) {
  turnState(turn).question = text;
  setView(turn, { prompt: html });
}

/// The model and thinking a prompt ran on, under it.
export function ranOn(turn: Turn, model: string, thinking: string) {
  if (!model) return;
  const line = canPickModel()
    ? (thinking ? `${modelName(model)} · ${thinking}` : modelName(model))
    : thinking;
  if (!line || viewOf(turn).prompt === null) return;
  setView(turn, { ranOn: line });
}

/// What the prompt of `turn` says.
export const askText = (turn: Turn) => turnState(turn).question ?? "";

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

/// Draws what the turn did before its answer, as a coding agent's turn shows
/// it: the heading with how long it worked, the thought, and the steps. A
/// running turn shows its newest step; a finished one folds them into the
/// heading until it is opened.
export function drawWork(turn: Turn) {
  const data = turnState(turn), view = viewOf(turn);
  const key = view.work?.key ?? String(++turnCount);
  const running = !data.done, steps = data.steps, th = data.thought, opened = !data.folded;
  const shown = opened ? steps : running ? steps.slice(-1) : [];
  let thought: WorkThought | null = null;

  if (th && (running || opened || !steps.length)) {
    const live = running && th.ms == null, folding = !live && !th.touched && !th.ended;
    if (!live) th.ended = true;
    const thoughtKey = `chat:${key}`;
    if (live) trackLive(thoughtKey); else if (folding) endLive(thoughtKey);
    thought = { key: thoughtKey, text: th.text, ms: th.ms ?? null, live, folding, open: live ? !th.touched || !!th.open : !!th.open || folding };
  }

  // Once the answer streams it says the turn is alive.
  const live = running && view.reply === null;
  const thinking = (th != null && th.ms == null) || !steps.some(t => t.state === "run");
  const ms = data.begun == null ? null : (data.ended ?? Date.now()) - data.begun;
  const stats = steps.length ? plural(steps.length, "tool call", "tool calls") : "";
  const work: WorkView = {
    key,
    heading: turnHeading({ state: running ? "working" : "done", start: data.begun, ms, stats, turn: steps.length ? key : null, opened }),
    thought, steps: shown.map(stepRow),
    live: live ? { thinking, start: data.begun } : null,
    toggleThought: open => {
      if (!data.thought) return;
      data.thought.touched = true;
      data.thought.open = open;
      drawWork(turn);
    },
    toggleSteps: () => { data.folded = !data.folded; drawWork(turn); },
    touched: () => { if (data.thought) data.thought.touched = true; },
  };
  setView(turn, { work });
}

/// The thought streaming into `turn` ended: something else came.
export function endThought(turn: Turn) {
  const data = turnState(turn);
  if (data.thought && data.thought.ms == null) data.thought.ms = Date.now() - data.thought.start;
}

/// Sets the reply's HTML; a reply the user is selecting in waits.
export function setChatReply(turn: Turn, html: string) {
  turnState(turn).reply = html;
  streamReply(turn, html);
}
export function appendChatReply(turn: Turn, html: string) { setChatReply(turn, turnState(turn).reply + html); }
export function chatReplyHtml(turn: Turn) { return turnState(turn).reply; }

/// What a tool sent, under the reply. `route` is a `get_route` call's id,
/// whose transit card is fetched.
export function toolOutput(turn: Turn, x: ToolOutput, route: string | null) {
  const before = viewOf(turn).sent, pictures = [...before?.pictures ?? []], items: SentItem[] = [...before?.items ?? []];

  if (x.file) {
    const mime = picMime(x.file);
    if (mime) pictures.push({ file: { ...x.file, mime }, caption: "" });
    else items.push({ kind: "file", file: x.file });
  }

  pictures.push(...x.images ?? []);
  if (x.products) items.push({ kind: "products", cards: { kind: "products", cards: x.products } });
  if (x.cards) items.push({ kind: "products", cards: { kind: "html", html: x.cards } });
  if (x.chart) items.push({ kind: "chart", chart: x.chart });
  if (route) items.push({ kind: "route", callId: route });
  const sent: Sent = { pictures, items };
  if (pictures.length || items.length) setView(turn, { sent });
  if (x.code_agent) coderCard(turn, x.code_agent);
}

/// Draws or updates the card for the session a `code_agent` result
/// describes, under the steps of `turn`.
function coderCard(turn: Turn, text: string) {
  const report = parseAgentReport(text);
  if (!report) return;
  const key = cardKey(report), coders = viewOf(turn).coders;
  setView(turn, { coders: coders.some(card => cardKey(card) === key) ? coders.map(card => cardKey(card) === key ? report : card) : [...coders, report] });
}

/// Follow-ups under the reply; picking one sends it.
export function suggestions(turn: Turn, replies: string[]) {
  setView(turn, { replies: { list: replies, send: reply => void send(reply) } });
}

export function removeChatSuggestions(owner: ChatBody) {
  for (const turn of turnsOf(owner)) if (viewOf(turn).replies) setView(turn, { replies: null });
}

export function failed(turn: Turn, message: string) {
  turnState(turn).failed = true;
  setView(turn, { error: message });
  if (/removed|not linked/i.test(message)) refreshStatus();
}
