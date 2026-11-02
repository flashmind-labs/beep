import { drawAgentButton } from "./agents.ts";
import { askFiles, attached, attachError, drawAttached } from "./attach.ts";
import { chartRow } from "./charts.ts";
import { drawChats, loadChats, touch } from "./chats.ts";
import { $, bar, Channel, entry, esc, h, here, ICON, invoke, live, newId, queued, S, stopping } from "./core.ts";
import { chatHints, drawStop, refreshHints } from "./hints.ts";
import { loadTurnAgent, sourceRow } from "./history.ts";
import { heard, takeDraft } from "./live.ts";
import { fillPics, picRow, productRow, replyPic, sentFile } from "./media.ts";
import { canPickModel, modelName, MODELS } from "./model.ts";
import { replyActs } from "./rerun.ts";
import { routeRow } from "./routes.ts";
import { DESCRIBED, doneLabel, QUIET_TOOLS, toolLabel, VISIBLE_STEPS } from "./tools.ts";
import { refreshStatus } from "./views.ts";

// ---------- Chat ----------

export function renderChat() {
  S.view = "chat";
  bar.innerHTML = `<div class="dinput dgrow"><textarea id="ask" rows="1" placeholder="Ask Beep…" autocomplete="off" spellcheck="false"></textarea><div class="dgrip" id="grip" role="separator" aria-orientation="horizontal" aria-label="Resize the prompt" title="Drag to resize, double-click to reset"></div></div>
    <div class="dbody" id="body"></div>${chatHints()}`;
  const kept = here()?.body;
  if (kept) $("#body").replaceWith(kept);
  const b = body();
  b.onscroll = () => { b.stuck = b.scrollHeight - b.scrollTop - b.clientHeight < 40; drawLatest(); markMap(); };
  drawLatest();
  drawQueue();
  drawAttached();
  const ask = $("#ask");
  let multi = false, typed = false;
  ask.addEventListener("input", () => {
    grow(ask);
    if (multi !== ask.value.includes("\n")) { multi = !multi; refreshHints(); }
    if (typed !== !!ask.value.trim()) { typed = !typed; refreshHints(); }
  });
  dragGrip(ask);
  takeDraft(ask);
  drawStop();
  drawAgentButton();
  focusAsk();
}
export const ASK_HEIGHT = "bar.askHeight", ASK_MAX = 420;
/// A height the user dragged the bar's prompt to; null lets it follow its lines.
export let askHeight = null;
try { askHeight = Number(localStorage.getItem(ASK_HEIGHT)) || null; } catch {}
/// The prompt grows with its lines, up to six, then scrolls. A height the
/// user dragged it to is the least it shows, so its lines still fit.
export function grow(ask) {
  ask.style.height = "auto";
  const lines = Math.min(ask.scrollHeight, 156);
  const dragged = askHeight && ask.id === "ask" ? Math.min(askHeight, ASK_MAX) : 0;
  ask.style.height = Math.max(lines, dragged) + "px";
}
/// Dragging the grip under the prompt sets its height, kept for next time.
export function dragGrip(ask) {
  const grip = $("#grip");
  grip.addEventListener("pointerdown", e => {
    e.preventDefault();
    grip.setPointerCapture(e.pointerId);
    grip.classList.add("on");
    const startY = e.clientY, startH = ask.offsetHeight;
    const move = m => {
      // A click on the grip is not a drag: it would pin the prompt to one line.
      if (Math.abs(m.clientY - startY) < 4) return;
      askHeight = Math.max(26, Math.min(startH + m.clientY - startY, ASK_MAX));
      grow(ask);
    };
    const up = () => {
      grip.classList.remove("on");
      grip.removeEventListener("pointermove", move);
      grip.removeEventListener("pointerup", up);
      try { if (askHeight) localStorage.setItem(ASK_HEIGHT, String(askHeight)); } catch {}
    };
    grip.addEventListener("pointermove", move);
    grip.addEventListener("pointerup", up);
  });
  grip.addEventListener("dblclick", () => {
    askHeight = null;
    try { localStorage.removeItem(ASK_HEIGHT); } catch {}
    grow(ask);
  });
  grow(ask);
}
export function focusAsk() { const ask = $("#ask"); if (ask) ask.focus(); }
export function body() { return $("#body"); }
export function scrollDown() { const b = body(); if (b) { b.stuck = true; b.scrollTop = b.scrollHeight; } }
/// Follows a streaming reply down, unless the user scrolled up to read.
export function keepDown() { const b = body(); if (b?.stuck !== false) scrollDown(); }

