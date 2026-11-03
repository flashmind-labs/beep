import { body, type ApprovalField, type Decision, forRoutine, keepDown } from "./chat.ts";
import { esc, h, here, ICON, invoke, listen, S } from "./core.ts";
import { refreshHints } from "./hints.ts";
import { notice } from "./media.ts";
import { AnsweredIds } from "../approvals.ts";
import { appTile, fillAppTiles } from "../applogo.ts";

/// Questions answered in this window. A list fetched before the answer
/// landed, or a notice repeated, would bring the card back.
export const answeredWork = new AnsweredIds();

// ---------- Background work ----------

/// A question from background work (WorkQuestion).
export interface WorkQuestion {
  session_id: string;
  query_id: string;
  app: string;
  action: string;
  fields: ApprovalField[];
}

/// A question as the chat keeps it: how this bar answered it, or that it
/// no longer waits.
export interface HeldQuestion extends WorkQuestion {
  answer: Decision | null;
  withdrawn: boolean;
}

// Background sub-agents that need an OK ask through a notice, so the card
// sits at the end of its chat until it is answered.
export const WORK = new Map<string | null, { questions: HeldQuestion[] }>();
export function workOf(id: string | null) {
  if (!WORK.has(id)) WORK.set(id, { questions: [] });
  return WORK.get(id)!;
}
export const waitingQuestion = (id: string | null) => WORK.get(id)?.questions.find(q => !q.answer && !q.withdrawn);
/// Redraws the open chat's questions and the hints, when `id` is open.
const redrawIfOpen = (id: string | null) => { if (id === S.session && S.view === "chat") { drawWorkQuestions(); refreshHints(); } };

const SAID = { allow: "Allowed once. The work goes on.", session: "Allowed for the rest of this chat. The work goes on.", always: "Allowed, and always allowed from now on. The work goes on.", deny: "Denied. The work goes on without it.", grant: "Allowed for this routine. The work goes on." };
const BUTTONS = [["allow", "Allow", "pri", `⌘${ICON.ret}`], ["session", "Allow for this session", "", `⇧⌘${ICON.ret}`], ["always", "Always", "", `⌥⌘${ICON.ret}`], ["deny", "Deny", "warn", "⌘D"]];

/// The open chat's questions from background work, at the end of its body.
export function drawWorkQuestions() {
  const b = body();
  if (S.view !== "chat" || !b) return;
  b.querySelector(".dworkq")?.remove();
  const qs = WORK.get(S.session)?.questions ?? [];
  if (!qs.length) return;
  const first = waitingQuestion(S.session);
  const card = (q: HeldQuestion) => q.withdrawn || q.answer
    ? `<div class="dmuted">${esc(q.withdrawn ? `${q.action} with ${q.app}: no longer waiting` : SAID[forRoutine(q.action) && q.answer === "allow" ? "grant" : q.answer!])}</div>`
    : `<div class="dcard" data-q="${esc(q.query_id)}">
    <div class="head">${appTile(q.app)}<div style="flex:1;min-width:0;display:grid"><b>${esc(q.action)}?</b><small>Background work asks first</small></div></div>
    ${q.fields.length ? `<div class="kv">${q.fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</div>` : ""}
    <div class="acts">${BUTTONS.filter(([d]) => (d !== "always" && d !== "session") || !forRoutine(q.action)).map(([d, t, cls, k]) =>
      `<button class="dbtn ${cls}" data-d="${d}">${t}${q === first && !here()?.approval ? `<kbd>${k}</kbd>` : ""}</button>`).join("")}</div></div>`;
  const box = h(`<div class="dworkq" style="display:grid;gap:8px"><div class="dmuted" style="justify-content:center;font-size:11.5px;color:var(--dim)">${first ? "Background work is waiting for you" : "Background work"}</div>${qs.map(card).join("")}</div>`);
  box.querySelectorAll<HTMLElement>(".dcard").forEach(c => c.querySelectorAll<HTMLElement>("[data-d]").forEach(btn => btn.addEventListener("click", () =>
    answerWork(S.session, qs.find(q => q.query_id === c.dataset.q), btn.dataset.d as Decision))));
  fillAppTiles(box);
  b.append(box);
  keepDown();
}

export async function answerWork(id: string | null, q: HeldQuestion | undefined, decision: Decision) {
  if (!q || q.answer || q.withdrawn) return;
  if ((decision === "always" || decision === "session") && forRoutine(q.action)) decision = "allow";
  q.answer = decision;
  answeredWork.mark(q.query_id, id);
  if (id === S.session) { drawWorkQuestions(); refreshHints(); }
  try {
    if (await invoke<"delivered" | "withdrawn">("answer_work", { session: id, queryId: q.query_id, decision }) === "withdrawn") {
      q.answer = null;
      q.withdrawn = true;
    }
  } catch (err) {
    q.answer = null;
    answeredWork.forget(q.query_id);
    if (id === S.session) notice(String(err), { bad: true });
  }
  if (id === S.session) { drawWorkQuestions(); refreshHints(); }
}

/// After the chat is drawn again from the server: answered questions go,
/// ones still waiting stay at the end.
export function keepWaitingQuestions(id: string | null) {
  const w = WORK.get(id);
  if (w) w.questions = w.questions.filter(q => !q.answer && !q.withdrawn);
  drawWorkQuestions();
}

/// Loads the questions chat `id`'s background work still waits on, so cards
/// outlive a restart and show on every device. Ones answered here stay to say
/// so; ones no longer listed went elsewhere and leave quietly.
export async function loadQuestions(id: string | null) {
  if (!id) return;
  const fetched = await invoke<WorkQuestion[]>("list_questions", { session: id }).catch(() => null);
  if (!fetched) return;
  const listed = answeredWork.keep(id, fetched, q => q.query_id);
  const w = workOf(id), known = new Map(w.questions.map(q => [q.query_id, q]));
  w.questions = [
    ...w.questions.filter(q => q.answer && !listed.some(x => x.query_id === q.query_id)),
    ...listed.map(q => known.get(q.query_id) ?? { ...q, answer: null, withdrawn: false }),
  ];
  redrawIfOpen(id);
}

// A question answered on another device, or whose work stopped. One this
// bar answered already says how.
listen<{ session_id: string; query_id: string }>("work-resolved", e => {
  const { session_id, query_id } = e.payload;
  const q = WORK.get(session_id)?.questions.find(x => x.query_id === query_id);
  if (!q || q.answer || q.withdrawn) return;
  q.withdrawn = true;
  redrawIfOpen(session_id);
});

listen("reconnected", () => {
  if (S.view === "chat" && S.session) loadQuestions(S.session);
});

listen<WorkQuestion>("work-question", e => {
  const q = e.payload, w = workOf(q.session_id);
  if (answeredWork.has(q.query_id) || w.questions.some(x => x.query_id === q.query_id)) return;
  w.questions.push({ ...q, answer: null, withdrawn: false });
  redrawIfOpen(q.session_id);
});
