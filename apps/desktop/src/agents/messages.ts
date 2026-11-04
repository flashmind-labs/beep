// Sending to a coding agent: messages queued while a turn runs, steers,
// answers to its questions, and interrupting it.
import { type Attached, batches, dropDelivered, steersToQueue, type Waiting } from "./agentqueue.ts";
import { composeMessage } from "../chat/compose.ts";
import { $, esc, h, ICON, invoke } from "../bar/core.ts";
import { withMentions } from "../agentpane/files-state.ts";
import { MN, takeMentions } from "./workdrop.ts";
import { type Started, type Steer, AG, AGENT_MAX_IMAGES, AGENT_MAX_IMAGES_TOTAL, AGENT_ACTIVE, where } from "./agents.ts";
import { openAgent, isOpen, resumeAgent, turnRuns, redrawAgentHints } from "./session.ts";
import { agentAttachError, drawAgentAttached, keepSentImages, keepAgentPics } from "./images.ts";
import { drawAgentBody } from "./logview.ts";

/// The steers and queued messages above the field, each with what goes
/// with it, to edit or take out.
export function drawQueue() {
  const field = $("#agfield");
  if (!field) return;
  dropDelivered(AG.queue);
  const old = $("#agqueue");
  const chip = (t: string) => `<span class="qatt" title="${esc(t)}">${esc(t)}</span>`;
  const row = (q: Waiting, i: number) => {
    const steer = q.how === "steer", done = q.state === "delivered";
    const label = steer ? (done ? "Steered" : "Steer") : "Queued";
    const when = !steer ? "after the turn" : done ? "delivered" : q.state === "sending" ? "sending" : "after the current step";
    const what = q.typed || (q.images.length || q.paths.length ? "" : q.text);
    const buttons = done ? "" : `<button data-agq="${i}" data-act="edit">Edit</button><button data-agq="${i}" aria-label="Remove">${ICON.x}</button>`;
    return `<div class="it${steer ? " steer" : ""}${done ? " done" : ""}"><span class="lbl">${label}</span><span class="t" title="${esc(q.text)}">${esc(what)}</span>`
      + `${q.images.map(a => chip(a.name)).join("")}${q.paths.map(chip).join("")}<span class="w">${when}</span>${buttons}</div>`;
  };
  const el = AG.queue.length ? h(`<div class="ag-queue" id="agqueue">${AG.queue.map(row).join("")}</div>`) : null;
  if (old) el ? old.replaceWith(el) : old.remove();
  else if (el) ($("#agatt") ?? field).before(el);
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
  const text = paths.length ? withMentions(typed, takeMentions()) : typed;
  if (input) input.value = "";
  if (images.length) { AG.att = []; drawAgentAttached(); }
  return { how: "queue", typed, text, paths, images, state: null, after: 0 };
}

/// Puts `q` back in the field, before what is typed there.
function backInField(q: Pick<Waiting, "typed" | "paths" | "images">, sep = "\n") {
  const input = $<HTMLTextAreaElement>("#agmsg");
  if (input && q.typed) input.value = input.value ? `${q.typed}${sep}${input.value}` : q.typed;
  MN.paths = [...q.paths, ...MN.paths.filter(p => !q.paths.includes(p))];
  AG.att = [...q.images, ...AG.att];
  drawAgentAttached();
  input?.focus();
}

/// Takes waiting message `i` out; to edit, it goes back in the field.
export async function editQueued(i: number, edit: boolean) {
  const q = AG.queue[i], o = AG.open;
  if (!q || q.state === "delivered") return;
  AG.queue.splice(i, 1);
  drawQueue();
  if (q.how === "steer" && q.state === "sent") {
    if (!await steerAgent({ drop_steers: null })) {
      if (isOpen(o)) { AG.queue.splice(i, 0, q); drawQueue(); }
      return;
    }
    if (!isOpen(o)) return;
    for (const other of AG.queue.filter(x => x.how === "steer" && x.state === "sent")) await sendSteer(other);
  }
  if (edit) backInField(q);
  drawQueue();
  redrawAgentHints();
}