/// While the user reads back, a button that jumps to the newest text.
export function drawLatest() {
  const b = body(), want = S.view === "chat" && b?.stuck === false;
  const btn = $("#latest");
  if (!want) return btn?.remove();
  if (btn) return;
  const el = h(`<button class="dbtn dlatest" id="latest"><span class="down">${ICON.chev}</span>Latest</button>`);
  el.addEventListener("click", () => { scrollDown(); drawLatest(); focusAsk(); });
  bar.append(el);
}

/// Past this many prompts the map hides until the chat is hovered.
export const MAP_QUIET_AFTER = 12;

/// A tick per prompt on the right edge once the chat scrolls: hover one to read
/// the prompt, click it to jump there. Ticks are only rebuilt when the prompts
/// change, since fit() calls this from the bar's MutationObserver.
export function drawMap() {
  const b = body();
  const turns = b && S.view === "chat" ? [...b.children].filter(t => t.querySelector(":scope > .ask")) : [];
  let map = $("#map");
  if (turns.length < 2 || b.scrollHeight <= b.clientHeight + 4) return map?.remove();
  if (!map) map = bar.appendChild(h('<div class="dmap" id="map" aria-label="Prompts in this chat"></div>'));
  // A long chat's ticks are a wall of noise: they show only on hover.
  map.classList.toggle("many", turns.length > MAP_QUIET_AFTER);
  if (map.turns?.length !== turns.length || map.turns.some((t, i) => t !== turns[i])) {
    map.turns = turns;
    map.replaceChildren(...turns.map(t => {
      const tk = h('<button class="tk"><i></i><span class="tip"></span></button>');
      const q = t.querySelector(":scope > .ask").textContent;
      tk.querySelector(".tip").textContent = q;
      tk.setAttribute("aria-label", q);
      tk.addEventListener("click", () => { b.scrollTo({ top: t.offsetTop - b.offsetTop - 6, behavior: "smooth" }); focusAsk(); });
      return tk;
    }));
  }
  map.style.top = b.offsetTop + "px";
  map.style.height = b.clientHeight + "px";
  turns.forEach((t, i) => {
    const at = (t.offsetTop - b.offsetTop) / b.scrollHeight * 100;
    map.children[i].style.top = Math.min(97, Math.max(3, at)) + "%";
  });
  markMap();
}
/// Marks the prompt whose turn is at the top of the view.
export function markMap() {
  const b = body(), map = $("#map");
  if (!b || !map?.turns) return;
  let cur = 0;
  map.turns.forEach((t, i) => { if (t.offsetTop - b.offsetTop <= b.scrollTop + b.clientHeight / 3) cur = i; });
  [...map.children].forEach((tk, i) => tk.classList.toggle("on", i === cur));
}

export function newTurn(question, images, files) {
  const turn = h(`<div class="turn">${question ? `<div class="ask">${esc(question)}</div>` : ""}${picRow(images)}</div>`);
  fillPics(turn);
  if (files?.length) turn.append(askFiles(files));
  turn.steps = [];
  turn.folded = true;
  body().append(turn);
  return turn;
}

/// The model and thinking a prompt ran on, under it. Everyone but an admin
/// is on Auto for thinking only, so they see just the level Jev picked. A
/// later line, such as Jev's pick, replaces it.
export function ranOn(turn, model, thinking) {
  if (!model) return;
  const line = canPickModel()
    ? (thinking ? `${modelName(model)} · ${thinking}` : modelName(model))
    : thinking;
  if (!line) return;
  turn.querySelector(".dranon")?.remove();
  turn.querySelector(".ask")?.after(h(`<div class="drerunof dranon">${esc(line)}</div>`));
}

