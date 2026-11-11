// Sending to a coding agent: messages, steers,
// answers to its questions, and interrupting it.
import { type Attached, dropDelivered, type OutboxRow, sentFromOutbox, type Waiting } from "./agentqueue.ts";
import { resumePicks } from "./modelpill.ts";
import { sessionPicks } from "./session.ts";
import { composeMessage } from "../chat/compose.ts";
import { S } from "../bar/core.ts";
import { withMentions } from "../agentpane/files-state.ts";
import { MN, takeMentions } from "./workdrop.ts";
import { disposeComposerUi, showAgentQueue } from "./composer-ui.tsx";
import { type AgentQuestion, type OpenAgent, type Started, type Steer, AG, AGENT_SEND_WAIT_MS, where } from "./agents.ts";
import { type SendError, sendError } from "./reconnect-state.ts";
import { openAgent, isOpen, resumeAgent, turnRuns } from "./session.ts";
import { redrawAgentHints } from "./session-head.ts";
import { agentAttachError, drawAgentAttached, keepSentImages, keepAgentPics } from "./images.ts";
import { drawAgentBody } from "./logview.ts";
import { sameSession } from "./running-state.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { agentMessage, sessionBody } from "./session-frame.tsx";
import { shownQuestionsIn } from "./mode-view.tsx";
import { closeMove } from "./move.ts";

export function disposeQueue() { disposeComposerUi(); closeMove(); }

/// The pending steers above the field, each with what goes with it.
export function drawQueue() {
  // Without a field the queue waits, delivered messages included, until one shows.
  if (!agentMessage.current?.isConnected) { if (S.view !== "agent" || !sessionBody()) disposeQueue(); return; }
  dropDelivered(AG.queue);
  showAgentQueue(AG.queue, editQueued, AG.outbox, editOutbox);
}

/// A message as `agent_steer` takes it, its images without what only the
/// field shows.
type Outgoing = { text: string; images?: { name: string; mime: string; data: string }[] };
const outgoing = (text: string, images: Attached[]): Outgoing => images.length ? { text, images: images.map(({ name, mime, data }) => ({ name, mime, data })) } : { text };

/// Takes what waits in the field for a message: the words, the paths it
/// mentions and the images.
function takeMessage(input: HTMLTextAreaElement | null): Waiting | null {
  if (MN.uploads.length) {
    agentAttachError(`Wait for ${MN.uploads[0].name} to upload`);
    return null;
  }

  // Copies: what is attached next must not join this message.
  const typed = composeMessage(input?.value ?? ""), images = AG.att.slice();
  if (!typed && !images.length && !MN.paths.length) return null;
  const paths = MN.paths.slice();
  const files = paths.flatMap(path => { const file = MN.files.get(path); return file ? [{ path, file }] : []; });
  const text = paths.length ? withMentions(typed, takeMentions()) : typed;
  if (input) input.value = "";
  if (images.length) { AG.att = []; drawAgentAttached(); }
  return { how: "steer", typed, text, paths, images, files, state: null, after: 0 };
}

/// Puts `q` back in the field, before what is typed there.
type Draft = Pick<Waiting, "typed" | "paths" | "images" | "files">;

function backInField(q: Draft, sep = "\n") {
  const input = agentMessage.current;
  if (input && q.typed) input.value = input.value ? `${q.typed}${sep}${input.value}` : q.typed;
  MN.paths = [...q.paths, ...MN.paths.filter(p => !q.paths.includes(p))];
  for (const { path, file } of q.files ?? []) MN.files.set(path, file);
  AG.att = [...q.images, ...AG.att];
  drawAgentAttached();
  input?.focus();
}

/// Takes waiting message `i` out; to edit, it goes back in the field.
export async function editQueued(i: number, edit: boolean) {
  const q = AG.queue[i], o = AG.open;
  if (!q || q.state === "delivered") return;
  AG.queue.splice(i, 1);
  drawAgentBody();

  if (q.how === "steer" && q.state === "sent") {
    if (!await steerAgent({ drop_steers: null })) {
      if (isOpen(o)) { AG.queue.splice(i, 0, q); drawAgentBody(); }
      return;
    }

    if (!isOpen(o)) return;
    for (const other of AG.queue.filter(x => x.how === "steer" && x.state === "sent")) await sendSteer(other);
  }

  if (edit) backInField(q);
  drawQueue();
  redrawAgentHints();
}