/// Option-Enter while the turn runs: queues the message, with what is
/// attached, for after the turn.
export function queueMessage(input: HTMLTextAreaElement) {
  const q = takeMessage(input);
  if (!q) return;
  AG.queue.push(q);
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

async function sendSteer(q: Waiting) {
  const o = AG.open;
  if (!o) return;
  q.state = "sending";
  drawQueue();
  redrawAgentHints();
  if (q.images.length) keepAgentPics(o, q.images);
  const ok = await steerAgent({ steer: outgoing(q.text, q.images) });
  if (!isOpen(o)) return;
  if (q.how !== "steer") {
    // Esc made it a queued message while it was on its way.
  } else if (ok) {
    q.state = "sent";
    q.after = AG.polls;
  } else {
    // Nothing went: it waits in the field again, under the note that says why.
    const at = AG.queue.indexOf(q);
    if (at >= 0) AG.queue.splice(at, 1);
    backInField(q);
  }
  drawQueue();
  redrawAgentHints();
}

/// Sends what was queued once the turn stopped or ended, as few messages as its
/// images allow, in order.
export function flushQueue() {
  const o = AG.open, page = AG.page, s = page?.session;
  const waiting = AG.queue.filter(q => q.how === "queue");
  if (!waiting.length || !o || !page || !s || AG.flushing || AGENT_ACTIVE.has(s.status)) return;
  const ended = s.status === "ended" || s.external;
  const input = $<HTMLTextAreaElement>("#agmsg");
  if (ended && !input) return;
  AG.queue = AG.queue.filter(q => q.how !== "queue");
  drawQueue();
  redrawAgentHints();
  if (ended) {
    backInField({
      typed: waiting.map(q => q.typed).filter(Boolean).join("\n\n"),
      paths: waiting.flatMap(q => q.paths),
      images: waiting.flatMap(q => q.images),
    }, "\n\n");
    return;
  }
  AG.flushing = true;
  (async () => {
    const parts = batches(waiting, AGENT_MAX_IMAGES, AGENT_MAX_IMAGES_TOTAL);
    for (const [n, part] of parts.entries()) {
      page.entries.push({ kind: "user", text: [part.text, ...part.images.map(a => `[image: ${a.name}]`)].filter(Boolean).join("\n") });
      drawAgentBody();
      if (part.images.length) keepAgentPics(o, part.images);
      if (!await steerAgent({ prompt: outgoing(part.text, part.images) })) {
        // What did not go waits again, ahead of anything queued since.
        if (isOpen(o)) {
          const left = parts.slice(n).map(b => ({ how: "queue" as const, typed: b.text, text: b.text, paths: [], images: b.images, state: null, after: 0 }));
          AG.queue.unshift(...left);
          drawQueue();
          redrawAgentHints();
        }
        break;
      }
    }
    AG.flushing = false;
  })();
}

/// Esc while the turn runs: stop it and keep the field, so a correction can be
/// typed and sent.
export function interruptAgent() {
  if (!turnRuns()) return false;
  steersToQueue(AG.queue);
  AG.stopping = AG.page!.session.id;
  drawQueue();
  redrawAgentHints();
  drawAgentBody();
  steerAgent({ cancel: null });
  $("#agmsg")?.focus();
  return true;
}

/// Answers the open question once: its buttons stay disabled, and a poll
/// that still shows it does not bring them back, until the node moves on.
export async function answerAgentQuestion(i: number) {
  const o = AG.open!, q = AG.page?.session.question, opt = q?.options[i];
  if (!q || !opt || AG.answering === q.id) return;
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
  const q = AG.page?.session.question;
  const i = q ? q.options.findIndex(o => o.allow === allow) : -1;
  if (i >= 0) answerAgentQuestion(i);
  return i >= 0;
}

/// Sends `steer` to the open session; true when the node took it.
export async function steerAgent(steer: Steer) {
  const o = AG.open;
  if (!o || AG.page?.session.external) return false;
  let ok = false;
  try {
    const keys = await invoke<string[]>("agent_steer", { ...where(o), steer: "prompt" in steer || "steer" in steer || "configure" in steer ? steer : Object.keys(steer)[0] });
    const sent = "prompt" in steer ? steer.prompt : "steer" in steer ? steer.steer : null;
    if (sent?.images) keepSentImages(keys, sent.images);
    AG.note = "";
    ok = true;
  } catch (e) {
    AG.note = String(e).replace(/^bad request: /, "");
  }
  if (isOpen(o)) { drawAgentBody(); resumeAgent(); }
  return ok;
}

export async function sendAgentMessage() {
  const input = $<HTMLTextAreaElement>("#agmsg"), typed = composeMessage(input?.value ?? ""), images = AG.att;
  if (!typed && !images.length && !MN.paths.length) return;
  // A file still on its way would be left out of the message.
  if (MN.uploads.length) return agentAttachError(`Wait for ${MN.uploads[0].name} to upload`);
  // Paths from the Files tab and uploads go after what was typed, as `@path`.
  const text = MN.paths.length ? withMentions(typed, takeMentions()) : typed;
  const o = AG.open!, page = AG.page!;
  if (page.session.status === "ended") return continueAgent(input!, text, images);
  input!.value = "";
  if (images.length) {
    AG.att = [];
    drawAgentAttached();
    keepAgentPics(o, images);
  }
  const mine = { kind: "user", text: [text, ...images.map(a => `[image: ${a.name}]`)].filter(Boolean).join("\n") };
  page.entries.push(mine);
  drawAgentBody();
  const sent = await steerAgent({ prompt: outgoing(text, images) });
  if (sent || !isOpen(o)) return;
  // Nothing went: the message and its images wait in the field again,
  // before anything typed since, under the note that says why.
  if (/does not take images yet/.test(AG.note)) AG.noImages = o.session;
  // A poll may have drawn more since: take out this message, not the last.
  const at = page.entries.indexOf(mine);
  if (at >= 0) page.entries.splice(at, 1);
  const field = $<HTMLTextAreaElement>("#agmsg");
  if (field && text) field.value = field.value ? `${text}\n${field.value}` : text;
  if (images.length) AG.att = images;
  drawAgentBody();
  drawAgentAttached();
}

/// Resumes the ended session with `text`, then follows the session that
/// goes on from it. Images go once it runs again.
export async function continueAgent(input: HTMLTextAreaElement, text: string, images: Attached[]) {
  const o = AG.open!;
  if (images.length && !text) { AG.note = "Say something to continue the session, then send the images."; return drawAgentBody(); }
  input.value = "";
  input.disabled = true;
  AG.note = "";
  AG.page!.entries.push({ kind: "user", text });
  drawAgentBody();
  try {
    const started = await invoke<Started>("agent_resume", { ...where(o), prompt: text });
    if (!isOpen(o)) return;
    openAgent(started.session, { back: AG.back });
    if (images.length) { AG.att = images; drawAgentAttached(); }
  } catch (e) {
    if (!isOpen(o)) return;
    AG.page!.entries.pop();
    input.disabled = false;
    input.value = text;
    AG.note = String(e).replace(/^(bad request|error):\s*/i, "");
    drawAgentBody();
  }
}
