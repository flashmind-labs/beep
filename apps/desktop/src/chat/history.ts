import { turnState } from "./turn-state.ts";
import { batchViews, placeTurns, prependTurns, setView, shownTurn, transcriptRevision, turnsOf, viewOf, type Turn } from "./transcript.ts";
import { appendChatReply, askText, chatReplyHtml, drawWork, failed, makeTurn, newTurn, ranOn, suggestions, toolOutput } from "./turns.ts";
import { answeredTurns, approvalCard } from "./approval-cards.ts";
import { body, drawLatest, forgetChat, renderChat, scrollDown } from "./chat.ts";
import { sendQueued } from "./queue.ts";
import { watchRun } from "./runs.ts";
import type { Asking } from "./approval-cards.ts";
import type { ToolOutput } from "./turns.ts";
import { answerHtml } from "../agents/turnrows.ts";
import { changed, live, queued, S, windowHooks } from "../bar/core.ts";
import type { AskFile } from "./attach.ts";
import type { ImageRef } from "./media.ts";
import { replyActs } from "./rerun.ts";
import { QUIET_TOOLS, toolLabel } from "./tools.ts";
import { HISTORY_CHUNK, windowStart } from "./history-state.ts";
import { keepWaitingQuestions, loadQuestions } from "../bar/work.ts";
import { invoke } from "../ui/tauri.ts";
import type { ChatBody, Step } from "../ui/turn.ts";

/// A tool call of a saved reply (`ToolStep`).
export interface ToolStep extends ToolOutput {
  name: string;
  /// What the step showed while it ran, or what it did, when the server kept it.
  label: string | null;
  success: boolean;
  /// A `get_route` call's id, to fetch its transit card.
  route: string | null;
}

/// One message of a chat, as `open_session` lists it (`Turn`).
export type HistoryTurn =
  | { role: "user"; text: string; at: number; images: ImageRef[]; model: string; thinking: string }
  | { role: "assistant"; html: string; tools: ToolStep[]; replies: string[]; model: string }
  | { role: "scheduled"; label: string }
  | ({ role: "approval" } & Asking);

/// A web page a reply used (`WebSource`).
export interface WebSource {
  url: string;
  /// Empty when unknown.
  title: string;
}

export async function openSession(id: string) {
  S.session = id;
  loadQuestions(id);
  // A chat still answering keeps its body; anything else comes from the server.
  const running = live.get(id)?.run;
  if (!running) forgetChat(id);
  renderChat();

  if (running) {
    drawChatTop();
    // renderChat put the kept body back where the reader left it.
    if (body()?.stuck !== false) scrollDown();
    invoke("open_session", { id }).catch(() => {});
    return;
  }

  try {
    const turns = await invoke<HistoryTurn[]>("open_session", { id });
    // The user may have moved on, or started a run here, while it loaded.
    if (S.session !== id || S.view !== "chat" || live.has(id)) return;
    drawHistory(id, turns);
    // The window's title line says when the chat started, known only now.
    changed();
    // A reply that finished while another chat was open left its queue waiting.
    const q = queued.get(id);
    if (q && !q.held) sendQueued(id);
    else watchRun(id);
  } catch (e) {
    if (S.session === id && S.view === "chat") failed(newTurn(""), String(e));
  }
}

/// Redraws the open chat from the server's copy without rebuilding the bar,
/// so a draft stays and a user reading back is not sent to the bottom.
interface Refresh { id: string; dirty: boolean; pending: Promise<void> }
const refreshing = new WeakMap<ChatBody, Refresh>();

export function refreshSession(id: string): Promise<void> {
  const b = body();
  if (!b) return Promise.resolve();
  const previous = refreshing.get(b);
  if (previous?.id === id) { previous.dirty = true; return previous.pending; }

  const current: Refresh = { id, dirty: false, pending: Promise.resolve() };
  refreshing.set(b, current);
  const active = () => refreshing.get(b) === current && S.session === id && S.view === "chat" && !live.has(id) && body() === b;
  current.pending = (async () => {
    do {
      current.dirty = false;
      const revision = transcriptRevision(b);
      const turns = await invoke<HistoryTurn[]>("open_session", { id }).catch(() => null);
      if (!active()) return;
      if (transcriptRevision(b) !== revision) current.dirty = true;
      // A later notification needs a fresh snapshot before this read can apply.
      if (current.dirty) continue;
      if (turns) {
        const stuck = b.stuck !== false, top = b.scrollTop;
        drawHistory(id, turns);
        if (!stuck) { b.scrollTop = top; b.stuck = false; drawLatest(); }
      }
    } while (current.dirty && active());
  })().finally(() => { if (refreshing.get(b) === current) refreshing.delete(b); });
  return current.pending;
}