/// Enter while the turn runs: steers it.
export function steerMessage(input: HTMLTextAreaElement) {
  const q = takeMessage(input);
  if (!q) return;
  q.how = "steer";
  AG.queue.push(q);
  sendSteer(q);
}

async function sendSteer(q: Waiting, back = true) {
  const o = AG.open, queue = AG.queue;
  if (!o) return false;
  q.state = "sending";
  drawAgentBody();
  if (q.images.length) keepAgentPics(o, q.images);
  const sent = await sendText({ steer: outgoing(q.text, q.images) });

  if (sent === "sent") {
    q.state = "sent";
    q.after = AG.polls;
  } else {
    // It waits on this device, in the outbox's rows; or nothing went, and it
    // waits in the field again, under the note that says why.
    const at = queue.indexOf(q);
    if (at >= 0) queue.splice(at, 1);
    if (sent === "failed" && back && AG.queue === queue && isOpen(AG.open)) backInField(q);
  }

  if (AG.queue === queue && isOpen(AG.open)) drawAgentBody();
  return sent !== "failed";
}

/// ⌘. while the turn runs: stop it and keep the field, so a correction can be
/// typed and sent. The node keeps the steers it holds or still gets and sends
/// them once the turn stops, so they stay steers here: sent again from here,
/// one the node had just delivered would reach the agent twice (#378).
export function interruptAgent() {
  if (!turnRuns()) return false;
  AG.stopping = { id: AG.page!.session.id, turns: AG.page!.turns };
  drawQueue();
  redrawAgentHints();
  drawAgentBody();
  steerAgent({ cancel: null });
  agentMessage.current?.focus();
  return true;
}

/// Stop all on the open session: the turn, its subagents, background commands and the agents
/// it started. The node drops the steers it holds, so those not yet delivered go back in the field.
export async function stopAllWork(target: { machine_id: string; agent: string; id: string }) {
  const s = AG.page?.session, o = AG.open;
  if (!sameSession(o, target) || !s) return;
  // What waits on this device comes back too, after what the node held.
  const kept = await Promise.all(AG.outbox.filter(row => row.state !== "sending").map(row => takeOutbox(row.id)));
  if (AG.open !== o) return;
  const unsent: Draft[] = [...AG.queue.filter(q => q.state !== "delivered"), ...kept.filter(draft => draft !== null)];
  AG.queue.splice(0, AG.queue.length);
  // Each goes in front of what the field holds: the newest first, so the oldest leads.
  for (const q of unsent.reverse()) backInField(q);
  if (turnRuns()) AG.stopping = { id: s.id, turns: AG.page?.turns ?? 0 };
  drawQueue();
  redrawAgentHints();
  drawAgentBody();
  await steerAgent({ stop_all: null });
}

/// Binds an approval action to the question and options shown to the user,
/// so an answer reaches only the question it was shown for.
export function questionKey(q: AgentQuestion, session: OpenAgent) {
  return JSON.stringify([session.machine_id, session.agent, session.session, q.id, q.text, q.detail, q.options.map(o => [o.id, o.name, o.allow])]);
}

/// Answers the open question once: its buttons stay disabled, and a poll
/// that still shows it does not bring them back, until the node moves on.
export async function answerAgentQuestion(i: number, shown: string) {
  const o = AG.open, q = AG.page?.session.question ?? null;
  if (!o || !q || questionKey(q, o) !== shown) { drawAgentBody(); return; }
  const opt = q.options[i];
  if (!opt || AG.answering === q.id) return;
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
  for (const shown of shownQuestionsIn(sessionBody())) {
    const index = shown.allows.indexOf(allow);
    if (index < 0) continue;
    if (!shown.disabled) void answerAgentQuestion(index, shown.key);
    return true;
  }

  return false;
}

