import { type AgentReport, cardKey, parseAgentReport, type ReportStep } from "../agentcard.ts";
import { AGENT_ACTIVE, type AgentPage, type AgentQuestion, type AgentSession, agentAvatar, agentKind, openAgent, stateTag } from "./agents.ts";
import { esc, h, ICON, invoke } from "./core.ts";
import type { Turn } from "./core.ts";

// ---------- Coding agent card in a reply ----------

// A `code_agent` call shows the session it started or checked as a card
// under the reply's steps: its state, plan, files and any question, live
// while it works, with the question's buttons and Open.

/// How often a working session's card asks again.
const CARD_POLL_MS = 3000;
/// How long one read of every session serves the cards that look for theirs.
const SESSIONS_FRESH_MS = 15000;

/// A card's element, which keeps what it shows and where its session is.
interface CoderCard extends HTMLElement {
  report: AgentReport;
  /// The session as the node knows it, once found.
  found?: { row: AgentSession; page: AgentPage | null };
  answering?: number | null;
  timer?: ReturnType<typeof setTimeout>;
}

let sessions: { at: number; list: Promise<AgentSession[]> } | null = null;

/// Every session on the user's machines, read at most every few seconds.
function allSessions() {
  if (!sessions || Date.now() - sessions.at > SESSIONS_FRESH_MS) {
    sessions = { at: Date.now(), list: invoke<AgentSession[]>("agent_sessions").catch((): AgentSession[] => []) };
  }
  return sessions.list;
}

/// Draws or updates the card for the session a `code_agent` result
/// describes, under the steps of `turn`.
export function coderCard(turn: Turn, text: string | null | undefined) {
  const report = parseAgentReport(text);
  if (!report) return;
  const key = cardKey(report);
  let card = [...turn.querySelectorAll<CoderCard>(".dcoder[data-card]")].find(c => c.dataset.card === key);
  if (!card) {
    card = h<CoderCard>(`<div class="dcard dcoder dcodercard"></div>`);
    card.dataset.card = key;
    card.addEventListener("click", e => cardClick(card!, e));
    const reply = turn.querySelector(":scope > .reply");
    reply ? turn.insertBefore(card, reply) : turn.append(card);
  }
  card.report = report;
  draw(card);
  if (!card.found) find(card);
}

async function find(card: CoderCard) {
  const r = card.report;
  const row = (await allSessions()).find(s => s.id === r.id && s.agent === r.agent);
  if (!row || !card.isConnected) return;
  card.found = { row, page: null };
  poll(card);
}

/// Reads the session again, then again while it works and the card shows.
async function poll(card: CoderCard) {
  clearTimeout(card.timer);
  const f = card.found;
  if (!f || !card.isConnected) return;
  try {
    const page = await invoke<AgentPage>("agent_view", { machineId: f.row.machine_id, agent: f.row.agent, session: f.row.id, last: 1 });
    f.page = page;
    if (page.session.question?.id !== card.answering) card.answering = null;
  } catch {
    // Gone from the machine, or the machine went offline: the card keeps
    // what the reply said.
    card.found = undefined;
    draw(card);
    return;
  }
  draw(card);
  if (card.isConnected && AGENT_ACTIVE.has(f.page?.session.status)) card.timer = setTimeout(() => poll(card), CARD_POLL_MS);
}

const mark = (status: string, asking: boolean) => status === "completed" ? `<span class="dok">${ICON.check}</span>`
  : status === "in_progress" ? (asking ? '<span class="ddot" style="background:var(--coral)"></span>' : '<span class="dspin"></span>')
  : '<span class="ddot off"></span>';

function questionHtml(q: AgentQuestion, busy: boolean) {
  const allow = q.options.findIndex(x => x.allow), deny = q.options.findIndex(x => !x.allow);
  const buttons = q.options.map((o, i) => `<button class="dbtn${i === allow ? " pri" : i === deny ? " warn" : ""}" data-cardopt="${i}"${busy ? " disabled" : ""}>${esc(o.name)}</button>`).join("");
  return `<div class="q"><b style="font-weight:600">${esc(q.text)}</b>${q.detail ? `<code style="font-family:var(--mono);font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere">${esc(q.detail)}</code>` : ""}<div class="dups">${buttons}</div></div>`;
}

function draw(card: CoderCard) {
  const r = card.report, f = card.found, page = f?.page, s = page?.session;
  const status = s?.status ?? r.status;
  const asking = status === "asking";
  const plan: ReportStep[] = page ? page.plan : r.plan;
  const files = page ? page.files : r.files;
  const doing = s ? s.doing : r.doing;
  const machine = s?.machine_name ?? f?.row.machine_name;
  const k = agentKind(r.agent);
  const head = `<div class="head" style="background:rgba(59,155,122,.1)">${agentAvatar(r.agent)}<div style="flex:1;min-width:0;display:grid"><b>${esc(k.name)}${machine ? ` on ${esc(machine)}` : ""}</b><small style="font-family:var(--mono);overflow:hidden;text-overflow:ellipsis;white-space:nowrap" title="${esc(r.cwd)}">${esc(r.cwd)}</small></div>${status === "unknown" ? "" : stateTag(status)}</div>`;
  const now = doing && status !== "ended" && status !== "idle"
    ? `<div class="row" style="padding:8px 12px 0;color:var(--muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis" title="${esc(doing)}">${esc(doing.split("\n")[0])}</div>` : "";
  const planHtml = plan.length ? `<div class="sec">Plan</div><div class="plan">${plan.map(x => `<div class="row"><span style="width:16px;display:grid;place-items:center">${mark(x.status, asking)}</span><span style="${x.status === "pending" ? "color:var(--muted)" : ""}">${esc(x.content)}</span></div>`).join("")}</div>` : "";
  const filesHtml = files.length ? `<div class="sec">Files changed</div><div class="files">${files.slice(-8).map(x => `<span><b>${esc(x)}</b></span>`).join("")}${files.length > 8 ? `<span style="color:var(--muted)">and ${files.length - 8} more</span>` : ""}</div>` : "";
  // A live question gets its buttons; one only the reply names is text.
  const q = s?.question ? questionHtml(s.question, card.answering === s.question.id)
    : !f && r.question && asking ? `<div class="q"><b style="font-weight:600">${esc(r.question)}</b></div>` : "";
  const open = f ? `<div class="dups" style="padding:10px 12px 0"><button class="dbtn" data-cardopen>Open</button></div>` : "";
  card.innerHTML = head + now + q + planHtml + filesHtml + open + '<div style="height:12px"></div>';
}

async function answer(card: CoderCard, i: number) {
  const f = card.found, q = f?.page?.session.question, opt = q?.options[i];
  if (!f || !q || !opt || card.answering === q.id) return;
  card.answering = q.id;
  draw(card);
  try {
    await invoke("center_answer_agent", { machineId: f.row.machine_id, agent: f.row.agent, session: f.row.id, question: q.id, reply: opt.id });
  } catch (e) {
    // A conflict means it was answered already, so it stays answered.
    if (!/^conflict/i.test(String(e))) card.answering = null;
    console.warn("answering the agent:", e);
  }
  poll(card);
}

function cardClick(card: CoderCard, e: MouseEvent) {
  const t = e.target as Element;
  const opt = t.closest<HTMLElement>("[data-cardopt]");
  if (opt) { e.stopPropagation(); return void answer(card, Number(opt.dataset.cardopt)); }
  if (t.closest("[data-cardopen]") && card.found) {
    e.stopPropagation();
    openAgent(card.found.page?.session ?? card.found.row);
  }
}
