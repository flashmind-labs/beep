// A chat's transcript: its turns as typed views in a store, drawn as one
// React list inside the chat's body. A turn is a handle; what it shows is
// its view here, and what it knows while it runs is its `turnState`.
import { createElement, type ReactElement } from "react";
import { flushSync } from "react-dom";
import { createStore, type StoreApi } from "zustand";
import { createDocumentView, type DocumentView } from "../ui/document-root.tsx";
import { showHtml, TranscriptView } from "./work-view.tsx";
import type { AgentReport } from "./agentcard.ts";
import type { ApprovalField, Decision } from "./approval-cards.ts";
import type { AskFile } from "./attach.ts";
import type { BackgroundQuestions } from "./background-view.tsx";
import type { Chart } from "./charts.ts";
import type { WebSource } from "./history.ts";
import type { PicRef, SentCards, SentFile, SentImage } from "./media.ts";
import type { ToolKind } from "../agents/agentturns.ts";
import type { TurnHeading } from "../agents/turn-heading-view.tsx";
import type { ChatBody } from "../ui/turn.ts";

/// A turn of one chat's transcript.
export interface Turn {
  readonly key: number;
  readonly body: ChatBody;
}

export interface WorkStep { key: string; kind: ToolKind; verb: string; target: string; label: string; state: "run" | "ok" | "bad" | "stop" }
export interface WorkThought { key: string; text: string; ms: number | null; live: boolean; open: boolean; folding: boolean }
export interface WorkLive { thinking: boolean; start: number | null }
/// What the turn did before its answer, with its toggles. `key` names the box.
export interface WorkView { key: string; heading: TurnHeading | null; thought: WorkThought | null; steps: WorkStep[]; live: WorkLive | null; toggleThought(open: boolean): void; toggleSteps(): void; touched(): void }
export type Source = WebSource & { site: string };
export interface RerunChoice { id: string; name: string; detail: string; used: boolean }
export interface RerunView { choices: RerunChoice[]; levels: { id: string; label: string }[]; run(model: string, thinking: string | null): void }
export interface ReplyActions { copy: boolean; rerun: (() => void) | null; agent: string | null }
export interface ApprovalView { app: string; action: string; fields: ApprovalField[]; grant: boolean; answer: (decision: Decision) => void }
export interface Replies { list: readonly string[]; send(text: string): void }
export interface StoppedView { message: string; retry: (() => void) | null }

/// What a tool sent under the reply, besides pictures.
export type SentItem =
  | { kind: "products"; cards: SentCards }
  | { kind: "file"; file: SentFile }
  | { kind: "chart"; chart: Chart }
  | { kind: "route"; callId: string };
export interface Sent { pictures: readonly SentImage[]; items: readonly SentItem[] }

/// What a turn shows, part by part, in the order it shows them. Null leaves
/// a part out.
export interface TurnView {
  turn: Turn;
  /// When the server wrote the prompt, for `data-at`.
  at: number | null;
  /// The line of a turn the server started, such as a routine's.
  system: string | null;
  /// The prompt, as HTML.
  prompt: string | null;
  /// The model and thinking the prompt ran on.
  ranOn: string | null;
  pictures: readonly PicRef[] | null;
  files: readonly AskFile[] | null;
  /// The model a rerun asked for.
  rerunOf: string | null;
  work: WorkView | null;
  coders: readonly AgentReport[];
  /// The answer, as sanitized HTML.
  reply: string | null;
  sent: Sent | null;
  sources: readonly Source[];
  approval: ApprovalView | null;
  /// How the approval card was answered, in its place.
  answered: string | null;
  error: string | null;
  stopped: StoppedView | null;
  actions: ReplyActions | null;
  menu: RerunView | null;
  /// Follow-ups under the reply; picking one sends it.
  replies: Replies | null;
}

/// What a chat's body draws: its turns and the background work waiting on it.
export interface TranscriptState {
  turns: readonly TurnView[];
  background: BackgroundQuestions | null;
  /// Counts the times the body was put back on screen.
  shown: number;
  /// What the window shows atop the chat, such as the card of the issue it is about.
  top: ReactElement | null;
}

interface Transcript { store: StoreApi<TranscriptState>; view: DocumentView; revision: number; disposed: boolean }

const transcripts = new WeakMap<ChatBody, Transcript>();
const views = new WeakMap<Turn, TurnView>();
const elements = new WeakMap<Turn, HTMLDivElement>();
const replyElements = new WeakMap<Turn, HTMLElement>();
let nextKey = 0;

/// A chat's scrolling body, which draws its transcript.
export function createChatBody(): ChatBody {
  const body = document.createElement("div");
  body.className = "dbody";
  body.id = "body";
  transcript(body);
  return body;
}

/// A body evicted from the cache: its transcript unmounts.
export function disposeChatBody(body: ChatBody) {
  const current = transcripts.get(body);
  if (!current || current.disposed) return;
  current.disposed = true;
  current.view.unmount();
}

function transcript(body: ChatBody): Transcript {
  const known = transcripts.get(body);
  if (known) return known;

  const store = createStore<TranscriptState>(() => ({ turns: [], background: null, shown: 0, top: null }));
  const made: Transcript = { store, view: createDocumentView(body), revision: 0, disposed: false };
  transcripts.set(body, made);
  flushSync(() => made.view.render(createElement(TranscriptView, { store })));
  return made;
}