export function stepIcon(state) {
  if (state === "run") return '<span class="dspin"></span>';
  if (state === "bad") return `<span class="dbad">${ICON.x}</span>`;
  if (state === "stop") return `<span style="color:var(--dim);display:grid">${ICON.x}</span>`;
  return `<span class="dok">${ICON.check}</span>`;
}

// Same fold as the phone: while running, the last three steps and a
// "+N tools" row; once done, one "N steps" row that opens them all.
export function drawSteps(turn, done) {
  let box = turn.querySelector(".tools");
  if (!turn.steps.length) { box?.remove(); return; }
  if (!box) {
    box = h('<div class="tools dsteps"></div>');
    const reply = turn.querySelector(".reply, .dcard, .derr");
    reply ? turn.insertBefore(box, reply) : turn.append(box);
  }
  const rows = s => s.map(t => `<div class="dstep${t.state === "run" ? " run" : ""}"><span class="s">${stepIcon(t.state)}</span><span>${esc(t.state === "ok" || t.state === "bad" ? doneLabel(t.label) : t.label)}</span></div>`).join("");
  const n = turn.steps.length;
  if (done) {
    const label = n === 1 ? "1 step" : `${n} steps`;
    box.innerHTML = `<button class="drow" aria-expanded="${!turn.folded}"><span class="chev">${ICON.chev}</span>${label}</button>${turn.folded ? "" : rows(turn.steps)}`;
  } else {
    const hidden = turn.folded ? Math.max(0, n - VISIBLE_STEPS) : 0;
    const more = hidden ? `<button class="drow" aria-expanded="false"><span class="chev">${ICON.chev}</span>+${hidden} tools</button>` : "";
    box.innerHTML = more + rows(turn.steps.slice(hidden));
  }
  box.querySelector(".drow")?.addEventListener("click", () => { turn.folded = !turn.folded; drawSteps(turn, turn.done); });
}

export function reply(turn) {
  let el = turn.querySelector(".reply");
  if (!el) {
    el = h('<div class="reply"></div>');
    const files = turn.querySelector(".files");
    files ? turn.insertBefore(el, files) : turn.append(el);
  }
  return el;
}

export function thinking(turn, delta) {
  let row = turn.querySelector(".thinkrow");
  if (!row) {
    row = h(`<div class="thinkrow" style="display:grid;gap:6px"><button class="drow" aria-expanded="false"><span class="chev">${ICON.chev}</span>Thinking</button><div class="think" hidden></div></div>`);
    const btn = row.querySelector("button"), text = row.querySelector(".think");
    btn.addEventListener("click", () => { text.hidden = !text.hidden; btn.setAttribute("aria-expanded", String(!text.hidden)); });
    const ask = turn.querySelector(".dachips") ?? turn.querySelector(".dpics") ?? turn.querySelector(".ask");
    ask ? ask.after(row) : turn.prepend(row);
  }
  row.querySelector(".think").textContent += delta;
}

/// A routine asking for a grant (`request_grant`). A grant always ends, so
/// its card has no Always, and the Always key allows it.
export const forRoutine = action => /for a routine$/.test(action ?? "");

