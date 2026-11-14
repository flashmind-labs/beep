// Sending to a coding agent: messages, steers,
// answers to its questions, and interrupting it.
import { type Attached, dropDelivered, type OutboxRow, sentFromOutbox, type Waiting } from "./agentqueue.ts";
import { resumePicks } from "./modelpill.ts";
import { sessionPicks } from "./session.ts";
import { composeMessage, prependDraft } from "../chat/compose.ts";
import { conversationKey } from "./conversation-state.ts";
import { S } from "../bar/core.ts";
import { withMentions } from "../agentpane/files-state.ts";
import { MN, currentMentions, mentionImages, setMentionImages, type MentionState } from "./workdrop.ts";
import { disposeComposerUi, showAgentQueue, showUploadPending } from "./composer-ui.tsx";
import { type AgentQuestion, type OpenAgent, type SentImage, type Started, type Steer, AG, AGENT_SEND_WAIT_MS, openOf, sentImage, where } from "./agents.ts";
import { type SendError, sendError } from "./reconnect-state.ts";
import { openAgent, isOpen, resumeAgent, turnRuns } from "./session.ts";
import { redrawAgentHints } from "./session-head.ts";
import { drawAgentAttached, keepSentImages, keepAgentPics } from "./images.ts";
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
type Outgoing = { text: string; images?: SentImage[] };
const outgoing = (text: string, images: Attached[]): Outgoing => images.length ? { text, images: images.map(sentImage) } : { text };

/// Takes what waits in the field for a message: the words, the paths it
/// mentions and the images.
function takeMessage(input: HTMLTextAreaElement | null, uploading = false, reserved = new Set<string>()): Waiting | null {

  // Copies: what is attached next must not join this message.
  const typed = composeMessage(input?.value ?? "");
  const images = AG.att.filter(image => typeof image.original !== "string" || !reserved.has(image.original));
  const paths = MN.paths.filter(path => !reserved.has(path));
  if (!typed && !images.length && !paths.length && !uploading) return null;
  const files = paths.flatMap(path => { const file = MN.files.get(path); return file ? [{ path, file }] : []; });
  const text = paths.length ? withMentions(typed, paths) : typed;
  MN.paths = MN.paths.filter(path => reserved.has(path));
  for (const path of paths) MN.files.delete(path);
  MN.redraw();
  if (input) input.value = "";
  if (images.length) { AG.att = AG.att.filter(image => !images.includes(image)); drawAgentAttached(); }
  return { typed, text, paths, images, files, state: null, after: 0 };
}

// Each batch belongs to at most one send, even if Enter is pressed twice.
const claimedUploads = new WeakSet<object>();

