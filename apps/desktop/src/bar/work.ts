import { body, forRoutine, keepDown } from "./chat.ts";
import { esc, h, here, ICON, invoke, listen, S } from "./core.ts";
import { refreshHints } from "./hints.ts";
import { notice } from "./media.ts";

// ---------- Background work ----------

// Background sub-agents that need an OK ask through a notice, so the card
// sits at the end of its chat until it is answered.
export const WORK = new Map(); // session id -> { questions: [{ query_id, app, action, fields, answer, withdrawn }] }
export function workOf(id) {
  if (!WORK.has(id)) WORK.set(id, { questions: [] });
  return WORK.get(id);
}
export const waitingQuestion = id => WORK.get(id)?.questions.find(q => !q.answer && !q.withdrawn);

/// The open chat's questions from background work, at the end of its body.
export function drawWorkQuestions() {
  const b = body();
  if (S.view !== "chat" || !b) return;
  b.querySelector(".dworkq")?.remove();
  const qs = WORK.get(S.session)?.questions ?? [];
  if (!qs.length) return;
  const said = { allow: "Allowed once. The work goes on.", session: "Allowed for the rest of this chat. The work goes on.", always: "Allowed, and always allowed from now on. The work goes on.", deny: "Denied. The work goes on without it.", grant: "Allowed for this routine. The work goes on." };
  const first = waitingQuestion(S.session);
  const card = q => q.withdrawn || q.answer
    ? `<div class="dmuted">${esc(q.withdrawn ? `${q.action} with ${q.app}: no longer waiting` : said[forRoutine(q.action) && q.answer === "allow" ? "grant" : q.answer])}</div>`
    : `<div class="dcard" data-q="${esc(q.query_id)}">
    <div class="head"><span class="dapp">${esc((q.app || "?")[0].toUpperCase())}</span><div style="flex:1;min-width:0;display:grid"><b>${esc(q.action)}?</b><small>Background work asks first</small></div></div>
    ${q.fields.length ? `<div class="kv">${q.fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</div>` : ""}
    <div class="acts">${[["allow", "Allow", "pri", `⌘${ICON.ret}`], ["session", "Allow for this session", "", `⇧⌘${ICON.ret}`], ["always", "Always", "", `⌥⌘${ICON.ret}`], ["deny", "Deny", "warn", "⌘D"]].filter(([d]) => (d !== "always" && d !== "session") || !forRoutine(q.action)).map(([d, t, cls, k]) =>
      `<button class="dbtn ${cls}" data-d="${d}">${t}${q === first && !here()?.approval ? `<kbd>${k}</kbd>` : ""}</button>`).join("")}</div></div>`;
  const box = h(`<div class="dworkq" style="display:grid;gap:8px"><div class="dmuted" style="justify-content:center;font-size:11.5px;color:var(--dim)">${first ? "Background work is waiting for you" : "Background work"}</div>${qs.map(card).join("")}</div>`);
  box.querySelectorAll(".dcard").forEach(c => c.querySelectorAll("[data-d]").forEach(btn => btn.addEventListener("click", () =>
    answerWork(S.session, qs.find(q => q.query_id === c.dataset.q), btn.dataset.d))));
  b.append(box);
  keepDown();
}

export async function answerWork(id, q, decision) {
  if (!q || q.answer || q.withdrawn) return;
  if ((decision === "always" || decision === "session") && forRoutine(q.action)) decision = "allow";
  q.answer = decision;
  if (id === S.session) { drawWorkQuestions(); refreshHints(); }
  try {
    if (await invoke("answer_work", { session: id, queryId: q.query_id, decision }) === "withdrawn") {
      q.answer = null;
      q.withdrawn = true;
    }
  } catch (err) {
    q.answer = null;
    if (id === S.session) notice(String(err), { bad: true });
  }
  if (id === S.session) { drawWorkQuestions(); refreshHints(); }
}

/// After the chat is drawn again from the server: answered questions go,
/// ones still waiting stay at the end.
export function keepWaitingQuestions(id) {
  const w = WORK.get(id);
  if (w) w.questions = w.questions.filter(q => !q.answer && !q.withdrawn);
  drawWorkQuestions();
}

/// Loads the questions chat `id`'s background work still waits on, so cards
/// outlive a restart and show on every device. Ones answered here stay to say
/// so; ones no longer listed went elsewhere and leave quietly.
export async function loadQuestions(id) {
  if (!id) return;
  const listed = await invoke("list_questions", { session: id }).catch(() => null);
  if (!listed) return;
  const w = workOf(id), known = new Map(w.questions.map(q => [q.query_id, q]));
  w.questions = [
    ...w.questions.filter(q => q.answer && !listed.some(x => x.query_id === q.query_id)),
    ...listed.map(q => known.get(q.query_id) ?? { ...q, answer: null, withdrawn: false }),
  ];
  if (id === S.session && S.view === "chat") { drawWorkQuestions(); refreshHints(); }
}

// A question answered on another device, or whose work stopped. One this
// bar answered already says how.
listen("work-resolved", e => {
  const { session_id, query_id } = e.payload;
  const q = WORK.get(session_id)?.questions.find(x => x.query_id === query_id);
  if (!q || q.answer || q.withdrawn) return;
  q.withdrawn = true;
  if (session_id === S.session && S.view === "chat") { drawWorkQuestions(); refreshHints(); }
});

listen("reconnected", () => {
  if (S.view === "chat" && S.session) loadQuestions(S.session);
});

listen("work-question", e => {
  const q = e.payload, w = workOf(q.session_id);
  if (w.questions.some(x => x.query_id === q.query_id)) return;
  w.questions.push({ ...q, answer: null, withdrawn: false });
  if (q.session_id === S.session && S.view === "chat") { drawWorkQuestions(); refreshHints(); }
});

