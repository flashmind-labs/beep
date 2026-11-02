import { loadAgentAccounts } from "./accounts.ts";
import { askFiles } from "./attach.ts";
import { chartRow } from "./charts.ts";
import { approvalCard, body, drawLatest, drawSteps, failed, newTurn, ranOn, renderChat, reply, scrollDown, sendQueued, suggestions, watchRun } from "./chat.ts";
import { $, changed, esc, h, invoke, live, queued, S } from "./core.ts";
import { productRow, replyPic, sentFile } from "./media.ts";
import { replyActs } from "./rerun.ts";
import { routeRow } from "./routes.ts";
import { QUIET_TOOLS, toolLabel } from "./tools.ts";
import { keepWaitingQuestions, loadQuestions } from "./work.ts";

export async function openSession(id) {
  S.session = id;
  loadQuestions(id);
  // A chat still answering keeps its body; anything else comes from the server.
  const running = live.get(id)?.run;
  if (!running) live.delete(id);
  renderChat();
  $("#ask").placeholder = "Ask a follow-up…";
  if (running) {
    scrollDown();
    invoke("open_session", { id }).catch(() => {});
    return;
  }
  try {
    const turns = await invoke("open_session", { id });
    // The user may have moved on, or started a run here, while it loaded.
    if (S.session !== id || S.view !== "chat" || live.has(id)) return;
    drawHistory(id, turns);
    // The window's title line says when the chat started, known only now.
    changed();
    // A reply that finished while another chat was open left its queue waiting.
    if (queued.get(id) && !queued.get(id).held) sendQueued(id);
    else watchRun(id);
  } catch (e) {
    if (S.session === id && S.view === "chat") failed(newTurn(""), String(e));
  }
}

/// Redraws the open chat from the server's copy without rebuilding the bar,
/// so a draft stays and a user reading back is not sent to the bottom.
export async function refreshSession(id) {
  const b = body(), stuck = b?.stuck !== false, top = b?.scrollTop;
  const turns = await invoke("open_session", { id }).catch(() => null);
  if (!turns || S.session !== id || S.view !== "chat" || live.has(id) || body() !== b) return;
  drawHistory(id, turns);
  if (!stuck) { b.scrollTop = top; b.stuck = false; drawLatest(); }
}

export function drawHistory(id, turns) {
  body().innerHTML = "";
  let turn = null, last = null;
  for (const t of turns) {
    if (t.role === "user" || t.role === "scheduled") {
      turn = t.role === "user" ? newTurn(t.text, t.images) : newTurn(t.label || "Routine");
      turn.user = t.role === "user";
      if (turn.user) turn.dataset.at = t.at;
      if (turn.user) ranOn(turn, t.model, t.thinking);
    } else if (t.role === "assistant") {
      turn ??= newTurn("");
      turn.steps.push(...t.tools.filter(x => !QUIET_TOOLS.has(x.name)).map(x => ({ id: "", label: x.label || toolLabel(x.name) || "Using a tool", state: x.success ? "ok" : "bad" })));
      if (t.html) reply(turn).innerHTML += t.html;
      for (const x of t.tools) {
        if (x.file) sentFile(turn, x.file);
        for (const p of x.images ?? []) replyPic(turn, p);
        if (x.products) productRow(turn, x.products);
        if (x.cards) productRow(turn, x.cards);
        if (x.chart) chartRow(turn, x.chart);
        if (x.route) routeRow(turn, x.route);
      }
      last = t.replies.length ? { turn, replies: t.replies } : null;
      turn.done = true;
      if (t.model) {
        turn.model = t.model;
        // A turn Auto did not pick still names the model that answered.
        if (!turn.querySelector(".dranon")) ranOn(turn, t.model, "");
      }
      drawSteps(turn, true);
      replyActs(turn);
    } else if (t.role === "approval") {
      turn ??= newTurn("");
      approvalCard(id, turn, t);
      last = null;
    }
  }
  // Only the latest reply's follow-ups still make sense.
  if (last) suggestions(last.turn, last.replies);
  scrollDown();
  loadAskFiles(id, body());
  loadAgentAccounts();
  loadSources(id, body());
  keepWaitingQuestions(id);
}

/// Puts the files sent in session `id` under their prompts, once the server
/// lists them. The chat may have been left or redrawn meanwhile.
export async function loadAskFiles(id, b) {
  const sent = await invoke("session_files", { id }).catch(() => []);
  if (S.session !== id || body() !== b) return;
  for (const { message, files } of sent) {
    const turn = b.querySelector(`.turn[data-at="${message}"]`);
    if (!turn || turn.querySelector(".dachips")) continue;
    (turn.querySelector(".dpics") ?? turn.querySelector(".ask"))?.after(askFiles(files));
  }
}

/// Puts the web pages each turn of session `id` used under its reply, once
/// the server lists them. A server from before source chips lists none.
export async function loadSources(id, b) {
  const turns = await invoke("session_sources", { id }).catch(() => []);
  if (S.session !== id || body() !== b) return;
  for (const { message, sources } of turns) {
    const turn = b.querySelector(`.turn[data-at="${message}"]`);
    if (turn) sourceRow(turn, sources);
  }
}

/// The site a source is on, without "www.".
export function sourceSite(url) {
  try { return new URL(url).hostname.replace(/^www\./, ""); } catch { return ""; }
}

/// The web pages a reply used, as chips under it: the site's initial and
/// name. Past three the rest fold into "+N", which shows them. A click opens
/// the page in the browser.
export function sourceRow(turn, sources) {
  turn.querySelector(".dsources")?.remove();
  const list = (sources ?? []).map(s => ({ ...s, site: sourceSite(s.url) })).filter(s => s.site);
  if (!list.length) return;
  const row = h('<div class="dsources" aria-label="Sources"></div>');
  const draw = open => {
    const shown = open ? list : list.slice(0, 3), more = list.length - shown.length;
    row.innerHTML = shown.map((s, i) => `<button class="dsrc" data-i="${i}" title="${esc(s.title || s.url)}"><i>${esc(s.site[0].toUpperCase())}</i><span>${esc(s.site)}</span></button>`).join("")
      + (more ? `<button class="dsrc more" aria-label="Show ${more} more sources">+${more}</button>` : "");
    row.querySelectorAll(".dsrc[data-i]").forEach(b => b.addEventListener("click", () => invoke("open_url", { url: shown[+b.dataset.i].url }).catch(() => {})));
    row.querySelector(".dsrc.more")?.addEventListener("click", () => draw(true));
  };
  draw(false);
  const anchor = turn.querySelector(".files") ?? turn.querySelector(".reply");
  anchor ? anchor.after(row) : turn.append(row);
}

/// Names the agent that answered a turn that just finished live: it is the
/// newest one the server lists.
export async function loadTurnAgent(id, turn) {
  const agents = await invoke("session_agents", { id }).catch(() => []);
  const agent = agents.at(-1)?.agent;
  if (!agent || !turn.isConnected || turn !== [...body().querySelectorAll(".turn")].findLast(t => t.user)) return;
  turn.agent = agent;
  replyActs(turn);
}

