import { subscribeChanges as subscribeTopics, topic } from "../ui/mesh-events.ts";
import { paneShown } from "../ui/active.ts";
import { type AgentReport, cardKey, parseAgentReport, type ReportStep } from "./agentcard.ts";
import { type AgentPage, type AgentSession, agentAvatar, agentKind, stateTag } from "../agents/agents.ts";
import { loadSessions } from "../agents/sessions.ts";
import { filesHtml, planHtml, questionHtml } from "../agents/coderhtml.ts";
import { openAgent } from "../agents/session.ts";
import { ANSWER } from "./chat.ts";
import { esc } from "../ui/format.ts";
import { h } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";
import type { Turn } from "../ui/turn.ts";

// ---------- Coding agent card in a reply ----------

// A `code_agent` call shows the session it started or checked as a card
// under the reply's steps: its state, plan, files and any question, live
// while it works, with the question's buttons and Open.

/// A card's element, which keeps what it shows and where its session is.
interface CoderCard extends HTMLElement {
  report: AgentReport;
  /// The session as the node knows it, once found.
  found?: { row: AgentSession; page: AgentPage | null };
  answering?: number | null;
  unfollow?: () => void;
}

const followed = new Set<CoderCard>();
let observer: MutationObserver | undefined;

function follow(card: CoderCard, row: AgentSession) {
  card.unfollow?.();
  card.unfollow = subscribeTopics(topic("agent", row.id), () => poll(card), row.machine_id, () => card.isConnected && paneShown());
  followed.add(card);
  if (observer) return;
  observer = new MutationObserver(records => {
    if (!records.some(record => record.removedNodes.length)) return;
    for (const card of followed) {
      if (card.isConnected) continue;
      card.unfollow?.();
      card.unfollow = undefined;
      followed.delete(card);
    }
    if (!followed.size) { observer?.disconnect(); observer = undefined; }
  });
  observer.observe(document, { subtree: true, childList: true });
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
    const reply = turn.querySelector(ANSWER);
    reply ? turn.insertBefore(card, reply) : turn.append(card);
  }
  card.report = report;
  draw(card);
  if (!card.found) find(card);
}

async function find(card: CoderCard) {
  const r = card.report;
  const row = (await loadSessions().catch((): AgentSession[] => [])).find(s => s.id === r.id && s.agent === r.agent);
  if (!row || !card.isConnected) return;
  card.found = { row, page: null };
  follow(card, row);
  void poll(card).catch(e => console.warn("agent card:", e));
}

/// Reads the session after a change while its card shows.
async function poll(card: CoderCard) {

  const f = card.found;
  if (!f || !card.isConnected) return;
  try {
    const page = await invoke<AgentPage>("agent_view", { machineId: f.row.machine_id, agent: f.row.agent, session: f.row.id, last: 1 });
    f.page = page;
    if (page.session.question?.id !== card.answering) card.answering = null;
  } catch (error) {
    // Keep the last view while the active subscription retries the read.
    draw(card);
    throw error;
  }
  draw(card);

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
  const head = `<div class="head">${agentAvatar(r.agent)}<div class="dcol"><b>${esc(k.name)}${machine ? ` on ${esc(machine)}` : ""}</b><small class="dmono dell" title="${esc(r.cwd)}">${esc(r.cwd)}</small></div>${status === "unknown" ? "" : stateTag(status)}</div>`;
  const now = doing && status !== "ended" && status !== "idle"
    ? `<div class="row note dell" title="${esc(doing)}">${esc(doing.split("\n")[0])}</div>` : "";
  // A live question gets its buttons; one only the reply names is text.
  const q = s?.question ? questionHtml(s.question, "data-cardopt", card.answering === s.question.id)
    : !f && r.question && asking ? `<div class="q"><b>${esc(r.question)}</b></div>` : "";
  const open = f ? `<div class="dups"><button class="dbtn" data-cardopen>Open</button></div>` : "";
  card.innerHTML = head + now + q + planHtml(plan, asking) + filesHtml(files, 8) + open;
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
  void poll(card).catch(e => console.warn("agent card:", e));
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
