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
  group: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0113 0"/><circle cx="17" cy="9" r="2.5"/><path d="M16 14a5 5 0 016 5"/></svg>',
  doc: '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>',
  phone: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18h2"/></svg>',
};
const COPY_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 00-1-1H5a1 1 0 00-1 1v10a1 1 0 001 1h3"/></svg>';
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
  send_file: "Sending a file",
  show_products: "Showing products",
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
  view: "chat", // chat | chats | group | ginfo | invite | gnew | link | revoked
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
    if (el.id === "latest") continue;
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
  const b = body();
  b.onscroll = () => { b.stuck = b.scrollHeight - b.scrollTop - b.clientHeight < 40; drawLatest(); };
  drawLatest();
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
function scrollDown() { const b = body(); if (b) { b.stuck = true; b.scrollTop = b.scrollHeight; } }
/// Follows a streaming reply down, unless the user scrolled up to read.
function keepDown() { const b = body(); if (b?.stuck !== false) scrollDown(); }

/// While the user reads back, a button that jumps to the newest text.
function drawLatest() {
  const b = body(), want = S.view === "chat" && b?.stuck === false;
  const btn = $("#latest");
  if (!want) return btn?.remove();
  if (btn) return;
  const el = h(`<button class="dbtn dlatest" id="latest"><span class="down">${ICON.chev}</span>Latest</button>`);
  el.addEventListener("click", () => { scrollDown(); drawLatest(); focusAsk(); });
  bar.append(el);
}

function newTurn(question, images) {
  const turn = h(`<div class="turn">${question ? `<div class="ask">${esc(question)}</div>` : ""}${picRow(images)}</div>`);
  fillPics(turn);
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
  if (!el) {
    el = h('<div class="reply"></div>');
    const files = turn.querySelector(".files");
    files ? turn.insertBefore(el, files) : turn.append(el);
  }
  return el;
}

