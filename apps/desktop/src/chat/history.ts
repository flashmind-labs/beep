import { askFiles } from "./attach.ts";
import { ANSWER, answeredTurns, approvalCard, ASK, askText, body, drawLatest, drawWork, failed, makeTurn, newTurn, ranOn, renderChat, reply, scrollDown, sendQueued, suggestions, toolOutput, watchRun } from "./chat.ts";
import type { Asking, ToolOutput } from "./chat.ts";
import { answerHtml } from "../agents/turnrows.ts";
import { $$, changed, live, queued, S, windowHooks } from "../bar/core.ts";
import type { ImageRef, SentFile } from "./media.ts";
import { replyActs } from "./rerun.ts";
import { QUIET_TOOLS, toolLabel } from "./tools.ts";
import { keepWaitingQuestions, loadQuestions } from "../bar/work.ts";
import { esc } from "../ui/format.ts";
import { h } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";
import type { ChatBody, Step, Turn } from "../ui/turn.ts";

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
  if (!running) live.delete(id);
  renderChat();
  $$<HTMLTextAreaElement>("#ask").placeholder = "Ask a follow-up…";
  if (running) {
    drawChatTop();
    scrollDown();
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
export async function refreshSession(id: string) {
  const b = body(), stuck = b?.stuck !== false, top = b?.scrollTop ?? 0;
  const turns = await invoke<HistoryTurn[]>("open_session", { id }).catch(() => null);
  if (!turns || S.session !== id || S.view !== "chat" || live.has(id) || body() !== b) return;
  drawHistory(id, turns);
  if (b && !stuck) { b.scrollTop = top; b.stuck = false; drawLatest(); }
}

export function drawHistory(id: string, turns: HistoryTurn[]) {
  // Approvals answered here stay gone until the server stops listing them.
  const asking = new Set(answeredTurns.keep(id, turns.flatMap(t => t.role === "approval" ? [t.query_id] : []), q => q));
  turns = turns.filter(t => t.role !== "approval" || asking.has(t.query_id));
  const b = body()!;
  // Built apart, then merged into what the chat already shows.
  const drawn: Turn[] = [];
  const drawTurn = (question: string, images?: ImageRef[]) => {
    const turn = makeTurn(question, images);
    drawn.push(turn);
    return turn;
  };
  let turn: Turn | null = null, last: { turn: Turn; replies: string[] } | null = null;
  for (const t of turns) {
    if (t.role === "user") {
      turn = drawTurn(t.text, t.images);
      turn.user = true;
      turn.dataset.at = String(t.at);
      ranOn(turn, t.model, t.thinking);
    } else if (t.role === "scheduled") {
      // The server started this turn: a small line, not a message the user sent.
      turn = drawTurn("");
      turn.prepend(h(`<div class="dsystem">${esc(t.label || "Routine")}</div>`));
      turn.user = false;
    } else if (t.role === "assistant") {
      turn ??= drawTurn("");
      turn.steps.push(...t.tools.filter(x => !QUIET_TOOLS.has(x.name)).map((x): Step => ({ id: "", name: x.name, label: x.label || toolLabel(x.name) || "Using a tool", state: x.success ? "ok" : "bad" })));
      if (t.html) reply(turn).innerHTML += answerHtml(t.html);
      for (const x of t.tools) toolOutput(turn, x, x.route);
      last = t.replies.length ? { turn, replies: t.replies } : null;
      turn.done = true;
      if (t.model) {
        turn.model = t.model;
        // A turn Auto did not pick still names the model that answered.
        if (!turn.querySelector(".dranon")) ranOn(turn, t.model, "");
      }
      drawWork(turn);
      replyActs(turn);
    } else if (t.role === "approval") {
      turn ??= drawTurn("");
      approvalCard(id, turn, t);
      last = null;
    }
  }
  // Only the latest reply's follow-ups still make sense.
  if (last) suggestions(last.turn, last.replies);
  const kept = mergeTurns(b, drawn);
  // An approval card drawn here belongs to the turn the chat keeps.
  const e = live.get(id);
  if (e) {
    if (e.approval) e.approval.turn = kept.get(e.approval.turn) ?? e.approval.turn;
    e.body ??= b;
  }
  drawChatTop();
  scrollDown();
  loadAskFiles(id, b);
  loadAgents(id, b);
  loadSources(id, b);
  keepWaitingQuestions(id);
}

/// Puts `drawn`, the chat's turns as the server lists them, into body `b`
/// without drawing again what it shows: a turn already there stays the same
/// element and takes the server's copy in place.
function mergeTurns(b: ChatBody, drawn: Turn[]) {
  const shown = [...b.children].filter(el => el.classList.contains("turn")) as Turn[];
  const kept = new Map<Turn, Turn>(), order: Turn[] = [];
  let next = 0;
  for (const fresh of drawn) {
    const old = shown[next];
    if (old && sameTurn(old, fresh)) {
      mergeTurn(old, fresh);
      kept.set(fresh, old);
      order.push(old);
      next++;
    } else {
      order.push(fresh);
    }
  }
  // A turn the server's copy no longer lists goes, unless it was drawn live
  // and the copy has not caught up with it.
  for (const t of shown.slice(next)) {
    if (t.dataset.at) t.remove();
    else order.push(t);
  }
  let prev: Element | null = null;
  for (const t of order) {
    if (t.parentElement !== b) {
      const first = b.querySelector(":scope > .turn");
      if (prev) prev.after(t);
      else if (first) first.before(t);
      else b.append(t);
    }
    prev = t;
  }
  return kept;
}

/// Whether `old`, on screen, is the turn the server lists as `fresh`. One
/// drawn live has no `at` yet, so its prompt says, or the line of a turn the
/// server started.
function sameTurn(old: Turn, fresh: Turn) {
  if (old.dataset.at && fresh.dataset.at) return old.dataset.at === fresh.dataset.at;
  const said = (t: Turn) => askText(t) || (t.querySelector(":scope > .dsystem")?.textContent ?? "");
  return said(old) === said(fresh);
}

/// Parts of a turn that stay the element they are: the prompt with its
/// pictures and files. The reply stays too, its text updated when it differs.
const STABLE = new Set(["dcoderen user", "dpics", "dachips"]);
/// Parts drawn again into the kept turn, so their buttons act on it.
const REDRAWN = new Set(["tools dcoderlog", "dreplyacts"]);
/// Parts that are only what the server lists now.
const LISTED = new Set(["dreplies", "dcard"]);

/// Takes the server's copy of a turn, `fresh`, into `old` on screen.
function mergeTurn(old: Turn, fresh: Turn) {
  if (fresh.dataset.at) old.dataset.at = fresh.dataset.at;
  // The server's copy may not have the reply yet: what shows stays.
  if (!fresh.done && !fresh.querySelector(":scope > .dcard")) return;
  const seen = new Map<string, number>();
  const keyOf = (el: Element) => {
    const n = seen.get(el.className) ?? 0;
    seen.set(el.className, n + 1);
    return `${el.className}#${n}`;
  };
  const before = new Map([...old.children].map(el => [keyOf(el), el]));
  seen.clear();
  const matched = new Set<Element>();
  let prev: Element | null = null;
  for (const el of [...fresh.children]) {
    const was = before.get(keyOf(el));
    if (was) matched.add(was);
    if (REDRAWN.has(el.className)) {
      if (was) prev = was;
      continue;
    }
    let now: Element = el;
    if (!was) {
      if (prev) prev.after(el);
      else old.prepend(el);
    } else if (STABLE.has(el.className)) {
      now = was;
    } else if (el.className === "dcoderen agent md") {
      if (was.innerHTML !== el.innerHTML) was.innerHTML = el.innerHTML;
      now = was;
    } else if (!LISTED.has(el.className) && was.outerHTML === el.outerHTML) {
      now = was;
    } else {
      was.replaceWith(el);
    }
    prev = now;
  }
  for (const el of before.values()) if (!matched.has(el) && LISTED.has(el.className)) el.remove();
  if (fresh.user !== undefined) old.user = fresh.user;
  if (fresh.model) old.model = fresh.model;
  if (!fresh.done) return;
  old.done = true;
  old.steps = fresh.steps;
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
  for (const item of items) {
    const turn = b.querySelector<Turn>(`.turn[data-at="${item.message}"]`);
    if (turn) draw(turn, item);
  }
}

/// Puts the files sent in session `id` under their prompts.
export function loadAskFiles(id: string, b: ChatBody) {
  return eachTurn(id, b, invoke<{ message: number; files: SentFile[] }[]>("session_files", { id }), (turn, { files }) => {
    if (turn.querySelector(".dachips")) return;
    (turn.querySelector(".dpics") ?? turn.querySelector(ASK))?.after(askFiles(files));
  });
}

/// Names the agent that answered each turn of session `id`. General and
/// turns without agents show nothing.
function loadAgents(id: string, b: ChatBody) {
  return eachTurn(id, b, invoke<TurnAgent[]>("session_agents", { id }), (turn, { agent }) => {
    if (!agent) return;
    turn.agent = agent;
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
  turn.querySelector(".dsources")?.remove();
  const list = (sources ?? []).map(s => ({ ...s, site: sourceSite(s.url) })).filter(s => s.site);
  if (!list.length) return;
  const row = h('<div class="dsources" aria-label="Sources"></div>');
  const draw = (open: boolean) => {
    const shown = open ? list : list.slice(0, 3), more = list.length - shown.length;
    row.innerHTML = shown.map((s, i) => `<button class="dsrc" data-i="${i}" title="${esc(s.title || s.url)}"><i>${esc(s.site[0].toUpperCase())}</i><span>${esc(s.site)}</span></button>`).join("")
      + (more ? `<button class="dsrc more" aria-label="Show ${more} more sources">+${more}</button>` : "");
    row.querySelectorAll<HTMLElement>(".dsrc[data-i]").forEach(b => b.addEventListener("click", () => invoke("open_url", { url: shown[+b.dataset.i!].url }).catch(() => {})));
    row.querySelector(".dsrc.more")?.addEventListener("click", () => draw(true));
  };
  draw(false);
  const anchor = turn.querySelector(".files") ?? turn.querySelector(ANSWER);
  anchor ? anchor.after(row) : turn.append(row);
}

/// The agent that answered one turn (`session_agents`).
type TurnAgent = { message: number; agent: string | null };

/// Names the agent that answered a turn that just finished live: it is the
/// newest one the server lists.
export async function loadTurnAgent(id: string | null, turn: Turn) {
  const agents = await invoke<TurnAgent[]>("session_agents", { id }).catch(() => []);
  const agent = agents.at(-1)?.agent;
  if (!agent || !turn.isConnected || turn !== [...body()!.querySelectorAll<Turn>(".turn")].findLast(t => t.user)) return;
  turn.agent = agent;
  replyActs(turn);
}
