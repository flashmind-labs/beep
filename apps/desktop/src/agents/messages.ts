// Sending to a coding agent: messages, steers,
// answers to its questions, and interrupting it.
import { appendEntry, clearQuestion, removeEntry } from "./open-page.ts";
import { type Attached, attachOf, dropDelivered, type Extras, type OutboxRow, type Resume, sentFromOutbox, type Waiting } from "./agentqueue.ts";
import { resumePicks } from "./modelpill.tsx";
import { sessionPicks } from "./session.tsx";
import { imageLines, sentText } from "./agentlog.ts";
import { promptParts } from "./promptparts-state.ts";
import { composeMessage, prependDraft, startField } from "../chat/compose.tsx";
import { NEW_THREAD } from "./threads-state.ts";
import { conversationKey } from "./conversation-state.ts";
import { S } from "../bar/core.ts";
import { withMentions } from "../agentpane/files-state.ts";
import { paneSession } from "../agentpane/media.ts";
import { MN, attachReads, attachmentKey, currentMentions, mentionImages, mentionsOf, setAttachError, setAttached, setMentionImages, type MentionState } from "./workdrop.ts";
import { disposeComposerUi, showAgentQueue } from "./composer-ui.tsx";
import { type AgentQuestion, type AgentRow, type AgentSession, type OpenAgent, type SentImage, type Started, type Steer, AG, AGENT_SEND_WAIT_MS, openOf, sentImage, where } from "./agents.ts";
import { type SendError, sendError } from "./reconnect-state.ts";
import { openAgent, isOpen, resumeAgent, setNote, turnRuns } from "./session.tsx";
import { redrawAgentHints } from "./session-head.tsx";
import { drawAgentAttached, fitAttachedImages, keepSentImages, keepAgentPics } from "./images.tsx";
import { drawAgentBody } from "./logview.tsx";
import { sameSession } from "./running-state.ts";
import { conflicted, invoke, listen } from "../ui/tauri.ts";
import { reopened } from "./closing.ts";
import { loadSessions, sessions } from "./sessions.ts";
import { agentMessage, sessionBody } from "./session-frame.tsx";
import { shownQuestionsIn } from "./mode-view.tsx";
import { closeMove } from "./move.ts";
import { clearHeldSteers, heldSteers } from "./session-follow.ts";

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
const outgoing = (text: string, images: Attached[]): Outgoing => images.length ? { text: withMentions(text, [], images.length), images: images.map(sentImage) } : { text };

/// Takes what waits in the field for a message: the words, the paths it
/// mentions, the files that go with it and the images. Files keep their words
/// alone: the outbox adds their paths once they are uploaded.
function takeMessage(input: HTMLTextAreaElement | null, reading = false): Waiting | null {
  // Copies: what is attached next must not join this message.
  const typed = composeMessage(input?.value ?? "");
  const images = [...AG.att], paths = [...MN.paths];
  // Files still being read join it when they are.
  if (!typed && !images.length && !paths.length && !reading) return null;
  const files = paths.flatMap(path => { const file = MN.files.get(path); return file ? [{ path, file }] : []; });
  const text = files.length ? typed : withMentions(typed, paths, images.length);
  const root = paneSession()?.cwd ?? "";
  MN.paths = [];
  MN.files.clear();
  MN.redraw();
  if (input) input.value = "";
  if (images.length) { setAttached([]); drawAgentAttached(); }
  return { typed, text, paths, images, files, root, state: null, after: 0 };
}

/// Puts `q` back in the field, before what is typed there.
type Draft = Pick<Waiting, "typed" | "paths" | "images" | "files">;

function backInField(q: Draft, sep = "\n") {
  const input = agentMessage.current;
  if (input && q.typed) input.value = input.value ? `${q.typed}${sep}${input.value}` : q.typed;
  MN.paths = [...q.paths, ...MN.paths.filter(p => !q.paths.includes(p))];
  for (const { path, file } of q.files ?? []) MN.files.set(path, file);
  setAttached([...q.images, ...AG.att]);
  drawAgentAttached();
  input?.focus();
}

