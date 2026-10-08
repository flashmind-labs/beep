// The bar's page. Rust does the networking and Markdown; this draws it.
"use strict";

const { invoke, Channel } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const ICON = {
  check: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>',
  x: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
  chev: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg>',
  ret: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M20 5v7a3 3 0 0 1-3 3H5m4-4l-4 4 4 4"/></svg>',
  pin: '<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg>',
  phone: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18h2"/></svg>',
};
/// Tool steps shown while a run works; older ones fold into "+N tools".
const VISIBLE_STEPS = 3;

/// What a tool is doing, in words, the same as the phone's toolLabel.
/// Unknown tools fall back to their name: "GMAIL_SEND_EMAIL" becomes "Gmail: send email".
const TOOL_LABELS = {
  web_search: "Searching the web",
  decision: "Weighing the options",
  get_location: "Checking your location",
  get_time: "Checking the time",
  open_app: "Opening an app",
  set_alarm: "Setting an alarm",
  open_browser: "Opening a page",
  browser: "Browsing",
  mcp_search: "Finding the right app tool",
  mcp: "Using a connected app",
  open_maps: "Looking up a place",
  query_contacts: "Looking up contacts",
  suggest_replies: "Suggesting replies",
  task_progress: "Updating progress",
  memory_store: "Remembering",
  memory_recall: "Recalling memories",
  memory_update: "Updating a memory",
  memory_forget: "Forgetting a memory",
  cron_create: "Setting a reminder",
  schedule_once: "Setting a reminder",
  cron_edit: "Changing a reminder",
  cron_list: "Checking reminders",
  cron_delete: "Deleting a reminder",
  cron_history: "Checking past reminders",
  habit_create: "Creating a habit",
  habit_log: "Logging a habit",
  habit_status: "Checking habits",
  habit_delete: "Deleting a habit",
};
const cap = s => s.charAt(0).toUpperCase() + s.slice(1);
function toolLabel(name) {
  if (TOOL_LABELS[name]) return TOOL_LABELS[name];
  const words = String(name ?? "").split(/[_.-]/).filter(w => w.trim()).map(w => w.toLowerCase());
  if (!words.length) return "";
  // Connected app tools are named APP_ACTION.
  if (name === name.toUpperCase() && words.length > 1) return cap(words[0]) + ": " + words.slice(1).join(" ");
  return cap(words.join(" "));
}

const bar = document.getElementById("bar");
const S = {
  view: "chat", // chat | chats | link | revoked
  link: "unlinked",
  session: null,
  chats: { list: [], sel: 0, query: "" },
  cache: [], // the last chat list, drawn at once while a fresh one loads
  offer: null,
  timer: null,
};

const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const h = html => { const t = document.createElement("template"); t.innerHTML = html.trim(); return t.content.firstElementChild; };
const $ = sel => bar.querySelector(sel);

// Chats with a run streaming or an approval waiting, by session id. Each keeps
// its own body, so the bar can show other chats and come back to it.
const live = new Map(); // id -> { run: { id } | null, approval: { queryId, turn, card } | null, body }
const here = () => live.get(S.session);
function entry(id) {
  if (!live.has(id)) live.set(id, { run: null, approval: null, body: null });
  return live.get(id);
}
// Made here so a new chat's run can be tracked before the server answers.
function newId() {
  const b = crypto.getRandomValues(new Uint8Array(16));
  b[6] = b[6] & 0x0f | 0x40;
  b[8] = b[8] & 0x3f | 0x80;
  const x = [...b].map(n => n.toString(16).padStart(2, "0")).join("");
  return `${x.slice(0, 8)}-${x.slice(8, 12)}-${x.slice(12, 16)}-${x.slice(16, 20)}-${x.slice(20)}`;
}

// ---------- Size ----------