/// The older messages of each chat body that are not drawn yet: a long chat
/// opens at its newest prompts, and draws earlier ones as the reader nears the top.
const undrawn = new WeakMap<ChatBody, HistoryTurn[]>();
const watching = new WeakSet<ChatBody>();
const snapshots = new WeakMap<ChatBody, { id: string; contents: string; revision: number }>();
/// How near the top, in pixels, the reader draws earlier turns.
const NEAR_TOP = 1500;

export function drawHistory(id: string, turns: HistoryTurn[]) {
  // Approvals answered here stay gone until the server stops listing them.
  const asking = new Set(answeredTurns.keep(id, turns.flatMap(t => t.role === "approval" ? [t.query_id] : []), q => q));
  turns = turns.filter(t => t.role !== "approval" || asking.has(t.query_id));
  const b = body()!, revision = transcriptRevision(b), contents = JSON.stringify(turns);
  const previous = snapshots.get(b);
  // A full snapshot can repeat without changing any turn. Side data still
  // refreshes independently, while the existing rendered turns stay intact.
  if (previous && previous.id === id && previous.contents === contents && previous.revision === revision) {
    drawChatTop();
    loadSides(id, b);
    keepWaitingQuestions(id);
    return;
  }
  // From the oldest prompt already shown, else the newest few.
  const shown = turnsOf(b).map(turnState).find(state => state.messageAt !== null)?.messageAt ?? null;
  const kept = turns.findIndex(t => t.role === "user" && t.at === shown);
  const start = kept >= 0 ? kept : windowStart(turns.map(t => t.role), HISTORY_CHUNK);
  undrawn.set(b, turns.slice(0, start));

  if (!watching.has(b)) {
    watching.add(b);
    b.addEventListener("scroll", () => { if (b.scrollTop < NEAR_TOP) drawEarlier(b); }, { passive: true });
  }

  const { drawn, last } = buildTurns(id, b, turns.slice(start));
  // Only the latest reply's follow-ups still make sense.
  if (last) suggestions(last.turn, last.replies);
  const merged = mergeTurns(b, drawn);
  snapshots.set(b, { id, contents, revision: transcriptRevision(b) });
  // An approval card drawn here belongs to the turn the chat keeps.
  const e = live.get(id);

  if (e) {
    if (e.approval) e.approval.turn = merged.get(e.approval.turn) ?? e.approval.turn;
    e.body ??= b;
  }

  drawChatTop();
  scrollDown();
  loadSides(id, b);
  keepWaitingQuestions(id);
}

/// Draws the next older prompts above the ones shown, keeping the reader's
/// place: nothing here anchors the scroll, so it is put back by hand.
function drawEarlier(b: ChatBody) {
  const old = undrawn.get(b), id = S.session;
  if (!old?.length || !id || body() !== b) return false;
  const start = windowStart(old.map(t => t.role), HISTORY_CHUNK);
  undrawn.set(b, old.slice(0, start));
  const { drawn } = buildTurns(id, b, old.slice(start));
  const fromBottom = b.scrollHeight - b.scrollTop;
  prependTurns(b, drawn);
  b.scrollTop = b.scrollHeight - fromBottom;
  loadSides(id, b);
  return true;
}

/// The prompts of `b` not drawn yet, oldest first.
export const earlierPrompts = (b: ChatBody) => (undrawn.get(b) ?? []).flatMap(t => t.role === "user" ? [t.text] : []);

/// Draws earlier turns until the `n`th of `earlierPrompts` is drawn.
export function drawThrough(b: ChatBody, n: number) {
  while (earlierPrompts(b).length > n && drawEarlier(b));
}

const loadSides = (id: string, b: ChatBody) => { loadAskFiles(id, b); loadAgents(id, b); loadSources(id, b); };