/// Sends `steer` to the open session; true when the node took it.
export async function steerAgent(steer: Steer) {
  const o = AG.open;
  if (!o || AG.page?.session.external) return false;
  const failed = await sendOnce(o, steer);
  if (isOpen(o)) { AG.note = failed?.message ?? ""; drawAgentBody(); resumeAgent(); }
  return !failed;
}

/// Sends a message to the open session: "sent" when the node took it,
/// "waiting" when it waits on this device until the machine can be reached,
/// else "failed", with the note saying why.
async function sendText(steer: { prompt: Outgoing } | { steer: Outgoing }): Promise<"sent" | "waiting" | "failed"> {
  const o = AG.open;
  if (!o || AG.page?.session.external) return "failed";
  const failed = await sendOnce(o, steer);
  const sent = !failed ? "sent" : failed.kind === "waiting" ? "waiting" : "failed";

  if (isOpen(o)) {
    AG.note = failed && sent === "failed" ? failed.message : "";
    if (sent === "waiting") void readOutbox(o);
    drawAgentBody();
    resumeAgent();
  }

  return sent;
}

/// One `agent_steer` call: null when the node took it, else why not.
async function sendOnce(o: OpenAgent, steer: Steer): Promise<SendError | null> {
  try {
    const keys = await invoke<string[]>("agent_steer", { ...where(o), steer: "prompt" in steer || "steer" in steer || "configure" in steer || "mode" in steer || "stop_task" in steer ? steer : Object.keys(steer)[0] }, AGENT_SEND_WAIT_MS);
    const sent = "prompt" in steer ? steer.prompt : "steer" in steer ? steer.steer : null;
    if (sent?.images) keepSentImages(keys, sent.images);
    return null;
  } catch (e) {
    return sendError(e);
  }
}

export async function sendAgentMessage() {
  const input = agentMessage.current, draft = takeMessage(input);
  if (!draft) return;
  const page = AG.page!;
  return page.session.status === "ended"
    ? continueAgent(input!, draft.text, draft.images, draft)
    : sendMessage(input!, draft.text, draft.images, draft);
}

/// Sends a prepared message, preserving it in the field if refused.
async function sendMessage(input: HTMLTextAreaElement, text: string, images: Attached[], draft?: Draft) {
  const o = AG.open!, page = AG.page!;
  input.value = "";

  if (images.length) {
    AG.att = [];
    drawAgentAttached();
    keepAgentPics(o, images);
  }

  const mine = { kind: "user", text: [text, ...images.map(a => `[image: ${a.name}]`)].filter(Boolean).join("\n") };
  page.entries.push(mine);
  drawAgentBody();
  const sent = await sendText({ steer: outgoing(text, images) });
  if (sent === "sent" || !isOpen(o)) return sent !== "failed";
  // A poll may have drawn more since: take out this message, not the last.
  const at = page.entries.indexOf(mine);
  if (at >= 0) page.entries.splice(at, 1);

  if (sent === "waiting") {
    // It shows in the queue, waiting for the connection; the field stays as it is.
    drawAgentBody();
    return true;
  }

  // Nothing went: the message and its images wait in the field again,
  // before anything typed since, under the note that says why.
  if (/does not take images yet/.test(AG.note)) AG.noImages = o.session;
  backInField(draft ?? { typed: text, paths: [], images });
  drawAgentBody();
  drawAgentAttached();
  return false;
}

/// Sends the diff review `text` as one message: a steer while a turn runs,
/// else a message, or what continues an ended session. "" when it went,
/// else why not; the field keeps what was typed.
export async function sendReview(text: string) {
  const o = AG.open, page = AG.page;
  if (!o || !page) return "No session is open.";
  let ok: boolean;

  if (page.session.status === "ended") {
    const input = agentMessage.current;
    if (!input) return "This session cannot be continued from here.";
    const typed = input.value;
    ok = await continueAgent(input, text, []);
    input.value = typed;
  } else if (turnRuns()) {
    const q: Waiting = { how: "steer", typed: text, text, paths: [], images: [], state: null, after: 0 };
    AG.queue.push(q);
    ok = await sendSteer(q, false);
  } else {
    const mine = { kind: "user", text };
    page.entries.push(mine);
    drawAgentBody();
    const sent = await sendText({ prompt: outgoing(text, []) });
    ok = sent !== "failed";

    if (sent !== "sent") {
      const at = page.entries.indexOf(mine);
      if (at >= 0) page.entries.splice(at, 1);
      drawAgentBody();
    }
  }

  return ok ? "" : AG.note || "Could not send the review.";
}

