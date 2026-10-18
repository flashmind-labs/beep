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
ICON.image = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="M21 15l-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/></svg>';
ICON.save = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v11M7 10l5 5 5-5M5 20h14"/></svg>';
const COPY_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 00-1-1H5a1 1 0 00-1 1v10a1 1 0 001 1h3"/></svg>';
/// Tool steps shown while a run works; older ones fold into "+N tools".
const VISIBLE_STEPS = 3;

/// Tools that get no step: what they do is already on screen, or is bookkeeping.
const QUIET_TOOLS = new Set(["suggest_replies", "show_products", "show_cards", "show_chart", "notify_user", "task_progress"]);

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
  show_cards: "Showing recommendations",
  show_chart: "Drawing a chart",
  show_stock_chart: "Looking up stocks",
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
  view: "chat", // chat | chats | group | ginfo | invite | gnew | link | revoked | settings | memories | routines | rnew | skills | model
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
const live = new Map(); // id -> { run: { id, turn } | null, approval: { queryId, turn, card } | null, body }
// Runs the bar let go of and is still telling the server to stop, by session id.
// The chat's next prompt waits for that, so the server has the stop first.
const stopping = new Map();
const here = () => live.get(S.session);
// Prompts typed while a chat's reply runs, by session id, oldest first. Each
// sends as its own turn once the reply before it finishes cleanly; a stopped
// or failed reply holds them until the user sends them with Cmd-Return.
const queued = new Map(); // id -> { list: [text], held }
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

/// Floating over the bar, so they take no height of their own.
const FLOATING = new Set(["latest", "map", "keys", "viewer", "saved"]);
/// How tall the bar gets while a picture is open, within the screen.
const VIEWER_HEIGHT = 560;
const scrolls = el => el.classList.contains("dbody") || el.classList.contains("dlist") || el.classList.contains("dkeys");
const natural = el => [...el.children].reduce((n, c) => n + (scrolls(c) ? c.scrollHeight : c.offsetHeight), 0);
let fitFrame = 0, barHeight = 0;
/// Sizes the bar to its content once per frame, so a streaming reply does
/// not resize the window on every token.
function fit() {
  fitFrame ||= requestAnimationFrame(() => {
    fitFrame = 0;
    let height = 2;
    for (const el of bar.children) if (!FLOATING.has(el.id)) height += scrolls(el) ? el.scrollHeight : el.offsetHeight;
    if ($("#viewer")) height = Math.max(height, VIEWER_HEIGHT);
    const keys = $("#keys");
    if (keys) height = Math.max(height, natural(keys) + 2);
    if (height !== barHeight) {
      barHeight = height;
      invoke("resize_bar", { height }).catch(() => {});
    }
    drawMap();
    bar.querySelectorAll(".dbody, .dlist").forEach(fadeEdges);
  });
}

/// Fades a scrolling list at each edge that has more above or below it.
function fadeEdges(el) {
  const up = el.scrollTop > 2, down = el.scrollHeight - el.scrollTop - el.clientHeight > 2;
  el.classList.toggle("dfade", up || down);
  el.style.setProperty("--ft", up ? "28px" : "0px");
  el.style.setProperty("--fb", down ? "28px" : "0px");
}
bar.addEventListener("scroll", e => { if (e.target.matches?.(".dbody, .dlist")) fadeEdges(e.target); }, true);
const sizer = new ResizeObserver(fit);
new MutationObserver(() => {
  sizer.disconnect();
  for (const el of bar.querySelectorAll(":scope > *, .dbody > *")) sizer.observe(el);
  fit();
}).observe(bar, { childList: true, subtree: true, characterData: true });

// ---------- Hints ----------