/// The turns for the server's messages `turns`, built apart from the page,
/// and the reply whose follow-ups show.
function buildTurns(id: string, b: ChatBody, turns: HistoryTurn[]) {
  const drawn: Turn[] = [];
  const drawTurn = (question: string, images?: ImageRef[]) => {
    const turn = makeTurn(b, question, images);
    drawn.push(turn);
    return turn;
  };
  let turn: Turn | null = null, last: { turn: Turn; replies: string[] } | null = null;

  for (const t of turns) {
    if (t.role === "user") {
      turn = drawTurn(t.text, t.images);
      turnState(turn).user = true;
      turnState(turn).messageAt = t.at;
      setView(turn, { at: t.at });
      ranOn(turn, t.model, t.thinking);
    } else if (t.role === "scheduled") {
      // The server started this turn: a small line, not a message the user sent.
      turn = drawTurn("");
      setView(turn, { system: t.label || "Routine" });
      turnState(turn).user = false;
    } else if (t.role === "assistant") {
      turn ??= drawTurn("");
      turnState(turn).steps.push(...t.tools.filter(x => !QUIET_TOOLS.has(x.name)).map((x): Step => ({ id: "", name: x.name, label: x.label || toolLabel(x.name) || "Using a tool", state: x.success ? "ok" : "bad" })));
      if (t.html) appendChatReply(turn, answerHtml(t.html));
      for (const x of t.tools) toolOutput(turn, x, x.route);
      last = t.replies.length ? { turn, replies: t.replies } : null;
      turnState(turn).done = true;

      if (t.model) {
        turnState(turn).model = t.model;
        // A turn Auto did not pick still names the model that answered.
        if (viewOf(turn).ranOn === null) ranOn(turn, t.model, "");
      }

      drawWork(turn);
      replyActs(turn);
    } else if (t.role === "approval") {
      turn ??= drawTurn("");
      approvalCard(id, turn, t);
      last = null;
    }
  }

  return { drawn, last };
}

/// Puts `drawn`, the chat's turns as the server lists them, into body `b`
/// without drawing again what it shows: a turn already there stays and
/// takes the server's copy in place.
const mergeTurns = (b: ChatBody, drawn: Turn[]) => batchViews(() => mergeInto(b, drawn));

function mergeInto(b: ChatBody, drawn: Turn[]) {
  const shown = turnsOf(b);
  const kept = new Map<Turn, Turn>(), order: Turn[] = [], used = new Set<Turn>();
  const byTime = new Map<number, Turn[]>();
  for (const turn of shown) {
    const at = turnState(turn).messageAt;
    if (at !== null) { const group = byTime.get(at) ?? []; group.push(turn); byTime.set(at, group); }
  }

  const unkeyed = shown.filter(turn => turnState(turn).messageAt === null);
  for (const fresh of drawn) {
    const at = turnState(fresh).messageAt;
    const old = (at !== null ? byTime.get(at)?.find(turn => !used.has(turn)) : null)
      ?? (at !== null ? unkeyed : shown).find(turn => !used.has(turn) && sameTurn(turn, fresh));

    if (old) {
      used.add(old);
      mergeTurn(old, fresh);
      kept.set(fresh, old);
      order.push(old);
    } else order.push(fresh);
  }

  // Keep a live turn until the server includes it in its history.
  for (const turn of shown) if (!used.has(turn) && turnState(turn).messageAt === null) order.push(turn);

  placeTurns(b, order);

  return kept;
}

/// Whether `old`, on screen, is the turn the server lists as `fresh`. One
/// drawn live has no `at` yet, so its prompt says, or the line of a turn the
/// server started.
function sameTurn(old: Turn, fresh: Turn) {
  const before = turnState(old).messageAt, next = turnState(fresh).messageAt;
  if (before !== null && next !== null) return before === next;
  const said = (t: Turn) => askText(t) || (viewOf(t).system ?? "");
  return said(old) === said(fresh);
}

/// `next` when it says something other than `was`, else `was`, so an
/// unchanged part does not draw again.
const changedPart = <T>(was: T, next: T) => JSON.stringify(was) === JSON.stringify(next) ? was : next;