export function approvalCard(id, turn, { query_id, app, action, fields }) {
  const grant = forRoutine(action);
  const card = h(`<div class="dcard">
    <div class="head"><span class="dapp">${esc((app || "?")[0].toUpperCase())}</span><div style="flex:1;min-width:0;display:grid"><b>${esc(action)}?</b><small>${esc(app)} asks first</small></div></div>
    ${fields.length ? `<div class="kv">${fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-d="allow">Allow<kbd>⌘${ICON.ret}</kbd></button>${grant ? "" : `<button class="dbtn" data-d="session" title="Allow this action without asking until this chat ends">Allow for this session<kbd>⇧⌘${ICON.ret}</kbd></button><button class="dbtn" data-d="always">Always<kbd>⌥⌘${ICON.ret}</kbd></button>`}<button class="dbtn warn" data-d="deny">Deny<kbd>⌘D</kbd></button></div>
  </div>`);
  card.querySelectorAll("[data-d]").forEach(b => b.addEventListener("click", () => answer(b.dataset.d)));
  turn.append(card);
  const e = entry(id);
  e.approval = { queryId: query_id, turn, card, grant };
  e.body ??= turn.parentElement;
  if (id === S.session) refreshHints();
}

/// Follow-ups under the reply; picking one sends it.
export function suggestions(turn, replies) {
  turn.querySelector(".dreplies")?.remove();
  const row = h(`<div class="dreplies">${replies.map(r => `<button class="dbtn">${esc(r)}</button>`).join("")}</div>`);
  row.querySelectorAll("button").forEach((b, i) => b.addEventListener("click", () => send(replies[i])));
  turn.append(row);
}

export function failed(turn, message) {
  turn.failed = true;
  turn.append(h(`<div class="derr">${esc(message)}</div>`));
  if (/removed|not linked/i.test(message)) refreshStatus();
}

export function finish(id, turn) {
  touch(id);
  turn.done = true;
  turn.querySelector(".caret")?.remove();
  for (const s of turn.steps) if (s.state === "run") s.state = "stop";
  turn.folded = true;
  drawSteps(turn, true);
  replyActs(turn);
  if (turn.user) loadTurnAgent(id, turn);
  const e = live.get(id);
  if (e) {
    e.run = null;
    if (!e.approval) live.delete(id);
  }
  if (id === S.session) refreshHints();
  else if (S.view === "chats") S.chats.mode === "list" ? loadChats(S.chats.query) : drawChats();
  const q = queued.get(id);
  if (q) {
    q.held ||= !!(turn.stopped || turn.failed || (turn.status && !["completed", "waiting"].includes(turn.status)));
    // An approval still waiting keeps the queue until it is answered.
    if (!q.held && !live.get(id)?.approval && id === S.session && S.view === "chat") setTimeout(() => sendQueued(id));
    else if (id === S.session) { drawQueue(); refreshHints(); }
  }
}

/// Holds a prompt typed while the open chat's reply runs.
export function enqueue(text) {
  const id = S.session;
  if (!id) return;
  const q = queued.get(id) ?? { list: [], held: false };
  q.list.push(text);
  queued.set(id, q);
  const ask = $("#ask");
  ask.value = "";
  grow(ask);
  drawQueue();
  refreshHints();
}

/// Sends the chat's first queued prompt; the rest follow as each reply finishes.
export function sendQueued(id) {
  const q = queued.get(id);
  if (!q?.list.length || id !== S.session || S.view !== "chat" || live.get(id)?.run || live.get(id)?.approval) return;
  const text = q.list.shift();
  q.held = false;
  if (!q.list.length) queued.delete(id);
  drawQueue();
  send(text);
}

/// The open chat's queued prompts, between the reply and the hints. x puts
/// one back in the field.
export function drawQueue() {
  const old = $("#queue"), q = queued.get(S.session);
  if (S.view !== "chat" || !q?.list.length) return old?.remove();
  const el = h(`<div class="dqueue" id="queue"><div class="dqh"><span>${q.held ? "Not sent" : "Queued, sends after this reply"}</span>${q.held ? `<button class="dbtn">Send<kbd>⌘${ICON.ret}</kbd></button>` : ""}</div>`
    + q.list.map((t, i) => `<div class="dq"><span class="n">${i + 1}</span><span class="t">${esc(t)}</span><button class="dcopy" aria-label="Edit queued prompt ${i + 1}">${ICON.x}</button></div>`).join("") + `</div>`);
  el.querySelector(".dqh .dbtn")?.addEventListener("click", () => sendQueued(S.session));
  el.querySelectorAll(".dq .dcopy").forEach((b, i) => b.addEventListener("click", () => {
    const [text] = q.list.splice(i, 1);
    if (!q.list.length) queued.delete(S.session);
    const ask = $("#ask");
    ask.value = [text, ask.value].filter(s => s.trim()).join("\n\n");
    grow(ask);
    drawQueue();
    refreshHints();
    focusAsk();
  }));
  if (old) old.replaceWith(el);
  else $(".dhints").before(el);
}

/// A channel that draws one run's events into `turn`, which stays in its
/// chat's body while the bar shows another chat.
export function follow(id, turn) {
  const channel = new Channel();
  let text = "";
  channel.onmessage = ev => {
    // A stopped turn only needs its run's id, to stop it.
    if (turn.stopped) return ev.kind === "started" && turn.started?.(ev.run_id);
    switch (ev.kind) {
      case "started":
        entry(id).run = { id: ev.run_id, turn };
        if (id === S.session) refreshHints();
        break;
      case "text":
        text = ev.html;
        reply(turn).innerHTML = text + '<span class="caret"></span>';
        break;
      case "reasoning":
        thinking(turn, ev.delta);
        break;
      case "tool_started":
        if (QUIET_TOOLS.has(ev.name)) break;
        turn.steps.push({ id: ev.call_id, label: (DESCRIBED.has(ev.name) && ev.description) || toolLabel(ev.name) || ev.description || "Using a tool", state: "run" });
        drawSteps(turn, false);
        break;
      case "tool_result": {
        const step = turn.steps.find(s => s.id === ev.call_id);
        if (step) step.state = ev.success ? "ok" : "bad";
        drawSteps(turn, false);
        if (ev.file) sentFile(turn, ev.file);
        for (const p of ev.images ?? []) replyPic(turn, p);
        if (ev.products) productRow(turn, ev.products);
        if (ev.cards) productRow(turn, ev.cards);
        if (ev.chart) chartRow(turn, ev.chart);
        if (ev.name === "get_route" && ev.success && ev.call_id) routeRow(turn, ev.call_id);
        break;
      }
      case "sources":
        sourceRow(turn, ev.sources);
        break;
      case "replies":
        suggestions(turn, ev.replies);
        break;
      case "transcript":
        heard(turn, ev.text);
        break;
      case "auto_picked":
        turn.model = ev.model;
        ranOn(turn, ev.model, ev.thinking);
        break;
      case "approval":
        turn.querySelector(".caret")?.remove();
        approvalCard(id, turn, ev);
        break;
      case "error":
        failed(turn, ev.message);
        break;
      case "finished":
        turn.status = ev.status;
        if (ev.status === "cancelled") turn.append(h('<div class="dmuted">Stopped</div>'));
        finish(id, turn);
        break;
    }
    if (turn.isConnected) keepDown();
  };
  return channel;
}

export async function send(message) {
  const images = attached.pics.slice(), files = attached.files.slice();
  const n = images.length + files.length;
  if (!message.trim() && !n) return;
  const busy = here();
  if (n && (busy?.run || busy?.approval)) return attachError(`${files.length ? "Files" : "Images"} send once this reply finishes`);
  if (busy?.run || busy?.approval) return enqueue(message.trim());
  message = message.trim() || (n > 1 ? "What are these?" : "What is this?");
  attached.pics = [];
  attached.files = [];
  drawAttached();
  const id = S.session ??= newId();
  const ask = $("#ask");
  body().querySelectorAll(".dreplies").forEach(r => r.remove());
  ask.value = "";
  ask.placeholder = "Ask a follow-up…";
  grow(ask);
  const turn = newTurn(message, images.map(url => ({ url })), files);
  turn.user = true;
  // Without Auto every prompt runs on the chosen model; with it, Jev's pick
  // arrives as an event.
  const m = MODELS.info;
  if (m && !m.auto && canPickModel()) ranOn(turn, m.selected, m.thinking);
  reply(turn).innerHTML = '<span class="caret"></span>';
  const e = entry(id);
  e.body = body();
  e.run = { id: null, turn };
  touch(id, message);
  refreshHints();
  scrollDown();
  await stopping.get(id);
  try {
    const uploads = files.map(({ name, mime, data }) => ({ name, mime, data }));
    await invoke("send_prompt", { session: id, message, images, files: uploads, onEvent: follow(id, turn) });
  } catch (err) {
    if (!turn.stopped) failed(turn, String(err));
  }
  turn.started?.(null);
  if (!turn.done) finish(id, turn);
}

// Chats whose run, started on another device, the bar is looking up or following.
export const watching = new Set();

/// Follows the open chat's run when another device started it, drawing it
/// as it streams like one sent from here.
export async function watchRun(id) {
  if (!id || live.has(id) || watching.has(id)) return;
  watching.add(id);
  try {
    const runId = await invoke("active_run", { session: id }).catch(() => null);
    if (!runId || live.has(id) || S.session !== id || S.view !== "chat") return;
    // The prompt is already drawn from the server's copy, waiting for its reply.
    const last = [...body().querySelectorAll(".turn")].at(-1);
    const turn = last?.user && !last.done ? last : newTurn("");
    turn.steps ??= [];
    reply(turn).innerHTML = '<span class="caret"></span>';
    const e = entry(id);
    e.body = body();
    e.run = { id: runId, turn };
    refreshHints();
    scrollDown();
    try {
      await invoke("follow_run", { runId, onEvent: follow(id, turn) });
    } catch (err) {
      if (!turn.stopped) failed(turn, String(err));
    }
    if (!turn.done) finish(id, turn);
  } finally {
    watching.delete(id);
  }
}

/// Takes chat `id`'s approval card away, saying how it was answered, and
/// readies its turn for the run the answer resumes.
export function settle(id, e, pending, decision) {
  e.approval = null;
  const said = pending.grant
    ? { allow: "Allowed for this routine", deny: "Denied" }[decision]
    : { allow: "Allowed once", session: "Allowed for the rest of this chat", always: "Allowed, and always allowed from now on", deny: "Denied" }[decision];
  pending.card.replaceWith(h(`<div class="dmuted">${said}</div>`));
  const turn = pending.turn;
  turn.done = false;
  e.run = { id: null, turn };
  if (id === S.session) refreshHints();
  return turn;
}

export async function answer(decision) {
  const id = S.session, e = live.get(id), pending = e?.approval;
  if (!pending) return;
  if (pending.grant && (decision === "always" || decision === "session")) decision = "allow";
  const turn = settle(id, e, pending, decision);
  await stopping.get(id);
  try {
    await invoke("answer_approval", { session: id, queryId: pending.queryId, decision, onEvent: follow(id, turn) });
  } catch (err) {
    if (!turn.stopped) failed(turn, String(err));
  }
  turn.started?.(null);
  if (!turn.done) finish(id, turn);
}

/// Follows run `runId`, resumed by an answer given in the orb, into `turn`.
export async function followAnswered(id, turn, runId) {
  try {
    await invoke("follow_run", { runId, onEvent: follow(id, turn) });
  } catch (err) {
    if (!turn.stopped) failed(turn, String(err));
  }
  turn.started?.(null);
  if (!turn.done) finish(id, turn);
}

/// Stops the run at once: the chat lets go of it and takes the next prompt
/// while the server is told to stop it.
export function stop() {
  const id = S.session, run = here()?.run;
  if (!run) return;
  const turn = run.turn;
  turn.stopped = true;
  // A run not started yet is stopped once it says its id.
  const runId = run.id ? Promise.resolve(run.id) : new Promise(resolve => { turn.started = resolve; });
  const stopped = runId
    .then(runId => runId && invoke("cancel_run", { runId }))
    .catch(() => {})
    .finally(() => { if (stopping.get(id) === stopped) stopping.delete(id); });
  stopping.set(id, stopped);
  turn.append(h('<div class="dmuted">Stopped</div>'));
  finish(id, turn);
}

export async function newChat() {
  S.session = null;
  renderChat();
  await invoke("new_chat");
}

/// Hands a view to the command center window, which hides the bar.
export function toWindow(target) {
  return invoke("center_open", { target }).catch(() => {});
}
/// Opens chat `id` (null for a new one) in the window.
export const openInWindow = id => toWindow({ chat: { session: id ?? null } });