/// Resumes the ended session with `text`, then follows the session that
/// goes on from it. Messages with images go together after resuming.
async function continueAgent(input: HTMLTextAreaElement, text: string, images: Attached[], draft?: Draft) {
  const o = AG.open!;
  input.value = "";
  input.disabled = true;
  AG.note = "";
  if (!images.length) AG.page!.entries.push({ kind: "user", text });
  drawAgentBody();

  try {
    const pick = resumePicks.get(o.session);
    const configure = pick && (pick.model || pick.effort) ? { model: pick.model || null, effort: pick.effort } : null;
    const started = await invoke<Started>("agent_resume", { ...where(o), prompt: images.length || configure ? null : text }, AGENT_SEND_WAIT_MS);

    if (configure && pick) {
      const target = { machineId: started.session.machine_id, agent: started.session.agent, session: started.session.id };
      await invoke("agent_steer", { ...target, steer: { configure } }, AGENT_SEND_WAIT_MS);
      if (!images.length) await invoke("agent_steer", { ...target, steer: { prompt: { text } } }, AGENT_SEND_WAIT_MS);
      sessionPicks.set(started.session.id, pick);
    }

    resumePicks.delete(o.session);
    if (!isOpen(o)) return true;
    // The same session goes on in the same field.
    input.disabled = false;
    input.focus();
    openAgent(started.session, { back: AG.back });
    if (images.length) return sendMessage(agentMessage.current ?? input, text, images, draft);
    return true;
  } catch (e) {
    if (!isOpen(o)) return false;
    if (!images.length) AG.page!.entries.pop();
    input.disabled = false;
    backInField(draft ?? { typed: text, paths: [], images });
    AG.note = String(e).replace(/^(bad request|error):\s*/i, "");
    drawAgentBody();
    return false;
  }
}

// ----- The outbox: messages waiting on this device for their machine -----

/// A waiting message as `outbox_take` gives it back.
interface Taken { text: string; images: { name: string; mime: string; data: string }[] }

/// What `outbox-changed` tells every window.
interface OutboxChanged { machine_id: string; agent: string; session: string; rows: OutboxRow[]; sent: OutboxRow[] }

/// Reads what waits on this device for `o`, as it opens.
export async function readOutbox(o: OpenAgent) {
  try {
    const rows = await invoke<OutboxRow[] | null>("outbox_list", where(o));
    if (AG.open !== o) return;
    AG.outbox = rows ?? [];
    drawQueue();
  } catch (e) {
    console.warn("reading the outbox:", e);
  }
}

listen<OutboxChanged>("outbox-changed", ({ payload }) => {
  const o = AG.open;
  if (!sameSession(o, { machine_id: payload.machine_id, agent: payload.agent, id: payload.session })) return;
  AG.outbox = payload.rows;
  // What went is the node's now: its row stays until the agent has it.
  for (const row of payload.sent) AG.queue.push(sentFromOutbox(row, AG.polls));
  drawQueue();
  redrawAgentHints();
});

/// Takes waiting message `id` out; to edit, it goes back in the field.
export async function editOutbox(id: number, edit: boolean) {
  const o = AG.open, taken = await takeOutbox(id);
  if (!taken || !isOpen(o)) return;
  AG.outbox = AG.outbox.filter(row => row.id !== id);
  if (edit) backInField(taken);
  drawQueue();
  redrawAgentHints();
}

/// Takes message `id` out of the outbox, as the field takes it back.
async function takeOutbox(id: number): Promise<Draft | null> {
  try {
    const { text, images } = await invoke<Taken>("outbox_take", { id });
    return {
      typed: text, paths: [],
      images: images.map(({ name, mime, data }) => ({ name, mime, data, size: Math.floor(data.length * 3 / 4), url: `data:${mime};base64,${data}` })),
    };
  } catch (e) {
    AG.note = String(e);
    drawAgentBody();
    return null;
  }
}