function restoreDraft(draft: Draft, state: MentionState, session: OpenAgent | null) {
  const target = session ? { ...session, id: session.session } : null;
  if ((!target || sameSession(AG.open, target)) && currentMentions() === state && S.view === "agent" && agentMessage.current?.isConnected) {
    setAttachError(state.error);
    backInField(draft);
    return;
  }
  state.paths = [...draft.paths, ...state.paths.filter(path => !draft.paths.includes(path))];
  for (const { path, file } of draft.files ?? []) state.files.set(path, file);
  setMentionImages(state, [...draft.images, ...mentionImages(state)]);
  if (target && draft.typed) prependDraft(conversationKey(target), draft.typed);
}

async function sendBackground(session: OpenAgent, draft: Waiting, state: MentionState, ended: boolean) {
  try {
    keepAgentPics(session, draft.images);
    const error = await sendOnce(session, { steer: outgoing(draft.text, draft.images) }, null, extrasOf(session, draft, ended));
    if (error && error.kind !== "waiting" && error.kind !== "timeout") {
      state.error = error.message;
      restoreDraft(draft, state, session);
    }
  } catch (error) {
    state.error = sendError(error).message;
    restoreDraft(draft, state, session);
  }
}

/// The picks each ended session was resumed with, until the new session is announced: what the
/// machine was sent, not what the picker says by then.
const resumedWith = new Map<string, NonNullable<Resume["pick"]>>();

/// The resume of the ended session `o`: on the model its picks name, and then the picks.
function resumeFor(o: OpenAgent): Resume {
  const pick = resumePicks.get(o.session);
  const configure = pick && (pick.model || pick.effort || pick.fast !== null) ? { model: pick.model || null, effort: pick.effort, fast: pick.fast } : null;

  return { model: o.agent === "codex" ? configure?.model ?? null : null, pick: configure };
}

/// What goes beside the message to `o`: its files, and the resume when its session ended.
function extrasOf(o: OpenAgent, q: Pick<Waiting, "paths" | "files" | "root"> | null, ended: boolean): Extras {
  const resume = ended ? resumeFor(o) : null;
  if (resume?.pick) resumedWith.set(o.session, resume.pick); else resumedWith.delete(o.session);

  return { attach: q ? attachOf(q) : null, resume };
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
  return submit(input, q => {
    if (session && AG.open !== session) return sendBackground(session, q, state, ended);
    queueSteer(q);
  });
}

// A batch of files being read goes with exactly one message.
const claimedReads = new WeakSet<object>();
// The latest message of each conversation, until its send has been handed to the app.
const handing = new Map<string, Promise<void>>();

/// Takes what the field holds and runs `send` with it. The field is free at
/// once; files still being read from the disk join the message when they are,
/// and its files upload behind its row in the outbox. Messages of one
/// conversation are handed to the app in the order of Enter: one waits only
/// for the one before to be handed over, never for the network.
function submit(input: HTMLTextAreaElement | null, send: (draft: Waiting) => unknown) {
  const o = AG.open, state = currentMentions(), key = o ? conversationKey({ ...o, id: o.session }) : "";
  const reads = attachReads().filter(batch => !claimedReads.has(batch));
  for (const batch of reads) claimedReads.add(batch);
  const draft = takeMessage(input, reads.length > 0);
  if (!draft) return;
  const before = handing.get(key);
  if (!before && !reads.length) return send(draft);

  let sending: unknown;
  const handed = (before ?? Promise.resolve()).then(async () => {
    if (reads.length) {
      await Promise.all(reads.map(batch => batch.ready));
      if (!await joinRead(draft, reads, state, o)) return;
    }

    sending = send(draft);
  });
  const mine: Promise<void> = handed.catch(() => {}).then(() => { if (handing.get(key) === mine) handing.delete(key); });
  handing.set(key, mine);

  return handed.then(() => sending);
}