function hints(items, extra = "") {
  const st = { unlinked: ["off", "Not linked"], revoked: ["bad", "Removed"] }[S.link];
  const link = st ? `<button class="st"><span class="ddot ${st[0]}"></span>${st[1]}</button>` : "";
  return `<div class="dhints">${items.map(([k, t]) => `<span><kbd>${k}</kbd>${t}</span>`).join("")}${extra}${link}</div>`;
}
function chatHints() {
  const e = here();
  const chip = modelChip();
  if (e?.approval) return hints([[`⌘${ICON.ret}`, "allow"], [`⌥⌘${ICON.ret}`, "always"], ["⌘D", "deny"], ["esc", "to hide"]], chip);
  if (e?.run) return hints([[ICON.ret, "queue"], ["⌘.", "stop"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  if (queued.get(S.session)?.held) return hints([[`⌘${ICON.ret}`, "send queued"], ["⌘N", "new chat"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  if (attached.pics.length) return hints([[ICON.ret, "send"], ["⌘V", "paste image"], ["⌫", "remove last"], ["esc", "to hide"]], chip);
  if ($("#ask")?.value.includes("\n")) return hints([[ICON.ret, "send"], [`⇧${ICON.ret}`, "new line"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  if (!knowsHotkey()) return hints([["⌥ ⌥", "open from anywhere"], ["⌘N", "new chat"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  return hints([["⌘N", "new chat"], ["⌘K", "chats"], ["⌘/", "shortcuts"], ["esc", "to hide"]], chip);
}
// Shown from first launch until the bar is first opened with a double tap of Option.
const KNOWS_HOTKEY = "beep.knowsHotkey";
function knowsHotkey() { try { return localStorage.getItem(KNOWS_HOTKEY) === "1"; } catch { return false; } }
function learnedHotkey() { try { localStorage.setItem(KNOWS_HOTKEY, "1"); } catch {} }
function refreshHints() {
  const old = $(".dhints");
  if (old && S.view === "chat") old.replaceWith(h(chatHints()));
}

// ---------- Chat ----------

function renderChat() {
  S.view = "chat";
  bar.innerHTML = `<div class="dinput"><textarea id="ask" rows="1" placeholder="Ask Beep…" autocomplete="off" spellcheck="false"></textarea></div>
    <div class="dbody" id="body"></div>${chatHints()}`;
  const kept = here()?.body;
  if (kept) $("#body").replaceWith(kept);
  const b = body();
  b.onscroll = () => { b.stuck = b.scrollHeight - b.scrollTop - b.clientHeight < 40; drawLatest(); markMap(); };
  drawLatest();
  drawQueue();
  drawAttached();
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

/// A tick per prompt on the right edge once the chat scrolls: hover one to read
/// the prompt, click it to jump there. Ticks are only rebuilt when the prompts
/// change, since fit() calls this from the bar's MutationObserver.
function drawMap() {
  const b = body();
  const turns = b && S.view === "chat" ? [...b.children].filter(t => t.querySelector(":scope > .ask")) : [];
  let map = $("#map");
  if (turns.length < 2 || b.scrollHeight <= b.clientHeight + 4) return map?.remove();
  if (!map) map = bar.appendChild(h('<div class="dmap" id="map" aria-label="Prompts in this chat"></div>'));
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
function markMap() {
  const b = body(), map = $("#map");
  if (!b || !map?.turns) return;
  let cur = 0;
  map.turns.forEach((t, i) => { if (t.offsetTop - b.offsetTop <= b.scrollTop + b.clientHeight / 3) cur = i; });
  [...map.children].forEach((tk, i) => tk.classList.toggle("on", i === cur));
}

function newTurn(question, images) {
  const turn = h(`<div class="turn">${question ? `<div class="ask">${esc(question)}</div>` : ""}${picRow(images)}</div>`);
  fillPics(turn);
  turn.steps = [];
  turn.folded = true;
  body().append(turn);
  return turn;
}

/// The model and thinking a prompt ran on, under it. Everyone but an admin
/// is on Auto for thinking only, so they see just the level Jev picked. A
/// later line, such as Jev's pick, replaces it.
function ranOn(turn, model, thinking) {
  if (!model) return;
  const line = isAdmin()
    ? (thinking ? `${modelName(model)} · ${thinking}` : modelName(model))
    : thinking;
  if (!line) return;
  turn.querySelector(".dranon")?.remove();
  turn.querySelector(".ask")?.after(h(`<div class="drerunof dranon">${esc(line)}</div>`));
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
    <div class="acts"><button class="dbtn pri" data-d="allow">Allow<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn" data-d="always">Always<kbd>⌥⌘${ICON.ret}</kbd></button><button class="dbtn warn" data-d="deny">Deny<kbd>⌘D</kbd></button></div>
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
  turn.failed = true;
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
  replyActs(turn);
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
function enqueue(text) {
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
function sendQueued(id) {
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
function drawQueue() {
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
function follow(id, turn) {
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
        turn.steps.push({ id: ev.call_id, label: toolLabel(ev.name) || ev.description || "Using a tool", state: "run" });
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
      case "replies":
        suggestions(turn, ev.replies);
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

async function send(message) {
  const images = attached.pics.slice();
  if (!message.trim() && !images.length) return;
  const busy = here();
  if (images.length && (busy?.run || busy?.approval)) return attachError("Images send once this reply finishes");
  if (busy?.run || busy?.approval) return enqueue(message.trim());
  message = message.trim() || (images.length > 1 ? "What are these?" : "What is this?");
  attached.pics = [];
  drawAttached();
  const id = S.session ??= newId();
  const ask = $("#ask");
  body().querySelectorAll(".dreplies").forEach(r => r.remove());
  ask.value = "";
  ask.placeholder = "Ask a follow-up…";
  grow(ask);
  const turn = newTurn(message, images.map(url => ({ url })));
  turn.user = true;
  // Without Auto every prompt runs on the chosen model; with it, Jev's pick
  // arrives as an event.
  const m = MODELS.info;
  if (m && !m.auto && isAdmin()) ranOn(turn, m.selected, m.thinking);
  reply(turn).innerHTML = '<span class="caret"></span>';
  const e = entry(id);
  e.body = body();
  e.run = { id: null, turn };
  touch(id, message);
  refreshHints();
  scrollDown();
  await stopping.get(id);
  try {
    await invoke("send_prompt", { session: id, message, images, onEvent: follow(id, turn) });
  } catch (err) {
    if (!turn.stopped) failed(turn, String(err));
  }
  turn.started?.(null);
  if (!turn.done) finish(id, turn);
}

// Chats whose run, started on another device, the bar is looking up or following.
const watching = new Set();

/// Follows the open chat's run when another device started it, drawing it
/// as it streams like one sent from here.
async function watchRun(id) {
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

async function answer(decision) {
  const id = S.session, e = live.get(id), pending = e?.approval;
  if (!pending) return;
  e.approval = null;
  const said = { allow: "Allowed once", always: "Allowed, and always allowed from now on", deny: "Denied" }[decision];
  pending.card.replaceWith(h(`<div class="dmuted">${said}</div>`));
  const turn = pending.turn;
  turn.done = false;
  e.run = { id: null, turn };
  refreshHints();
  await stopping.get(id);
  try {
    await invoke("answer_approval", { session: id, queryId: pending.queryId, decision, onEvent: follow(id, turn) });
  } catch (err) {
    if (!turn.stopped) failed(turn, String(err));
  }
  turn.started?.(null);
  if (!turn.done) finish(id, turn);
}

/// Stops the run at once: the chat lets go of it and takes the next prompt
/// while the server is told to stop it.
function stop() {
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
    // A reply that finished while another chat was open left its queue waiting.
    if (queued.get(id) && !queued.get(id).held) sendQueued(id);
    else watchRun(id);
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
      turn.user = t.role === "user";
      if (turn.user) ranOn(turn, t.model, t.thinking);
    } else if (t.role === "assistant") {
      turn ??= newTurn("");
      turn.steps.push(...t.tools.filter(x => !QUIET_TOOLS.has(x.name)).map(x => ({ id: "", label: toolLabel(x.name) || "Using a tool", state: x.success ? "ok" : "bad" })));
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
}

// ---------- Chats ----------

function when(ms) {
  const d = new Date(ms), now = new Date();
  const mins = Math.round((now - d) / 60000);
  if (mins < 1) return "now";
  if (mins < 60) return `${mins} min ago`;
  const start = x => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((start(now) - start(d)) / 86400000);
  if (days === 0) return `${Math.round(mins / 60)} h ago`;
  if (days === 1) return "Yesterday";
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
  bar.innerHTML = `<div class="dinput"><input id="find" placeholder="Search chats and groups" autocomplete="off" spellcheck="false"><kbd>⌘K</kbd></div>
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
    // An older search can finish after a newer one: keep only the box's.
    if (($("#find")?.value.trim() ?? "") !== query) return;
    const same = query === S.chats.query;
    S.chats = { ...S.chats, list, query, sel: same ? Math.min(S.chats.sel, Math.max(0, list.length - 1)) : 0, mode: same ? S.chats.mode : "list" };
    drawChats();
  } catch (e) {
    if (S.view === "chats") $("#list").innerHTML = `<div class="derr" style="padding:8px 10px">${esc(e)}</div>`;
  }
}

function mark(text, query) {
  const words = (query ?? "").replace(/[.*+?^${}()|[\]\\]/g, "\\$&").split(/\s+/).filter(Boolean);
  if (!words.length) return esc(text);
  // Split the raw text, so a match never lands inside an escaped entity.
  const re = new RegExp(`(${words.join("|")})`, "gi");
  return text.split(re).map((part, i) => i % 2 ? `<mark>${esc(part)}</mark>` : esc(part)).join("");
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

// ---------- Attached images ----------

// Pictures pasted or dropped into the prompt, sent with the next message.
// Each is shrunk and sent as JPEG, as the phone does.
const attached = { pics: [], error: "" };
const MAX_IMAGES = 8;
const PIC_SIDE = 1024;

/// The strip of attached pictures above the prompt, or the error from the
/// last paste, with the placeholder and hints to match.
function drawAttached() {
  const old = $("#thumbs"), input = $("#ask")?.closest(".dinput");
  if (!input) return;
  const n = attached.pics.length;
  const el = n || attached.error ? h(`<div class="dthumbs" id="thumbs">${attached.pics.map((url, i) =>
    `<span class="dthumb"><img src="${esc(url)}" alt="Image ${i + 1}"><button aria-label="Remove image ${i + 1}" data-i="${i}">${ICON.x}</button></span>`).join("")}${attached.error ? `<span class="derr">${esc(attached.error)}</span>` : ""}</div>`) : null;
  el?.querySelectorAll("button").forEach(b => b.addEventListener("click", () => { unattach(+b.dataset.i); focusAsk(); }));
  if (old) el ? old.replaceWith(el) : old.remove();
  else if (el) input.before(el);
  const ask = $("#ask");
  if (n) {
    ask.dataset.ph ??= ask.placeholder;
    ask.placeholder = `Ask about ${n > 1 ? "these images" : "this image"}…`;
  } else if (ask.dataset.ph) {
    ask.placeholder = ask.dataset.ph;
    delete ask.dataset.ph;
  }
  refreshHints();
}

function unattach(i) {
  attached.pics.splice(i, 1);
  attached.error = "";
  drawAttached();
}

function attachError(text) {
  attached.error = text;
  drawAttached();
  clearTimeout(attached.timer);
  attached.timer = setTimeout(() => { attached.error = ""; drawAttached(); }, 4000);
}

/// Adds image files from a paste or a drop, up to `MAX_IMAGES` in all.
async function attach(files) {
  const pics = [...files].filter(f => f.type.startsWith("image/"));
  if (!pics.length) return false;
  for (const f of pics) {
    if (attached.pics.length >= MAX_IMAGES) { attachError(`Up to ${MAX_IMAGES} images in one message`); break; }
    try { attached.pics.push(await shrinkPic(f)); } catch { attachError("That image could not be read"); }
  }
  drawAttached();
  return true;
}

/// `file` as a JPEG data URI no longer than `PIC_SIDE` on its longest side.
async function shrinkPic(file) {
  const img = await createImageBitmap(file);
  const scale = Math.min(1, PIC_SIDE / Math.max(img.width, img.height, 1));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(img.width * scale));
  canvas.height = Math.max(1, Math.round(img.height * scale));
  const cx = canvas.getContext("2d");
  // JPEG has no transparency; a screenshot's clear parts become white, not black.
  cx.fillStyle = "#fff";
  cx.fillRect(0, 0, canvas.width, canvas.height);
  cx.drawImage(img, 0, 0, canvas.width, canvas.height);
  img.close();
  return canvas.toDataURL("image/jpeg", 0.8);
}

document.addEventListener("paste", e => {
  if (S.view !== "chat" || !$("#ask")) return;
  const files = [...(e.clipboardData?.items ?? [])].filter(i => i.kind === "file").map(i => i.getAsFile()).filter(Boolean);
  if (files.some(f => f.type.startsWith("image/"))) { e.preventDefault(); attach(files); }
});

// Dropping images on the chat attaches them too.
bar.addEventListener("dragover", e => {
  if (S.view !== "chat" || !e.dataTransfer?.types.includes("Files")) return;
  e.preventDefault();
  if (!$("#ddrop")) bar.append(h(`<div class="ddrop" id="ddrop">${ICON.image}Drop images to ask about them</div>`));
});
bar.addEventListener("dragleave", e => { if (!bar.contains(e.relatedTarget)) $("#ddrop")?.remove(); });
bar.addEventListener("drop", e => {
  $("#ddrop")?.remove();
  if (S.view !== "chat" || !e.dataTransfer?.files.length) return;
  e.preventDefault();
  attach(e.dataTransfer.files).then(() => focusAsk());
});

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

/// A file Beep sent: a picture when it is an image, a card otherwise.
function sentFile(turn, file) {
  const mime = picMime(file);
  mime ? replyPic(turn, { file: { ...file, mime }, caption: "" }) : fileCard(turn, file);
}

const PIC_TYPES = { png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp" };
/// The picture type of `file`, by its MIME type or else its name, or null.
function picMime(file) {
  if (Object.values(PIC_TYPES).includes(file.mime)) return file.mime;
  const ext = file.name.slice(file.name.lastIndexOf(".") + 1).toLowerCase();
  return file.mime === "application/octet-stream" || !file.mime ? PIC_TYPES[ext] ?? null : null;
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
      const gone = String(e) === GONE;
      sz.textContent = gone ? String(e) : `${about}. ${e}`;
      if (gone) { card.classList.add("gone"); go.remove(); } else go.innerHTML = ICON.chev;
    }
    busy = false;
  });
  box.append(card);
}

// ---------- Pictures ----------

/// What Rust says for a file or picture the server no longer has.
const GONE = "No longer available";

/// Pictures people sent, by id: a data URL once fetched, `null` once the
/// server said it no longer has it, a promise while it loads.
const pictures = new Map();

/// The pictures a message carried, as a row of tiles under its text.
function picRow(images) {
  if (!images?.length) return "";
  return `<div class="dpics">${images.map(picTile).join("")}</div>`;
}

function picTile(p) {
  if (p.url) return `<img class="dpic" src="${esc(p.url)}" alt="">`;
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
      if (String(e) === GONE) pictures.set(id, null);
      else pictures.delete(id);
      return null;
    });
    pictures.set(id, got);
  }
  return Promise.resolve(got);
}

// ---------- Pictures Beep sends ----------

/// The row of thumbnails under a reply; `pics` lists them for the viewer.
function picsRow(turn) {
  const box = sentBox(turn);
  let row = box.querySelector(".drpics");
  if (!row) {
    row = h('<div class="drpics"></div>');
    row.pics = [];
    box.prepend(row);
  }
  return row;
}

/// One picture from `send_image` or an image file: `{ url }` from the web or
/// `{ file }` kept with the chat. Hover shows Save, a click opens the viewer.
function replyPic(turn, pic) {
  const row = picsRow(turn), i = row.pics.push(pic) - 1;
  const name = pic.file?.name ?? "";
  const tile = h(`<span class="drpic wait"><button class="pic" aria-label="${esc(pic.caption || name || "Picture")}"></button>${pic.file ? `<button class="save" aria-label="Save ${esc(name)}" title="Save to Downloads">${ICON.save}</button>` : ""}</span>`);
  const gone = () => { if (tile.isConnected) tile.replaceWith(h(`<span class="drpic gone">${GONE_PIC}</span>`)); };
  const show = src => {
    if (!src) return gone();
    const img = h('<img alt="">');
    img.onload = () => tile.classList.remove("wait");
    img.onerror = gone;
    img.src = src;
    tile.querySelector(".pic").append(img);
  };
  tile.querySelector(".pic").addEventListener("click", () => openViewer(row.pics, i));
  tile.querySelector(".save")?.addEventListener("click", () => savePic(pic.file));
  row.append(tile);
  pic.file ? loadPic(pic.file.id, pic.file.mime).then(show) : show(pic.url);
}

const GONE_PIC = "Picture no longer available";
const V = { pics: [], at: 0 };

function openViewer(pics, at) {
  Object.assign(V, { pics, at });
  $("#keys")?.remove();
  drawViewer();
}

function closeViewer() {
  $("#viewer")?.remove();
  focusAsk();
}

/// The open picture over the whole bar, with arrows when there are more.
function drawViewer() {
  const p = V.pics[V.at], many = V.pics.length > 1;
  const name = p.file ? p.file.name : hostOf(p.url);
  const about = [many ? `${V.at + 1} of ${V.pics.length}` : "", p.file ? [fileKind(p.file), fileSize(p.file.size)].filter(Boolean).join(", ") : ""].filter(Boolean).join(" · ");
  const acts = p.file
    ? `<button class="dbtn" data-v="copy">${COPY_ICON}Copy<kbd>⌘C</kbd></button><button class="dbtn pri" data-v="save">${ICON.save}Save<kbd>⌘S</kbd></button>`
    : `<button class="dbtn" data-v="open">Open in browser</button>`;
  const nav = (dir, on) => many ? `<button class="nav${dir === "prev" ? " prev" : ""}" data-v="${dir}" aria-label="${dir === "prev" ? "Previous" : "Next"}"${on ? "" : " disabled"}>${ICON.chev}</button>` : "";
  const keys = [...(many ? [["← →", "move"]] : []), ...(p.file ? [["⌘S", "save"], ["⌘C", "copy"]] : []), ["esc", "close"]];
  const el = h(`<div class="dover dviewer" id="viewer" role="dialog" aria-label="${esc(name)}">
    <div class="vt"><span class="tt"><b>${esc(name)}</b><small>${esc(about)}</small></span>${acts}<button class="dbtn" data-v="close" aria-label="Close">${ICON.x}</button></div>
    <div class="vi">${nav("prev", V.at > 0)}<img alt="${esc(p.caption)}">${nav("next", V.at < V.pics.length - 1)}</div>
    <div class="cap">${esc(p.caption)}</div>${hints(keys)}</div>`);
  const img = el.querySelector("img");
  const gone = () => img.replaceWith(h(`<div class="dmuted" style="flex:1;justify-content:center">${GONE_PIC}</div>`));
  img.onerror = gone;
  if (p.file) loadPic(p.file.id, p.file.mime).then(url => url ? (img.src = url) : gone());
  else img.src = p.url;
  el.addEventListener("click", e => { const b = e.target.closest("[data-v]"); if (b) viewerAct(b.dataset.v); });
  const old = $("#viewer");
  old ? old.replaceWith(el) : bar.append(el);
}

function viewerAct(a) {
  const p = V.pics[V.at];
  if (a === "prev" || a === "next") {
    const at = V.at + (a === "next" ? 1 : -1);
    if (at >= 0 && at < V.pics.length) { V.at = at; drawViewer(); }
  } else if (a === "close") closeViewer();
  else if (a === "save" && p.file) savePic(p.file);
  else if (a === "copy" && p.file) copyPic(p.file);
  else if (a === "open" && p.url) invoke("open_url", { url: p.url }).catch(() => {});
}

function viewerKeys(e, cmd) {
  const key = e.key.toLowerCase();
  const act = e.key === "Escape" ? "close" : e.key === "ArrowLeft" ? "prev" : e.key === "ArrowRight" ? "next"
    : cmd && key === "s" ? "save" : cmd && key === "c" ? "copy" : null;
  if (act) { e.preventDefault(); viewerAct(act); }
  else if (cmd && key === "q") invoke("quit");
}

function hostOf(url) {
  try { return new URL(url).hostname; } catch { return "Picture"; }
}

/// Saves a picture Beep sent to Downloads and says where it went.
async function savePic(file) {
  try {
    const path = await invoke("save_file", { id: file.id, name: file.name });
    notice(`Saved ${file.name} to Downloads`, { path });
  } catch (e) {
    notice(String(e) === GONE ? GONE_PIC : `Could not save it. ${e}`, { bad: true });
  }
}

/// Copies a picture as PNG, the type every app pastes.
async function copyPic(file) {
  const url = await loadPic(file.id, file.mime);
  if (!url) return notice(GONE_PIC, { bad: true });
  const png = (async () => {
    const img = new Image();
    img.src = url;
    await img.decode();
    const canvas = document.createElement("canvas");
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    canvas.getContext("2d").drawImage(img, 0, 0);
    return new Promise(done => canvas.toBlob(done, "image/png"));
  })();
  try {
    // Handing over the promise keeps the click's permission to write.
    await navigator.clipboard.write([new ClipboardItem({ "image/png": png })]);
    notice("Copied the picture");
  } catch {
    notice("Could not copy the picture", { bad: true });
  }
}

/// A short note at the bottom of the bar; one that names a saved file can
/// show it in Finder.
function notice(text, { path, bad } = {}) {
  $("#saved")?.remove();
  const el = h(`<div class="dsaved${bad ? " bad" : ""}" id="saved" role="status"><span>${esc(text)}</span>${path ? '<button class="dbtn">Show in Finder</button>' : ""}</div>`);
  el.querySelector("button")?.addEventListener("click", () => invoke("reveal_file", { path }).catch(() => {}));
  bar.append(el);
  clearTimeout(notice.timer);
  notice.timer = setTimeout(() => el.remove(), 5000);
}

// ---------- Shortcuts ----------

const SHORTCUTS = [
  ["Anywhere", [["⌥ ⌥", "Open or hide Beep"], ["⌘N", "New chat"], ["⌘K", "Chats"], ["⌘G", "New group"], ["⌘M", "Memories"], ["⌘J", "Routines"], ["⌘L", "Skills"], ["⌘,", "Settings"], ["⌘Q", "Quit"]]],
  ["In a chat", [["⌘E", "Model, for admins"], [ICON.ret, "Send"], [`⇧${ICON.ret}`, "New line"], ["↑", "Chats, when empty"], ["⌘.", "Stop"], [`⌘${ICON.ret}`, "Allow"], [`⌥⌘${ICON.ret}`, "Always allow"], ["⌘D", "Deny"]]],
  ["In a list", [["↑↓", "Move"], [ICON.ret, "Open"], ["tab", "Actions"], ["⌘R", "Rename"], ["⌘P", "Pin"], ["⌘⌫", "Delete or forget"], ["esc", "Back"]]],
];

/// Opens or closes the sheet of every key, over whatever the bar shows.
function toggleKeys() {
  const open = $("#keys");
  if (open) { open.remove(); return focusAsk(); }
  $("#viewer")?.remove();
  bar.append(h(`<div class="dover" id="keys" role="dialog" aria-label="Keyboard shortcuts">
    <div class="vt"><span class="tt"><b>Keyboard shortcuts</b></span></div>
    <div class="dkeys">${SHORTCUTS.map(([sec, rows]) => `<div class="kg"><div class="dsec">${sec}</div>${rows.map(([k, t]) => `<div class="kr"><span>${t}</span><kbd>${k}</kbd></div>`).join("")}</div>`).join("")}</div>
    ${hints([["esc", "close"]])}</div>`));
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
    // The local copy does not know whether Beep is typing, so ask the server.
    if (!chat.group.beep_typing) {
      const typing = await invoke("group_typing", { groupId: id }).catch(() => false);
      if (G.chat !== chat || !typing) return;
      chat.group.beep_typing = true;
      if (S.view === "group") drawGroup();
    }
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
    const sent = await invoke("send_to_group", { groupId: id, message: text, images: [] });
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
  const fields = schedule.trim().split(/\s+/);
  if (fields.length !== 5) return schedule;
  const [min, hour, dom, mon, dow] = fields;
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
  const alone = state === "linked" ? "" : `<div class="dor"><span>or</span></div>
    <button class="dbtn pri" id="here">Use Beep on this Mac</button>
    <p style="font-size:12px">Starts an account on this Mac. You can link your phone to it later.</p>
    <button class="dbtn quiet" id="havecode">Have a join code?</button>`;
  bar.innerHTML = `<div class="dwin">${parts[state]}${alone}</div>`;
  $("#again")?.addEventListener("click", renderLink);
  $("#here")?.addEventListener("click", useHere);
  $("#havecode")?.addEventListener("click", () => renderJoinCode());
}

async function useHere(e) {
  const button = e.currentTarget;
  button.disabled = true;
  clearInterval(S.timer);
  // Not "link", so the link-state event does not show "Linked to your phone".
  S.view = "starting";
  try {
    await invoke("use_here");
    S.link = "linked";
    loadModels();
    renderSettings();
  } catch (err) {
    S.view = "link";
    button.disabled = false;
    button.insertAdjacentHTML("afterend", `<p class="derr">${esc(err)}</p>`);
  }
}

/// Typing the code another computer shows, from the link view or Settings.
function renderJoinCode(error = "", back = renderLink) {
  clearInterval(S.timer);
  S.view = "joincode";
  bar.innerHTML = `<div class="dwin"><h4>Join an account</h4>
    <label class="dfield${error ? " err" : ""}" style="width:100%;text-align:left">Join code<input id="jcode" class="mono" maxlength="20" autocomplete="off" spellcheck="false" placeholder="K7QM4-X9TRP">
      <span class="note">${esc(error || "The code your other computer shows under Settings, Devices.")}</span></label>
    <p style="font-size:12px">This Mac's chats and memories move to that account.</p>
    <div class="row" style="display:flex;gap:8px"><button class="dbtn" id="jback">Back</button><button class="dbtn pri" id="jjoin">Join<kbd>${ICON.ret}</kbd></button></div></div>`;
  $("#jback").addEventListener("click", back);
  const go = async () => {
    const code = $("#jcode").value;
    $("#jjoin").disabled = true;
    S.view = "joining";
    try {
      const name = await invoke("join_account", { code });
      S.link = "linked";
      bar.innerHTML = `<div class="dwin"><h4>Joined</h4><p>This Mac is now on ${esc(name)}'s account.</p></div>`;
      setTimeout(renderChat, 1800);
    } catch (err) {
      renderJoinCode(/not found/i.test(err) ? "That code is used up or expired. Ask for a new one." : String(err), back);
    }
  };
  $("#jjoin").addEventListener("click", go);
  $("#jcode").addEventListener("keydown", e => { if (e.key === "Enter") go(); });
  $("#jcode").focus();
}

/// A code for a phone to join this Mac's account with.
async function renderInvitePhone() {
  S.view = "invite-phone";
  clearInterval(S.timer);
  let inv;
  try {
    inv = await invoke("invite_phone");
  } catch (e) {
    bar.innerHTML = `<div class="dwin"><h4>Link a phone</h4><p class="derr">${esc(e)}</p></div>`;
    return;
  }
  if (S.view !== "invite-phone") return;
  const until = Date.now() + inv.expires_in * 1000;
  bar.innerHTML = `<div class="dwin"><h4>Link a phone</h4>
    <p>Scan with Beep on your phone: Settings, Linked devices. The phone's chats and memories move to this Mac's account.</p>
    <div class="qr" id="iqr">${inv.qr_svg}</div><div class="code" style="font-size:22px;font-weight:600">${esc(inv.code)}</div>
    <p style="font-size:12px" id="ileft">A code lasts ten minutes.</p>
    <div style="display:flex;gap:8px"><button class="dbtn" id="iback">Back</button><button class="dbtn pri" id="icopy">Copy code</button></div></div>`;
  $("#icopy").addEventListener("click", e => copy(inv.code, e.currentTarget));
  $("#iback").addEventListener("click", () => { clearInterval(S.timer); renderSettings(); });
  S.timer = setInterval(() => {
    if (S.view !== "invite-phone") return clearInterval(S.timer);
    if (Date.now() < until) return;
    clearInterval(S.timer);
    $("#iqr")?.classList.add("dim");
    $("#ileft").innerHTML = 'Code expired. <button class="dbtn quiet" id="inew">New code</button>';
    $("#inew").addEventListener("click", renderInvitePhone);
  }, 1000);
}

function renderRevoked() {
  S.view = "revoked";
  bar.innerHTML = `<div class="dwin"><h4>This Mac was removed</h4>
    <p>Your phone took this computer off the account. To use Beep here again, start over with a new key and link it from the phone.</p>
    <button class="dbtn pri" id="over">Start over</button></div>`;
  $("#over").addEventListener("click", () => invoke("start_over"));
}

// ---------- Settings ----------

async function renderSettings(confirm = false) {
  S.view = "settings";
  let set;
  try {
    set = await invoke("settings");
  } catch (e) {
    bar.innerHTML = `<div class="dwin"><h4>Settings</h4><p class="derr">${esc(e)}</p></div>`;
    return;
  }
  if (S.view !== "settings") return;
  const linked = S.link === "linked";
  let devices = [];
  if (linked) {
    try {
      devices = await invoke("list_devices");
    } catch (e) {
      console.warn(e);
    }
    if (S.view !== "settings") return;
    if (!A.apps) A.apps = await invoke("list_apps").catch(() => null);
    if (S.view !== "settings") return;
  }
  // Whether this Mac made the account, and so links and removes devices.
  const owner = devices.some(d => d.this_device && d.primary);
  const row = d => {
    const tags = [d.this_device ? "This Mac" : "", d.primary ? "Owner" : ""].filter(Boolean)
      .map(t => `<span class="dtag">${t}</span>`).join("");
    const remove = owner && !d.this_device ? `<button class="dbtn quiet" data-remove="${esc(d.id)}" aria-label="Remove ${esc(d.name)}">${ICON.x}</button>` : "";
    return `<div class="dtoggle"><span><b style="display:flex;gap:7px;align-items:center">${esc(d.name)}${tags}</b><small>${d.kind === "phone" ? "Phone" : "Computer"}</small></span>${remove}</div>`;
  };
  const link = !linked
    ? `<div class="dtoggle"><span><b>Not linked</b><small>Link to see your chats and memories here</small></span><button class="dbtn pri" id="link">Link to phone…</button></div>`
    : devices.map(row).join("") + (owner
      ? '<div class="dtoggle"><span><small>Scan its code from Beep on the phone to bring its chats here</small></span><button class="dbtn pri" id="invite">Link a phone…</button></div>'
      : confirm ? "" : `<div class="dtoggle"><span><small>This Mac: ${esc(set.name)}</small></span><button class="dbtn" id="invite">Link a phone…</button><button class="dbtn warn" id="unlink">Unlink this Mac</button></div>`);
  const ask = linked && confirm ? `<div class="dcard"><div style="padding:10px 12px;display:flex;flex-direction:column;gap:8px">
      <b style="font-size:13.5px">Unlink this Mac?</b><div class="dmuted" style="display:block">This Mac stops seeing your chats and memories. Your phone keeps them. To use Beep here again, start over and link it from the phone.</div>
      <div style="display:flex;gap:8px"><button class="dbtn warn" id="unlinkYes">Unlink</button><button class="dbtn" id="unlinkNo">Cancel</button></div>
      <div class="derr" id="unlinkErr"></div></div></div>` : "";
  const on = (A.apps ?? []).filter(a => a.connected).map(a => a.name);
  const apps = linked ? `<div class="dsec flat" style="padding-top:6px">Apps</div>
      <div class="dtoggle"><span><b>Connected apps</b><small>${on.length ? `${on.length} connected: ${esc(on.join(", "))}` : "Gmail, Calendar, Notion and more"}</small></span><button class="dbtn" id="apps">Manage…</button></div>` : "";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Settings</b><small>Beep for macOS ${esc(set.version)}</small></span></div>
    <div class="dform" style="padding-top:12px;display:flex;flex-direction:column">
      <div class="dsec flat">Devices</div>${link}${ask}${apps}
      <div class="dsec flat" style="padding-top:6px">General</div>
      <div class="dtoggle"><span><b>Launch at login</b><small>Keep Beep in the menu bar after a restart</small></span>
        <button class="dswitch" id="autostart" role="switch" aria-checked="${set.autostart}" aria-label="Launch at login"></button></div>
    </div>${hints([["esc", "close"]])}`;
  $("#link")?.addEventListener("click", renderLink);
  $("#invite")?.addEventListener("click", renderInvitePhone);
  $("#apps")?.addEventListener("click", renderApps);
  bar.querySelectorAll("[data-remove]").forEach(b => b.addEventListener("click", async () => {
    b.disabled = true;
    try {
      await invoke("remove_device", { id: b.dataset.remove });
    } catch (e) {
      console.warn(e);
    }
    renderSettings();
  }));
  $("#unlink")?.addEventListener("click", () => renderSettings(true));
  $("#unlinkNo")?.addEventListener("click", () => renderSettings());
  $("#unlinkYes")?.addEventListener("click", async () => {
    try {
      await invoke("unlink");
      show(null);
    } catch (e) {
      $("#unlinkErr").textContent = String(e);
    }
  });
  $("#autostart").addEventListener("click", async e => {
    const sw = e.currentTarget, on = sw.getAttribute("aria-checked") !== "true";
    try {
      await invoke("set_autostart", { enabled: on });
      sw.setAttribute("aria-checked", on);
    } catch (err) {
      console.warn(err);
    }
  });
}

// ---------- Apps ----------

/// The Apps view: every app, the one being connected and the one whose
/// permissions are open.
const A = { apps: null, error: "", connecting: null, poll: null, open: null, perms: null,
  // MCP servers added by URL: null until listed, the add form, and the one
  // whose sign-in is open in the browser.
  servers: null, adding: false, addError: "", waiting: null, busy: null };

const APP_LEVELS = [
  ["ask", "Ask first", "Beep shows what it is about to do and waits for you."],
  ["allow", "Always allow", "Beep sends, changes and deletes without asking."],
  ["read", "Read only", "Beep can search and read, never change anything."],
];

function appLogo(app) {
  const letter = esc(app.name.slice(0, 1).toUpperCase());
  return app.logo_url ? `<img class="dalogo" src="${esc(app.logo_url)}" alt="" data-letter="${letter}">` : `<span class="dalogo">${letter}</span>`;
}
/// A logo that does not load becomes the app's first letter.
function logoFallbacks(root) {
  root.querySelectorAll("img.dalogo").forEach(img => img.addEventListener("error", () => img.replaceWith(h(`<span class="dalogo">${img.dataset.letter}</span>`)), { once: true }));
}

function renderApps() {
  S.view = "apps";
  A.open = null;
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Apps</b><small>Connect once, then just ask from any chat</small></span></div>
    <div class="dlist dapps" id="alist"></div>${hints([["esc", "back"]])}`;
  $("#alist").addEventListener("click", appsClick);
  drawApps();
  loadApps();
}

async function loadApps() {
  try {
    A.apps = await invoke("list_apps");
    A.error = "";
  } catch (e) {
    A.error = String(e);
  }
  // A server too old to know them leaves the section out.
  A.servers = await invoke("list_mcp_servers").catch(() => A.servers);
  if (A.waiting && A.servers?.find(m => m.id === A.waiting)?.connected) A.waiting = null;
  // A Composio sign-in ends on the web page; the app shows up connected here.
  if (A.connecting && A.apps?.find(a => a.slug === A.connecting)?.connected) stopConnecting();
  if (S.view === "apps") drawApps();
}

function drawApps() {
  const list = $("#alist");
  if (!list) return;
  const apps = A.apps ?? [];
  const row = app => {
    let end;
    if (A.connecting === app.slug) end = `<span class="dmuted"><span class="dspin"></span> Waiting for browser…</span><button class="dbtn quiet" data-cancel>Cancel</button>`;
    else if (app.connected) end = `<span class="dtag">Connected</span><span class="dmuted">${ICON.chev}</span>`;
    else if (!app.configured) end = '<span class="dmuted">Unavailable</span>';
    else end = `<button class="dbtn pri" data-connect="${esc(app.slug)}"${A.connecting ? " disabled" : ""}>Connect</button>`;
    return `<div class="dappr${app.connected ? " on" : ""}" ${app.connected ? `data-open="${esc(app.slug)}" role="button" tabindex="0"` : ""}>
      ${appLogo(app)}<span class="tt"><b>${esc(app.name)}</b><small>${esc(app.description)}</small></span>${end}</div>`;
  };
  const connected = apps.filter(a => a.connected), more = apps.filter(a => !a.connected);
  let html = A.error ? `<div class="derr" style="padding:8px 10px">${esc(A.error)}</div>` : "";
  if (!A.apps && !A.error) html += '<div class="dmuted" style="padding:14px 10px"><span class="dspin"></span></div>';
  if (connected.length) html += '<div class="dsec">Connected</div>' + connected.map(row).join("");
  if (more.length) html += `<div class="dsec">${connected.length ? "More apps" : "Available"}</div>` + more.map(row).join("");
  if (A.servers) html += drawServers();
  // Redrawing keeps what is typed in the add form.
  const typed = { name: $("#mname")?.value, url: $("#murl")?.value }, focus = document.activeElement?.id;
  list.innerHTML = html;
  logoFallbacks(list);
  if (typed.name != null && $("#mname")) $("#mname").value = typed.name;
  if (typed.url != null && $("#murl")) $("#murl").value = typed.url;
  if (focus && $("#" + focus)) $("#" + focus).focus();
}

function hostOf(url) {
  try { return new URL(url).host; } catch { return url; }
}

/// The servers the user added, then the add form or its button.
function drawServers() {
  const row = m => {
    let end;
    if (A.waiting === m.id) end = `<span class="dmuted"><span class="dspin"></span> Waiting for browser…</span><button class="dbtn quiet" data-mcancel>Cancel</button>`;
    else if (A.busy === m.id) end = '<span class="dmuted"><span class="dspin"></span></span>';
    else if (m.connected) end = `<span class="dtag ok">${m.tools.length === 1 ? "1 tool" : `${m.tools.length} tools`}</span><button class="dbtn quiet" data-mremove="${esc(m.id)}">Remove</button>`;
    else end = `<span class="dtag ask">${m.sign_in ? "Sign-in expired" : "Not connected"}</span><button class="dbtn pri" data-mconnect="${esc(m.id)}">${m.sign_in ? "Sign in" : "Retry"}</button>`
      + `<button class="dbtn quiet" data-mremove="${esc(m.id)}">Remove</button>`;
    const tools = m.connected && m.tools.length ? ` title="${esc(m.tools.join(", "))}"` : "";
    return `<div class="dappr"${tools}><span class="dalogo">${esc(m.name.slice(0, 1).toUpperCase())}</span><span class="tt"><b>${esc(m.name)}</b><small>${esc(hostOf(m.url))}</small></span>${end}</div>`;
  };
  let html = '<div class="dsec">Your MCP servers</div>' + A.servers.map(row).join("");
  if (A.adding) {
    const busy = A.busy === "new";
    html += `<div class="dform dmcp">
      <label class="dfield"><span>Name</span><input id="mname" maxlength="60" placeholder="Team notes" autocomplete="off" spellcheck="false"${busy ? " disabled" : ""}></label>
      <label class="dfield${A.addError ? " err" : ""}"><span>${A.addError ? esc(A.addError) : "Server URL"}</span><input id="murl" class="url" maxlength="2000" placeholder="https://example.com/mcp" autocomplete="off" spellcheck="false"${busy ? " disabled" : ""}></label>
      <div style="display:flex;gap:8px;align-items:center">${busy ? '<span class="dmuted"><span class="dspin"></span> Connecting…</span>'
        : `<button class="dbtn pri" data-madd>Add server<kbd>${ICON.ret}</kbd></button><button class="dbtn quiet" data-mclose>Cancel</button>`}</div></div>`;
  } else {
    html += `<div style="padding:4px 10px"><button class="dbtn" data-mnew>Add MCP server</button></div>`;
  }
  return html + '<div class="dmuted" style="padding:6px 10px;display:block">Beep asks before every tool on a server you add.</div>';
}

async function addServer() {
  const name = $("#mname")?.value.trim(), url = $("#murl")?.value.trim();
  if (!name || !url) return (name ? $("#murl") : $("#mname"))?.focus();
  A.busy = "new";
  A.addError = "";
  drawApps();
  try {
    const server = await invoke("add_mcp_server", { name, url });
    A.adding = false;
    if (!server.connected) A.waiting = server.id;
  } catch (e) {
    A.addError = String(e);
  }
  A.busy = null;
  await loadApps();
  if (A.adding) $("#murl")?.focus();
}

async function serverAction(kind, id) {
  A.busy = id;
  A.error = "";
  drawApps();
  try {
    if (kind === "connect") {
      const server = await invoke("connect_mcp_server", { id });
      if (!server.connected) A.waiting = id;
    } else {
      await invoke("remove_mcp_server", { id });
    }
  } catch (e) {
    A.error = String(e);
  }
  A.busy = null;
  loadApps();
}

// The link back from a server's sign-in, which Rust already handed on.
listen("mcp-signed-in", e => {
  A.waiting = null;
  if (e.payload) A.error = `Sign-in did not finish: ${e.payload}`;
  if (S.view === "apps") loadApps();
});

async function appsClick(e) {
  const m = e.target.closest("[data-mnew],[data-mclose],[data-madd],[data-mcancel],[data-mconnect],[data-mremove]");
  if (m) {
    const d = m.dataset;
    if (d.mnew != null) { A.adding = true; A.addError = ""; drawApps(); return $("#mname")?.focus(); }
    if (d.mclose != null) { A.adding = false; A.addError = ""; return drawApps(); }
    if (d.madd != null) return addServer();
    if (d.mcancel != null) { A.waiting = null; return drawApps(); }
    if (d.mconnect) return serverAction("connect", d.mconnect);
    if (d.mremove) return serverAction("remove", d.mremove);
  }
  if (e.target.closest("[data-cancel]")) {
    invoke("cancel_app_sign_in").catch(() => {});
    return stopConnecting();
  }
  const connect = e.target.closest("[data-connect]");
  if (connect) return connectApp(A.apps.find(a => a.slug === connect.dataset.connect));
  const open = e.target.closest("[data-open]");
  if (open) return renderAppPermissions(A.apps.find(a => a.slug === open.dataset.open));
}

async function connectApp(app) {
  if (!app || A.connecting) return;
  A.connecting = app.slug;
  A.error = "";
  drawApps();
  try {
    await invoke("connect_app", { slug: app.slug, kind: app.kind });
  } catch (e) {
    if (A.connecting === app.slug && String(e) !== "cancelled") A.error = String(e);
    stopConnecting();
    return;
  }
  // Fastmail is connected once the call returns. A Composio app finishes on
  // its web page, so look again until it shows up, for ten minutes at most.
  if (app.kind === "fastmail") return stopConnecting();
  const until = Date.now() + 10 * 60e3;
  A.poll = setInterval(() => {
    if (Date.now() > until || S.view !== "apps") return stopConnecting();
    loadApps();
  }, 3000);
}

function stopConnecting() {
  clearInterval(A.poll);
  A.poll = null;
  A.connecting = null;
  loadApps();
}

async function renderAppPermissions(app) {
  if (!app) return;
  S.view = "appperms";
  A.open = app;
  A.perms = null;
  bar.innerHTML = `<div class="dhead">${appLogo(app)}<span class="tt"><b>${esc(app.name)}</b><small>What Beep may do here</small></span></div>
    <div class="dlist dapps" id="plist"></div>${hints([["esc", "back"]])}`;
  logoFallbacks(bar);
  $("#plist").addEventListener("click", permsClick);
  drawPerms();
  try {
    A.perms = await invoke("app_permissions", { slug: app.slug });
    A.error = "";
  } catch (e) {
    A.error = String(e);
  }
  if (S.view === "appperms" && A.open === app) drawPerms();
}

function drawPerms() {
  const list = $("#plist"), app = A.open, p = A.perms;
  if (!list || !app) return;
  let html = A.error ? `<div class="derr" style="padding:8px 10px">${esc(A.error)}</div>` : "";
  if (!p && !A.error) html += '<div class="dmuted" style="padding:14px 10px"><span class="dspin"></span></div>';
  if (p) {
    html += `<div class="dsec">When Beep acts in ${esc(app.name)}</div>` + APP_LEVELS.map(([level, title, detail]) =>
      `<button class="dlevel" role="radio" aria-checked="${p.level === level}" data-level="${level}"><span class="dradio"></span>
        <span class="tt"><b>${title}</b><small>${detail}</small></span></button>`).join("");
    if (p.actions.length) {
      html += '<div class="dsec">Actions</div>' + p.actions.map(a => {
        const [tag, cls] = p.level === "read" ? ["Off", ""] : p.level === "allow" || a.allowed ? ["Allowed", " ok"] : ["Asks", " ask"];
        const sw = p.level === "ask" ? `<button class="dswitch" role="switch" aria-checked="${a.allowed}" data-tool="${esc(a.tool)}" aria-label="Always allow ${esc(a.label)}"></button>` : "";
        return `<div class="daction"><span>${esc(a.label)}</span><span class="dtag${cls}">${tag}</span>${sw}</div>`;
      }).join("");
    }
    html += '<div class="dmuted" style="padding:8px 10px;display:block">Searching and reading is always allowed.</div>';
  }
  html += `<div style="padding:6px 10px 2px"><button class="dbtn warn" id="disconnect">Disconnect ${esc(app.name)}</button></div>`;
  list.innerHTML = html;
}

async function permsClick(e) {
  const app = A.open, p = A.perms;
  if (e.target.closest("#disconnect")) {
    const b = e.target.closest("#disconnect");
    b.disabled = true;
    try {
      await invoke("disconnect_app", { slug: app.slug });
    } catch (err) {
      A.error = String(err);
      return drawPerms();
    }
    A.apps = null;
    return renderApps();
  }
  if (!p) return;
  const level = e.target.closest("[data-level]")?.dataset.level;
  const tool = e.target.closest("[data-tool]")?.dataset.tool;
  if (!level && !tool) return;
  const next = {
    level: level ?? p.level,
    actions: p.actions.map(a => a.tool === tool ? { ...a, allowed: !a.allowed } : a),
  };
  if (next.level === p.level && !tool) return;
  A.perms = next;
  drawPerms();
  try {
    await invoke("set_app_permissions", { slug: app.slug, level: next.level, allowed: next.actions.filter(a => a.allowed).map(a => a.tool) });
    A.error = "";
  } catch (err) {
    A.perms = p;
    A.error = String(err);
  }
  if (S.view === "appperms") drawPerms();
}

// ---------- Charts ----------

/// show_chart and show_stock_chart results. The plot is drawn at the card's
/// own width, so text never stretches, and again when the bar resizes.
const CHART_COLORS = ["#3B9B7A", "#F07A6A", "#8FB3E8", "#E8C46A"];
const CHART_RANGES = [["1d", "1D"], ["5d", "5D"], ["1mo", "1M"], ["6mo", "6M"], ["ytd", "YTD"], ["1y", "1Y"], ["5y", "5Y"], ["max", "Max"]];
const CHART_H = 150, CHART_PAD = { top: 8, right: 52, bottom: 20 };
const chartSizer = new ResizeObserver(entries => {
  for (const { target } of entries) if (target.chart && target.clientWidth !== target.drawnWidth) drawPlot(target);
});

function chartRow(turn, chart) {
  const card = h('<div class="dchart"></div>');
  card.chart = chart;
  card.off = new Set();
  sentBox(turn).append(card);
  drawChart(card);
}

/// A value in the chart's unit: a currency code as money, % with its sign,
/// anything else after the number.
/// `digits` fixes the decimals, as an axis needs; otherwise they follow the size.
function chartValue(chart, v, signed = false, digits = null) {
  const unit = chart.unit;
  const sign = signed && v > 0 ? "+" : "";
  const most = digits ?? (Math.abs(v) < 10 ? 2 : Math.abs(v) < 1000 ? 2 : 0);
  const fixed = { minimumFractionDigits: digits ?? 0, maximumFractionDigits: most };
  if (unit === "%") return sign + new Intl.NumberFormat(undefined, fixed).format(v) + "%";
  if (/^[A-Z]{3}$/.test(unit)) {
    try {
      return sign + new Intl.NumberFormat(undefined, { style: "currency", currency: unit, ...fixed, ...(digits == null && Math.abs(v) < 1000 ? { minimumFractionDigits: 2 } : {}) }).format(v);
    } catch {}
  }
  return sign + new Intl.NumberFormat(undefined, fixed).format(v) + (unit ? " " + unit : "");
}
/// Round values between `lo` and `hi` for the value axis, about `n` apart.
function niceTicks(lo, hi, n) {
  const raw = (hi - lo) / n, mag = 10 ** Math.floor(Math.log10(raw || 1));
  const step = [1, 2, 2.5, 5, 10].map(m => m * mag).find(st => st >= raw) ?? raw;
  const ticks = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi + step * 1e-9; v += step) ticks.push(Math.abs(v) < step * 1e-9 ? 0 : v);
  return { ticks, digits: Math.max(0, -Math.floor(Math.log10(step) + 1e-9)) };
}
/// An axis label for time `ms`, as fine as the chart's span needs.
function chartTime(ms, span, full = false) {
  const d = new Date(ms);
  if (span < 2 * 86400e3) return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" }) + (full ? ", " + d.toLocaleDateString(undefined, { day: "numeric", month: "short" }) : "");
  if (span < 400 * 86400e3) return d.toLocaleDateString(undefined, { day: "numeric", month: "short", ...(full ? { year: "numeric" } : {}) });
  return d.toLocaleDateString(undefined, { month: "short", year: "numeric", ...(full ? { day: "numeric" } : {}) });
}
function xLabel(chart, x, span, full) {
  return chart.x === "time" ? chartTime(x, span, full) : chart.categories[Math.round(x)] ?? "";
}

function drawChart(card) {
  const c = card.chart, st = c.stock;
  let head;
  if (st) {
    const change = st.last - st.previous, pct = st.previous ? change / st.previous * 100 : 0;
    const range = CHART_RANGES.find(([r]) => r === st.range)?.[1] ?? "";
    const meta = [st.name, st.exchange, st.currency].filter(Boolean).map(esc).join(" · ");
    head = `<div class="ch"><span class="tt"><b>${esc(c.series.length > 1 ? c.title : st.symbol)}</b><small>${c.series.length > 1 ? esc("Change over the range, in %") : meta}</small></span>
      ${c.series.length > 1 ? "" : `<span class="px"><b>${esc(chartValue({ unit: st.currency }, st.last))}</b><small class="${change >= 0 ? "up" : "down"}">${esc(chartValue({ unit: st.currency }, change, true))} (${pct > 0 ? "+" : ""}${pct.toFixed(2)}%) ${range}</small></span>`}</div>`;
  } else {
    head = `<div class="ch"><span class="tt"><b>${esc(c.title)}</b>${c.unit ? `<small>${esc(c.unit)}</small>` : ""}</span></div>`;
  }
  const legend = c.series.length > 1
    ? `<span class="legend">${c.series.map((s, i) => `<button data-series="${i}" aria-pressed="${!card.off.has(i)}"><i style="background:${CHART_COLORS[i]}"></i>${esc(s.name)}</button>`).join("")}</span>`
    : "";
  // A comparison has no range buttons: StockChart answers one symbol.
  const ranges = st && c.series.length === 1 ? `<span class="ranges">${CHART_RANGES.map(([r, l]) => `<button data-range="${r}" aria-pressed="${r === st.range}">${l}</button>`).join("")}</span>` : "";
  const source = c.source ? `<span class="src">${esc(c.source)}</span>` : "";
  card.innerHTML = `${head}<div class="plot" role="img" aria-label="${esc(c.title)}"></div><div class="derr"></div>${ranges || legend || source ? `<div class="foot">${ranges}${legend}${source}</div>` : ""}`;
  const plot = card.querySelector(".plot");
  plot.chart = c;
  plot.card = card;
  chartSizer.observe(plot);
  drawPlot(plot);
}

/// The plot's geometry: what each axis spans and where a value lands.
function plotScale(plot) {
  const c = plot.chart, card = plot.card, W = plot.clientWidth || 600;
  const shown = c.series.filter((_, i) => !card.off.has(i));
  const xs = c.series.flatMap(s => s.points.map(p => p.x)), ys = shown.flatMap(s => s.points.map(p => p.y));
  const bar = c.kind === "bar";
  let lo = Math.min(...ys), hi = Math.max(...ys);
  if (bar) { lo = Math.min(0, lo); hi = Math.max(0, hi); }
  if (c.stock && !bar && shown.length === 1 && c.series.length === 1) { lo = Math.min(lo, c.stock.previous); hi = Math.max(hi, c.stock.previous); }
  if (!isFinite(lo)) { lo = 0; hi = 1; }
  const pad = (hi - lo) * .06 || Math.abs(hi) * .05 || 1;
  if (!bar) { lo -= pad; hi += pad; } else hi += pad;
  const x0 = bar ? -.5 : Math.min(...xs), x1 = bar ? c.categories.length - .5 : Math.max(...xs);
  const plotW = W - CHART_PAD.right, plotH = CHART_H - CHART_PAD.bottom;
  return {
    W, plotW, plotH, lo, hi, x0, x1, span: x1 - x0,
    X: x => (x1 === x0 ? plotW / 2 : (x - x0) / (x1 - x0) * plotW),
    Y: y => CHART_PAD.top + (1 - (y - lo) / (hi - lo)) * (plotH - CHART_PAD.top),
  };
}

function drawPlot(plot) {
  const c = plot.chart, card = plot.card, g = plotScale(plot);
  plot.drawnWidth = plot.clientWidth;
  const { ticks, digits } = niceTicks(g.lo, g.hi, 3);
  const grid = ticks.map(v => `<line x1="0" x2="${g.plotW}" y1="${g.Y(v)}" y2="${g.Y(v)}" class="gl"/><text x="${g.plotW + 6}" y="${g.Y(v) + 4}" class="ax">${esc(chartValue(c, v, false, digits))}</text>`).join("");
  let xticks = "";
  if (c.x === "category") {
    const every = Math.ceil(c.categories.length / Math.max(1, Math.floor(g.plotW / 70)));
    xticks = c.categories.map((l, i) => i % every ? "" : `<text x="${g.X(i)}" y="${CHART_H - 4}" class="ax" text-anchor="middle">${esc(l)}</text>`).join("");
  } else {
    const n = Math.max(2, Math.min(5, Math.floor(g.plotW / 110)));
    xticks = Array.from({ length: n }, (_, i) => {
      const x = g.x0 + g.span * i / (n - 1);
      return `<text x="${g.X(x)}" y="${CHART_H - 4}" class="ax" text-anchor="${i === 0 ? "start" : i === n - 1 ? "end" : "middle"}">${esc(chartTime(x, g.span))}</text>`;
    }).join("");
  }
  let marks = "";
  const shown = c.series.map((s, i) => ({ s, i })).filter(({ i }) => !card.off.has(i));
  if (c.kind === "bar") {
    const slot = g.plotW / c.categories.length, bw = slot * .7 / Math.max(1, shown.length);
    marks = shown.map(({ s, i }, k) => s.points.map(p => {
      const x = g.X(p.x) - slot * .35 + k * bw, y = g.Y(Math.max(p.y, 0)), y0 = g.Y(Math.min(p.y, 0));
      return `<rect x="${x}" y="${y}" width="${Math.max(1, bw - 2)}" height="${Math.max(1, y0 - y)}" rx="2.5" fill="${CHART_COLORS[i]}"/>`;
    }).join("")).join("");
  } else {
    marks = shown.map(({ s, i }) => {
      const d = s.points.map((p, j) => `${j ? "L" : "M"}${g.X(p.x).toFixed(1)},${g.Y(p.y).toFixed(1)}`).join("");
      const area = c.kind === "area" && shown.length === 1
        ? `<defs><linearGradient id="cg${i}" x1="0" x2="0" y1="0" y2="1"><stop offset="0" stop-color="${CHART_COLORS[i]}" stop-opacity=".35"/><stop offset="1" stop-color="${CHART_COLORS[i]}" stop-opacity="0"/></linearGradient></defs><path d="${d}L${g.X(s.points.at(-1).x)},${g.plotH}L${g.X(s.points[0].x)},${g.plotH}Z" fill="url(#cg${i})"/>`
        : "";
      return `${area}<path d="${d}" fill="none" stroke="${CHART_COLORS[i]}" stroke-width="1.8" stroke-linejoin="round" stroke-linecap="round"/>`;
    }).join("");
  }
  const base = c.stock && c.series.length === 1 && c.kind !== "bar"
    ? `<line x1="0" x2="${g.plotW}" y1="${g.Y(c.stock.previous)}" y2="${g.Y(c.stock.previous)}" class="base"/>`
    : c.unit === "%" ? `<line x1="0" x2="${g.plotW}" y1="${g.Y(0)}" y2="${g.Y(0)}" class="base"/>` : "";
  plot.innerHTML = `<svg width="${g.W}" height="${CHART_H}" viewBox="0 0 ${g.W} ${CHART_H}">${grid}${base}${marks}${xticks}<g class="hov"></g></svg><div class="tipc" hidden></div>`;
  plot.onmousemove = e => hoverPlot(plot, e);
  plot.onmouseleave = () => { plot.querySelector(".hov").innerHTML = ""; plot.querySelector(".tipc").hidden = true; };
}

/// The point of `points` nearest to `x`, by binary search on sorted x.
function nearest(points, x) {
  let a = 0, b = points.length - 1;
  while (b - a > 1) { const m = (a + b) >> 1; points[m].x < x ? a = m : b = m; }
  return Math.abs(points[a].x - x) <= Math.abs(points[b].x - x) ? points[a] : points[b];
}
function hoverPlot(plot, e) {
  const c = plot.chart, card = plot.card, g = plotScale(plot);
  const px = e.clientX - plot.getBoundingClientRect().left;
  if (px > g.plotW + 4) return plot.onmouseleave();
  const x = c.kind === "bar" ? Math.round(g.x0 + px / g.plotW * g.span) : g.x0 + Math.max(0, Math.min(1, px / g.plotW)) * g.span;
  const hits = c.series.map((s, i) => ({ s, i })).filter(({ s, i }) => !card.off.has(i) && s.points.length)
    .map(({ s, i }) => ({ s, i, p: nearest(s.points, x) }));
  if (!hits.length) return;
  const at = hits[0].p.x, hx = g.X(at);
  plot.querySelector(".hov").innerHTML = (c.kind === "bar" ? "" : `<line x1="${hx}" x2="${hx}" y1="0" y2="${g.plotH}" class="cross"/>`)
    + hits.map(({ i, p }) => c.kind === "bar" ? "" : `<circle cx="${g.X(p.x)}" cy="${g.Y(p.y)}" r="3.5" fill="${CHART_COLORS[i]}" stroke="#121916" stroke-width="2"/>`).join("");
  const tip = plot.querySelector(".tipc");
  const values = hits.map(({ s, p }) => hits.length > 1 ? `${esc(s.name)} ${esc(chartValue(c, p.y, c.unit === "%"))}` : esc(chartValue(c, p.y))).join(" <span>·</span> ");
  tip.innerHTML = `${esc(xLabel(c, at, g.span, true))} <span>·</span> ${values}`;
  tip.hidden = false;
  const half = tip.offsetWidth / 2;
  tip.style.left = Math.max(half, Math.min(g.plotW - half, hx)) + "px";
}

async function chartClick(e) {
  const card = e.target.closest(".dchart");
  if (!card) return false;
  const series = e.target.closest("[data-series]");
  if (series) {
    const i = +series.dataset.series;
    if (card.off.has(i)) card.off.delete(i);
    else if (card.off.size < card.chart.series.length - 1) card.off.add(i);
    drawChart(card);
    return true;
  }
  const range = e.target.closest("[data-range]");
  if (!range || !card.chart.stock || range.getAttribute("aria-pressed") === "true") return !!range;
  const plot = card.querySelector(".plot");
  plot.classList.add("wait");
  card.querySelectorAll("[data-range]").forEach(b => b.setAttribute("aria-pressed", b === range));
  try {
    card.chart = await invoke("stock_chart", { symbol: card.chart.stock.symbol, range: range.dataset.range });
    card.off.clear();
    drawChart(card);
  } catch (err) {
    plot.classList.remove("wait");
    card.querySelector(".derr").textContent = String(err);
  }
  return true;
}

// ---------- Transit routes ----------

/// A get_route result. The routes live on the server, fetched by the call's
/// id, so their shapes never sit in the model's context.
const ROUTE_WALK = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="13" cy="4" r="2"/><path d="M15 21l-2-5-3-2 1-5 3 3h3M9 9l-3 3v4"/></svg>';

const ROUTE_CHEV = '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg>';

async function routeRow(turn, callId) {
  const card = h('<div class="dchart droute"><div class="dmuted"><span class="dspin"></span>Loading the route</div></div>');
  card.pick = 0;
  // The rides whose stops are open, as "route:leg".
  card.open = new Set();
  sentBox(turn).append(card);
  try {
    card.route = await invoke("route_card", { callId });
    drawRoute(card);
  } catch {
    card.innerHTML = '<div class="dmuted">Route no longer available</div>';
  }
}

function routeChanges(r, sep) {
  return r.transfers ? `${sep}${r.transfers} change${r.transfers > 1 ? "s" : ""}` : "";
}

/// A station code as Tokyo's signs draw it, ringed in the line's colour.
const stationCode = (code, color) => code ? `<span class="rc" style="border-color:${color}">${esc(code)}</span>` : "";

function drawRoute(card) {
  const routes = card.route.routes, r = routes[card.pick] ?? routes[0];
  if (!r) return void (card.innerHTML = '<div class="dmuted">No route found</div>');
  const leg = (l, i) => {
    if (l.mode === "walk") {
      const km = l.meters >= 1000 ? `, ${(l.meters / 1000).toFixed(1)} km` : "";
      return `<div class="dmuted" style="gap:10px"><span class="rw">${ROUTE_WALK}</span>Walk ${l.minutes} min${km} to ${esc(l.to || "destination")}</div>`;
    }
    const color = l.color || "var(--emerald)", badge = l.line || l.name || l.mode;
    const key = `${card.pick}:${i}`, open = card.open.has(key), between = l.between ?? [];
    const label = `${l.stops} stop${l.stops > 1 ? "s" : ""}`;
    const stops = !l.stops ? "" : between.length
      ? ` <button class="rst m" data-stops="${key}" aria-expanded="${open}">· ${ROUTE_CHEV}${label} ·</button>`
      : ` <span class="m">· ${label} ·</span>`;
    const list = open ? `<span class="rsl">${between.map(s => `<span><span class="n">${esc(s.time)}</span>${stationCode(s.code, color)}${esc(s.name)}</span>`).join("")}</span>` : "";
    return `<div class="rl"><span class="rb" style="background:${color};color:${l.text_color || "var(--ink)"}">${esc(badge)}</span>
      <span class="rt" style="border-color:${color}"><b>${esc(l.name || badge)}${l.headsign ? ` <span class="m">toward ${esc(l.headsign)}</span>` : ""}</b>
      <span><b class="n">${esc(l.departs)}</b> ${stationCode(l.from_code, color)}${esc(l.from)}${l.from_track ? `, platform ${esc(l.from_track)}` : ""}${stops} <b class="n">${esc(l.arrives)}</b> ${stationCode(l.to_code, color)}${esc(l.to || "destination")}</span>${list}</span></div>`;
  };
  const picks = routes.length > 1
    ? `<span class="ranges">${routes.map((o, i) => `<button data-route="${i}" aria-pressed="${i === card.pick}">${o.minutes} min${routeChanges(o, ", ")}</button>`).join("")}</span>` : "";
  card.innerHTML = `<div class="ch"><span class="tt"><b>${esc(`${r.departs} to ${r.arrives} · ${r.minutes} min${routeChanges(r, " · ")}`)}</b>
      <small>${r.live ? "Live times" : "Timetable times, delays not included"}</small></span></div>
    ${picks ? `<div class="foot">${picks}</div>` : ""}
    <div class="rs">${routeSketch(r)}</div>
    <div class="rlegs">${r.legs.map(leg).join("")}</div>
    <div class="foot"><button class="dbtn" data-maps>Open in Maps</button><span class="src">Transitous</span></div>`;
}

const MAP_TILES = "https://server.arcgisonline.com/ArcGIS/rest/services/Canvas/World_Dark_Gray_Base/MapServer/tile";

/// The route over Esri's dark gray map tiles: walks dotted, rides in their
/// line's colour with a stop at each end. Both are in Web Mercator. The tiles
/// reach past the viewBox, so a wide card has map out to its edges.
function routeSketch(r) {
  const pts = r.legs.flatMap(l => l.points);
  if (!pts.length) return "";
  const W = 300, H = 132, pad = 14;
  const mx = p => (p.longitude + 180) / 360;
  const my = p => { const s = Math.sin(p.latitude * Math.PI / 180); return 0.5 - Math.log((1 + s) / (1 - s)) / (4 * Math.PI); };
  const xs = pts.map(mx), ys = pts.map(my);
  const left = Math.min(...xs), right = Math.max(...xs), top = Math.min(...ys), bottom = Math.max(...ys);
  // Units per world width, never closer than street level.
  const k = Math.min((W - 2 * pad) / Math.max(right - left, 1e-12), (H - 2 * pad) / Math.max(bottom - top, 1e-12), 256 * 2 ** 17);
  const cx = (left + right) / 2, cy = (top + bottom) / 2;
  const sx = x => W / 2 + (x - cx) * k, sy = y => H / 2 + (y - cy) * k;
  const at = p => [sx(mx(p)).toFixed(1), sy(my(p)).toFixed(1)];
  // A zoom level finer than the scale, so 256 px tiles stay sharp on Retina; the tiles stop at 16.
  const z = Math.max(0, Math.min(16, Math.ceil(Math.log2(k / 256)) + 1)), n = 2 ** z, size = (k / n + 0.5).toFixed(1);
  const world = v => v / k;
  let tiles = "";
  for (let tx = Math.floor((cx + world(-2 * W)) * n); tx <= Math.floor((cx + world(2 * W)) * n); tx++)
    for (let ty = Math.max(0, Math.floor((cy + world(-H)) * n)); ty <= Math.min(n - 1, Math.floor((cy + world(H)) * n)); ty++)
      tiles += `<image href="${MAP_TILES}/${z}/${ty}/${((tx % n) + n) % n}" x="${sx(tx / n).toFixed(1)}" y="${sy(ty / n).toFixed(1)}" width="${size}" height="${size}" preserveAspectRatio="none"/>`;
  const path = ps => ps.map((p, i) => `${i ? "L" : "M"}${at(p).join(",")}`).join("");
  const dot = (p, rad, attrs) => { const [x, y] = at(p); return `<circle cx="${x}" cy="${y}" r="${rad}" ${attrs}/>`; };
  const lines = r.legs.filter(l => l.points.length).map(l => l.mode === "walk"
    ? `<path d="${path(l.points)}" fill="none" stroke="var(--muted)" stroke-width="2.5" stroke-linecap="round" stroke-dasharray="0.1 6"/>`
    : `<path d="${path(l.points)}" fill="none" stroke="${l.color || "var(--emerald)"}" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/>`).join("");
  const rides = r.legs.filter(l => l.mode !== "walk" && l.points.length);
  const passed = rides.flatMap(l => (l.between ?? []).map(s => dot(s.at, 2.2, `fill="var(--surface)" stroke="${l.color || "var(--emerald)"}" stroke-width="1.4"`))).join("");
  const stops = rides.flatMap(l => [l.points[0], l.points.at(-1)])
    .map(p => dot(p, 4, 'fill="var(--surface)" stroke="var(--fg)" stroke-width="1.8"')).join("");
  return `<svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="xMidYMid meet" role="img" aria-label="Map of the route"><g opacity=".85">${tiles}</g>${lines}${passed}${stops}
    ${dot(pts[0], 5, 'fill="var(--emerald)"')}${dot(pts.at(-1), 6, 'fill="var(--coral)" stroke="var(--surface)" stroke-width="2"')}${routeLabels(rides, p => at(p).map(Number), W, H)}</svg>
    <small>Esri, HERE, Garmin, © OpenStreetMap</small>`;
}

/// A label for each station to get on or off at, with its codes: a change
/// at one station shares one label. A label goes beside its stop, toward the
/// middle, and is left out when no side has room.
function routeLabels(rides, at, W, H) {
  const labels = [];
  for (const l of rides) {
    const color = l.color || "var(--emerald)";
    for (const [p, name, code] of [[l.points[0], l.from, l.from_code], [l.points.at(-1), l.to, l.to_code]]) {
      const [x, y] = at(p);
      const near = labels.find(b => Math.hypot(b.x - x, b.y - y) < 18);
      if (near) {
        if (code && !near.codes.some(c => c[0] === code)) near.codes.push([code, color]);
        continue;
      }
      if (name) labels.push({ x, y, name, codes: code ? [[code, color]] : [] });
    }
  }
  const placed = [];
  const hits = (a, b) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
  return labels.map(b => {
    const codesW = b.codes.reduce((w, [c]) => w + c.length * 5.6 + 8, 0);
    const w = 8 + codesW + b.name.length * 5.2, h = 16;
    const sides = b.x < W / 2 ? [1, -1] : [-1, 1];
    const tries = sides.flatMap(s => [0, -14, 14].map(dy => ({ x: s > 0 ? b.x + 7 : b.x - 7 - w, y: b.y - h / 2 + dy, w, h })));
    const box = tries.find(t => t.x >= 0 && t.x + w <= W && t.y >= 0 && t.y + h <= H && !placed.some(o => hits(o, t)));
    if (!box) return "";
    placed.push(box);
    let x = box.x + 4;
    const codes = b.codes.map(([c, color]) => {
      const cw = c.length * 5.6 + 4, out = `<rect x="${x.toFixed(1)}" y="${(box.y + 2.5).toFixed(1)}" width="${cw.toFixed(1)}" height="11" rx="2.5" fill="#fff" stroke="${color}" stroke-width="1.5"/><text x="${(x + cw / 2).toFixed(1)}" y="${(box.y + 11).toFixed(1)}" text-anchor="middle" font-size="8" font-weight="700" fill="#10201A">${esc(c)}</text>`;
      x += cw + 4;
      return out;
    }).join("");
    return `<g><rect x="${box.x.toFixed(1)}" y="${box.y.toFixed(1)}" width="${w.toFixed(1)}" height="${h}" rx="4.5" fill="var(--surface)" fill-opacity=".92" stroke="var(--border)"/>${codes}<text x="${x.toFixed(1)}" y="${(box.y + 11.3).toFixed(1)}" font-size="9" font-weight="600" fill="var(--fg)">${esc(b.name)}</text></g>`;
  }).join("");
}

function routeClick(e) {
  const card = e.target.closest(".droute");
  const stops = e.target.closest("[data-stops]");
  if (stops) {
    const key = stops.dataset.stops;
    if (!card.open.delete(key)) card.open.add(key);
    return drawRoute(card);
  }
  const pick = e.target.closest("[data-route]");
  if (pick) {
    card.pick = +pick.dataset.route;
    return drawRoute(card);
  }
  const r = card.route.routes[card.pick], pts = r.legs.flatMap(l => l.points);
  if (!pts.length) return;
  const at = p => `${p.latitude},${p.longitude}`;
  // Maps plans the trip again between the ends, with its own live times.
  invoke("open_url", { url: `https://maps.apple.com/?saddr=${at(pts[0])}&daddr=${at(pts.at(-1))}&dirflg=r` }).catch(() => {});
}

// ---------- Model (admins) ----------

/// The caller's model settings. `choices` is empty unless they are an
/// admin, and then the bar shows no chip, no model view and no rerun.
/// `agents` is whether an admin turned agents on for everyone.
const MODELS = { info: null, sel: 0, agents: false };
const THINKING = ["off", "low", "medium", "high"];

function isAdmin() { return !!MODELS.info?.choices.length; }
async function loadModels() {
  try {
    MODELS.info = await invoke("get_models");
  } catch {
    return;
  }
  refreshHints();
  if (!isAdmin()) return;
  try {
    MODELS.agents = (await invoke("get_agents")).enabled;
  } catch {
    return;
  }
  if (S.view === "model") drawModel();
}
function modelName(id) { return MODELS.info?.choices.find(c => c.id === id)?.name ?? id.split(/[:/]/).pop(); }
function modelPrice(c) {
  const usd = n => "$" + (Math.round(n * 100) / 100);
  const price = c.input_price != null && c.output_price != null ? `${usd(c.input_price)} in, ${usd(c.output_price)} out per M tokens` : "No price listed";
  return c.sees_images ? price : price + ", text only";
}
/// The hint row's chip: the model and thinking every turn uses, or Auto.
function modelChip() {
  const m = MODELS.info;
  if (!m?.choices.length) return "";
  const label = m.auto ? "Auto" : `${modelName(m.selected)} · ${cap(m.thinking)}`;
  return `<button class="dmodel" data-act="model">${esc(label)}<kbd>⌘E</kbd></button>`;
}

function renderModel() {
  const m = MODELS.info;
  if (!m?.choices.length) return;
  S.view = "model";
  MODELS.sel = Math.max(0, m.choices.findIndex(c => c.id === m.selected));
  drawModel();
}
function drawModel() {
  const m = MODELS.info, agents = MODELS.agents;
  // With agents on, the agent Jev picks brings the model, and a failed pick
  // falls back to General, so Auto on leaves nothing to choose here.
  const agentPicks = agents && m.auto_available && m.auto;
  const autoSub = m.auto
    ? (agents ? "Jev picks an agent, which brings its model and thinking" : "Jev picks the model and thinking for each prompt")
    : (agents ? "Use the model below, with the agent's prompt" : "Off: every prompt uses the model below");
  const auto = m.auto_available
    ? `<div class="dtoggle"><span><b>Auto</b><small>${autoSub}</small></span><button class="dswitch" data-act="auto" role="switch" aria-checked="${m.auto}" aria-label="Auto"></button></div>${m.auto && !agents ? '<div class="dmuted">Used when Jev cannot pick:</div>' : ""}`
    : "";
  const rows = m.choices.map((c, i) => `<button class="drt dmrow" role="option" data-pick="${i}" aria-selected="${i === MODELS.sel}"><span class="tt"><b>${esc(c.name)}</b><span>${esc(modelPrice(c))}</span></span>${c.id === m.selected ? `<span class="dok">${ICON.check}</span>` : ""}</button>`).join("");
  const picker = agentPicks ? "" : `<div class="dlist" style="padding:0" role="listbox" aria-label="Models">${rows}</div>
      <div class="dfield"><span>Thinking</span><div class="dchips">${THINKING.map(l => `<button class="dchip" data-level="${l}" aria-pressed="${l === m.thinking}">${cap(l)}</button>`).join("")}</div></div>`;
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Model</b><small>${agents ? "Your own turns only." : "Your own turns only. Everyone else stays on the default."}</small></span></div>
    <div class="dform" style="padding-top:12px">${auto}
      ${picker}
      <div class="derr" id="modelErr"></div>
    </div>${hints(agentPicks ? [["esc", "back"]] : [["↑↓", "move"], [ICON.ret, "use"], ["esc", "back"]])}`;
}
async function changeModel(command, args) {
  try {
    MODELS.info = await invoke(command, args);
  } catch (e) {
    const err = $("#modelErr");
    if (err) err.textContent = String(e);
    return;
  }
  if (S.view === "model") drawModel();
}
function modelClick(e) {
  const pick = e.target.closest("[data-pick]");
  if (pick) { MODELS.sel = +pick.dataset.pick; return changeModel("set_model", { id: MODELS.info.choices[MODELS.sel].id }); }
  const level = e.target.closest("[data-level]");
  if (level) return changeModel("set_thinking", { level: level.dataset.level });
  if (e.target.closest('[data-act="auto"]')) return changeModel("set_auto_model", { enabled: !MODELS.info.auto });
}
function modelKeys(e) {
  if (MODELS.agents && MODELS.info.auto_available && MODELS.info.auto) return;
  const n = MODELS.info.choices.length;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    MODELS.sel = (MODELS.sel + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
    return drawModel();
  }
  if (e.key === "Enter") { e.preventDefault(); return changeModel("set_model", { id: MODELS.info.choices[MODELS.sel].id }); }
}

// ---------- Reply actions ----------

/// Copy, and for admins Rerun, under a finished reply.
function replyActs(turn) {
  turn.querySelector(".dreplyacts")?.remove();
  const text = turn.querySelector(".reply")?.innerText.trim();
  const rerun = isAdmin() && turn.user;
  if (!text && !rerun) return;
  const row = h(`<div class="dreplyacts">${text ? `<button class="dcopy" data-act="copyReply">${COPY_ICON}Copy</button>` : ""}${rerun ? `<span class="drerun"><button class="dcopy" data-act="rerunMenu" aria-haspopup="menu" aria-expanded="false">${PLAY_ICON}Rerun<span class="dchev">${ICON.chev}</span></button></span>` : ""}</div>`);
  const replies = turn.querySelector(".dreplies");
  replies ? turn.insertBefore(row, replies) : turn.append(row);
}
function closeRerunMenu() {
  const menu = $(".dmenu");
  if (!menu) return false;
  $('[data-act="rerunMenu"][aria-expanded="true"]')?.setAttribute("aria-expanded", "false");
  menu.remove();
  return true;
}
function rerunMenu(button) {
  if (closeRerunMenu()) return;
  const turn = button.closest(".turn"), m = MODELS.info;
  const rows = m.choices.map(c => {
    const used = c.id === turn.model;
    return `<button class="mi" role="menuitem" data-rerun="${esc(c.id)}"${used ? ' aria-current="true"' : ""}><b>${esc(c.name)}</b>${used ? `<span class="ok">${ICON.check}</span>` : ""}<small>${used ? "Used for this reply" : esc(modelPrice(c))}</small></button>`;
  }).join("");
  const levels = ["default", ...THINKING].map(l => `<button class="dchip" data-rthink="${l}" aria-pressed="${l === "default"}">${cap(l)}</button>`).join("");
  button.setAttribute("aria-expanded", "true");
  button.closest(".dreplyacts").after(h(`<div class="dmenu" role="menu" aria-label="Rerun on">${rows}<div class="think">Thinking<span class="dchips">${levels}</span></div></div>`));
  keepDown();
}
function rerunClick(e) {
  const level = e.target.closest("[data-rthink]");
  if (level) {
    level.parentElement.querySelectorAll(".dchip").forEach(c => c.setAttribute("aria-pressed", c === level));
    return true;
  }
  const pick = e.target.closest("[data-rerun]");
  if (!pick) return false;
  const menu = pick.closest(".dmenu"), turn = pick.closest(".turn");
  const level2 = menu.querySelector('[data-rthink][aria-pressed="true"]')?.dataset.rthink;
  closeRerunMenu();
  rerun(turn, pick.dataset.rerun, level2 && level2 !== "default" ? level2 : null);
  return true;
}
/// Drops `turn` and every turn after it, then sends its prompt again on
/// `model`, as the phone's Rerun does.
async function rerun(turn, model, thinking) {
  const id = S.session, busy = here();
  if (!id || busy?.run || busy?.approval) return;
  const turns = [...body().querySelectorAll(".turn")];
  const index = turns.filter(t => t.user).indexOf(turn);
  if (index < 0) return;
  const question = turn.querySelector(".ask")?.textContent ?? "";
  for (const t of turns.slice(turns.indexOf(turn))) t.remove();
  const fresh = newTurn(question);
  fresh.user = true;
  fresh.model = model;
  fresh.append(h(`<div class="drerunof">Rerun on ${esc(modelName(model))}${thinking ? " · " + thinking : ""}</div>`));
  reply(fresh).innerHTML = '<span class="caret"></span>';
  const e = entry(id);
  e.body = body();
  e.run = { id: null, turn: fresh };
  refreshHints();
  scrollDown();
  await stopping.get(id);
  try {
    await invoke("rerun_prompt", { session: id, index, pick: { model, thinking }, onEvent: follow(id, fresh) });
  } catch (err) {
    if (!fresh.stopped) failed(fresh, String(err));
  }
  fresh.started?.(null);
  if (!fresh.done) finish(id, fresh);
}

// ---------- Memories ----------

const MEM_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 4a3 3 0 00-3 3 3 3 0 00-2 5 3 3 0 002 5 3 3 0 006 0V4a3 3 0 00-3 0zM15 4a3 3 0 013 3 3 3 0 012 5 3 3 0 01-2 5 3 3 0 01-6 0"/></svg>';
const CLOCK_ICON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>';
const PLUS_ICON = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>';
const PLAY_ICON = '<svg width="10" height="10" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>';
const MEM_SOURCE = { user: "You told Beep", learned: "Learned in a chat", email: "From your email", slack: "From Slack" };
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

// mode: list | edit | clear
const M = { all: [], list: [], sel: 0, query: "", mode: "list", error: "", loaded: false };

/// "2 Sep" from `YYYY-MM-DD`.
function shortDate(day) {
  const [, m, d] = String(day ?? "").split("-").map(Number);
  return m && d ? `${d} ${MONTHS[m - 1]}` : "";
}

function renderMemories() {
  S.view = "memories";
  invoke("close_group").catch(() => {});
  Object.assign(M, { sel: 0, query: "", mode: "list", error: "" });
  bar.innerHTML = `<div class="dinput"><input id="mfind" placeholder="Search memories" autocomplete="off" spellcheck="false"><span class="dmuted" id="mcount"></span><kbd>⌘M</kbd></div>
    <div class="dlist" id="mlist" role="listbox" aria-label="Memories" style="border-top:1px solid var(--border);padding-top:2px"></div><div id="mhints"></div>`;
  const find = $("#mfind");
  find.focus();
  find.addEventListener("input", () => { M.query = find.value.trim(); M.sel = 0; M.mode = "list"; drawMemories(); });
  $("#mlist").addEventListener("click", memClick);
  drawMemories();
  loadMemories();
}

async function loadMemories() {
  try {
    M.all = await invoke("list_memories");
    M.loaded = true;
    M.error = "";
  } catch (e) {
    M.error = String(e);
  }
  if (S.view === "memories") drawMemories();
}

function drawMemories() {
  const list = $("#mlist");
  if (!list) return;
  const q = M.query.toLowerCase();
  M.list = M.all.filter(m => !q || [m.content, m.category, m.quote].some(t => t?.toLowerCase().includes(q)));
  // Grouped by category; the server lists newest first, so each group keeps that order.
  const cats = [...new Set(M.list.map(m => m.category || "Other"))];
  M.list = cats.flatMap(c => M.list.filter(m => (m.category || "Other") === c));
  M.sel = Math.max(0, Math.min(M.sel, M.list.length - 1));
  $("#mcount").textContent = M.loaded ? (q ? `${M.list.length} of ${M.all.length}` : String(M.all.length)) : "";
  const now = new Date();
  const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  let html = M.error ? `<div class="derr" style="padding:8px 10px">${esc(M.error)}</div>` : "";
  if (M.mode === "clear") {
    html += `<div class="dcard" style="margin:8px"><div style="padding:10px 12px;display:grid;gap:8px">
      <b style="font-size:13.5px">Forget everything?</b><div class="dmuted" style="display:block">Beep will forget all ${M.all.length} memories. This cannot be undone.</div>
      <div style="display:flex;gap:8px"><button class="dbtn warn" data-m="clear-yes">Forget all</button><button class="dbtn" data-m="list">Cancel</button></div></div></div>`;
  } else if (M.loaded && !M.all.length) {
    html += `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${MEM_ICON}</span>
      <b style="font-size:14px">Nothing remembered yet.</b><span class="dmuted" style="display:block">Beep learns as you chat, or tell it "remember that…"</span></div>`;
  } else {
    let last;
    M.list.forEach((m, i) => {
      const cat = m.category || "Other";
      if (cat !== last) html += `<div class="dsec" style="border:0;padding-left:10px">${esc(cat)}</div>`;
      last = cat;
        const on = i === M.sel, ended = m.valid_until && m.valid_until < today;
        const meta = [MEM_SOURCE[m.source] ?? "", shortDate(m.updated)].filter(Boolean).join(" · ")
          + (m.valid_from || m.valid_until ? ` · ${m.valid_from ? shortDate(m.valid_from) : ""} to ${m.valid_until ? shortDate(m.valid_until) : ""}` : "");
        if (on && M.mode === "edit") {
          html += `<div class="dmem" aria-selected="true" data-i="${i}"><div class="top"><textarea id="medit" rows="2" aria-label="Memory">${esc(m.content)}</textarea>
            <span style="font-size:12px;color:var(--muted);flex:none;display:inline-flex;gap:4px;align-items:center">${ICON.ret} save</span></div><div class="m">${esc(meta)}</div></div>`;
        } else {
          const acts = on ? `<span class="acts"><button class="dbtn" data-m="edit">Edit</button><button class="dbtn warn" data-m="forget">Forget<kbd>⌘⌫</kbd></button></span>` : "";
          html += `<div class="dmem${ended ? " ended" : ""}" role="option" aria-selected="${on}" data-i="${i}"><div class="top"><span class="c">${mark(m.content, M.query)}</span>${acts}</div>
            <div class="m">${esc(meta)}</div>${m.quote ? `<q>${mark(m.quote, M.query)}</q>` : ""}</div>`;
      }
    });
    if (M.all.length && !M.query) html += `<div style="padding:6px 10px 4px"><button class="dbtn warn" data-m="clear">Forget all…</button></div>`;
  }
  list.innerHTML = html;
  list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  const edit = $("#medit");
  if (edit) { edit.focus(); edit.setSelectionRange(edit.value.length, edit.value.length); }
  $("#mhints").innerHTML = M.mode === "edit"
    ? hints([[ICON.ret, "save"], [`⇧${ICON.ret}`, "new line"], ["esc", "cancel"]])
    : hints([["↑↓", "move"], [ICON.ret, "edit"], ["⌘⌫", "forget"], ["esc", "back"]]);
}

function memClick(e) {
  const btn = e.target.closest("[data-m]");
  const row = e.target.closest(".dmem[data-i]");
  if (btn) return memAction(btn.dataset.m);
  if (row && M.mode !== "edit") {
    M.sel = Number(row.dataset.i);
    M.mode = "list";
    drawMemories();
  }
}

async function memAction(a) {
  const m = M.list[M.sel];
  if (a === "list" || a === "clear" || a === "edit") {
    if (a === "edit" && !m) return;
    M.mode = a;
    return drawMemories();
  }
  try {
    if (a === "save" && m) {
      const content = $("#medit").value.trim();
      // Shown at once, so a second Return cannot send it again.
      M.mode = "list";
      if (content && content !== m.content) {
        const old = m.content;
        m.content = content;
        drawMemories();
        try {
          await invoke("update_memory", { id: m.id, content });
        } catch (e) {
          m.content = old;
          throw e;
        }
      }
    } else if (a === "forget" && m) {
      await invoke("delete_memory", { id: m.id });
      M.all = M.all.filter(x => x.id !== m.id);
    } else if (a === "clear-yes") {
      await invoke("clear_memories");
      M.all = [];
    }
    M.error = "";
  } catch (e) {
    M.error = String(e);
  }
  M.mode = "list";
  if (S.view === "memories") drawMemories();
  $("#mfind")?.focus();
}

function memKeys(e, cmd) {
  if (M.mode === "edit") {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); memAction("save"); }
    return;
  }
  if (M.mode === "clear") {
    if (e.key === "Enter") { e.preventDefault(); memAction("clear-yes"); }
    return;
  }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    M.sel = Math.max(0, Math.min(M.list.length - 1, M.sel + (e.key === "ArrowDown" ? 1 : -1)));
    drawMemories();
  } else if (e.key === "Enter" && !e.isComposing && M.list.length) {
    e.preventDefault();
    memAction("edit");
  } else if (cmd && e.key === "Backspace" && M.list.length) {
    e.preventDefault();
    memAction("forget");
  }
}

// ---------- Routines ----------

// mode: list | actions | delete
const R = { list: [], sel: 0, mode: "list", error: "", loaded: false };
// The new routine form. repeat: day | weekdays | week | hour | once | cron
const NR = { label: "", prompt: "", repeat: "day", days: [1], time: "08:00", date: "", cron: "", error: "", busy: false };
const CRON_DAYS = [1, 2, 3, 4, 5, 6, 0];
const DAY_NAMES = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const DAY_FULL = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

const clock = (h, m) => `${h}:${String(m).padStart(2, "0")}`;

/// "today 9:00", "tomorrow 9:00", "Mon 9:00" or, further out, "12 Oct 9:00".
function describeDay(at) {
  const day = d => new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const ahead = Math.round((day(at) - day(new Date())) / 86400000);
  const label = ahead === 0 ? "today" : ahead === 1 ? "tomorrow" : ahead > 1 && ahead < 7 ? DAY_NAMES[at.getDay()] : `${at.getDate()} ${MONTHS[at.getMonth()]}`;
  return `${label} ${clock(at.getHours(), at.getMinutes())}`;
}

/// Reads the cron shapes the form writes as words, the same as the phone. Anything else shows as written.
function routineWhen(s) {
  const place = /^(arrive|leave):[^:]+(?::(daily|once))?$/.exec(s);
  if (place) return (place[1] === "arrive" ? "When you arrive at a saved place" : "When you leave a saved place")
    + ({ daily: " · once a day at most", once: " · next time only" }[place[2]] ?? "");
  if (/^\d{4}-\d\d-\d\dT/.test(s)) {
    const at = new Date(s);
    if (!isNaN(at)) return "Once, " + describeDay(at);
  }
  const p = s.trim().split(/\s+/);
  if (p.length !== 5 || p[2] !== "*" || p[3] !== "*" || !/^\d+$/.test(p[0])) return s;
  const min = Number(p[0]), days = p[4];
  if (p[1] === "*" && days === "*") return min ? `Every hour at :${String(min).padStart(2, "0")}` : "Every hour";
  if (!/^\d+$/.test(p[1])) return s;
  const at = clock(Number(p[1]), min);
  if (days === "*") return `Every day at ${at}`;
  if (days === "1-5") return `Weekdays at ${at}`;
  if (/^[0-6]$/.test(days)) return `Every ${DAY_FULL[days]} at ${at}`;
  if (/^[0-6](,[0-6])*$/.test(days)) return days.split(",").map(d => DAY_NAMES[d]).join(", ") + ` at ${at}`;
  return s;
}

function renderRoutines() {
  S.view = "routines";
  invoke("close_group").catch(() => {});
  Object.assign(R, { mode: "list", error: "" });
  drawRoutines();
  loadRoutines();
}

async function loadRoutines() {
  try {
    R.list = await invoke("list_crons");
    R.loaded = true;
    R.error = "";
  } catch (e) {
    R.error = String(e);
  }
  if (S.view === "routines") drawRoutines();
}

function drawRoutines() {
  R.sel = Math.max(0, Math.min(R.sel, R.list.length - 1));
  const on = R.list.filter(c => c.enabled).length, off = R.list.length - on;
  const sum = !R.loaded ? "" : !R.list.length ? "Nothing scheduled" : [on && `${on} on`, off && `${off} paused`].filter(Boolean).join(", ");
  const head = `<div class="dhead"><span class="tt"><b>Routines</b><small>${sum}</small></span><button class="dbtn" data-r="new">${PLUS_ICON}New<kbd>⌘N</kbd></button></div>`;
  const err = R.error ? `<div class="derr" style="padding:8px 18px 0">${esc(R.error)}</div>` : "";
  if (R.loaded && !R.list.length) {
    bar.innerHTML = head + err + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${CLOCK_ICON}</span>
      <b style="font-size:14px">No routines yet.</b><span class="dmuted" style="display:block">Schedule tasks for Beep to run on a timer.</span><button class="dbtn pri" data-r="new" style="margin-top:4px">New routine</button></div>`
      + hints([["⌘N", "new routine"], ["esc", "back"]]);
  } else {
    const rows = R.list.map((c, i) => {
      const cur = i === R.sel, label = esc(c.label || "Reminder");
      if (cur && R.mode === "delete") {
        return `<div class="drt" aria-selected="true" data-i="${i}"><span class="tt"><span style="font-size:14px">Delete <b>${label}</b>?</span></span>
          <span class="acts"><button class="dbtn warn" data-r="delete-yes">Delete</button><button class="dbtn" data-r="list">Cancel</button></span></div>`;
      }
      const next = c.enabled ? (c.next_run_at && !c.run_once ? "Next: " + describeDay(new Date(c.next_run_at)) : "") : "Paused";
      const tail = cur && R.mode === "actions"
        ? `<span class="acts"><button class="dbtn" data-r="run">${PLAY_ICON}Run now</button><button class="dbtn warn" data-r="delete">Delete<kbd>⌘⌫</kbd></button></span>`
        : `<button class="dswitch" role="switch" aria-checked="${c.enabled}" aria-label="${label} on" data-r="toggle" data-i="${i}"></button>`;
      return `<div class="drt${c.enabled ? "" : " off"}" role="option" aria-selected="${cur}" data-i="${i}"><span class="ic">${CLOCK_ICON}</span>
        <span class="tt"><b>${label}</b><span>${esc(c.prompt)}</span><small>${esc([routineWhen(c.schedule), next].filter(Boolean).join(" · "))}</small></span>${tail}</div>`;
    }).join("");
    bar.innerHTML = head + err + `<div class="dlist" id="rlist" role="listbox" aria-label="Routines" style="padding-top:8px">${rows}</div>`
      + hints(R.mode === "actions" ? [["tab", "hide actions"], ["esc", "back"]] : [["↑↓", "move"], ["space", "on or off"], ["tab", "actions"], ["⌘N", "new routine"], ["esc", "back"]]);
    bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }
  bar.onclick = routineClick;
}

function routineClick(e) {
  if (S.view !== "routines") return;
  const btn = e.target.closest("[data-r]");
  const row = e.target.closest(".drt[data-i]");
  if (btn?.dataset.i) R.sel = Number(btn.dataset.i);
  if (btn) return routineAction(btn.dataset.r);
  if (row) {
    R.sel = Number(row.dataset.i);
    R.mode = "actions";
    drawRoutines();
  }
}

async function routineAction(a) {
  const c = R.list[R.sel];
  if (a === "new") return renderRoutineNew();
  if (a === "list" || a === "actions" || a === "delete") {
    if (!c && a !== "list") return;
    R.mode = a;
    return drawRoutines();
  }
  if (!c) return;
  try {
    if (a === "toggle") {
      c.enabled = await invoke("toggle_cron", { id: c.id });
    } else if (a === "delete-yes") {
      await invoke("delete_cron", { id: c.id });
      R.list = R.list.filter(x => x.id !== c.id);
    } else if (a === "run") {
      const session = await invoke("run_cron", { id: c.id });
      return openSession(session);
    }
    R.error = "";
  } catch (e) {
    R.error = String(e);
  }
  R.mode = "list";
  if (S.view === "routines") drawRoutines();
}

function routineKeys(e, cmd) {
  if (R.mode === "delete") {
    if (e.key === "Enter") { e.preventDefault(); routineAction("delete-yes"); }
    return;
  }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    R.sel = Math.max(0, Math.min(R.list.length - 1, R.sel + (e.key === "ArrowDown" ? 1 : -1)));
    R.mode = "list";
    drawRoutines();
  } else if (e.key === "Tab" && R.list.length) {
    e.preventDefault();
    routineAction(R.mode === "actions" ? "list" : "actions");
  } else if (e.key === " " && R.mode === "list" && R.list.length) {
    e.preventDefault();
    routineAction("toggle");
  } else if (cmd && e.key === "Backspace" && R.list.length) {
    e.preventDefault();
    routineAction("delete");
  } else if (e.key === "Enter" && R.mode === "actions") {
    e.preventDefault();
    routineAction("run");
  }
}

/// The schedule the form describes, or "" when it is not complete.
function routineSchedule() {
  if (NR.repeat === "cron") return NR.cron.trim();
  const [h, m] = NR.time.split(":").map(Number);
  if (isNaN(h) || isNaN(m)) return "";
  if (NR.repeat === "hour") return `${m} * * * *`;
  if (NR.repeat === "day") return `${m} ${h} * * *`;
  if (NR.repeat === "weekdays") return `${m} ${h} * * 1-5`;
  if (NR.repeat === "week") {
    const days = CRON_DAYS.filter(d => NR.days.includes(d));
    return days.length ? `${m} ${h} * * ${days.join(",")}` : "";
  }
  if (!NR.date) return "";
  const at = new Date(`${NR.date}T${NR.time}`);
  return isNaN(at) ? "" : at.toISOString();
}

function renderRoutineNew() {
  S.view = "rnew";
  const today = new Date();
  Object.assign(NR, { label: "", prompt: "", repeat: "day", days: [1], time: "08:00", cron: "", error: "", busy: false,
    date: `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}` });
  drawRoutineNew();
  $("#nrlabel").focus();
}

function drawRoutineNew() {
  const chip = (k, l) => `<button class="dchip" aria-pressed="${NR.repeat === k}" data-rep="${k}">${l}</button>`;
  const days = CRON_DAYS.map(d => `<button class="dchip day" aria-pressed="${NR.days.includes(d)}" data-day="${d}">${DAY_NAMES[d]}</button>`).join("");
  const time = `<label class="dfield" style="max-width:140px"><span>${NR.repeat === "hour" ? "Minute past" : "Time"}</span><input id="nrtime" type="time" value="${esc(NR.time)}"></label>`;
  const when = NR.repeat === "cron"
    ? `<label class="dfield"><span>Cron expression</span><input id="nrcron" class="mono" style="text-transform:none" placeholder="30 7 * * 1-5" value="${esc(NR.cron)}" spellcheck="false"></label>
       <span class="dmuted" id="nrcronsays" style="margin-top:-6px"></span>`
    : NR.repeat === "once"
      ? `<div style="display:grid;grid-template-columns:1fr 1fr;gap:10px"><label class="dfield"><span>Date</span><input id="nrdate" type="date" value="${esc(NR.date)}"></label>${time.replace(' style="max-width:140px"', "")}</div>`
      : (NR.repeat === "week" ? `<div class="dchips">${days}</div>` : "") + time;
  const missing = NR.error === "prompt";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>New routine</b><small>Runs on this account, phone or computer off</small></span></div>
    <div class="dform" style="padding-top:12px;flex:1;min-height:0;overflow:auto;scrollbar-width:none">
      <label class="dfield"><span>Name</span><input id="nrlabel" placeholder="Morning briefing" value="${esc(NR.label)}"></label>
      <label class="dfield${missing ? " err" : ""}"><span>${missing ? "Tell Beep what to do" : "What should Beep do?"}</span>
        <textarea id="nrprompt" rows="2" placeholder="Summarise my calendar and the weather">${esc(NR.prompt)}</textarea></label>
      <div class="dfield"><span>Repeat</span><div class="dchips">${chip("day", "Every day")}${chip("weekdays", "Weekdays")}${chip("week", "Weekly")}${chip("hour", "Every hour")}${chip("once", "Once")}${chip("cron", "Cron")}</div></div>
      ${when}
      ${NR.error && !missing ? `<div class="derr">${esc(NR.error)}</div>` : ""}
      <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri" id="nrgo"${NR.busy ? " disabled" : ""}>Create<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn" id="nrno">Cancel</button>
        <span class="dmuted" style="margin-left:auto">Or just tell Beep in a chat</span></div>
    </div>${hints([["tab", "next field"], [`⌘${ICON.ret}`, "create"], ["esc", "cancel"]])}`;
  const bind = (id, key) => $(id)?.addEventListener("input", e => { NR[key] = e.target.value; if (id === "#nrcron") cronSays(); });
  bind("#nrlabel", "label"); bind("#nrprompt", "prompt"); bind("#nrtime", "time"); bind("#nrdate", "date"); bind("#nrcron", "cron");
  cronSays();
  bar.onclick = e => {
    if (S.view !== "rnew") return;
    const rep = e.target.closest("[data-rep]"), day = e.target.closest("[data-day]");
    if (rep) { NR.repeat = rep.dataset.rep; drawRoutineNew(); }
    else if (day) {
      const d = Number(day.dataset.day);
      NR.days = NR.days.includes(d) ? NR.days.filter(x => x !== d) : [...NR.days, d];
      day.setAttribute("aria-pressed", NR.days.includes(d));
    } else if (e.target.closest("#nrgo")) createRoutine();
    else if (e.target.closest("#nrno")) renderRoutines();
  };
}

function cronSays() {
  const el = $("#nrcronsays");
  if (!el) return;
  const said = NR.cron.trim() && routineWhen(NR.cron.trim());
  el.textContent = said && said !== NR.cron.trim() ? said : "";
}

async function createRoutine() {
  if (NR.busy) return;
  const prompt = NR.prompt.trim(), schedule = routineSchedule();
  NR.error = !prompt ? "prompt" : !schedule ? (NR.repeat === "week" ? "Pick at least one day" : "Set when it runs") : "";
  if (NR.error) return drawRoutineNew();
  NR.busy = true;
  try {
    await invoke("create_cron", { label: NR.label.trim() || prompt.slice(0, 40), prompt, schedule });
    if (S.view === "rnew") renderRoutines();
  } catch (e) {
    NR.error = String(e);
    NR.busy = false;
    if (S.view === "rnew") drawRoutineNew();
  }
}

// ---------- State ----------

async function refreshStatus() {
  const st = await invoke("status");
  S.link = st.link;
  if (S.session === null) S.session = st.session;
  return st;
}

async function show(view) {
  const st = await refreshStatus();
  if (S.link === "linked") loadModels();
  if (S.link === "revoked") return renderRevoked();
  if (view === "settings") return renderSettings();
  if (S.link === "unlinked" || view === "link") return S.link === "linked" ? renderChat() : renderLink();
  if (view === "new") return newChat();
  if (view === "memories") return renderMemories();
  if (view === "routines") return renderRoutines();
  if (view === "skills") return renderSkills();
  if (view === "apps") return renderApps();
  // Back from the browser: look again at what is connected.
  if (S.view === "apps") return loadApps();
  // A chat picked from the tray's Recent menu, already made current in Rust.
  if (view === "open" && st.session) return openSession(st.session);
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

// Only the Option double tap shows the bar without naming a view.
listen("bar-shown", e => { barHeight = 0; if (e.payload == null) learnedHotkey(); show(e.payload); });
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
  if (S.view === "chat" && !here()?.run && e.payload && e.payload === S.session) refreshSession(S.session).then(() => watchRun(e.payload));
  if (listIdle()) loadChats("");
});

// ---------- Skills ----------

// How the user, or a group, likes a task done. Beep writes them on its own;
// here they can be read, edited, paused, undone and deleted.
// mode: list | actions | delete | view | edit | vdelete
const K = { list: [], sel: 0, mode: "list", error: "", loaded: false };
const SKILL_ICON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 19l9-9"/><path d="M14 4l1.2 2.8L18 8l-2.8 1.2L14 12l-1.2-2.8L10 8l2.8-1.2z"/><path d="M19 13l.6 1.4L21 15l-1.4.6L19 17l-.6-1.4L17 15l1.4-.6z"/></svg>';

/// "Used today", "Used yesterday" or "Used 8 Oct".
function skillUsed(iso) {
  if (!iso) return "Not used yet";
  const at = new Date(iso);
  if (isNaN(at)) return "";
  const day = d => new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const ago = Math.round((day(new Date()) - day(at)) / 86400000);
  return "Used " + (ago <= 0 ? "today" : ago === 1 ? "yesterday" : `${at.getDate()} ${MONTHS[at.getMonth()]}`);
}
function skillMeta(k) {
  if (!k.enabled) return "Paused";
  const uses = k.uses === 1 ? "1 use" : `${k.uses} uses`;
  return [skillUsed(k.last_used), uses, k.edited && "edited by you"].filter(Boolean).join(" · ");
}

function renderSkills() {
  S.view = "skills";
  invoke("close_group").catch(() => {});
  Object.assign(K, { mode: "list", error: "" });
  drawSkills();
  loadSkills();
}

async function loadSkills() {
  try {
    K.list = await invoke("list_skills");
    K.loaded = true;
    K.error = "";
  } catch (e) {
    K.error = String(e);
  }
  if (S.view === "skills" && (K.mode === "list" || K.mode === "actions")) drawSkills();
}

function drawSkills() {
  K.sel = Math.max(0, Math.min(K.sel, K.list.length - 1));
  if (K.mode === "view" || K.mode === "edit" || K.mode === "vdelete") return drawSkill();
  const paused = K.list.filter(k => !k.enabled).length;
  const sum = !K.loaded ? "" : !K.list.length ? "None yet" : [`${K.list.length} ${K.list.length === 1 ? "skill" : "skills"}`, paused && `${paused} paused`].filter(Boolean).join(", ");
  const head = `<div class="dhead"><span class="tt"><b>Skills</b><small>${sum}</small></span></div>`;
  const err = K.error ? `<div class="derr" style="padding:8px 18px 0">${esc(K.error)}</div>` : "";
  if (K.loaded && !K.list.length) {
    bar.innerHTML = head + err + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${SKILL_ICON}</span>
      <b style="font-size:14px">No skills yet.</b><span class="dmuted" style="display:block;max-width:44ch">When Beep works out how you like something done, like booking a court or a weekly summary, it writes it down here and does it your way next time.</span></div>`
      + hints([["esc", "back"]]);
  } else {
    let owner, html = "";
    K.list.forEach((k, i) => {
      if (k.group_id !== owner || i === 0) {
        owner = k.group_id;
        html += `${i ? "</div>" : ""}<div class="dsec flat" style="padding:8px 10px 4px">${esc(k.group_name || (k.group_id ? "Group" : "Yours"))}</div><div class="dlist" style="padding:0">`;
      }
      const cur = i === K.sel, name = esc(k.name);
      if (cur && K.mode === "delete") {
        html += `<div class="drt" aria-selected="true" data-i="${i}"><span class="tt"><span style="font-size:14px">Delete <b>${name}</b>?</span></span>
          <span class="acts"><button class="dbtn warn" data-k="delete-yes">Delete</button><button class="dbtn" data-k="list">Cancel</button></span></div>`;
        return;
      }
      const tail = cur && K.mode === "actions"
        ? `<span class="acts"><button class="dbtn" data-k="view">Open<kbd>${ICON.ret}</kbd></button>${k.can_undo ? `<button class="dbtn" data-k="undo">Undo<kbd>⌘Z</kbd></button>` : ""}<button class="dbtn warn" data-k="delete">Delete<kbd>⌘⌫</kbd></button></span>`
        : `<button class="dswitch" role="switch" aria-checked="${k.enabled}" aria-label="Use ${name}" data-k="toggle" data-i="${i}"></button>`;
      html += `<div class="drt${k.enabled ? "" : " off"}" role="option" aria-selected="${cur}" data-i="${i}" style="cursor:pointer"><span class="ic">${SKILL_ICON}</span>
        <span class="tt"><b>${name}</b><span>${esc(k.description)}</span><small>${esc(skillMeta(k))}</small></span>${tail}</div>`;
    });
    bar.innerHTML = head + err + `<div class="dlist" id="klist" role="listbox" aria-label="Skills" style="padding-top:4px;overflow:auto;scrollbar-width:none">${html}</div></div>`
      + hints(K.mode === "actions" ? [[ICON.ret, "open"], ["⌘Z", "undo"], ["⌘⌫", "delete"], ["tab", "hide actions"], ["esc", "back"]]
        : [["↑↓", "move"], [ICON.ret, "open"], ["space", "on or off"], ["tab", "actions"], ["esc", "back"]]);
    bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }
  bar.onclick = skillClick;
}

/// One skill: its instructions, or the editor.
function drawSkill() {
  const k = K.list[K.sel];
  if (!k) { K.mode = "list"; return drawSkills(); }
  const err = K.error ? `<div class="derr" style="padding:8px 18px 0">${esc(K.error)}</div>` : "";
  if (K.mode === "edit") {
    bar.innerHTML = `<div class="dhead"><span class="tt"><b>Edit skill</b><small>Beep keeps the old version, so you can undo</small></span></div>${err}
      <div class="dform" style="padding-top:12px;overflow:auto;scrollbar-width:none">
        <label class="dfield"><span>Name</span><input id="kname" maxlength="60" value="${esc(k.name)}" spellcheck="false"></label>
        <label class="dfield"><span>When it applies</span><input id="kdesc" maxlength="200" value="${esc(k.description)}"></label>
        <label class="dfield"><span>Instructions</span><textarea id="kbody" rows="12" style="font-size:13px;line-height:19px">${esc(k.body)}</textarea></label>
        <div style="display:flex;gap:8px"><button class="dbtn pri" data-k="save">Save<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn" data-k="view">Cancel</button></div>
      </div>${hints([["tab", "next field"], [`⌘${ICON.ret}`, "save"], ["esc", "cancel"]])}`;
    bar.onclick = skillClick;
    return $("#kbody")?.focus();
  }
  const confirm = K.mode === "vdelete" ? `<div class="dcard" style="margin:12px 16px 0"><div style="padding:10px 12px;display:grid;gap:8px">
      <b style="font-size:13.5px">Delete this skill?</b><div class="dmuted" style="display:block">Beep forgets how you like "${esc(k.name)}" done. It may learn it again if you keep asking for it.</div>
      <div style="display:flex;gap:8px"><button class="dbtn warn" data-k="delete-yes">Delete<kbd>${ICON.ret}</kbd></button><button class="dbtn" data-k="view">Cancel</button></div></div></div>` : "";
  const undo = k.can_undo ? `<div class="dmuted" style="border-top:1px solid var(--border);padding-top:10px"><span style="flex:1">${k.edited ? "You changed this last." : "Beep changed this last."}</span><button class="dbtn" data-k="undo">Undo<kbd>⌘Z</kbd></button></div>` : "";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>${esc(k.name)}</b><small>${esc(k.group_name ? `${k.group_name} · ${k.description}` : k.description)}</small></span>
      <button class="dswitch" role="switch" aria-checked="${k.enabled}" aria-label="Use this skill" data-k="toggle"></button>
      <button class="dbtn" data-k="edit">Edit<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn warn" data-k="vdelete">Delete<kbd>⌘⌫</kbd></button></div>${err}${confirm}
    <div class="dbody" style="padding-top:12px"><div class="dmuted">${esc(skillMeta(k))}</div><div class="reply">${k.html}</div>${undo}</div>`
    + hints(K.mode === "vdelete" ? [[ICON.ret, "delete"], ["esc", "cancel"]] : [["space", "on or off"], [`⌘${ICON.ret}`, "edit"], ...(k.can_undo ? [["⌘Z", "undo"]] : []), ["esc", "back"]]);
  bar.onclick = skillClick;
}

function skillClick(e) {
  if (S.view !== "skills") return;
  const btn = e.target.closest("[data-k]");
  const row = e.target.closest(".drt[data-i]");
  if (btn?.dataset.i) K.sel = Number(btn.dataset.i);
  if (btn) return skillAction(btn.dataset.k);
  if (row && K.mode !== "delete") {
    K.sel = Number(row.dataset.i);
    skillAction("view");
  }
}

/// Replaces the skill at `K.sel` with what the server sent back.
function putSkill(k) { K.list[K.sel] = k; }

async function skillAction(a) {
  const k = K.list[K.sel];
  if (a === "list" || a === "actions" || a === "delete" || a === "view" || a === "edit" || a === "vdelete") {
    if (!k && a !== "list") return;
    K.mode = a;
    K.error = "";
    return drawSkills();
  }
  if (!k) return;
  const inView = K.mode === "view" || K.mode === "edit" || K.mode === "vdelete";
  try {
    if (a === "toggle") {
      putSkill(await invoke("update_skill", { id: k.id, change: { enabled: !k.enabled } }));
    } else if (a === "undo") {
      if (!k.can_undo) return;
      putSkill(await invoke("undo_skill", { id: k.id }));
    } else if (a === "save") {
      const name = $("#kname").value.trim(), description = $("#kdesc").value.trim(), body = $("#kbody").value.trim();
      if (!name || !body) { K.error = name ? "Write what Beep should do." : "Give the skill a name."; return drawSkill(); }
      const change = {};
      if (name !== k.name) change.name = name;
      if (description !== k.description) change.description = description;
      if (body !== k.body) change.body = body;
      if (Object.keys(change).length) putSkill(await invoke("update_skill", { id: k.id, change }));
    } else if (a === "delete-yes") {
      await invoke("delete_skill", { id: k.id });
      K.list = K.list.filter(x => x.id !== k.id);
      K.mode = "list";
      K.error = "";
      return S.view === "skills" && drawSkills();
    }
    K.error = "";
  } catch (e) {
    K.error = String(e);
  }
  K.mode = inView ? "view" : "list";
  if (S.view === "skills") drawSkills();
}

function skillKeys(e, cmd) {
  const key = e.key.toLowerCase();
  if (K.mode === "edit") {
    if (cmd && e.key === "Enter") { e.preventDefault(); skillAction("save"); }
    return;
  }
  if (K.mode === "delete" || K.mode === "vdelete") {
    if (e.key === "Enter") { e.preventDefault(); skillAction("delete-yes"); }
    return;
  }
  if (!K.list.length) return;
  if (cmd && key === "z") { e.preventDefault(); return skillAction("undo"); }
  if (cmd && e.key === "Backspace") { e.preventDefault(); return skillAction(K.mode === "view" ? "vdelete" : "delete"); }
  if (e.key === " ") { e.preventDefault(); return skillAction("toggle"); }
  if (K.mode === "view") {
    if (cmd && e.key === "Enter") { e.preventDefault(); skillAction("edit"); }
    return;
  }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    K.sel = Math.max(0, Math.min(K.list.length - 1, K.sel + (e.key === "ArrowDown" ? 1 : -1)));
    K.mode = "list";
    drawSkills();
  } else if (e.key === "Tab") {
    e.preventDefault();
    skillAction(K.mode === "actions" ? "list" : "actions");
  } else if (e.key === "Enter") {
    e.preventDefault();
    skillAction("view");
  }
}

// ---------- Keys ----------

function back() {
  if (S.view === "chats") {
    if (S.chats.mode !== "list") return chatMode("list");
    if (S.chats.back) return renderGroup(S.chats.back);
    return S.session ? openSession(S.session) : renderChat();
  }
  if (S.view === "group" && G.mention) return closeMention();
  if (S.view === "memories" && M.mode !== "list") { M.mode = "list"; drawMemories(); return $("#mfind")?.focus(); }
  if (S.view === "routines" && R.mode !== "list") return routineAction("list");
  if (S.view === "skills" && K.mode !== "list") return skillAction(K.mode === "edit" || K.mode === "vdelete" ? "view" : "list");
  if (S.view === "rnew") return renderRoutines();
  if (S.view === "appperms") return renderApps();
  if (S.view === "apps" && A.adding) { A.adding = false; A.addError = ""; return drawApps(); }
  if (S.view === "apps") return renderSettings();
  if (S.view === "model") return S.session ? openSession(S.session) : renderChat();
  if ((S.view === "memories" || S.view === "routines" || S.view === "skills" || S.view === "settings") && S.link === "linked") return S.session ? openSession(S.session) : renderChat();
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
  if ($("#viewer")) return viewerKeys(e, cmd);
  if (cmd && e.key === "/") { e.preventDefault(); return toggleKeys(); }
  if (e.key === "Escape" && $("#keys")) { e.preventDefault(); return toggleKeys(); }
  if (e.key === "Escape" && closeRerunMenu()) { e.preventDefault(); return; }
  if (e.key === "Escape") {
    e.preventDefault();
    return back();
  }
  if (S.view === "link" || S.view === "revoked") return;
  const key = e.key.toLowerCase();
  if (cmd && e.key === ",") { e.preventDefault(); return renderSettings(); }
  if (cmd && key === "q") { e.preventDefault(); return invoke("quit"); }
  if (cmd && key === "m") { e.preventDefault(); return renderMemories(); }
  if (cmd && key === "j") { e.preventDefault(); return renderRoutines(); }
  if (cmd && key === "l") { e.preventDefault(); return renderSkills(); }
  if (cmd && e.key === "Enter" && S.view === "rnew") { e.preventDefault(); return createRoutine(); }
  if (cmd && key === "n" && S.view === "routines") { e.preventDefault(); return renderRoutineNew(); }
  if (cmd && key === "n") { e.preventDefault(); return newChat(); }
  if (cmd && key === "g") { e.preventDefault(); J.error = ""; J.name = ""; return renderGroupNew("new"); }
  if (cmd && key === "k") { e.preventDefault(); return S.view === "chats" ? null : renderChats(); }
  if (cmd && key === "i" && (S.view === "group" || S.view === "invite")) { e.preventDefault(); return renderGroupInfo(); }
  if (cmd && e.key === ".") { e.preventDefault(); return stop(); }
  if (cmd && key === "e" && isAdmin()) { e.preventDefault(); return S.view === "model" ? back() : renderModel(); }
  if (S.view === "model") return modelKeys(e);
  if (S.view === "chat") {
    if (cmd && e.key === "Enter" && here()?.approval) { e.preventDefault(); return answer(e.altKey ? "always" : "allow"); }
    if (cmd && key === "d" && here()?.approval) { e.preventDefault(); return answer("deny"); }
    if (cmd && e.key === "Enter" && queued.get(S.session)?.held && !here()) { e.preventDefault(); return sendQueued(S.session); }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.target.id === "ask") { e.preventDefault(); return send(e.target.value); }
    if (e.key === "ArrowUp" && e.target.id === "ask" && !e.target.value) { e.preventDefault(); return renderChats(); }
    if (e.key === "Backspace" && !cmd && e.target.id === "ask" && !e.target.value && attached.pics.length) { e.preventDefault(); return unattach(attached.pics.length - 1); }
  }
  if (S.view === "chats") return chatsKeys(e, cmd);
  if (S.view === "memories") return memKeys(e, cmd);
  if (S.view === "routines") return routineKeys(e, cmd);
  if (S.view === "skills") return skillKeys(e, cmd);
  if (S.view === "group") return groupKeys(e, cmd);
  if (S.view === "gnew" && e.key === "Enter" && !e.isComposing && e.target.tagName === "INPUT") { e.preventDefault(); return groupFormGo(); }
  if (S.view === "apps" && e.key === "Enter" && !e.isComposing && (e.target.id === "mname" || e.target.id === "murl")) {
    e.preventDefault();
    return e.target.id === "mname" && !$("#murl").value.trim() ? $("#murl").focus() : addServer();
  }
});

// Links in replies open in the browser. A product card's corner button
// copies its link instead.
bar.addEventListener("click", e => {
  if (e.target.closest(".dhints .st")) return S.link === "revoked" ? renderRevoked() : renderLink();
  if (e.target.closest('[data-act="model"]')) return renderModel();
  if (S.view === "model") return modelClick(e);
  if (rerunClick(e)) return;
  if (e.target.closest(".dchart [data-series], .dchart [data-range]")) return void chartClick(e);
  if (e.target.closest(".droute [data-route], .droute [data-maps], .droute [data-stops]")) return void routeClick(e);
  const menuButton = e.target.closest('[data-act="rerunMenu"]');
  if (menuButton) return rerunMenu(menuButton);
  if (!e.target.closest(".dmenu")) closeRerunMenu();
  const copyReply = e.target.closest('[data-act="copyReply"]');
  if (copyReply) {
    copy(copyReply.closest(".turn").querySelector(".reply")?.innerText.trim() ?? "");
    copyReply.innerHTML = ICON.check + "Copied";
    clearTimeout(copyReply.undo);
    copyReply.undo = setTimeout(() => { copyReply.innerHTML = COPY_ICON + "Copy"; }, 1200);
    return;
  }
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
bar.addEventListener("load", e => { if (e.target.matches?.(".dprod-img img, .drec img")) e.target.classList.add("on"); }, true);
bar.addEventListener("error", e => { if (e.target.matches?.(".dprod-img img, .drec img")) e.target.remove(); }, true);

show(null);