function fit() {
  let height = 2;
  for (const el of bar.children) {
    height += el.classList.contains("dbody") || el.classList.contains("dlist") ? el.scrollHeight : el.offsetHeight;
  }
  invoke("resize_bar", { height }).catch(() => {});
}
const sizer = new ResizeObserver(fit);
new MutationObserver(() => {
  sizer.disconnect();
  for (const el of bar.querySelectorAll(":scope > *, .dbody > *")) sizer.observe(el);
  fit();
}).observe(bar, { childList: true, subtree: true, characterData: true });

// ---------- Hints ----------

function hints(items) {
  const st = { linked: ["", "Linked"], unlinked: ["off", "Not linked"], revoked: ["bad", "Removed"] }[S.link];
  return `<div class="dhints">${items.map(([k, t]) => `<span><kbd>${k}</kbd>${t}</span>`).join("")}<span class="st"><span class="ddot ${st[0]}"></span>${st[1]}</span></div>`;
}
function chatHints() {
  const e = here();
  if (e?.approval) return hints([[`⌘${ICON.ret}`, "allow"], ["esc", "to hide"]]);
  if (e?.run) return hints([["⌘.", "stop"], ["⌘N", "new chat"], ["⌘K", "chats"], ["esc", "to hide"]]);
  if ($("#ask")?.value.includes("\n")) return hints([[ICON.ret, "send"], [`⇧${ICON.ret}`, "new line"], ["⌘K", "chats"], ["esc", "to hide"]]);
  return hints([["⌘N", "new chat"], ["⌘K", "chats"], ["esc", "to hide"], ["⌘Q", "quit"]]);
}
function refreshHints() {
  const old = $(".dhints");
  if (old && S.view === "chat") old.replaceWith(h(chatHints()));
}

// ---------- Chat ----------

function renderChat() {
  S.view = "chat";
  bar.innerHTML = `<div class="dinput"><span class="dlogo">b</span><textarea id="ask" rows="1" placeholder="Ask Beep…" autocomplete="off" spellcheck="false"></textarea></div>
    <div class="dbody" id="body"></div>${chatHints()}`;
  const kept = here()?.body;
  if (kept) $("#body").replaceWith(kept);
  const ask = $("#ask");
  let multi = false;
  ask.addEventListener("input", () => {
    grow(ask);
    if (multi !== ask.value.includes("\n")) { multi = !multi; refreshHints(); }
  });
  focusAsk();
}
/// The prompt grows with its lines, up to six, then scrolls.
function grow(ask) {
  ask.style.height = "auto";
  ask.style.height = Math.min(ask.scrollHeight, 156) + "px";
}
function focusAsk() { const ask = $("#ask"); if (ask) ask.focus(); }
function body() { return $("#body"); }
function scrollDown() { const b = body(); if (b) b.scrollTop = b.scrollHeight; }

function newTurn(question) {
  const turn = h(`<div class="turn">${question ? `<div class="ask">${esc(question)}</div>` : ""}</div>`);
  turn.steps = [];
  turn.folded = true;
  body().append(turn);
  return turn;
}

function stepIcon(state) {
  if (state === "run") return '<span class="dspin"></span>';
  if (state === "bad") return `<span class="dbad">${ICON.x}</span>`;
  if (state === "stop") return `<span style="color:var(--dim);display:grid">${ICON.x}</span>`;
  return `<span class="dok">${ICON.check}</span>`;
}

