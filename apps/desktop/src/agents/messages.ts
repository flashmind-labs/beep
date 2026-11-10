// Sending to a coding agent: messages, steers,
// answers to its questions, and interrupting it.
import { type Attached, dropDelivered, type Waiting } from "./agentqueue.ts";
import { resumePicks } from "./modelpill.ts";
import { sessionPicks } from "./session.ts";
import { composeMessage } from "../chat/compose.ts";
import { $, S } from "../bar/core.ts";
import { withMentions } from "../agentpane/files-state.ts";
import { MN, takeMentions } from "./workdrop.ts";
import { disposeComposerUi, showAgentQueue } from "./composer-ui.tsx";
import { type AgentPage, type AgentQuestion, type OpenAgent, type Started, type Steer, AG, AGENT_LAST, AGENT_READ_WAIT_MS, AGENT_SEND_WAIT_MS, where } from "./agents.ts";
import { HOLD_MS, type SendError, sendError, tookIt, viewError } from "./reconnect-state.ts";
import { openAgent, isOpen, resumeAgent, turnRuns } from "./session.ts";
import { redrawAgentHints } from "./session-head.ts";
import { agentAttachError, drawAgentAttached, keepSentImages, keepAgentPics } from "./images.ts";
import { drawAgentBody } from "./logview.ts";
import { invoke } from "../ui/tauri.ts";
import { sessionBody } from "./session-frame.tsx";

export function disposeQueue() { disposeComposerUi(); }

/// The pending steers above the field, each with what goes with it.
export function drawQueue() {
  const field = $("#agfield");
  if (!field) { if (S.view !== "agent" || !sessionBody()) disposeQueue(); return; }
  dropDelivered(AG.queue);
  showAgentQueue(field, AG.queue, editQueued);
}

/// A message as `agent_steer` takes it, its images without what only the
/// field shows.
const outgoing = (text: string, images: Attached[]) => images.length ? { text, images: images.map(({ name, mime, data }) => ({ name, mime, data })) } : { text };

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
  const input = $<HTMLTextAreaElement>("#agmsg");
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
  const ok = await steerAgent({ steer: outgoing(q.text, q.images) });

  if (ok) {
    q.state = "sent";
    q.after = AG.polls;
  } else {
    // Nothing went: it waits in the field again, under the note that says why.
    const at = queue.indexOf(q);
    if (at >= 0) queue.splice(at, 1);
    if (back && AG.queue === queue && isOpen(AG.open)) backInField(q);
  }

  if (AG.queue === queue && isOpen(AG.open)) drawAgentBody();
  return ok;
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
  $("#agmsg")?.focus();
  return true;
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
  const option = `[data-agopt][data-agallow="${allow}"]`;
  const button = sessionBody()?.querySelector<HTMLButtonElement>(option) ?? $<HTMLButtonElement>(`#agplan ${option}`);
  if (!button) return false;
  if (!button.disabled) void answerAgentQuestion(Number(button.dataset.agopt), button.dataset.agquestion ?? "");
  return true;
}

/// Sends `steer` to the open session; true when the node took it. A message
/// or steer waits up to `HOLD_MS` for its machine to come back. One that may
/// have gone before the link dropped is sent again only once a read of the
/// session shows the node does not have it.
export async function steerAgent(steer: Steer) {
  const o = AG.open;
  if (!o || AG.page?.session.external) return false;
  const message = "prompt" in steer ? steer.prompt : "steer" in steer ? steer.steer : null;
  const until = Date.now() + HOLD_MS, queued = AG.page?.queued ?? 0;
  let failed = await sendOnce(o, steer);

  if (message && failed && failed.kind !== "failed") {
    AG.held++;
    if (isOpen(o)) drawAgentBody();

    while (failed && failed.kind !== "failed" && Date.now() < until && AG.open === o) {
      await new Promise(r => setTimeout(r, HOLD_RETRY_MS));
      if (failed.kind === "lost") { failed = await sendOnce(o, steer); continue; }
      const took = await takenAfterDrop(o, queued, message.text);
      if (took === true) failed = null;
      else if (took === false) failed = await sendOnce(o, steer);
      else if (took.kind === "failed") failed = took;
    }

    if (AG.open === o) AG.held--;
  }

  if (isOpen(o)) { AG.note = failed?.message ?? ""; drawAgentBody(); resumeAgent(); }
  return !failed;
}

const HOLD_RETRY_MS = 2000;

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

/// After a send the link dropped on: whether the session, read again, shows
/// the node took `text`, or why it could not be read.
async function takenAfterDrop(o: OpenAgent, queued: number, text: string): Promise<boolean | SendError> {
  try {
    return tookIt(await invoke<AgentPage>("agent_view", { ...where(o), last: AGENT_LAST }, AGENT_READ_WAIT_MS), queued, text);
  } catch (e) {
    const v = viewError(e);
    return v.kind === "lost" ? { kind: "lost", message: "" } : { kind: "failed", message: v.kind === "gone" ? "This session is no longer on the machine." : v.message };
  }
}

export async function sendAgentMessage() {
  const input = $<HTMLTextAreaElement>("#agmsg"), draft = takeMessage(input);
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
  const sent = await steerAgent({ steer: outgoing(text, images) });
  if (sent || !isOpen(o)) return sent;
  // Nothing went: the message and its images wait in the field again,
  // before anything typed since, under the note that says why.
  if (/does not take images yet/.test(AG.note)) AG.noImages = o.session;
  // A poll may have drawn more since: take out this message, not the last.
  const at = page.entries.indexOf(mine);
  if (at >= 0) page.entries.splice(at, 1);
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
    const input = $<HTMLTextAreaElement>("#agmsg");
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
    ok = await steerAgent({ prompt: outgoing(text, []) });

    if (!ok) {
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
    if (images.length) return sendMessage($<HTMLTextAreaElement>("#agmsg") ?? input, text, images, draft);
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
