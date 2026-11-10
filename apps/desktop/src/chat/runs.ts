// A chat's runs: sending a prompt, following what it streams, finishing,
// watching another device's run and stopping.
import { turnState } from "./turn-state.ts";
import { conversation } from "./conversation.ts";
import { attached, attachError, drawAttached } from "./attach.ts";
import { drawChats, loadChats, touch } from "./chats.ts";
import { $$, changed, entry, here, live, newId, queued, S, stopping } from "../bar/core.ts";
import { refreshHints } from "../bar/hints.ts";
import { loadTurnAgent, sourceRow } from "./history.ts";
import type { WebSource } from "./history.ts";
import { heard } from "../bar/live.ts";
import { canPickModel, MODELS } from "../settings/model.ts";
import { replyActs, stoppedRow } from "./rerun.ts";
import { DESCRIBED, QUIET_TOOLS, toolLabel } from "./tools.ts";
import { answerHtml } from "../agents/turnrows.ts";
import { Channel, invoke } from "../ui/tauri.ts";
import type { Turn } from "../ui/turn.ts";
import { body, drawQueue, forgetChat, keepDown, retireFinished, scrollDown } from "./chat.ts";
import { grow } from "./ask-height.ts";
import { ANSWER, drawWork, endThought, failed, newTurn, owningChatBody, ranOn, removeChatSuggestions, setChatReply, suggestions, toolOutput, type ToolOutput } from "./turns.ts";
import { approvalCard, type Asking } from "./approval-cards.ts";
import { enqueue, sendQueued } from "./queue.ts";

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
  retireFinished(owningChatBody(turn));

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