// Same fold as the phone: while running, the last three steps and a
// "+N tools" row; once done, one "N steps" row that opens them all.
function drawSteps(turn, done) {
  let box = turn.querySelector(".tools");
  if (!turn.steps.length) { box?.remove(); return; }
  if (!box) {
    box = h('<div class="tools dsteps"></div>');
    const reply = turn.querySelector(".reply, .dcard, .derr");
    reply ? turn.insertBefore(box, reply) : turn.append(box);
  }
  const rows = s => s.map(t => `<div class="dstep${t.state === "run" ? " run" : ""}"><span class="s">${stepIcon(t.state)}</span><span>${esc(t.label)}</span></div>`).join("");
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

function reply(turn) {
  let el = turn.querySelector(".reply");
  if (!el) { el = h('<div class="reply"></div>'); turn.append(el); }
  return el;
}

function thinking(turn, delta) {
  let row = turn.querySelector(".thinkrow");
  if (!row) {
    row = h(`<div class="thinkrow" style="display:grid;gap:6px"><button class="drow" aria-expanded="false"><span class="chev">${ICON.chev}</span>Thinking</button><div class="think" hidden></div></div>`);
    const btn = row.querySelector("button"), text = row.querySelector(".think");
    btn.addEventListener("click", () => { text.hidden = !text.hidden; btn.setAttribute("aria-expanded", String(!text.hidden)); });
    const ask = turn.querySelector(".ask");
    ask ? ask.after(row) : turn.prepend(row);
  }
  row.querySelector(".think").textContent += delta;
}

function approvalCard(id, turn, { query_id, app, action, fields }) {
  const card = h(`<div class="dcard">
    <div class="head"><span class="dapp">${esc((app || "?")[0].toUpperCase())}</span><div style="flex:1;min-width:0;display:grid"><b>${esc(action)}?</b><small>${esc(app)} asks first</small></div></div>
    ${fields.length ? `<div class="kv">${fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-d="allow">Allow<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn" data-d="always">Always</button><button class="dbtn warn" data-d="deny">Deny</button></div>
  </div>`);
  card.querySelectorAll("[data-d]").forEach(b => b.addEventListener("click", () => answer(b.dataset.d)));
  turn.append(card);
  const e = entry(id);
  e.approval = { queryId: query_id, turn, card };
  e.body ??= turn.parentElement;
  if (id === S.session) refreshHints();
}

/// Follow-ups under the reply; picking one sends it.
function suggestions(turn, replies) {
  turn.querySelector(".dreplies")?.remove();
  const row = h(`<div class="dreplies">${replies.map(r => `<button class="dbtn">${esc(r)}</button>`).join("")}</div>`);
  row.querySelectorAll("button").forEach((b, i) => b.addEventListener("click", () => send(replies[i])));
  turn.append(row);
}

function failed(turn, message) {
  turn.append(h(`<div class="derr">${esc(message)}</div>`));
  if (/removed|not linked/i.test(message)) refreshStatus();
}

function finish(id, turn) {
  turn.done = true;
  turn.querySelector(".caret")?.remove();
  for (const s of turn.steps) if (s.state === "run") s.state = "stop";
  turn.folded = true;
  drawSteps(turn, true);
  const e = live.get(id);
  if (e) {
    e.run = null;
    if (!e.approval) live.delete(id);
  }
  if (id === S.session) refreshHints();
  else if (S.view === "chats") drawChats();
}

/// A channel that draws one run's events into `turn`, which stays in its
/// chat's body while the bar shows another chat.
function follow(id, turn) {
  const channel = new Channel();
  let text = "";
  channel.onmessage = ev => {
    switch (ev.kind) {
      case "started":
        entry(id).run = { id: ev.run_id };
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
        turn.steps.push({ id: ev.call_id, label: toolLabel(ev.name) || ev.description || "Using a tool", state: "run" });
        drawSteps(turn, false);
        break;
      case "tool_result": {
        const step = turn.steps.find(s => s.id === ev.call_id);
        if (step) step.state = ev.success ? "ok" : "bad";
        drawSteps(turn, false);
        break;
      }
      case "replies":
        suggestions(turn, ev.replies);
        break;
      case "approval":
        turn.querySelector(".caret")?.remove();
        approvalCard(id, turn, ev);
        break;
      case "error":
        failed(turn, ev.message);
        break;
      case "finished":
        if (ev.status === "cancelled") turn.append(h('<div class="dmuted">Stopped</div>'));
        finish(id, turn);
        break;
    }
    if (turn.isConnected) scrollDown();
  };
  return channel;
}

async function send(message) {
  const busy = here();
  if (busy?.run || busy?.approval || !message.trim()) return;
  const id = S.session ??= newId();
  const ask = $("#ask");
  body().querySelectorAll(".dreplies").forEach(r => r.remove());
  ask.value = "";
  ask.placeholder = "Ask a follow-up…";
  grow(ask);
  const turn = newTurn(message);
  reply(turn).innerHTML = '<span class="caret"></span>';
  const e = entry(id);
  e.body = body();
  e.run = { id: null };
  refreshHints();
  scrollDown();
  try {
    await invoke("send_prompt", { session: id, message, onEvent: follow(id, turn) });
  } catch (err) {
    failed(turn, String(err));
  }
  if (!turn.done) finish(id, turn);
}

async function answer(decision) {
  const id = S.session, e = live.get(id), pending = e?.approval;
  if (!pending) return;
  e.approval = null;
  const said = { allow: "Allowed once", always: "Allowed, and always allowed from now on", deny: "Denied" }[decision];
  pending.card.replaceWith(h(`<div class="dmuted">${said}</div>`));
  const turn = pending.turn;
  turn.done = false;
  e.run = { id: null };
  refreshHints();
  try {
    await invoke("answer_approval", { session: id, queryId: pending.queryId, decision, onEvent: follow(id, turn) });
  } catch (err) {
    failed(turn, String(err));
  }
  if (!turn.done) finish(id, turn);
}

async function stop() {
  const run = here()?.run;
  if (run?.id) await invoke("cancel_run", { runId: run.id }).catch(() => {});
}

async function newChat() {
  S.session = null;
  renderChat();
  await invoke("new_chat");
}

async function openSession(id) {
  S.session = id;
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
  } catch (e) {
    if (S.session === id && S.view === "chat") failed(newTurn(""), String(e));
  }
}

function drawHistory(id, turns) {
  body().innerHTML = "";
  let turn = null, last = null;
  for (const t of turns) {
    if (t.role === "user" || t.role === "scheduled") {
      turn = newTurn(t.role === "user" ? t.text : t.label || "Routine");
    } else if (t.role === "assistant") {
      turn ??= newTurn("");
      turn.steps.push(...t.tools.map(x => ({ id: "", label: toolLabel(x.name) || "Using a tool", state: x.success ? "ok" : "bad" })));
      if (t.html) reply(turn).innerHTML += t.html;
      last = t.replies.length ? { turn, replies: t.replies } : null;
      turn.done = true;
      drawSteps(turn, true);
    } else if (t.role === "approval") {
      turn ??= newTurn("");
      approvalCard(id, turn, t);
      last = null;
    }
  }
  // Only the latest reply's follow-ups still make sense.
  if (last) suggestions(last.turn, last.replies);
  scrollDown();
}

// ---------- Chats ----------

function when(ms) {
  const d = new Date(ms), now = new Date();
  const mins = Math.round((now - d) / 60000);
  if (mins < 1) return "now";
  if (mins < 60) return `${mins} min ago`;
  if (mins < 24 * 60 && d.getDate() === now.getDate()) return `${Math.round(mins / 60)} h ago`;
  if (mins < 48 * 60) return "Yesterday";
  return d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

function renderChats() {
  S.view = "chats";
  S.chats = { list: S.cache, sel: 0, query: "" };
  bar.innerHTML = `<div class="dinput"><span class="dlogo">b</span><input id="find" placeholder="Search chats" autocomplete="off" spellcheck="false"><kbd>⌘K</kbd></div>
    <div class="dsec" id="sec">Recent</div><div class="dlist" id="list" role="listbox" aria-label="Chats"></div>
    ${hints([[ICON.ret, "open"], ["⌘N", "new chat"], ["esc", "back"]])}`;
  const find = $("#find");
  find.focus();
  let pending = 0;
  find.addEventListener("input", () => {
    const q = find.value.trim(), mine = ++pending;
    setTimeout(() => { if (mine === pending) loadChats(q); }, 180);
  });
  drawChats();
  loadChats("");
}

async function loadChats(query) {
  try {
    const rows = query
      ? (await invoke("search_sessions", { query })).map(m => ({ ...m.session, snippet: m.snippet }))
      : await invoke("list_sessions");
    if (!query) S.cache = rows.slice(0, 50);
    if (S.view !== "chats") return;
    const sel = query === S.chats.query ? Math.min(S.chats.sel, Math.max(0, rows.length - 1)) : 0;
    S.chats = { list: rows.slice(0, 50), sel, query };
    drawChats();
  } catch (e) {
    if (S.view === "chats") $("#list").innerHTML = `<div class="derr" style="padding:8px 10px">${esc(e)}</div>`;
  }
}

function mark(text, query) {
  const safe = esc(text);
  if (!query) return safe;
  const re = new RegExp(`(${query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&").split(/\s+/).map(esc).join("|")})`, "gi");
  return safe.replace(re, "<mark>$1</mark>");
}

function drawChats() {
  const { list, sel, query } = S.chats;
  $("#sec").textContent = query ? `${list.length} chat${list.length === 1 ? "" : "s"}` : "Recent";
  $("#list").innerHTML = list.map((c, i) => `<button class="dchat" role="option" aria-selected="${i === sel}" data-i="${i}">
      ${c.pinned ? `<span class="pin" aria-label="Pinned">${ICON.pin}</span>` : ""}
      <span class="t"><span>${mark(c.title || "New chat", query)}</span>${c.snippet ? `<small>${mark(c.snippet, query)}</small>` : ""}</span>
      ${live.get(c.id)?.run ? '<span class="run"><span class="dspin"></span>Answering</span>' : `<span class="w">${when(c.updated_at)}</span>`}</button>`).join("") || '<div class="dmuted" style="padding:8px 10px">No chats</div>';
  $("#list").querySelectorAll(".dchat").forEach(b => b.addEventListener("click", () => openSession(list[+b.dataset.i].id)));
  $(`[data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" });
}

// ---------- Link ----------

async function renderLink() {
  S.view = "link";
  clearInterval(S.timer);
  try {
    S.offer = await invoke("link_offer");
    S.offer.until = Date.now() + S.offer.expires_in * 1000;
  } catch (e) {
    bar.innerHTML = `<div class="dwin"><h4>Link to your phone</h4><p class="derr">${esc(e)}</p></div>`;
    return;
  }
  drawLink("waiting");
  S.timer = setInterval(() => {
    const left = Math.max(0, Math.round((S.offer.until - Date.now()) / 1000));
    const el = $("#count");
    if (el) el.textContent = `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
    if (!left) { clearInterval(S.timer); drawLink("expired"); }
  }, 1000);
}

function drawLink(state) {
  const o = S.offer;
  const over = state === "linked"
    ? `<div class="over"><span style="display:grid;justify-items:center;gap:6px"><span style="width:44px;height:44px;border-radius:50%;background:var(--emerald);color:#fff;display:grid;place-items:center">${ICON.check}</span>Linked</span></div>`
    : state === "expired" ? '<div class="over">Code expired</div>' : "";
  const qr = `<div class="qr${state === "waiting" ? "" : " dim"}">${o.qr_svg}${over}</div>`;
  const parts = {
    waiting: `<h4>Link to your phone</h4><p>Scan with Beep on your phone: Settings, Linked devices.</p>${qr}<div class="code">${esc(o.code)}</div>
      <div class="wait"><span class="dspin"></span>Waiting for phone…</div><p style="font-size:12px">Code expires in <span id="count" style="font-variant-numeric:tabular-nums">10:00</span></p>`,
    linked: `<h4>Linked</h4><p>This computer now sees your chats and memories.</p>${qr}`,
    expired: `<h4>Link to your phone</h4><p>Scan with Beep on your phone: Settings, Linked devices.</p>${qr}<button class="dbtn pri" id="again">New code</button><p style="font-size:12px">A code lasts ten minutes.</p>`,
  };
  bar.innerHTML = `<div class="dwin">${parts[state]}</div>`;
  $("#again")?.addEventListener("click", renderLink);
}

function renderRevoked() {
  S.view = "revoked";
  bar.innerHTML = `<div class="dwin"><h4>This Mac was removed</h4>
    <p>Your phone took this computer off the account. To use Beep here again, start over with a new key and link it from the phone.</p>
    <button class="dbtn pri" id="over">Start over</button></div>`;
  $("#over").addEventListener("click", () => invoke("start_over"));
}

// ---------- State ----------

async function refreshStatus() {
  const st = await invoke("status");
  S.link = st.link;
  if (S.session === null) S.session = st.session;
  return st;
}

async function show(view) {
  await refreshStatus();
  if (S.link === "revoked") return renderRevoked();
  if (S.link === "unlinked" || view === "link") return S.link === "linked" ? renderChat() : renderLink();
  if (view === "new") return newChat();
  // A chat picked from the tray's Recent menu, already made current in Rust.
  if (view === "open") {
    const { session } = await invoke("status");
    if (session) return openSession(session);
  }
  if (S.view !== "chat" || !$("#ask")) {
    if (S.session) return openSession(S.session);
    return renderChat();
  }
  focusAsk();
}

listen("bar-shown", e => show(e.payload));
listen("link-state", async e => {
  S.link = e.payload;
  if (S.view === "link" && S.link === "linked") {
    clearInterval(S.timer);
    drawLink("linked");
    setTimeout(renderChat, 1800);
  } else if (S.link === "revoked") {
    renderRevoked();
  }
});
listen("sessions-changed", e => {
  if (S.view === "chats" && !S.chats.query) loadChats("");
  else if (S.view === "chat" && !here()?.run && e.payload && e.payload === S.session) openSession(S.session);
});

// ---------- Keys ----------

document.addEventListener("keydown", e => {
  const cmd = e.metaKey;
  if (e.key === "Escape") {
    e.preventDefault();
    if (S.view === "chats") return S.session ? openSession(S.session) : renderChat();
    return invoke("hide_bar");
  }
  if (S.view === "link" || S.view === "revoked") return;
  if (cmd && e.key.toLowerCase() === "q") { e.preventDefault(); return invoke("quit"); }
  if (cmd && e.key.toLowerCase() === "n") { e.preventDefault(); return newChat(); }
  if (cmd && e.key.toLowerCase() === "k") { e.preventDefault(); return S.view === "chats" ? null : renderChats(); }
  if (cmd && e.key === ".") { e.preventDefault(); return stop(); }
  if (S.view === "chat") {
    if (cmd && e.key === "Enter" && here()?.approval) { e.preventDefault(); return answer("allow"); }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.target.id === "ask") { e.preventDefault(); return send(e.target.value); }
    if (e.key === "ArrowUp" && e.target.id === "ask" && !e.target.value) { e.preventDefault(); return renderChats(); }
  }
  if (S.view === "chats") {
    const { list } = S.chats;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      S.chats.sel = Math.max(0, Math.min(list.length - 1, S.chats.sel + (e.key === "ArrowDown" ? 1 : -1)));
      drawChats();
    } else if (e.key === "Enter" && list[S.chats.sel]) {
      e.preventDefault();
      openSession(list[S.chats.sel].id);
    }
  }
});

// Links in replies open in the browser.
bar.addEventListener("click", e => {
  const a = e.target.closest("a[href]");
  if (!a) return;
  e.preventDefault();
  invoke("open_url", { url: a.href }).catch(() => {});
});

show(null);
