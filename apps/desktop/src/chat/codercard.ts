import { createElement } from "react";
import { renderChatCoderCards } from "./work-view.tsx";
import { CoderCardView, type CoderCardState } from "./codercard-view.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { paneShown } from "../ui/active.ts";
import { type AgentReport, cardKey, parseAgentReport } from "./agentcard.ts";
import { type AgentPage, type AgentSession } from "../agents/agents.ts";
import { loadSessions } from "../agents/sessions.ts";
import { openAgent } from "../agents/session.ts";
import { ANSWER, owningChatBody } from "./chat.ts";
import { h } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";
import type { ChatBody, Turn } from "../ui/turn.ts";

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
  readVersion?: number;
  unfollow?: () => void;
  owner?: CardOwner;
}

interface CardOwner { body: ChatBody; observer: MutationObserver | null; cards: Map<CoderCard, CoderCardState>; pending: Map<CoderCard, CoderCardState>; watching: boolean }
const owners = new WeakMap<ChatBody, CardOwner>();
function sync(owner: CardOwner) {
  renderChatCoderCards(owner.body, [...owner.cards].map(([card, value]) => ({ host: card, view: createElement(CoderCardView, { value, answer: index => void answer(card, index), open: () => { if (card.found) openAgent(card.found.page?.session ?? card.found.row); } }) })));
}
function selected(card: CoderCard) { const selection = document.getSelection(); return !!selection && !selection.isCollapsed && selection.containsNode(card, true); }
function remove(card: CoderCard) {
  card.unfollow?.(); card.unfollow = undefined;
  card.readVersion = (card.readVersion ?? 0) + 1;
  const owner = card.owner;
  if (!owner) return;
  owner.cards.delete(card); owner.pending.delete(card); card.owner = undefined;
  if (owner.watching && !owner.pending.size) { document.removeEventListener("selectionchange", ownerChanged(owner)); owner.watching = false; }
}
const selectionCallbacks = new WeakMap<CardOwner, () => void>();
function ownerChanged(owner: CardOwner) {
  let callback = selectionCallbacks.get(owner);
  if (!callback) {
    callback = () => {
      let changed = false;
      for (const [card, value] of owner.pending) {
        if (selected(card)) continue;
        owner.pending.delete(card);
        const entry = owner.cards.get(card);
        if (entry) { owner.cards.set(card, value); changed = true; }
      }
      if (!owner.pending.size) { document.removeEventListener("selectionchange", ownerChanged(owner)); owner.watching = false; }
      if (changed) sync(owner);
    };
    selectionCallbacks.set(owner, callback);
  }
  return callback;
}
function prune(owner: CardOwner) {
  if (!owner.body.isConnected) return;
  let changed = false;
  for (const card of owner.cards.keys()) if (!owner.body.contains(card)) { remove(card); changed = true; }
  if (changed) sync(owner);
}
// A card stays subscribed while its chat is detached, idle until it shows
// again; its chat's observer or `disposeCoderCards` lets it go.
function follow(card: CoderCard, row: AgentSession) {
  card.unfollow?.();
  card.unfollow = subscribeChanges(topic("agent", row.id), { deliver: "latest", receive: () => poll(card) }, { machine: row.machine_id, when: () => card.isConnected && paneShown() });
}

/// Releases subscriptions and selection listeners when a chat is evicted.
export function disposeCoderCards(body: ChatBody) {
  const owner = owners.get(body);
  if (!owner) return;
  owners.delete(body);
  owner.observer?.disconnect();
  for (const card of owner.cards.keys()) remove(card);
  owner.cards.clear(); owner.pending.clear();
  if (owner.watching) { document.removeEventListener("selectionchange", ownerChanged(owner)); owner.watching = false; }
}

/// A live chat keeps its body while another page shows. Follow its cards again
/// after that body returns, and read what changed while it was away.
export function resumeCoderCards(body: ChatBody) {
  const owner = owners.get(body);
  if (!owner) return;
  for (const card of owner.cards.keys()) {
    if (!card.isConnected || !card.found) continue;
    if (!card.unfollow) follow(card, card.found.row);
    void poll(card).catch(error => console.warn("agent card:", error));
  }
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
    const body = owningChatBody(turn);
    if (!body) return;
    let owner = owners.get(body);
    if (!owner) {
      owner = { body, observer: null, cards: new Map(), pending: new Map(), watching: false };
      owners.set(body, owner);
      const current = owner;
      const watch = new MutationObserver(records => { if (records.some(record => record.removedNodes.length)) queueMicrotask(() => prune(current)); });
      watch.observe(body, { childList: true, subtree: true });
      owner.observer = watch;
    }
    card.owner = owner;
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
  const found = card.found;
  if (!found || !card.isConnected) return;
  const version = card.readVersion = (card.readVersion ?? 0) + 1;

  try {
    const page = await invoke<AgentPage>("agent_view", { machineId: found.row.machine_id, agent: found.row.agent, session: found.row.id, last: 1 });
    if (card.readVersion !== version || card.found !== found || !card.isConnected) return;
    found.page = page;
    if (page.session.question?.id !== card.answering) card.answering = null;
  } catch (error) {
    if (card.readVersion !== version || card.found !== found || !card.isConnected) return;
    // Keep the last view while the active subscription retries the read.
    draw(card);
    throw error;
  }

  draw(card);
}

function draw(card: CoderCard) {
  const owner = card.owner;
  if (!owner) return;
  const old = owner.cards.get(card);
  const value: CoderCardState = { report: card.report, row: card.found?.row ?? null, page: card.found?.page ?? null, answering: card.answering ?? null };
  if (old && selected(card)) {
    owner.pending.set(card, value);
    if (!owner.watching) { owner.watching = true; document.addEventListener("selectionchange", ownerChanged(owner)); }
  } else {
    owner.pending.delete(card);
    if (owner.watching && !owner.pending.size) { document.removeEventListener("selectionchange", ownerChanged(owner)); owner.watching = false; }
    owner.cards.set(card, value);
    sync(owner);
  }
  queueMicrotask(() => prune(owner));
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