function update(body: ChatBody, change: (state: TranscriptState) => Partial<TranscriptState>) {
  const current = transcript(body);
  if (current.disposed) return;
  flushSync(() => current.store.setState(change(current.store.getState())));
}

// ---------- Turns ----------

/// A new turn of `body`, not yet in its transcript.
export function createTurn(body: ChatBody, view: Partial<Omit<TurnView, "turn">> = {}): Turn {
  const turn: Turn = { key: ++nextKey, body };
  views.set(turn, {
    turn, at: null, system: null, prompt: null, ranOn: null, pictures: null, files: null, rerunOf: null, work: null, coders: [],
    reply: null, sent: null, sources: [], approval: null, answered: null, error: null, stopped: null, actions: null, menu: null, replies: null, ...view,
  });
  return turn;
}

export function viewOf(turn: Turn): TurnView {
  const view = views.get(turn);
  if (!view) throw new Error("A chat turn has no view");
  return view;
}

/// Changes what `turn` shows. A turn on screen draws again at once, alone,
/// unless `batchViews` holds its drawing; a change to nothing draws nothing.
export function setView(turn: Turn, change: Partial<Omit<TurnView, "turn">>) {
  const was = viewOf(turn), next = { ...was, ...change };
  const before = Object.values(was), after = Object.values(next);
  if (after.every((value, i) => Object.is(value, before[i]))) return;
  views.set(turn, next);
  const current = transcripts.get(turn.body);
  if (!current || current.disposed) return;

  if (batched) {
    batched.add(current);
    return;
  }

  const list = current.store.getState().turns, index = list.findIndex(view => view.turn === turn);
  if (index < 0) return;

  const turns = list.slice();
  turns[index] = next;
  flushSync(() => current.store.setState({ turns }));
}

/// Streams `html` in as the reply of `turn`. Once its reply shows, the
/// markup goes straight into the reply's element and the turn does not draw
/// again; the turn's view takes it for its next draw.
export function streamReply(turn: Turn, html: string) {
  const el = replyElements.get(turn), was = viewOf(turn);
  if (!el?.isConnected || was.reply === null) return setView(turn, { reply: html });
  views.set(turn, { ...was, reply: html });
  showHtml(el, html);
}

/// Transcripts whose turns changed while `batchViews` runs.
let batched: Set<Transcript> | null = null;

/// Runs `change`, then draws each transcript it changed once.
export function batchViews<T>(change: () => T): T {
  if (batched) return change();
  const dirty = new Set<Transcript>();
  batched = dirty;
  let result: T;

  try {
    result = change();
  } finally {
    batched = null;
  }

  for (const current of dirty) {
    if (current.disposed) continue;
    const turns = current.store.getState().turns;
    const next = turns.map(view => viewOf(view.turn));
    if (next.some((view, i) => view !== turns[i])) flushSync(() => current.store.setState({ turns: next }));
  }

  return result;
}

/// The turns `body` shows, in order.
export const turnsOf = (body: ChatBody): readonly Turn[] => transcript(body).store.getState().turns.map(view => view.turn);
/// Whether `turn` is still in its chat.
export const shownTurn = (turn: Turn) => turnsOf(turn.body).includes(turn);

/// Puts `turns` in `body` in this order; turns left out go.
export function placeTurns(body: ChatBody, turns: readonly Turn[]) {
  const current = transcript(body), before = turnsOf(body);
  if (turns.length === before.length && turns.every((turn, index) => turn === before[index])) return;
  current.revision++;
  update(body, () => ({ turns: turns.map(viewOf) }));
}

export const appendTurn = (turn: Turn) => placeTurns(turn.body, [...turnsOf(turn.body), turn]);
export const prependTurns = (body: ChatBody, turns: readonly Turn[]) => placeTurns(body, [...turns, ...turnsOf(body)]);
/// Drops `turn` and every turn after it.
export function dropFrom(turn: Turn) {
  const turns = turnsOf(turn.body), index = turns.indexOf(turn);
  if (index >= 0) placeTurns(turn.body, turns.slice(0, index));
}

/// Counts changes to which turns `body` shows, so a read of the server's
/// copy started before one knows it is stale.
export const transcriptRevision = (body: ChatBody) => transcript(body).revision;
export function invalidateTranscript(body: ChatBody) { transcript(body).revision++; }

export function setBackground(body: ChatBody, background: BackgroundQuestions | null) { update(body, () => ({ background })); }
export function setChatTop(body: ChatBody, top: ReactElement | null) { if (transcript(body).store.getState().top !== top) update(body, () => ({ top })); }

/// The body is on screen again: parts that read while it was away read now.
export function transcriptShown(body: ChatBody) { update(body, state => ({ shown: state.shown + 1 })); }

/// The element a turn on screen draws into, for scrolling to it.
export const turnElement = (turn: Turn) => elements.get(turn) ?? null;
export function placeReplyElement(turn: Turn, element: HTMLElement) { replyElements.set(turn, element); }
export function placeTurnElement(turn: Turn, element: HTMLDivElement | null) {
  if (element) elements.set(turn, element);
  else if (!shownTurn(turn)) elements.delete(turn);
}