function prepareMessage(input: HTMLTextAreaElement | null): Waiting | null | Promise<Waiting | null> {
  if (input?.readOnly) return null;
  const typed = input?.value ?? "", state = currentMentions();
  const batches = [...MN.pending].filter(batch => !claimedUploads.has(batch));
  const reserved = new Set([...MN.pending].filter(batch => claimedUploads.has(batch)).flatMap(batch => batch.paths));
  for (const batch of batches) claimedUploads.add(batch);
  const draft = takeMessage(input, batches.length > 0, reserved), session = AG.open;
  if (!draft || !batches.length) return draft;
  if (input) input.value = typed;
  state.sending = true;
  showUploadPending(true);
  return finishMessage(draft);

  async function finishMessage(draft: Waiting) {
    try {
      await Promise.all(batches.map(batch => batch.ready));
    } finally {
      state.sending = false;
      if (input) input.value = "";
      if (currentMentions() === state) {
        if (agentMessage.current) agentMessage.current.value = "";
        showUploadPending(false);
      }
    }

    const paths = new Set(batches.flatMap(batch => batch.paths));
    const images = mentionImages(state).filter(image => typeof image.original === "string" && paths.has(image.original));
    for (const path of paths) {
      if (!draft.paths.includes(path)) draft.paths.push(path);
      const file = state.files.get(path);
      if (file) (draft.files ??= []).push({ path, file });
      state.files.delete(path);
    }

    draft.images.push(...images);
    draft.text = withMentions(draft.typed, draft.paths);
    state.paths = state.paths.filter(path => !paths.has(path));
    setMentionImages(state, mentionImages(state).filter(image => !images.includes(image)));
    if (currentMentions() === state) drawAgentAttached();

    if (batches.some(batch => batch.failed)) {
      restoreUploadDraft(draft, state, session);
      return null;
    }

    return draft;
  }
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

function restoreUploadDraft(draft: Draft, state: MentionState, session: OpenAgent | null) {
  if (currentMentions() === state && S.view === "agent" && agentMessage.current?.isConnected) {
    AG.attError = state.error;
    backInField(draft);
    return;
  }
  state.paths = [...draft.paths, ...state.paths.filter(path => !draft.paths.includes(path))];
  for (const { path, file } of draft.files ?? []) state.files.set(path, file);
  setMentionImages(state, [...draft.images, ...mentionImages(state)]);
  if (session) prependDraft(conversationKey({ ...session, id: session.session }), draft.typed);
}

async function sendBackground(session: OpenAgent, draft: Waiting, state: MentionState, ended: boolean) {
  let target = session;
  try {
    if (ended) {
      const started = await invoke<Started>("agent_resume", { ...where(session), prompt: null }, AGENT_SEND_WAIT_MS);
      target = openOf(started.session);
      const pick = resumePicks.get(session.session);
      if (pick && (pick.model || pick.effort)) {
        await invoke("agent_steer", { ...where(target), steer: { configure: { model: pick.model || null, effort: pick.effort } } }, AGENT_SEND_WAIT_MS);
        sessionPicks.set(target.session, pick);
      }
      resumePicks.delete(session.session);
    }

    keepAgentPics(target, draft.images);
    const error = await sendOnce(target, { steer: outgoing(draft.text, draft.images) }, null);
    if (error && error.kind !== "waiting" && error.kind !== "timeout") {
      state.error = error.message;
      restoreUploadDraft(draft, state, session);
    }
  } catch (error) {
    state.error = sendError(error).message;
    restoreUploadDraft(draft, state, session);
  }
}

/// Takes waiting message `i` out; to edit, it goes back in the field.
export async function editQueued(i: number, edit: boolean) {
  const q = AG.queue[i], o = AG.open;
  if (!q || q.state === "delivered") return;
  AG.queue.splice(i, 1);
  drawAgentBody();

  if (q.state === "sent") {
    if (!await steerAgent({ drop_steers: null })) {
      if (isOpen(o)) { AG.queue.splice(i, 0, q); drawAgentBody(); }
      return;
    }

    if (!isOpen(o)) return;
    for (const other of AG.queue.filter(x => x.state === "sent")) await sendSteer(other);
  }

  if (edit) backInField(q);
  drawQueue();
  redrawAgentHints();
}

/// Enter while the turn runs: steers it.
export function steerMessage(input: HTMLTextAreaElement) {
  const session = AG.open, state = currentMentions(), ended = AG.page?.session.status === "ended";
  const draft = prepareMessage(input);
  if (draft instanceof Promise) return draft.then(q => {
    if (q && session && AG.open !== session) return sendBackground(session, q, state, ended);
    queueSteer(q);
  });
  queueSteer(draft);
}

function queueSteer(q: Waiting | null) {
  if (!q) return;
  AG.queue.push(q);
  sendSteer(q);
}

async function sendSteer(q: Waiting, back = true) {
  const o = AG.open, queue = AG.queue;
  if (!o) return false;
  q.state = "sending";
  drawAgentBody();
  if (q.images.length) keepAgentPics(o, q.images);
  const sent = await sendText({ steer: outgoing(q.text, q.images) }, seenNow());

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
  if (!o || !sameSession(o, target) || !s) return;
  // What waits on this device comes back too, after what the node held,
  // taken out in one step so none of it starts going meanwhile.
  const kept = await drainOutbox(o);
  if (AG.open !== o) return;
  const unsent: Draft[] = [...AG.queue.filter(q => q.state !== "delivered"), ...kept];
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
  const failed = await sendOnce(o, steer, null);
  if (isOpen(o)) { AG.note = failed?.message ?? ""; drawAgentBody(); resumeAgent(); }
  return !failed;
}

/// What the open session showed before a message, so a check after a
/// dropped send counts only what came after it.
type Seen = { entries: number; queued: string[] };
const seenNow = (): Seen | null => AG.page ? { entries: AG.page.earlier + AG.page.entries.length, queued: AG.page.queued_messages ?? [] } : null;

/// Sends a message to the open session: "sent" when the node took it,
/// "waiting" when it waits on this device until the machine can be reached,
/// else "failed", with the note saying why. The outbox owns a message once
/// it has it, so one whose send got no answer in time waits there too.
async function sendText(steer: { prompt: Outgoing } | { steer: Outgoing }, seen: Seen | null): Promise<"sent" | "waiting" | "failed"> {
  const o = AG.open;
  if (!o || AG.page?.session.external) return "failed";
  const failed = await sendOnce(o, steer, seen);
  const sent = !failed ? "sent" : failed.kind === "waiting" || failed.kind === "timeout" ? "waiting" : "failed";

  if (isOpen(o)) {
    AG.note = failed && sent === "failed" ? failed.message : "";
    if (sent === "waiting") void readOutbox(o);
    drawAgentBody();
    resumeAgent();
  }

  return sent;
}

/// One `agent_steer` call: null when the node took it, else why not.
async function sendOnce(o: OpenAgent, steer: Steer, seen: Seen | null): Promise<SendError | null> {
  try {
    const keys = await invoke<string[]>("agent_steer", { ...where(o), steer: "prompt" in steer || "steer" in steer || "configure" in steer || "mode" in steer || "stop_task" in steer ? steer : Object.keys(steer)[0], seen }, AGENT_SEND_WAIT_MS);
    const sent = "prompt" in steer ? steer.prompt : "steer" in steer ? steer.steer : null;
    if (sent?.images) keepSentImages(keys, sent.images);
    return null;
  } catch (e) {
    return sendError(e);
  }
}

export async function sendAgentMessage() {
  const input = agentMessage.current, session = AG.open, state = currentMentions(), ended = AG.page?.session.status === "ended", prepared = prepareMessage(input);
  const draft = prepared instanceof Promise ? await prepared : prepared;
  if (!draft || !session) return;
  if (AG.open !== session) return sendBackground(session, draft, state, ended);
  const page = AG.page!;
  return page.session.status === "ended"
    ? continueAgent(input!, draft.text, draft.images, draft)
    : sendMessage(input!, draft.text, draft.images, draft);
}

/// Sends a prepared message, preserving it in the field if refused.
async function sendMessage(input: HTMLTextAreaElement, text: string, images: Attached[], draft?: Draft) {
  const o = AG.open!, page = AG.page!;
  if (!draft) input.value = "";

  if (images.length) {
    if (!draft) AG.att = [];
    drawAgentAttached();
    keepAgentPics(o, images);
  }

  const mine = { kind: "user", text: [text, ...images.map(a => `[image: ${a.name}]`)].filter(Boolean).join("\n") };
  const seen = seenNow();
  page.entries.push(mine);
  drawAgentBody();
  const sent = await sendText({ steer: outgoing(text, images) }, seen);
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
    const q: Waiting = { typed: text, text, paths: [], images: [], state: null, after: 0 };
    AG.queue.push(q);
    ok = await sendSteer(q, false);
  } else {
    const mine = { kind: "user", text }, seen = seenNow();
    page.entries.push(mine);
    drawAgentBody();
    const sent = await sendText({ prompt: outgoing(text, []) }, seen);
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
  if (!draft) input.value = "";
  input.disabled = true;
  AG.note = "";
  if (!images.length) AG.page!.entries.push({ kind: "user", text });
  drawAgentBody();

  try {
    const pick = resumePicks.get(o.session);
    const configure = pick && (pick.model || pick.effort) ? { model: pick.model || null, effort: pick.effort } : null;
    const started = await invoke<Started>("agent_resume", { ...where(o), prompt: images.length || configure ? null : text }, AGENT_SEND_WAIT_MS);

    if (configure && pick) {
      const target = where(openOf(started.session));
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
    AG.note = sendError(e).message.replace(/^error:\s*/i, "");
    drawAgentBody();
    return false;
  }
}

// ----- The outbox: messages waiting on this device for their machine -----

/// A waiting message as `outbox_take` gives it back.
interface Taken { text: string; images: SentImage[] }

/// What `outbox-changed` tells every window.
interface OutboxChanged extends OpenAgent { rows: OutboxRow[]; sent: OutboxRow[] }

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
    return draftOf(await invoke<Taken>("outbox_take", { id }));
  } catch (e) {
    AG.note = String(e);
    drawAgentBody();
    return null;
  }
}

/// Takes every message waiting on this device for `o` out, oldest first.
export async function drainOutbox(o: OpenAgent): Promise<Draft[]> {
  try {
    const taken = await invoke<Taken[] | null>("outbox_drain", where(o));
    return (taken ?? []).map(draftOf);
  } catch (e) {
    console.warn("taking back the outbox:", e);
    return [];
  }
}

const draftOf = ({ text, images }: Taken): Draft => ({
  typed: text, paths: [],
  images: images.map(({ name, mime, data }) => ({ name, mime, data, size: Math.floor(data.length * 3 / 4), url: `data:${mime};base64,${data}`, original: null })),
});