/// Takes the server's copy of a turn, `fresh`, into `old` on screen. The
/// prompt with its pictures and files stays as it is; the reply takes the
/// server's text; follow-ups and the approval card are only what the server
/// lists now; any other part takes the server's copy when it has one.
function mergeTurn(old: Turn, fresh: Turn) {
  const at = turnState(fresh).messageAt;
  if (at !== null) { turnState(old).messageAt = at; setView(old, { at }); }
  const was = viewOf(old), next = viewOf(fresh);
  // The server's copy may not have the reply yet: what shows stays.
  if (!turnState(fresh).done && !next.approval) return;
  if (next.reply !== null) turnState(old).reply = chatReplyHtml(fresh);

  setView(old, {
    system: next.system === null ? was.system : changedPart(was.system, next.system),
    prompt: was.prompt ?? next.prompt,
    ranOn: next.ranOn ?? was.ranOn,
    pictures: was.pictures ?? next.pictures,
    files: was.files ?? next.files,
    rerunOf: next.rerunOf ?? was.rerunOf,
    coders: next.coders.length ? changedPart(was.coders, next.coders) : was.coders,
    reply: next.reply ?? was.reply,
    sent: next.sent ? changedPart(was.sent, next.sent) : was.sent,
    approval: next.approval,
    answered: next.answered ?? was.answered,
    error: next.error ?? was.error,
    stopped: next.stopped ?? was.stopped,
    replies: next.replies && was.replies && JSON.stringify(next.replies.list) === JSON.stringify(was.replies.list) ? was.replies : next.replies,
  });

  if (turnState(fresh).user !== null) turnState(old).user = turnState(fresh).user;
  if (turnState(fresh).model) turnState(old).model = turnState(fresh).model;
  if (!turnState(fresh).done) return;
  turnState(old).done = true;
  turnState(old).steps = turnState(fresh).steps;
  drawWork(old);
  replyActs(old);
}

/// Puts what the window shows atop the open chat first in its body, such
/// as the card of the pull request it is about.
export function drawChatTop() {
  const b = body();
  if (!b || S.view !== "chat" || !S.session) return;
  const top = windowHooks?.chatTop?.(S.session) ?? null;
  b.querySelectorAll(":scope > .dchattop").forEach(el => { if (el !== top) el.remove(); });
  if (!top) return;
  top.classList.add("dchattop");
  if (b.firstElementChild !== top) b.prepend(top);
}

/// Calls `draw` with each turn of session `id` that `list` names by its
/// `at`, once the server lists them. The chat may have been left or redrawn meanwhile.
async function eachTurn<T extends { message: number }>(id: string, b: ChatBody, list: Promise<T[]>, draw: (turn: Turn, item: T) => void) {
  const items = await list.catch(() => []);
  if (S.session !== id || body() !== b) return;

  const byTime = new Map<number, Turn>();
  for (const turn of turnsOf(b)) {
    const at = turnState(turn).messageAt;
    if (at !== null && !byTime.has(at)) byTime.set(at, turn);
  }
  for (const item of items) {
    const turn = byTime.get(item.message);
    if (turn) draw(turn, item);
  }
}

/// Puts the files sent in session `id` under their prompts.
export function loadAskFiles(id: string, b: ChatBody) {
  return eachTurn(id, b, invoke<{ message: number; files: AskFile[] }[]>("session_files", { id }), (turn, { files }) => {
    if (!viewOf(turn).files && files.length) setView(turn, { files });
  });
}

/// Names the agent that answered each turn of session `id`. General and
/// turns without agents show nothing.
function loadAgents(id: string, b: ChatBody) {
  return eachTurn(id, b, invoke<TurnAgent[]>("session_agents", { id }), (turn, { agent }) => {
    if (!agent) return;
    turnState(turn).agent = agent;
    replyActs(turn);
  });
}

/// Puts the web pages each turn of session `id` used under its reply. A
/// server from before source chips lists none.
export function loadSources(id: string, b: ChatBody) {
  return eachTurn(id, b, invoke<{ message: number; sources: WebSource[] }[]>("session_sources", { id }), (turn, { sources }) => sourceRow(turn, sources));
}

/// The site a source is on, without "www.".
export function sourceSite(url: string) {
  try { return new URL(url).hostname.replace(/^www\./, ""); } catch { return ""; }
}

/// The web pages a reply used, as chips under it: the site's initial and name.
export function sourceRow(turn: Turn, sources: WebSource[] | null | undefined) {
  const list = (sources ?? []).map(source => ({ ...source, site: sourceSite(source.url) })).filter(source => source.site);
  setView(turn, { sources: list.length ? changedPart(viewOf(turn).sources, list) : [] });
}

/// The agent that answered one turn (`session_agents`).
type TurnAgent = { message: number; agent: string | null };

/// Names the agent that answered a turn that just finished live: it is the
/// newest one the server lists.
export async function loadTurnAgent(id: string | null, turn: Turn) {
  const agents = await invoke<TurnAgent[]>("session_agents", { id }).catch(() => []);
  const agent = agents.at(-1)?.agent;
  if (!agent || !shownTurn(turn) || turn !== turnsOf(turn.body).findLast(t => turnState(t).user)) return;
  turnState(turn).agent = agent;
  replyActs(turn);
}