/// Adds what `reads` attached since the message was taken from its conversation
/// `state`. False when nothing is left to send.
async function joinRead(draft: Waiting, reads: ReturnType<typeof attachReads>, state: MentionState, session: OpenAgent | null) {
  // Only what is still in the conversation: another message may have taken a file already.
  const paths = reads.flatMap(batch => batch.paths).filter(path => state.files.has(path)), own = new Set(reads.flatMap(batch => batch.images));
  const images = mentionImages(state).filter(image => own.has(image));

  for (const path of paths) {
    if (!draft.paths.includes(path)) draft.paths.push(path);
    const file = state.files.get(path);
    if (file) (draft.files ??= []).push({ path, file });
    state.files.delete(path);
  }

  draft.images.push(...images);
  state.paths = state.paths.filter(path => !paths.includes(path));
  setMentionImages(state, mentionImages(state).filter(image => !images.includes(image)));
  if (currentMentions() === state) drawAgentAttached();
  if (!draft.typed && !draft.images.length && !draft.paths.length) return false;

  try {
    draft.images = await fitAttachedImages(draft.images);
  } catch (e) {
    state.error = String(e);
    restoreDraft(draft, state, session);
    return false;
  }

  draft.text = draft.files?.length ? draft.typed : withMentions(draft.typed, draft.paths, draft.images.length);
  return true;
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
  const sent = await sendText({ steer: outgoing(q.text, q.images) }, seenNow(), extrasOf(o, q, false));

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

/// ⌘. while the turn runs or asks: stop it and keep the field, so a correction can be
/// typed and sent. The node answers an open question as cancelled. It keeps the
/// steers it holds or still gets and sends them once the turn stops, so they
/// stay steers here: sent again from here, one the node had just delivered
/// would reach the agent twice (#378).
export function interruptAgent() {
  const s = AG.page?.session;
  if (!turnRuns() && !(s?.question && !s.external)) return false;
  AG.stopping = { id: AG.page!.session.id, turns: AG.page!.turns };
  drawQueue();
  redrawAgentHints();
  drawAgentBody();
  steerAgent({ cancel: null });
  agentMessage.current?.focus();
  return true;
}

type StopAllPhase = { kind: "draining" } | { kind: "stopping"; drafts: Draft[] };
const stoppingSessions = new Map<string, StopAllPhase>();

/// Stops the session and its side work, returning the pending messages to its draft.
export async function stopAllWork(target: Pick<AgentRow, "machine_id" | "agent" | "id">) {
  const key = conversationKey(target), o = openOf(target);
  if (stoppingSessions.has(key)) return;

  const showing = () => S.view === "agent" && sameSession(AG.open, target);
  const queueAtStart = showing() ? AG.queue : null;
  const mentions = showing() ? currentMentions() : mentionsOf(target);
  stoppingSessions.set(key, { kind: "draining" });
  let stoppedSteers: Draft[] = [];

  try {
    const drafts = await drainOutbox(o);
    stoppingSessions.set(key, { kind: "stopping", drafts });

    const queue = showing() ? AG.queue : queueAtStart;
    const waiting = queue?.filter(q => q.state !== "delivered");
    const held = waiting ? [] : heldSteers(o);
    const stopping = showing() && turnRuns() ? { id: target.id, turns: AG.page?.turns ?? 0 } : null;

    if (stopping) {
      AG.stopping = stopping;
      drawQueue();
      redrawAgentHints();
      drawAgentBody();
    }

    const failed = await sendOnce(o, { stop_all: null }, null);
    if (!failed) {
      clearHeldSteers(o);

      if (queue && waiting) {
        stoppedSteers = waiting.filter(q => q.state !== "delivered");
        for (const q of waiting) {
          const at = queue.indexOf(q);
          if (at >= 0) queue.splice(at, 1);
        }
      } else {
        stoppedSteers = held.map(typed => ({ typed, paths: [], images: [] }));
      }
    }

    if (showing()) {
      if (failed && AG.stopping === stopping) AG.stopping = null;
      setNote(failed?.message ?? "");
    } else if (failed) {
      throw new Error(failed.message);
    }
  } finally {
    const phase = stoppingSessions.get(key);
    stoppingSessions.delete(key);

    if (phase?.kind === "stopping") {
      const state = showing() ? currentMentions() : mentions;
      // Prepend newest first so the field keeps the original order.
      for (const q of [...stoppedSteers, ...phase.drafts].reverse()) restoreDraft(q, state, o);
    }

    if (showing()) {
      drawQueue();
      redrawAgentHints();
      drawAgentBody();
      resumeAgent();
    }
  }
}

/// Binds an approval action to the question and options shown to the user,
/// so an answer reaches only the question it was shown for.
export function questionKey(q: AgentQuestion, session: OpenAgent) {
  return JSON.stringify([session.machine_id, session.agent, session.session, q.id, q.text, q.detail, q.options.map(o => [o.id, o.name, o.allow])]);
}

/// Sends `reply` to question `id` of `o`: null when the node took it, else what it refused with.
export async function replyToQuestion(o: OpenAgent, id: number, reply: string): Promise<{ error: unknown } | null> {
  try {
    await invoke("center_answer_agent", { ...where(o), question: id, reply });
    return null;
  } catch (error) {
    return { error };
  }
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

  const failed = await replyToQuestion(o, q.id, opt.id);

  if (!failed) {
    if (isOpen(o) && AG.page?.session.question?.id === q.id) clearQuestion();
  } else {
    // A conflict means it was answered already, so it stays answered.
    const conflict = conflicted(failed.error);
    if (!conflict && AG.answering === q.id) AG.answering = null;
    if (isOpen(o)) setNote(conflict ? "" : String(failed.error));
  }

  if (isOpen(o)) { drawAgentBody(); resumeAgent(); }
}
/// The first option of the open permission question that allows, or that does not.
export function answerFirst(allow: boolean) {
  for (const shown of shownQuestionsIn(sessionBody())) {
    // A choice has nothing that allows, so neither key picks from it.
    const index = shown.allows.includes(true) ? shown.allows.indexOf(allow) : -1;
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
  if (isOpen(o)) { setNote(failed?.message ?? ""); drawAgentBody(); resumeAgent(); }
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
async function sendText(steer: { prompt: Outgoing } | { steer: Outgoing }, seen: Seen | null, extras: Extras = {}): Promise<"sent" | "waiting" | "failed"> {
  const o = AG.open;
  if (!o || (AG.page?.session.external && !extras.resume)) return "failed";
  const failed = await sendOnce(o, steer, seen, extras);
  const sent = !failed ? "sent" : failed.kind === "waiting" || failed.kind === "timeout" ? "waiting" : "failed";

  if (isOpen(o)) {
    setNote(failed && sent === "failed" ? failed.message : "");
    drawAgentBody();
    resumeAgent();
  }

  return sent;
}

/// One `agent_steer` call: null when the node took it, else why not.
export async function sendOnce(o: OpenAgent, steer: Steer, seen: Seen | null, extras: Extras = {}): Promise<SendError | null> {
  try {
    const keys = await invoke<string[]>("agent_steer", { ...where(o), steer: "prompt" in steer || "steer" in steer || "configure" in steer || "mode" in steer || "stop_task" in steer ? steer : Object.keys(steer)[0], seen, ...(extras.attach ? { attach: extras.attach } : {}), ...(extras.resume ? { resume: extras.resume } : {}) }, AGENT_SEND_WAIT_MS);
    const sent = "prompt" in steer ? steer.prompt : "steer" in steer ? steer.steer : null;
    if (sent?.images) keepSentImages(keys, sent.images);
    return null;
  } catch (e) {
    return sendError(e);
  }
}

export function sendAgentMessage() {
  const input = agentMessage.current, session = AG.open, state = currentMentions(), ended = AG.page?.session.status === "ended";
  if (!session) return;
  return submit(input, draft => {
    if (AG.open !== session) return sendBackground(session, draft, state, ended);
    const field = agentMessage.current ?? input;
    if (!field) return;
    return sendMessage(field, draft.text, draft.images, draft, ended);
  });
}

/// Sends a prepared message, preserving it in the field if refused.
async function sendMessage(input: HTMLTextAreaElement, text: string, images: Attached[], draft?: Draft, ended = false) {
  const o = AG.open!;
  if (!draft) input.value = "";

  if (images.length) {
    if (!draft) setAttached([]);
    drawAgentAttached();
    keepAgentPics(o, images);
  }

  const mine = { kind: "user", text: sentText(text, images) };
  const seen = seenNow();
  appendEntry(mine);
  drawAgentBody();
  const sent = await sendText({ steer: outgoing(text, images) }, seen, extrasOf(o, draft ?? null, ended));
  if (sent === "sent" || !isOpen(o)) return sent !== "failed";
  // A poll may have drawn more since: take out this message, not the last.
  removeEntry(mine);

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

/// Sends the last prompt's words again, without Beep's context or images.
export async function resendPrompt() {
  const said = AG.page?.entries.findLast(en => en.kind === "user");
  const own = said ? promptParts(said.text).filter(p => p.kind === "user").map(p => p.text).join("\n") : "";
  const text = imageLines(own).text.trim();
  if (text) await sendPrompt(text);
}

/// Sends `text` as one message: a steer while a turn runs, else a message,
/// or what continues an ended session. "" when it went, else why not; the
/// field keeps what was typed.
export async function sendPrompt(text: string) {
  const o = AG.open, page = AG.page;
  if (!o || !page) return "No session is open.";
  let ok: boolean;

  if (page.session.status === "ended") {
    ok = await sendText({ prompt: outgoing(text, []) }, seenNow(), { resume: resumeFor(o) }) !== "failed";
  } else if (turnRuns()) {
    const q: Waiting = { typed: text, text, paths: [], images: [], state: null, after: 0 };
    AG.queue.push(q);
    ok = await sendSteer(q, false);
  } else {
    const mine = { kind: "user", text }, seen = seenNow();
    appendEntry(mine);
    drawAgentBody();
    const sent = await sendText({ prompt: outgoing(text, []) }, seen);
    ok = sent !== "failed";

    if (sent !== "sent") {
      removeEntry(mine);
      drawAgentBody();
    }
  }

  return ok ? "" : AG.note || "Could not send the review.";
}

/// Resumes with the composer's selection. Older nodes still need the configure order.
export async function resumeEndedSession(session: OpenAgent): Promise<Started> {
  const pick = resumePicks.get(session.session), { model, pick: configure } = resumeFor(session);
  const started = await invoke<Started>("agent_resume", { ...where(session), prompt: null, model }, AGENT_SEND_WAIT_MS);
  reopened({ machine_id: session.machine_id, id: session.session });
  reopened(started.session);
  if (configure && pick) {
    await invoke("agent_steer", { ...where(openOf(started.session)), steer: { configure } }, AGENT_SEND_WAIT_MS);
    sessionPicks.set(started.session.id, pick);
  }
  resumePicks.delete(session.session);
  return started;
}

// ----- The outbox: messages waiting on this device for their machine -----

/// A waiting message as `outbox_take` gives it back.
interface Taken { text: string; images: SentImage[]; files: SentImage[]; mentions: string[] }

/// What `outbox-changed` tells every window. A session that ended and was
/// resumed for a message comes as `resumed`, the session that goes on from it.
interface OutboxChanged extends OpenAgent { revision: number; rows: OutboxRow[]; sent: OutboxRow[]; resumed: AgentSession | null }

/// Draws `rows` as of `revision`, unless the window already drew a newer set.
function showOutbox(rows: OutboxRow[], revision: number) {
  if (revision < AG.outboxRevision) return;
  AG.outbox = rows;
  AG.outboxRevision = revision;
  drawQueue();
  redrawAgentHints();
}

/// Reads the open session's rows once, as it opens; every change after comes pushed. A session
/// that was resumed lists the rows of the one that runs: the page goes on there, as it does when
/// the resume is pushed.
export async function readOutbox(o: OpenAgent) {
  try {
    const listed = await invoke<{ revision: number; session: string; rows: OutboxRow[] } | null>("outbox_list", where(o));
    if (!listed || AG.open !== o) return;

    if (listed.session === o.session) return showOutbox(listed.rows, listed.revision);

    const known = (list: readonly AgentSession[]) => list.find(s => s.machine_id === o.machine_id && s.id === listed.session);
    const resumed = known(sessions) ?? known(await loadSessions());
    if (resumed && AG.open === o) openAgent(resumed, { back: AG.back });
  } catch (e) {
    console.warn("reading the outbox:", e);
  }
}

listen<OutboxChanged>("outbox-changed", ({ payload }) => {
  const from = { machine_id: payload.machine_id, id: payload.session };

  if (payload.resumed) {
    const pick = resumedWith.get(payload.session);
    reopened(from);
    reopened(payload.resumed);
    if (pick) sessionPicks.set(payload.resumed.id, { model: pick.model ?? "", effort: pick.effort, fast: pick.fast });
    resumedWith.delete(payload.session);
    resumePicks.delete(payload.session);
  }

  const o = AG.open;
  if (!o || !sameSession(o, { machine_id: payload.machine_id, agent: payload.agent, id: payload.session })) return;
  // The conversation goes on in the session that resumed, in the background.
  if (payload.resumed) { openAgent(payload.resumed, { back: AG.back }); return; }
  // What went is the node's now: its page shows the row until the agent has it.
  for (const row of payload.sent) {
    const sent = sentFromOutbox(row, AG.queue, AG.page, AG.polls);
    if (sent) AG.queue.push(sent);
  }
  showOutbox(payload.rows, payload.revision);
});

/// Takes waiting message `id` out; to edit, it goes back in the field.
async function editOutbox(id: number, edit: boolean) {
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
    setNote(String(e));
    drawAgentBody();
    return null;
  }
}

/// Takes every message waiting on this device for `o` out, oldest first.
async function drainOutbox(o: OpenAgent): Promise<Draft[]> {
  try {
    const taken = await invoke<Taken[] | null>("outbox_drain", where(o));
    return (taken ?? []).map(draftOf);
  } catch (e) {
    console.warn("taking back the outbox:", e);
    return [];
  }
}

const draftOf = ({ text, images, files, mentions }: Taken): Draft => {
  const kept = files.map(({ name, mime, data }) => ({ path: attachmentKey(name), file: { name, mime, size: base64Size(data), data } }));
  return {
    typed: text, paths: [...mentions, ...kept.map(({ path }) => path)], files: kept, images: imagesOf(images),
  };
};

const imagesOf = (images: SentImage[]): Attached[] =>
  images.map(({ name, mime, data }) => ({ name, mime, data, size: Math.floor(data.length * 3 / 4), url: `data:${mime};base64,${data}`, original: null }));

/// The bytes a base64 text holds.
const base64Size = (data: string) => Math.floor(data.length * 3 / 4) - (data.endsWith("==") ? 2 : data.endsWith("=") ? 1 : 0);

/// Starts the message that asks the chat `s` for a thread, for the user to say what it is for:
/// in its field when it is open, else in its kept draft, which shows once it opens.
export function draftNewThread(s: AgentRow) {
  const field = sameSession(AG.open, s) && AG.page ? agentMessage.current : null;
  if (field) startField(field, NEW_THREAD); else prependDraft(conversationKey(s), NEW_THREAD);
}
