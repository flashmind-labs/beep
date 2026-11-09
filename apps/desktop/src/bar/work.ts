import { body, type ApprovalField, type Decision, forRoutine, keepDown } from "../chat/chat.ts";
import { here, S } from "./core.ts";
import { refreshHints } from "./hints.ts";
import { notice } from "../chat/media.ts";
import { AnsweredIds } from "../chat/approvals.ts";
import { discardChatBackground, renderChatBackground } from "../chat/work-view.tsx";
import { invoke, listen } from "../ui/tauri.ts";

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

const backgrounds = new WeakMap<HTMLElement, HTMLElement>();

/// Keeps a chat's background questions mounted as their state changes.
export function drawWorkQuestions() {
  const b = body();
  if (S.view !== "chat" || !b) return;
  const questions = WORK.get(S.session)?.questions ?? [];
  let box = backgrounds.get(b);
  if (!questions.length) {
    if (box) { discardChatBackground(b, box); box.remove(); backgrounds.delete(b); }
    return;
  }
  if (!box) {
    box = document.createElement("div");
    box.className = "dworkq dgrid8";
    backgrounds.set(b, box);
    b.append(box);
  }
  const moved = b.lastElementChild !== box;
  if (moved) b.append(box);
  const changed = renderChatBackground(b, box, { session: S.session, questions, first: waitingQuestion(S.session)?.query_id ?? null, chatApproval: !!here()?.approval });
  if (changed || moved) keepDown();
}

export function answerWorkById(id: string | null, queryId: string, decision: Decision) {
  return answerWork(id, WORK.get(id)?.questions.find(question => question.query_id === queryId), decision);
}

export async function answerWork(id: string | null, q: HeldQuestion | undefined, decision: Decision) {
  if (!q || q.answer || q.withdrawn) return;
  if ((decision === "always" || decision === "session") && forRoutine(q.action)) decision = "allow";
  q.answer = decision;
  answeredWork.mark(q.query_id, id);
  redrawIfOpen(id);

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

  redrawIfOpen(id);
}

/// After the chat is drawn again from the server: answered questions go,
/// ones still waiting stay at the end.
export function keepWaitingQuestions(id: string | null) {
  const w = WORK.get(id);
  if (w) w.questions = w.questions.filter(q => !q.answer && !q.withdrawn);
  drawWorkQuestions();
}

/// Loads the questions chat `id`'s background work still waits on, so cards
/// outlive a restart and show on every device.
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