function thinking(turn, delta) {
  let row = turn.querySelector(".thinkrow");
  if (!row) {
    row = h(`<div class="thinkrow" style="display:grid;gap:6px"><button class="drow" aria-expanded="false"><span class="chev">${ICON.chev}</span>Thinking</button><div class="think" hidden></div></div>`);
    const btn = row.querySelector("button"), text = row.querySelector(".think");
    btn.addEventListener("click", () => { text.hidden = !text.hidden; btn.setAttribute("aria-expanded", String(!text.hidden)); });
    const ask = turn.querySelector(".dpics") ?? turn.querySelector(".ask");
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
  touch(id);
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
  else if (S.view === "chats") S.chats.mode === "list" ? loadChats(S.chats.query) : drawChats();
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
        if (ev.file) fileCard(turn, ev.file);
        if (ev.products) productRow(turn, ev.products);
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
    if (turn.isConnected) keepDown();
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
  touch(id, message);
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

/// Redraws the open chat from the server's copy without rebuilding the bar,
/// so a draft stays and a user reading back is not sent to the bottom.
async function refreshSession(id) {
  const b = body(), stuck = b?.stuck !== false, top = b?.scrollTop;
  const turns = await invoke("open_session", { id }).catch(() => null);
  if (!turns || S.session !== id || S.view !== "chat" || live.has(id) || body() !== b) return;
  drawHistory(id, turns);
  if (!stuck) { b.scrollTop = top; b.stuck = false; drawLatest(); }
}

function drawHistory(id, turns) {
  body().innerHTML = "";
  let turn = null, last = null;
  for (const t of turns) {
    if (t.role === "user" || t.role === "scheduled") {
      turn = t.role === "user" ? newTurn(t.text, t.images) : newTurn(t.label || "Routine");
    } else if (t.role === "assistant") {
      turn ??= newTurn("");
      turn.steps.push(...t.tools.map(x => ({ id: "", label: toolLabel(x.name) || "Using a tool", state: x.success ? "ok" : "bad" })));
      if (t.html) reply(turn).innerHTML += t.html;
      for (const x of t.tools) {
        if (x.file) fileCard(turn, x.file);
        if (x.products) productRow(turn, x.products);
      }
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

// Chats and groups share the list: pinned first, then the latest.
const byRecent = (a, b) => (b.pinned - a.pinned) || (b.updated_at - a.updated_at);
function rows(sessions, groups) {
  const all = [
    ...sessions.map(s => ({ kind: "chat", id: s.id, title: s.title || "New chat", pinned: s.pinned, updated_at: s.updated_at, snippet: s.snippet })),
    ...groups.map(g => ({ kind: "group", id: g.id, title: g.name, pinned: g.pinned, updated_at: g.updated_at, typing: g.beep_typing, snippet: g.last ? `${authorName(g.last.author)}: ${g.last.text}` : "" })),
  ];
  return all.sort(byRecent).slice(0, 50);
}

/// Moves chat `id` to the top of the cached list now, so ⌘K shows it there
/// before the server's list comes back. A new chat is titled by its first message.
function touch(id, message) {
  const old = S.cache.find(c => c.id === id);
  if (!old && !message) return;
  // A new array: the open list may be drawing the old one.
  const c = { ...(old ?? { kind: "chat", id, title: message.slice(0, 80), pinned: false }), updated_at: Date.now() };
  S.cache = [c, ...S.cache.filter(x => x.id !== id)].sort(byRecent);
}

function renderChats() {
  const back = S.view === "group" ? G.id : null;
  S.view = "chats";
  invoke("close_group").catch(() => {});
  S.chats = { list: S.cache, sel: 0, query: "", mode: "list", back, share: null };
  bar.innerHTML = `<div class="dinput"><span class="dlogo">b</span><input id="find" placeholder="Search chats and groups" autocomplete="off" spellcheck="false"><kbd>⌘K</kbd></div>
    <div class="dsec" id="sec">Recent</div><div class="dlist" id="list" role="listbox" aria-label="Chats"></div>
    ${hints([[ICON.ret, "open"], ["tab", "actions"], ["⌘N", "new chat"], ["⌘G", "new group"], ["esc", "back"]])}`;
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
    const [sessions, groups] = await Promise.all([
      query ? invoke("search_sessions", { query }).then(ms => ms.map(m => ({ ...m.session, snippet: m.snippet }))) : invoke("list_sessions"),
      invoke("list_groups"),
    ]);
    const q = query.toLowerCase();
    const list = rows(sessions, query ? groups.filter(g => g.name.toLowerCase().includes(q)) : groups);
    if (!query) S.cache = list;
    if (S.view !== "chats") return;
    const same = query === S.chats.query;
    S.chats = { ...S.chats, list, query, sel: same ? Math.min(S.chats.sel, Math.max(0, list.length - 1)) : 0, mode: same ? S.chats.mode : "list" };
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

function rowActions(c) {
  const pin = `<button class="dbtn" data-a="pin">${c.pinned ? "Unpin" : "Pin"}<kbd>⌘P</kbd></button>`;
  return c.kind === "group"
    ? `<span class="acts">${pin}<button class="dbtn" data-a="invite">Invite</button><button class="dbtn" data-a="info">Info</button><button class="dbtn warn" data-a="leave">Leave</button></span>`
    : `<span class="acts"><button class="dbtn" data-a="rename">Rename<kbd>⌘R</kbd></button>${pin}<button class="dbtn" data-a="share">Share</button><button class="dbtn warn" data-a="delete">Delete<kbd>⌘⌫</kbd></button></span>`;
}

function chatRow(c, i) {
  const { sel, query, mode } = S.chats, on = i === sel;
  if (on && mode === "rename") return `<div class="dchat" aria-selected="true"><input id="rename" value="${esc(c.title)}" maxlength="200" spellcheck="false"></div>`;
  if (on && mode === "delete") return `<div class="dchat" aria-selected="true"><span class="t"><span>Delete <b>${esc(c.title)}</b>?</span></span><span class="acts"><button class="dbtn warn" data-a="delete-yes">Delete<kbd>${ICON.ret}</kbd></button><button class="dbtn" data-a="cancel">Cancel</button></span></div>`;
  const tail = on && mode === "actions" ? rowActions(c)
    : live.get(c.id)?.run || c.typing ? `<span class="run"><span class="dspin"></span>${c.typing ? "Beep is typing" : "Answering"}</span>`
    : `<span class="w">${when(c.updated_at)}</span>`;
  // A button cannot hold the action buttons, so the open row is a div.
  const tag = on && mode === "actions" ? "div" : "button";
  return `<${tag} class="dchat" role="option" aria-selected="${on}" data-i="${i}">
      ${c.pinned ? `<span class="pin" aria-label="Pinned">${ICON.pin}</span>` : ""}${c.kind === "group" ? `<span class="grp" aria-label="Group">${ICON.group}</span>` : ""}
      <span class="t"><span>${mark(c.title, query)}</span>${c.snippet ? `<small>${mark(c.snippet, query)}</small>` : ""}</span>${tail}</${tag}>`;
}

function shareCard() {
  const sh = S.chats.share;
  if (!sh || S.chats.mode !== "share") return "";
  const body = sh.error ? `<div class="derr">${esc(sh.error)}</div>`
    : !sh.url ? '<div class="dmuted"><span class="dspin"></span>Making a link…</div>'
    : `<div class="code">${esc(sh.url)}</div><div style="display:flex;gap:8px"><button class="dbtn pri" data-a="copy-share">Copy link</button><button class="dbtn warn" data-a="unshare">Stop sharing</button></div>`;
  return `<div class="dcard dshare">${body}<div class="dmuted" style="display:block">A copy of the chat as it is now. Only your messages and Beep's replies are shared, not tool results, thoughts or your location. Anyone with Beep can open the link.</div></div>`;
}

function drawChats() {
  const { list, sel, query } = S.chats;
  $("#sec").textContent = query ? `${list.length} result${list.length === 1 ? "" : "s"}` : "Recent";
  $("#list").innerHTML = list.map((c, i) => chatRow(c, i) + (i === sel ? shareCard() : "")).join("") || '<div class="dmuted" style="padding:8px 10px">No chats</div>';
  $("#list").querySelectorAll(".dchat[data-i]").forEach(b => b.addEventListener("click", e => {
    const act = e.target.closest("[data-a]");
    S.chats.sel = +b.dataset.i;
    if (act) { e.stopPropagation(); return chatAction(act.dataset.a); }
    openRow(list[+b.dataset.i]);
  }));
  $("#list").querySelectorAll("[data-a]").forEach(b => {
    if (!b.closest(".dchat[data-i]")) b.addEventListener("click", () => chatAction(b.dataset.a));
  });
  const rename = $("#rename");
  if (rename) { rename.focus(); rename.select(); }
  $(`[data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" });
}

function openRow(c) {
  if (!c) return;
  return c.kind === "group" ? renderGroup(c.id) : openSession(c.id);
}

function chatMode(mode) {
  S.chats.mode = mode;
  drawChats();
  if (mode === "list" || mode === "actions") $("#find")?.focus();
}

async function chatAction(a) {
  const c = S.chats.list[S.chats.sel];
  if (!c) return;
  const reload = () => { S.chats.mode = "list"; loadChats(S.chats.query); };
  try {
    switch (a) {
      case "rename": case "delete": if (c.kind === "chat") chatMode(a); break;
      case "cancel": chatMode("list"); break;
      case "rename-save": {
        const title = $("#rename").value.trim();
        if (title && title !== c.title) await invoke("rename_session", { id: c.id, title });
        reload();
        break;
      }
      case "delete-yes":
        await invoke("delete_session", { id: c.id });
        live.delete(c.id);
        if (S.session === c.id) S.session = null;
        reload();
        break;
      case "pin":
        await invoke(c.kind === "group" ? "pin_group" : "pin_session", { id: c.id, pinned: !c.pinned });
        reload();
        break;
      case "share":
        S.chats.share = { id: c.id };
        chatMode("share");
        try {
          const link = await invoke("share_session", { id: c.id });
          if (S.chats.share?.id === c.id) S.chats.share.url = link.url;
        } catch (e) {
          if (S.chats.share?.id === c.id) S.chats.share.error = String(e);
        }
        if (S.view === "chats" && S.chats.mode === "share") drawChats();
        break;
      case "copy-share":
        await copy(S.chats.share.url, $('[data-a="copy-share"]'));
        break;
      case "unshare":
        await invoke("unshare_session", { id: c.id });
        S.chats.share = null;
        chatMode("list");
        break;
      case "invite": renderInvite(c.id, c.title); break;
      case "info": await renderGroup(c.id, "info"); break;
      case "leave": await renderGroup(c.id, "leave"); break;
    }
  } catch (e) {
    $("#list").insertAdjacentHTML("afterbegin", `<div class="derr" style="padding:6px 10px">${esc(e)}</div>`);
  }
}

/// Copies `text` and says so on the button for a moment.
async function copy(text, button) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // The webview may refuse the async clipboard; the old way still works.
    const t = h("<textarea></textarea>");
    t.value = text;
    document.body.append(t);
    t.select();
    document.execCommand("copy");
    t.remove();
  }
  if (!button) return;
  const was = button.innerHTML;
  button.textContent = "Copied";
  setTimeout(() => { if (button.isConnected) button.innerHTML = was; }, 1200);
}

// ---------- Files ----------

function fileKind(f) {
  if (f.mime === "application/pdf") return "PDF";
  const dot = f.name.lastIndexOf(".");
  return dot > 0 ? f.name.slice(dot + 1).toUpperCase() : "";
}
function fileSize(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/// What Beep sent with its reply: files and product cards, under the text
/// and above the suggested replies.
function sentBox(turn) {
  let box = turn.querySelector(".files");
  if (!box) {
    box = h('<div class="files" style="display:grid;gap:6px"></div>');
    const after = turn.querySelector(".dreplies");
    after ? turn.insertBefore(box, after) : turn.append(box);
  }
  return box;
}

/// A `show_products` row, built from escaped fields on the Rust side. The
/// bar's click handler opens and copies its links.
function productRow(turn, html) {
  sentBox(turn).append(h(html));
}

/// A file Beep sent. Opening it downloads it once, then hands it to its app.
function fileCard(turn, file) {
  const box = sentBox(turn);
  const about = [fileKind(file), fileSize(file.size)].filter(Boolean).join(", ");
  const card = h(`<button class="dfile"><span class="ic">${ICON.doc}</span><span style="flex:1;min-width:0;display:grid"><span class="nm">${esc(file.name)}</span><span class="sz">${esc(about)}</span></span><span class="go" style="color:var(--muted);display:grid">${ICON.chev}</span></button>`);
  const sz = card.querySelector(".sz"), go = card.querySelector(".go");
  let busy = false;
  card.addEventListener("click", async () => {
    if (busy || card.classList.contains("gone")) return;
    busy = true;
    sz.textContent = "Downloading…";
    go.innerHTML = '<span class="dspin"></span>';
    try {
      await invoke("open_file", { id: file.id, name: file.name });
      sz.textContent = about;
      go.innerHTML = ICON.chev;
    } catch (e) {
      const gone = String(e) === "No longer available";
      sz.textContent = gone ? String(e) : `${about}. ${e}`;
      if (gone) { card.classList.add("gone"); go.remove(); } else go.innerHTML = ICON.chev;
    }
    busy = false;
  });
  box.append(card);
}

// ---------- Pictures ----------

/// Pictures people sent, by id: a data URL once fetched, `null` once the
/// server said it no longer has it, a promise while it loads.
const pictures = new Map();

/// The pictures a message carried, as a row of tiles under its text.
function picRow(images) {
  if (!images?.length) return "";
  return `<div class="dpics">${images.map(picTile).join("")}</div>`;
}

function picTile(p) {
  const got = pictures.get(p.id);
  if (typeof got === "string") return `<img class="dpic" src="${got}" alt="">`;
  if (got === null) return goneTile();
  return `<span class="dpic wait" data-id="${esc(p.id)}" data-mime="${esc(p.mime)}"></span>`;
}

function goneTile() { return '<span class="dpic gone">Picture no longer available</span>'; }

/// Fetches the pictures still loading under `root` and swaps each in.
function fillPics(root) {
  for (const el of root.querySelectorAll(".dpic.wait")) {
    loadPic(el.dataset.id, el.dataset.mime).then(url => {
      if (el.isConnected) el.replaceWith(h(url ? `<img class="dpic" src="${url}" alt="">` : goneTile()));
    });
  }
}

function loadPic(id, mime) {
  let got = pictures.get(id);
  if (got === undefined) {
    got = invoke("picture", { id, mime }).then(url => {
      pictures.set(id, url);
      return url;
    }, e => {
      // Only a picture the server dropped stays gone; anything else tries again next time.
      if (String(e) === "No longer available") pictures.set(id, null);
      else pictures.delete(id);
      return null;
    });
    pictures.set(id, got);
  }
  return Promise.resolve(got);
}

// ---------- Groups ----------

/// The group on screen. Drafts and approvals are kept per group.
const G = { id: null, chat: null, pending: [], error: "", mention: null, poll: null, leave: false, drafts: new Map(), approvals: new Map() };
const GROUP_VIEWS = new Set(["group", "ginfo", "invite", "gnew"]);
/// The server refuses longer messages.
const GROUP_LIMIT = 8192;
const AV_COLORS = ["#F0C14B", "#F07A6A", "#7AB8F0", "#C99BF0", "#F0A35E", "#8FD18B"];

function authorName(a) { return a.kind === "beep" ? "Beep" : a.you ? "You" : a.name; }
function avColor(name) {
  if (name === "Beep") return "var(--emerald)";
  let n = 0;
  for (const c of name) n = (n * 31 + c.codePointAt(0)) >>> 0;
  return AV_COLORS[n % AV_COLORS.length];
}
function av(name, label = name) {
  return `<span class="dav" style="background:${avColor(name)}">${esc(name === "Beep" ? "b" : [...label][0]?.toUpperCase() ?? "?")}</span>`;
}
/// "You, Sam and Beep".
function people(group) {
  const names = [...group.members].sort((a, b) => b.you - a.you).map(m => m.you ? "You" : m.name);
  names.push("Beep");
  return names.length > 1 ? `${names.slice(0, -1).join(", ")} and ${names.at(-1)}` : names[0];
}
function dayLabel(ms) {
  const d = new Date(ms), now = new Date();
  const start = x => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((start(now) - start(d)) / 86400000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, days < 7 ? { weekday: "long" } : { day: "numeric", month: "short" });
}
function mentions(text) { return esc(text).replace(/@beep\b/gi, m => `<span class="dmention">${m}</span>`); }

function groupHead() {
  const g = G.chat?.group;
  const members = g ? [...g.members].sort((a, b) => b.you - a.you).slice(0, 3) : [];
  return `<div class="dhead" id="ghead"><span class="davs">${members.map(m => av(m.you ? "You" : m.name, m.name)).join("")}${av("Beep")}</span>
    <button class="tt" id="gtitle"><b>${esc(g?.name ?? "Group")}</b><small>${g ? esc(people(g)) : "Loading…"}</small></button>
    <button class="dbtn" id="ginvite">Invite</button><button class="dbtn" id="ginfo">Info<kbd>⌘I</kbd></button></div>`;
}
function wireHead() {
  $("#gtitle")?.addEventListener("click", renderGroupInfo);
  $("#ginfo")?.addEventListener("click", renderGroupInfo);
  $("#ginvite")?.addEventListener("click", () => renderInvite(G.id, G.chat?.group.name));
}
function redrawHead() {
  const old = $("#ghead");
  if (!old) return;
  old.replaceWith(h(groupHead()));
  wireHead();
}

function groupHints() {
  if (G.mention) return hints([["↑↓", "pick"], [ICON.ret, "insert"], ["esc", "close"]]);
  return hints([[ICON.ret, "send"], ["⌘I", "info"], ["⌘K", "chats"], ["esc", "to hide"]]);
}

/// Opens a group in the bar. `then` opens its info, or info with Leave asked.
async function renderGroup(id, then) {
  if (G.id !== id) { G.chat = null; G.pending = []; G.error = ""; }
  G.id = id;
  G.mention = null;
  G.leave = false;
  S.view = "group";
  const name = G.chat?.group.name ?? "the group";
  bar.innerHTML = `${groupHead()}<div class="dbody" id="gbody" style="padding-left:18px;padding-top:12px"></div><div id="mention"></div>
    <div class="dinput small"><textarea id="gask" rows="1" placeholder="Message ${esc(name)}, @beep to ask Beep" autocomplete="off" spellcheck="true"></textarea></div>${groupHints()}`;
  wireHead();
  const ask = $("#gask");
  ask.value = G.drafts.get(id) ?? "";
  grow(ask);
  ask.addEventListener("input", () => { G.drafts.set(G.id, ask.value); grow(ask); findMention(); });
  ask.addEventListener("click", findMention);
  if (G.chat) drawGroup(true);
  if (then) {
    await loadGroup();
    if (G.id === id) { G.leave = then === "leave"; renderGroupInfo(G.leave); }
    return;
  }
  ask.focus();
  loadGroup();
}

async function loadGroup() {
  const id = G.id;
  clearTimeout(G.poll);
  try {
    const chat = await invoke("open_group", { id });
    if (G.id !== id) return;
    G.chat = chat;
    if (S.view === "group") { redrawHead(); drawGroup(); $("#gask").placeholder = `Message ${chat.group.name}, @beep to ask Beep`; }
    else if (S.view === "ginfo") drawGroupInfo();
    // No notice comes while Beep types, so look again until it is done.
    if (chat.group.beep_typing) G.poll = setTimeout(() => { if (G.id === id && GROUP_VIEWS.has(S.view)) loadGroup(); }, 5000);
  } catch (e) {
    if (G.id !== id || S.view !== "group") return;
    G.error = String(e);
    drawGroup();
  }
}

function drawGroup(jump) {
  const b = $("#gbody");
  if (!b || !G.chat) {
    if (b && G.error) b.innerHTML = `<div class="derr">${esc(G.error)}</div>`;
    return;
  }
  const near = jump || b.scrollHeight - b.scrollTop - b.clientHeight < 40;
  const msgs = G.chat.messages;
  let html = "", day = "";
  for (const m of msgs) {
    const d = dayLabel(m.timestamp);
    if (d !== day) { html += `<div class="dday">${esc(d)}</div>`; day = d; }
    html += groupMessage(m);
  }
  for (const p of G.pending) html += `<div class="dg me pending"><div class="b">${mentions(p.text)}</div></div>`;
  if (!msgs.length && !G.pending.length) html += '<div class="dmuted" style="display:block;text-align:center;white-space:pre-line;padding:12px 0">No messages yet.\nSay hi. Beep joins in when it can help.</div>';
  if (G.chat.group.beep_typing) html += '<div class="dmuted"><span class="dtyping"><i></i><i></i><i></i></span>Beep is typing</div>';
  if (G.error) html += `<div class="derr">${esc(G.error)}</div>`;
  b.innerHTML = html;
  fillPics(b);
  for (const a of G.approvals.get(G.id)?.values() ?? []) b.append(groupApproval(a));
  if (near) b.scrollTop = b.scrollHeight;
}

function groupMessage(m) {
  const a = m.author;
  const pics = picRow(m.images);
  // A message of pictures alone shows no empty bubble.
  const bubble = content => m.text || !pics ? `<div class="b">${content}</div>` : "";
  if (a.kind === "member" && a.you) return `<div class="dg me">${bubble(mentions(m.text))}${pics}</div>`;
  const who = authorName(a);
  const content = a.kind === "beep" ? m.html : mentions(m.text);
  return `<div class="dg${a.kind === "beep" ? " beep" : ""}"><span class="who" style="color:${avColor(who)}">${esc(who)}</span>${bubble(content)}${pics}</div>`;
}

/// Beep asks the member whose apps it would use. The card waits on the group.
function groupApproval(a) {
  const card = h(`<div class="dcard">
    <div class="head"><span class="dapp">${esc((a.app || "?")[0].toUpperCase())}</span><div style="flex:1;min-width:0;display:grid"><b>${esc(a.action)}?</b><small>${esc(a.app)} asks you first</small></div></div>
    ${a.fields.length ? `<div class="kv">${a.fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-allow="1">Allow</button><button class="dbtn warn" data-allow="">Deny</button></div></div>`);
  card.querySelectorAll("[data-allow]").forEach(btn => btn.addEventListener("click", async () => {
    const allow = !!btn.dataset.allow;
    G.approvals.get(a.group_id)?.delete(a.query_id);
    card.replaceWith(h(`<div class="dmuted">${allow ? "Allowed" : "Denied"}</div>`));
    try { await invoke("answer_group_approval", { queryId: a.query_id, allow }); } catch (e) { G.error = String(e); drawGroup(); }
  }));
  return card;
}

async function sendGroup() {
  const ask = $("#gask"), text = ask.value.trim();
  if (!text || !G.chat) return;
  if (new TextEncoder().encode(text).length > GROUP_LIMIT) { G.error = "This message is too long to send."; return drawGroup(); }
  const id = G.id, p = { text };
  G.error = "";
  G.pending.push(p);
  G.drafts.delete(id);
  ask.value = "";
  grow(ask);
  closeMention();
  drawGroup(true);
  try {
    const sent = await invoke("send_to_group", { groupId: id, message: text });
    if (G.id !== id) return;
    G.pending = G.pending.filter(x => x !== p);
    if (!G.chat.messages.some(m => m.seq === sent.seq)) G.chat.messages.push(sent);
    if (S.view === "group") drawGroup(true);
    loadGroup();
  } catch (e) {
    if (G.id !== id) return;
    G.pending = G.pending.filter(x => x !== p);
    G.error = `Not sent: ${e}`;
    if (S.view === "group") {
      if (!ask.value) { ask.value = text; G.drafts.set(id, text); grow(ask); }
      drawGroup(true);
    }
  }
}

// @ opens a picker of Beep and the other members, like the phone's.
function findMention() {
  const ask = $("#gask");
  if (!ask || !G.chat) return;
  const before = ask.value.slice(0, ask.selectionStart);
  const m = before.match(/(^|\s)@([\p{L}\p{N}_]*)$/u);
  if (!m) return closeMention();
  const typed = m[2].toLowerCase();
  const items = [{ name: "Beep", note: "Make sure Beep replies" },
    ...G.chat.group.members.filter(x => !x.you).map(x => ({ name: x.name, note: "Member" }))]
    .filter(x => x.name.toLowerCase().startsWith(typed));
  if (!items.length) return closeMention();
  const sel = G.mention && G.mention.items.length === items.length ? Math.min(G.mention.sel, items.length - 1) : 0;
  G.mention = { at: before.length - m[2].length - 1, end: before.length, items, sel };
  drawMention();
}
function drawMention() {
  const box = $("#mention");
  if (!box) return;
  const mn = G.mention;
  box.innerHTML = mn ? `<div class="dmlist" role="listbox" aria-label="Mention">${mn.items.map((x, i) =>
    `<button role="option" aria-selected="${i === mn.sel}" data-m="${i}">${av(x.name)}${esc(x.name)}<small>${esc(x.note)}</small></button>`).join("")}</div>` : "";
  box.querySelectorAll("[data-m]").forEach(b => b.addEventListener("mousedown", e => { e.preventDefault(); insertMention(+b.dataset.m); }));
  const old = $(".dhints");
  if (old) old.replaceWith(h(groupHints()));
}
function closeMention() {
  if (!G.mention) return;
  G.mention = null;
  drawMention();
}
function insertMention(i) {
  const ask = $("#gask"), mn = G.mention;
  if (!ask || !mn) return;
  const word = mn.items[i].name === "Beep" ? "@beep " : `@${mn.items[i].name} `;
  ask.value = ask.value.slice(0, mn.at) + word + ask.value.slice(mn.end);
  const caret = mn.at + word.length;
  ask.setSelectionRange(caret, caret);
  G.drafts.set(G.id, ask.value);
  grow(ask);
  closeMention();
  ask.focus();
}

// ---------- Group info ----------

function renderGroupInfo(leave = false) {
  if (!G.id) return;
  S.view = "ginfo";
  G.leave = leave === true;
  G.mention = null;
  bar.innerHTML = `${groupHead()}<div class="dbody" id="ginfobody" style="padding-left:18px;padding-top:12px;gap:14px"></div>${hints([["esc", "back to the chat"], ["⌘K", "chats"]])}`;
  wireHead();
  G.reminders = null;
  drawGroupInfo();
  const id = G.id;
  invoke("list_group_reminders", { groupId: id })
    .then(list => { if (G.id === id) { G.reminders = list; drawGroupInfo(); } })
    .catch(() => { if (G.id === id) { G.reminders = []; drawGroupInfo(); } });
}

// "Every day at 9:00" for the cron shapes Beep writes, "Tomorrow 10:00" for a one-off.
function describeSchedule(schedule) {
  const clock = (h, m) => new Date(2000, 0, 1, h, m).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  const at = new Date(schedule);
  if (/^\d{4}-\d\d-\d\dT/.test(schedule) && !isNaN(at)) {
    const days = Math.round((new Date(at).setHours(0, 0, 0, 0) - new Date().setHours(0, 0, 0, 0)) / 864e5);
    const day = days === 0 ? "Today" : days === 1 ? "Tomorrow" : days > 1 && days < 7
      ? at.toLocaleDateString(undefined, { weekday: "short" }) : at.toLocaleDateString(undefined, { day: "numeric", month: "short" });
    return `${day} ${clock(at.getHours(), at.getMinutes())}`;
  }
  const [min, hour, dom, mon, dow] = schedule.trim().split(/\s+/);
  const m = Number(min), h = Number(hour);
  if (dom !== "*" || mon !== "*" || !Number.isInteger(m) || !Number.isInteger(h)) return schedule;
  const names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  if (dow === "*") return `Every day at ${clock(h, m)}`;
  if (dow === "1-5") return `Weekdays at ${clock(h, m)}`;
  if (dow.split(",").every(d => /^[0-6]$/.test(d))) return `${dow.split(",").map(d => names[d]).join(", ")} at ${clock(h, m)}`;
  return schedule;
}

function groupReminders(g) {
  const ask = 'Ask Beep in the chat to add one, like "remind us at 10 to check out".';
  const list = G.reminders;
  if (!list) return '<div class="dmuted"><span class="dspin"></span>Loading…</div>';
  if (!list.length) return `<div class="dinfo" style="color:var(--muted)">No reminders yet. ${esc(ask)}</div>`;
  const you = g.members.find(m => m.you)?.name;
  return `<div class="dinfo">${list.map(r => `<div class="member" style="align-items:flex-start">
    <span style="flex:1;min-width:0"><b style="font-weight:500">${esc(r.label)}</b><small>${esc(r.message)}</small><small>${esc(describeSchedule(r.schedule))} · set by ${esc(r.set_by === you ? "you" : r.set_by)}</small></span>
    <button class="dbtn" data-rem="${esc(r.id)}" aria-label="Delete reminder">Delete</button></div>`).join("")}
    <div style="font-size:12px;color:var(--muted)">${esc(ask)}</div></div>`;
}

function drawGroupInfo() {
  const b = $("#ginfobody"), g = G.chat?.group;
  if (!b) return;
  if (!g) { b.innerHTML = '<div class="dmuted"><span class="dspin"></span>Loading…</div>'; return; }
  const date = ms => new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short" });
  const member = (name, label, note) => `<div class="member">${av(name, label)}<span><b style="font-weight:500">${esc(name)}</b><small>${esc(note)}</small></span></div>`;
  const members = [...g.members].sort((a, b) => b.you - a.you).map(m => m.you
    ? member("You", m.name, `${m.name}, ${m.creator ? "created the group" : `joined ${date(m.joined_at)}`}`)
    : member(m.name, m.name, m.creator ? "Created the group" : `Joined ${date(m.joined_at)}`)).join("");
  b.innerHTML = `<div class="dsec flat">Members</div>
    <div class="dinfo">${members}${member("Beep", "Beep", "Joins in when it can help, or when someone writes @beep")}</div>
    <div class="dsec flat">Reminders</div>
    ${groupReminders(g)}
    <div class="dsec flat">Your apps</div>
    <div class="dtoggle"><span><b>Share my apps with Beep here</b><small>${g.apps_shared
      ? "Beep can read your calendar, mail, files and tasks to plan for the group, and never shows them here. Creating or sending anything asks you first."
      : "Beep uses your apps here only when you ask it yourself."}</small></span>
      <button class="dswitch" id="gapps" role="switch" aria-checked="${g.apps_shared}" aria-label="Share my apps with Beep here"></button></div>
    <div class="dsec flat">Privacy</div>
    <div class="dinfo" style="color:var(--muted)"><div>Beep reads every member's memories but never quotes them here. Memories from email and Slack stay out of groups.</div><div>Beep's thinking and tool calls are hidden in groups. Members only see its replies.</div></div>
    ${G.leave ? `<div class="dcard"><div style="padding:10px 12px;display:flex;gap:8px;align-items:center;flex-wrap:wrap"><span style="flex:1;font-size:13px">Leave ${esc(g.name)}? You stop getting its messages, and need a new code to come back.</span><button class="dbtn" id="gstay">Cancel</button><button class="dbtn warn" id="gleave">Leave</button></div></div>`
      : '<div><button class="dbtn warn" id="gaskleave">Leave group</button></div>'}`;
  $("#gapps").addEventListener("click", async () => {
    const shared = !g.apps_shared;
    try { await invoke("share_group_apps", { id: g.id, shared }); g.apps_shared = shared; } catch (e) { b.insertAdjacentHTML("afterbegin", `<div class="derr">${esc(e)}</div>`); return; }
    drawGroupInfo();
  });
  b.querySelectorAll("[data-rem]").forEach(btn => btn.addEventListener("click", async () => {
    try { await invoke("delete_group_reminder", { groupId: g.id, id: btn.dataset.rem }); } catch (e) { b.insertAdjacentHTML("afterbegin", `<div class="derr">${esc(e)}</div>`); return; }
    G.reminders = (G.reminders || []).filter(r => r.id !== btn.dataset.rem);
    drawGroupInfo();
  }));
  $("#gaskleave")?.addEventListener("click", () => { G.leave = true; drawGroupInfo(); });
  $("#gstay")?.addEventListener("click", () => { G.leave = false; drawGroupInfo(); });
  $("#gleave")?.addEventListener("click", async () => {
    try {
      await invoke("leave_group", { id: g.id });
    } catch (e) { b.insertAdjacentHTML("afterbegin", `<div class="derr">${esc(e)}</div>`); return; }
    G.approvals.delete(g.id);
    G.drafts.delete(g.id);
    G.id = null;
    G.chat = null;
    S.cache = S.cache.filter(c => c.id !== g.id);
    renderChats();
  });
}

// ---------- Invite ----------

async function renderInvite(id, name, reset = false) {
  S.view = "invite";
  if (G.id !== id) { G.chat = null; G.pending = []; G.error = ""; }
  G.id = id;
  invite.name = name ?? G.chat?.group.name ?? "the group";
  bar.innerHTML = `<div class="dwin"><h4>Invite people</h4><p>They scan it with Beep on their phone, or enter the code under New group, Join with a code.</p><div class="dmuted"><span class="dspin"></span>Making a code…</div></div>`;
  try {
    const link = await invoke("group_invite", { id, reset });
    if (S.view !== "invite" || G.id !== id) return;
    bar.innerHTML = `<div class="dwin"><h4>Invite to ${esc(invite.name)}</h4><p>They scan it with Beep on their phone, or enter the code under New group, Join with a code.</p>
      <div class="qr">${link.qr_svg}</div><div class="code" style="font-size:22px;font-weight:600">${esc(link.code)}</div>
      <div style="display:flex;gap:8px;flex-wrap:wrap;justify-content:center"><button class="dbtn pri" id="icode">Copy code</button><button class="dbtn" id="ilink">Copy link</button><button class="dbtn warn" id="ireset">Reset code</button></div>
      <p style="font-size:12px">${reset ? "New code made. The old one no longer works. " : ""}Everyone who joins sees the whole chat. Beep never shows one person's memories to the others.</p></div>`;
    $("#icode").addEventListener("click", e => copy(link.code, e.currentTarget));
    $("#ilink").addEventListener("click", e => copy(link.url, e.currentTarget));
    $("#ireset").addEventListener("click", () => renderInvite(id, invite.name, true));
  } catch (e) {
    if (S.view === "invite") bar.innerHTML = `<div class="dwin"><h4>Invite people</h4><p class="derr">${esc(e)}</p></div>`;
  }
}
const invite = { name: "" };

// ---------- New or join a group ----------

const MEMBER_NAME = "beep.memberName";
function memberName() { try { return localStorage.getItem(MEMBER_NAME) ?? ""; } catch { return ""; } }
function keepMemberName(name) { try { localStorage.setItem(MEMBER_NAME, name); } catch {} }

/// The form under ⌘G: a new group, a code to join one, or the group a code found.
const J = { mode: "new", name: "", found: null, error: "", busy: false };

function renderGroupNew(mode = "new") {
  S.view = "gnew";
  if (mode !== J.mode) J.error = "";
  J.mode = mode;
  const head = (t, sub, icon) => `<div class="dhead">${icon}<span class="tt"><b>${esc(t)}</b><small>${esc(sub)}</small></span></div>`;
  const logo = `<span class="dlogo" style="background:var(--emerald)">${ICON.group}</span>`;
  if (mode === "new") {
    bar.innerHTML = head("New group", "Chat with people, and Beep helps when it can", logo) + `<div class="dform" style="padding-top:12px">
      <label class="dfield">Group name<input id="jname" maxlength="80" autocomplete="off" spellcheck="false" value="${esc(J.name ?? "")}"></label>
      <label class="dfield">Your name in the group<input id="jme" maxlength="40" autocomplete="off" spellcheck="false" value="${esc(memberName())}"></label>
      <div class="dmuted" style="display:block">You get a code to send to people. Anyone with Beep who enters it can join.</div>
      ${J.error ? `<div class="derr">${esc(J.error)}</div>` : ""}
      <div class="row"><button class="dbtn" id="jcode">Join with a code</button><span style="flex:1"></span><button class="dbtn" id="jcancel">Cancel</button><button class="dbtn pri" id="jgo">Create<kbd>${ICON.ret}</kbd></button></div>
    </div>` + hints([[ICON.ret, "create"], ["tab", "next field"], ["esc", "back"]]);
    $("#jcode").addEventListener("click", () => renderGroupNew("code"));
    $("#jcancel").addEventListener("click", renderChats);
    $("#jgo").addEventListener("click", groupFormGo);
    $("#jname").focus();
  } else if (mode === "code") {
    bar.innerHTML = head("Join a group", "Enter the code someone sent you", logo) + `<div class="dform" style="padding-top:12px">
      <label class="dfield${J.error ? " err" : ""}">Invite code<input id="jtoken" class="mono" maxlength="80" autocomplete="off" spellcheck="false" placeholder="K7QM4-X9TRP">
        <span class="note">${esc(J.error || "Ten letters and digits, like K7QM4-X9TRP. A beep://group link works too.")}</span></label>
      <div class="row"><button class="dbtn" id="jcancel">Cancel</button><button class="dbtn pri" id="jgo">Find group<kbd>${ICON.ret}</kbd></button></div>
    </div>` + hints([[ICON.ret, "find"], ["esc", "back"]]);
    $("#jcancel").addEventListener("click", () => renderGroupNew("new"));
    $("#jgo").addEventListener("click", groupFormGo);
    $("#jtoken").focus();
  } else {
    const p = J.found.preview;
    const others = p.members.length ? `${p.members.join(", ")} and Beep are in it` : "Beep is in it";
    bar.innerHTML = head(`Join ${p.name}?`, others, `<span class="davs">${p.members.slice(0, 3).map(n => av(n)).join("")}${av("Beep")}</span>`) + `<div class="dform" style="padding-top:12px">
      <div class="dinfo" style="color:var(--muted)"><div>Beep can use your memories to help the group. It never shows or quotes them to others.</div><div>When Beep answers you, it runs with your apps and your account, never someone else's.</div><div>You get a notification for each new message.</div></div>
      <label class="dfield${J.error ? " err" : ""}">Your name in the group<input id="jme" maxlength="40" autocomplete="off" spellcheck="false" value="${esc(memberName())}">
        <span class="note">${esc(J.error || "The others see this name next to your messages")}</span></label>
      <div class="row"><button class="dbtn" id="jcancel">Not now</button><button class="dbtn pri" id="jgo">Join group<kbd>${ICON.ret}</kbd></button></div>
    </div>` + hints([[ICON.ret, "join"], ["esc", "back"]]);
    $("#jcancel").addEventListener("click", renderChats);
    $("#jgo").addEventListener("click", groupFormGo);
    $("#jme").focus();
  }
}

async function groupFormGo() {
  if (J.busy) return;
  const val = sel => $(sel)?.value.trim() ?? "";
  J.busy = true;
  try {
    if (J.mode === "new") {
      const name = val("#jname"), me = val("#jme");
      if (!name) return $("#jname").focus();
      if (!me) return $("#jme").focus();
      keepMemberName(me);
      J.name = name;
      const group = await invoke("create_group", { name, memberName: me });
      J.name = "";
      renderInvite(group.id, group.name);
    } else if (J.mode === "code") {
      const code = val("#jtoken");
      if (!code) return;
      try {
        J.found = await invoke("peek_group", { code });
      } catch (e) {
        J.error = String(e);
        return renderGroupNew("code");
      }
      if (J.found.preview.joined) return renderGroup(J.found.preview.id);
      J.error = "";
      renderGroupNew("join");
    } else {
      const me = val("#jme");
      if (!me) return $("#jme").focus();
      try {
        const group = await invoke("join_group", { token: J.found.token, memberName: me });
        keepMemberName(me);
        renderGroup(group.id);
      } catch (e) {
        J.error = String(e);
        renderGroupNew("join");
      }
    }
  } catch (e) {
    J.error = String(e);
    renderGroupNew(J.mode);
  } finally {
    J.busy = false;
  }
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
  // A group stays up between showings, fetched again for what came in.
  if (GROUP_VIEWS.has(S.view)) {
    if (S.view === "group") { loadGroup(); $("#gask")?.focus(); }
    else if (S.view === "ginfo") loadGroup();
    else bar.querySelector("input")?.focus();
    return;
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
// Outside the list, a reload only refreshes the cache it opens with.
const listIdle = () => S.view !== "chats" || (!S.chats.query && S.chats.mode === "list");
listen("group-changed", e => {
  if (G.id === e.payload && (S.view === "group" || S.view === "ginfo")) loadGroup();
  else if (listIdle()) loadChats("");
});
listen("group-approval", e => {
  const a = e.payload;
  if (!G.approvals.has(a.group_id)) G.approvals.set(a.group_id, new Map());
  G.approvals.get(a.group_id).set(a.query_id, a);
  if (G.id === a.group_id && S.view === "group") drawGroup();
});
listen("sessions-changed", e => {
  if (S.view === "chat" && !here()?.run && e.payload && e.payload === S.session) refreshSession(S.session);
  if (listIdle()) loadChats("");
});

// ---------- Keys ----------

function back() {
  if (S.view === "chats") {
    if (S.chats.mode !== "list") return chatMode("list");
    if (S.chats.back) return renderGroup(S.chats.back);
    return S.session ? openSession(S.session) : renderChat();
  }
  if (S.view === "group" && G.mention) return closeMention();
  if (S.view === "ginfo" || S.view === "invite") return renderGroup(G.id);
  if (S.view === "gnew") {
    if (J.mode === "join") return renderGroupNew("code");
    if (J.mode === "code") return renderGroupNew("new");
    return renderChats();
  }
  invoke("hide_bar");
}

function chatsKeys(e, cmd) {
  const { list, mode } = S.chats, c = list[S.chats.sel];
  const key = e.key.toLowerCase();
  if (mode === "rename") {
    if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); chatAction("rename-save"); }
    return;
  }
  if (mode === "delete" && e.key === "Enter") { e.preventDefault(); return chatAction("delete-yes"); }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    S.chats.sel = Math.max(0, Math.min(list.length - 1, S.chats.sel + (e.key === "ArrowDown" ? 1 : -1)));
    S.chats.mode = "list";
    drawChats();
  } else if (e.key === "Tab" && c) {
    e.preventDefault();
    chatMode(mode === "actions" ? "list" : "actions");
  } else if (cmd && key === "p" && c) {
    e.preventDefault();
    chatAction("pin");
  } else if (cmd && key === "r" && c?.kind === "chat") {
    e.preventDefault();
    chatAction("rename");
  } else if (cmd && e.key === "Backspace" && c?.kind === "chat") {
    e.preventDefault();
    chatAction("delete");
  } else if (e.key === "Enter" && c && mode === "list") {
    e.preventDefault();
    openRow(c);
  }
}

function groupKeys(e, cmd) {
  if (e.target.id !== "gask") return;
  const mn = G.mention;
  if (mn && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
    e.preventDefault();
    mn.sel = (mn.sel + (e.key === "ArrowDown" ? 1 : mn.items.length - 1)) % mn.items.length;
    return drawMention();
  }
  if (mn && (e.key === "Enter" || e.key === "Tab") && !e.isComposing) { e.preventDefault(); return insertMention(mn.sel); }
  if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); return sendGroup(); }
  if (e.key === "ArrowUp" && !e.target.value) { e.preventDefault(); return renderChats(); }
  if (e.key === "ArrowLeft" || e.key === "ArrowRight") setTimeout(findMention);
}

document.addEventListener("keydown", e => {
  const cmd = e.metaKey;
  if (e.key === "Escape") {
    e.preventDefault();
    return back();
  }
  if (S.view === "link" || S.view === "revoked") return;
  const key = e.key.toLowerCase();
  if (cmd && key === "q") { e.preventDefault(); return invoke("quit"); }
  if (cmd && key === "n") { e.preventDefault(); return newChat(); }
  if (cmd && key === "g") { e.preventDefault(); J.error = ""; J.name = ""; return renderGroupNew("new"); }
  if (cmd && key === "k") { e.preventDefault(); return S.view === "chats" ? null : renderChats(); }
  if (cmd && key === "i" && (S.view === "group" || S.view === "invite")) { e.preventDefault(); return renderGroupInfo(); }
  if (cmd && e.key === ".") { e.preventDefault(); return stop(); }
  if (S.view === "chat") {
    if (cmd && e.key === "Enter" && here()?.approval) { e.preventDefault(); return answer("allow"); }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.target.id === "ask") { e.preventDefault(); return send(e.target.value); }
    if (e.key === "ArrowUp" && e.target.id === "ask" && !e.target.value) { e.preventDefault(); return renderChats(); }
  }
  if (S.view === "chats") return chatsKeys(e, cmd);
  if (S.view === "group") return groupKeys(e, cmd);
  if (S.view === "gnew" && e.key === "Enter" && !e.isComposing && e.target.tagName === "INPUT") { e.preventDefault(); return groupFormGo(); }
});

// Links in replies open in the browser. A product card's corner button
// copies its link instead.
bar.addEventListener("click", e => {
  const c = e.target.closest("[data-copy]");
  if (c) {
    e.preventDefault();
    copy(c.dataset.copy);
    c.innerHTML = ICON.check;
    clearTimeout(c.undo);
    c.undo = setTimeout(() => { c.innerHTML = COPY_ICON; }, 1200);
    return;
  }
  const a = e.target.closest("a[href]");
  if (!a) return;
  e.preventDefault();
  invoke("open_url", { url: a.href }).catch(() => {});
});

// Product pictures show over their placeholder once loaded; a broken one
// leaves the placeholder.
bar.addEventListener("load", e => { if (e.target.matches?.(".dprod-img img")) e.target.classList.add("on"); }, true);
bar.addEventListener("error", e => { if (e.target.matches?.(".dprod-img img")) e.target.remove(); }, true);

show(null);
