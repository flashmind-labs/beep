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
  mac_files: "Using files on this Mac",
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
  // One tool per family with an action argument. While it runs the server's
  // own description names the action; a saved step carries a label read from
  // its result, and these cover the rest.
  memory: "Checking memories",
  skill: "Loading a skill",
  habit: "Checking habits",
  background: "Checking background work",
  reminder: "Setting a reminder",
  node: "Using your machines",
  // Commands stay out of the chat: every one reads the same.
  exec: "Running code",
  process: "Running code",
  desktop: "Using the Mac",
  code_agent: "Using a coding agent",
  get_route: "Planning a route",
  send_image: "Sending pictures",
  chat_search: "Searching this chat",
  chat_read: "Reading this chat",
  place_save: "Saving a place",
  place_list: "Checking your places",
  place_delete: "Forgetting a place",
  place_routine: "Setting up a place routine",
  event_topics: "Checking which updates you get",
  hold_events: "Holding updates for later",
  recent_events: "Checking recent updates",
  execute_code: "Running code",
  download_file: "Downloading a file",
  web_scrape: "Reading a page",
  web_extract: "Reading a page",
  web_map: "Mapping a site",
  web_crawl: "Reading a site",
  web_search_read: "Reading search results",
  start_research: "Starting research",
  research_more: "Asking the research for more",
  stop_research: "Stopping the research",
  end_routine: "Ending this routine",
  request_grant: "Asking to allow a routine",
  // Names from before the fold, still in older chats.
  memory_history: "Checking how a memory changed",
  spawn_agent: "Starting an agent",
  message_agent: "Messaging an agent",
  run_in_background: "Moving work to the background",
  jobs: "Checking background work",
  wait_job: "Waiting for background work",
  cancel_job: "Stopping background work",
};
// Tools whose own description says more than their name, such as "Reading
// lease.pdf" or which action of a folded tool runs.
const DESCRIBED = new Set([
  "mac_files", "memory", "skill", "habit", "background", "reminder", "node",
  "desktop", "code_agent", "send_file",
]);
/// A finished step's label in the past tense, as on the phone: "Recalling
/// memories" becomes "Recalled memories". Unknown verbs stay as they are.
const PAST = {
  searching: "Searched", reading: "Read", checking: "Checked", looking: "Looked", fetching: "Fetched",
  opening: "Opened", saving: "Saved", finding: "Found", getting: "Got", creating: "Created",
  updating: "Updated", deleting: "Deleted", sending: "Sent", setting: "Set", using: "Used",
  remembering: "Remembered", loading: "Loaded", browsing: "Browsed", scheduling: "Scheduled",
  running: "Ran", weighing: "Weighed", recalling: "Recalled", forgetting: "Forgot", changing: "Changed",
  logging: "Logged", suggesting: "Suggested", showing: "Showed", starting: "Started", messaging: "Messaged",
  moving: "Moved", waiting: "Waited", stopping: "Stopped", holding: "Held", planning: "Planned",
  drawing: "Drew", downloading: "Downloaded", mapping: "Mapped", asking: "Asked", ending: "Ended",
  following: "Followed", unfollowing: "Unfollowed", taking: "Took", answering: "Answered",
  clicking: "Clicked", typing: "Typed", pressing: "Pressed", scrolling: "Scrolled", writing: "Wrote",
  linking: "Linked", copying: "Copied", typesetting: "Typeset",
};
function doneLabel(label) {
  const text = String(label).replace(/[.\u2026\s]+$/, "");
  const head = text.split(" ", 1)[0];
  const past = PAST[head.toLowerCase()];
  return past ? past + text.slice(head.length) : text;
}
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
/// True when this page runs as a pane in the command center window rather
/// than as the bar. A few bar-only spots check it.
const PANE = bar.dataset.mode === "window";
/// Told when the view or the open chat changes, for the window's sidebar.
const changeListeners = [];
let changeQueued = false;
function changed() {
  if (changeQueued || !changeListeners.length) return;
  changeQueued = true;
  queueMicrotask(() => {
    changeQueued = false;
    const now = { view: S.view, session: S.session, group: GROUP_VIEWS.has(S.view) ? G.id : null };
    for (const fn of changeListeners) { try { fn(now); } catch {} }
  });
}
const S = new Proxy({
  view: "chat", // chat | chats | group | ginfo | invite | gnew | link | revoked | settings | memories | routines | rnew | skills | model
  link: "unlinked",
  session: null,
  chats: { list: [], sel: 0, query: "" },
  cache: [], // the last chat list, drawn at once while a fresh one loads
  offer: null,
  timer: null,
}, {
  set(t, k, v) {
    const old = t[k];
    t[k] = v;
    if ((k === "view" || k === "session") && old !== v) changed();
    return true;
  },
});

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
    if (!PANE && height !== barHeight) {
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
  items = forPane(items);
  const st = { unlinked: ["off", "Not linked"], revoked: ["bad", "Removed"] }[S.link];
  const link = st ? `<button class="st"><span class="ddot ${st[0]}"></span>${st[1]}</button>` : "";
  // The first two say the most; the rest give way when the row is narrow.
  // The bar's chat opens in the window from here, before the model.
  const win = !PANE && S.view === "chat" ? '<button class="win" data-act="openWindow"><kbd>⌘O</kbd>open in window</button>' : "";
  return `<div class="dhints">${items.map(([k, t], i) => `<span${i >= 2 ? ' class="lo"' : ""}><kbd>${k}</kbd>${t}</span>`).join("")}${win}${extra}${link}</div>`;
}
/// The window does not hide and has no live mode, and ⌘K searches there.
function forPane(items) {
  if (!PANE) return items;
  return items
    .filter(([k, t]) => t !== "to hide" && t !== "live" && t !== "talk")
    .map(([k, t]) => k === "⌘K" ? [k, "search"] : [k, t]);
}
function chatHints() {
  const e = here();
  const chip = modelChip();
  const asking = e?.approval ?? (!e?.run && waitingQuestion(S.session));
  if (asking) {
    const grant = e?.approval ? e.approval.grant : forRoutine(asking.action);
    return hints([[`⌘${ICON.ret}`, "allow"], ...(grant ? [] : [[`⌥⌘${ICON.ret}`, "always"]]), ["⌘D", "deny"], ["esc", "to hide"]], chip);
  }
  if (e?.run) return hints([[ICON.ret, "queue"], ["⌘.", "stop"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  if (queued.get(S.session)?.held) return hints([[`⌘${ICON.ret}`, "send queued"], ["⌘N", "new chat"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  if (attachedCount()) return hints([[ICON.ret, "send"], ["⌘V", "paste"], ["⌫", "remove last"], ["esc", "to hide"]], chip);
  if ($("#ask")?.value.includes("\n")) return hints([[ICON.ret, "send"], [`⇧${ICON.ret}`, "new line"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  if ($("#ask")?.value.trim()) return hints([[ICON.ret, "ask"], ["⌘R", "research"], ["esc", "to hide"]], chip);
  if (PANE) return hints([["⌘K", "search"], ["⌘N", "new chat"], [`⇧${ICON.ret}`, "new line"], ["⌥ ⌥", "quick chat"]], chip);
  if (!knowsHotkey()) return hints([["⌥ ⌥", "open from anywhere"], ["⌘N", "new chat"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  return hints([["⌘N", "new chat"], ["⌘K", "chats"], ["tap ⌥", "live"], ["hold ⌥", "talk"], ["⌘/", "shortcuts"], ["esc", "to hide"]], chip);
}
// Shown from first launch until the bar is first opened with a double tap of Option.
const KNOWS_HOTKEY = "beep.knowsHotkey";
function knowsHotkey() { try { return localStorage.getItem(KNOWS_HOTKEY) === "1"; } catch { return false; } }
function learnedHotkey() { try { localStorage.setItem(KNOWS_HOTKEY, "1"); } catch {} }
function refreshHints() {
  const old = bar.querySelector(":scope > .dhints");
  if (!old || S.view !== "chat") return;
  old.replaceWith(h(LV.on && LV.view && $("#ldock") ? hints(liveParts(LV.view).keys) : chatHints()));
  drawStop();
}
/// The Stop button beside the prompt while a reply runs, as ⌘. does.
function drawStop() {
  const row = bar.querySelector(":scope > .dinput.dgrow"), btn = $("#stopRun");
  const running = S.view === "chat" && !!here()?.run;
  if (!row || running === !!btn) return;
  if (!running) return btn.remove();
  const stopBtn = h(`<button class="dbtn" id="stopRun" aria-label="Stop the reply"><span style="width:9px;height:9px;border-radius:2px;background:currentColor"></span>Stop<kbd>⌘.</kbd></button>`);
  stopBtn.onclick = () => { stop(); focusAsk(); };
  row.insertBefore(stopBtn, $("#grip"));
}

// ---------- Chat ----------

function renderChat() {
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
const ASK_HEIGHT = "bar.askHeight", ASK_MAX = 420;
/// A height the user dragged the bar's prompt to; null lets it follow its lines.
let askHeight = null;
try { askHeight = Number(localStorage.getItem(ASK_HEIGHT)) || null; } catch {}
/// The prompt grows with its lines, up to six, then scrolls. A height the
/// user dragged it to is the least it shows, so its lines still fit.
function grow(ask) {
  ask.style.height = "auto";
  const lines = Math.min(ask.scrollHeight, 156);
  const dragged = askHeight && ask.id === "ask" ? Math.min(askHeight, ASK_MAX) : 0;
  ask.style.height = Math.max(lines, dragged) + "px";
}
/// Dragging the grip under the prompt sets its height, kept for next time.
function dragGrip(ask) {
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

/// Past this many prompts the map hides until the chat is hovered.
const MAP_QUIET_AFTER = 12;

/// A tick per prompt on the right edge once the chat scrolls: hover one to read
/// the prompt, click it to jump there. Ticks are only rebuilt when the prompts
/// change, since fit() calls this from the bar's MutationObserver.
function drawMap() {
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
function markMap() {
  const b = body(), map = $("#map");
  if (!b || !map?.turns) return;
  let cur = 0;
  map.turns.forEach((t, i) => { if (t.offsetTop - b.offsetTop <= b.scrollTop + b.clientHeight / 3) cur = i; });
  [...map.children].forEach((tk, i) => tk.classList.toggle("on", i === cur));
}

// ---------- Research ----------

/// Chats started as researches, which open in their own window.
const RESEARCHES = new Set();
async function loadResearches() {
  const ids = await invoke("research_sessions").catch(() => []);
  for (const id of ids) RESEARCHES.add(id);
}

/// Starts a research on `topic` in its own window.
async function research(topic) {
  topic = topic.trim();
  if (!topic) return;
  try {
    const id = await invoke("research_begin", { topic });
    RESEARCHES.add(id);
    const ask = $("#ask");
    if (ask && ask.value.trim() === topic) { ask.value = ""; grow(ask); refreshHints(); }
  } catch (err) {
    if (S.view === "research") { RL.error = String(err); return drawResearches(); }
    failed(newTurn(""), String(err));
  }
}

/// The bar's Research view: every research on the account, newest first,
/// from the server, so ones started on another computer show too. The field
/// on top starts a new one; Return on a row opens its window.
const RL = { list: [], loaded: false, sel: 0, mode: "list", error: "" };
const RSTATUS = {
  planning: "Waiting for your go", researching: "Researching", reviewing: "Reviewing",
  writing: "Writing", done: "Done", failed: "Failed", stopped: "Stopped", interrupted: "Interrupted",
};
const RUNNING = new Set(["researching", "reviewing", "writing"]);

function renderResearches(topic = "") {
  S.view = "research";
  invoke("close_group").catch(() => {});
  Object.assign(RL, { mode: "list", error: "", sel: 0 });
  drawResearches();
  const field = $("#rtopic");
  if (field) { field.value = topic; field.focus(); }
  loadResearchList();
}

async function loadResearchList() {
  try {
    RL.list = await invoke("list_researches");
    for (const r of RL.list) RESEARCHES.add(r.session);
    RL.loaded = true;
    RL.error = "";
  } catch (e) {
    RL.error = String(e);
  }
  if (S.view === "research") drawResearches();
}

function researchLine(r) {
  const parts = [RSTATUS[r.status] ?? r.status];
  if (r.round > 1) parts.unshift(`Round ${r.round}`);
  if (r.sources) parts.push(`${r.sources} source${r.sources === 1 ? "" : "s"}`);
  parts.push(when(r.finished_at ?? r.started_at));
  return parts.join(" · ");
}

function drawResearches() {
  const typed = $("#rtopic")?.value ?? "";
  const focused = document.activeElement?.id === "rtopic";
  RL.sel = Math.max(0, Math.min(RL.sel, RL.list.length - 1));
  const running = RL.list.filter(r => RUNNING.has(r.status)).length;
  const waiting = RL.list.filter(r => r.status === "planning").length;
  const sum = !RL.loaded ? "" : !RL.list.length ? "Nothing researched yet"
    : [running && `${running} running`, waiting && `${waiting} waiting`].filter(Boolean).join(", ") || `${RL.list.length} researched`;
  const head = `<div class="dhead"><span class="tt"><b>Research</b><small>${esc(sum)}</small></span></div>`;
  const field = `<div class="dinput"><input id="rtopic" placeholder="What should the team research?" autocomplete="off" spellcheck="false"><kbd>⌘R</kbd></div>`;
  const err = RL.error ? `<div class="derr" style="padding:8px 18px 0">${esc(RL.error)}</div>` : "";
  if (RL.loaded && !RL.list.length) {
    bar.innerHTML = field + head + err + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${ICON.browser}</span>
      <b style="font-size:14px">No research yet.</b><span class="dmuted" style="display:block">A team of agents digs into a question and writes it up as a paper.</span></div>`
      + hints([[ICON.ret, "research"], ["esc", "back"]]);
  } else {
    const rows = RL.list.map((r, i) => {
      const cur = i === RL.sel && !typed.trim();
      const ic = RUNNING.has(r.status) ? '<span class="dspin"></span>'
        : r.status === "planning" ? '<span class="ddot" style="background:var(--coral)"></span>'
        : r.status === "done" ? `<span class="dok">${ICON.check}</span>` : ICON.doc;
      const tail = cur && RL.mode === "actions" && r.sources
        ? '<span class="acts"><button class="dbtn" data-rl="pdf">Export PDF</button></span>' : "";
      const off = ["failed", "stopped", "interrupted"].includes(r.status);
      return `<div class="drt${off ? " off" : ""}" role="option" aria-selected="${cur}" data-i="${i}"><span class="ic">${ic}</span>
        <span class="tt"><b>${esc(r.topic)}</b><small>${esc(researchLine(r))}</small></span>${tail}</div>`;
    }).join("");
    const keys = typed.trim() ? [[ICON.ret, "research"], ["esc", "back"]]
      : RL.mode === "actions" ? [["tab", "hide actions"], ["esc", "back"]]
      : [["↑↓", "move"], [ICON.ret, "open"], ["tab", "actions"], ["esc", "back"]];
    bar.innerHTML = field + head + err + `<div class="dlist" role="listbox" aria-label="Research" style="padding-top:8px">${rows}</div>` + hints(keys);
    bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }
  const input = $("#rtopic");
  input.value = typed;
  if (focused || !RL.list.length) input.focus();
  input.oninput = () => { const at = input.selectionStart; drawResearches(); $("#rtopic").setSelectionRange(at, at); };
  bar.onclick = researchClick;
}

function researchClick(e) {
  if (S.view !== "research") return;
  const btn = e.target.closest("[data-rl]");
  const row = e.target.closest(".drt[data-i]");
  if (row) RL.sel = Number(row.dataset.i);
  if (btn) return researchListAction(btn.dataset.rl);
  if (row) return researchListAction("open");
}

async function researchListAction(a) {
  const r = RL.list[RL.sel];
  if (!r) return;
  try {
    if (a === "open") return await invoke("open_research", { session: r.session });
    if (a === "pdf") {
      const to = await invoke("export_research", { session: r.session });
      RL.error = "";
      RL.mode = "list";
      drawResearches();
      return notice(`Saved ${to.split("/").pop()} to Downloads`, { path: to });
    }
  } catch (e) {
    RL.error = String(e);
    drawResearches();
  }
}

/// Cmd-R: researches what is typed, and otherwise opens the Research view.
function researchShortcut() {
  const typed = (S.view === "chat" ? $("#ask") : S.view === "research" ? $("#rtopic") : null)?.value.trim();
  if (typed) return research(typed);
  if (S.view !== "research") return renderResearches();
}

function researchKeys(e) {
  const typed = $("#rtopic")?.value.trim();
  if (e.key === "Enter" && !e.isComposing) {
    e.preventDefault();
    return typed ? research(typed) : researchListAction("open");
  }
  if (typed) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    RL.sel = Math.max(0, Math.min(RL.list.length - 1, RL.sel + (e.key === "ArrowDown" ? 1 : -1)));
    RL.mode = "list";
    drawResearches();
  } else if (e.key === "Tab") {
    e.preventDefault();
    RL.mode = RL.mode === "actions" ? "list" : "actions";
    drawResearches();
  }
}

function newTurn(question, images, files) {
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
function ranOn(turn, model, thinking) {
  if (!model) return;
  const line = canPickModel()
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
    const ask = turn.querySelector(".dachips") ?? turn.querySelector(".dpics") ?? turn.querySelector(".ask");
    ask ? ask.after(row) : turn.prepend(row);
  }
  row.querySelector(".think").textContent += delta;
}

/// A routine asking for a grant (`request_grant`). A grant always ends, so
/// its card has no Always, and the Always key allows it.
const forRoutine = action => /for a routine$/.test(action ?? "");

function approvalCard(id, turn, { query_id, app, action, fields }) {
  const grant = forRoutine(action);
  const card = h(`<div class="dcard">
    <div class="head"><span class="dapp">${esc((app || "?")[0].toUpperCase())}</span><div style="flex:1;min-width:0;display:grid"><b>${esc(action)}?</b><small>${esc(app)} asks first</small></div></div>
    ${fields.length ? `<div class="kv">${fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-d="allow">Allow<kbd>⌘${ICON.ret}</kbd></button>${grant ? "" : `<button class="dbtn" data-d="always">Always<kbd>⌥⌘${ICON.ret}</kbd></button>`}<button class="dbtn warn" data-d="deny">Deny<kbd>⌘D</kbd></button></div>
  </div>`);
  card.querySelectorAll("[data-d]").forEach(b => b.addEventListener("click", () => answer(b.dataset.d)));
  turn.append(card);
  const e = entry(id);
  e.approval = { queryId: query_id, turn, card, grant };
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

async function send(message) {
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

/// Takes chat `id`'s approval card away, saying how it was answered, and
/// readies its turn for the run the answer resumes.
function settle(id, e, pending, decision) {
  e.approval = null;
  const said = pending.grant
    ? { allow: "Allowed for this routine", deny: "Denied" }[decision]
    : { allow: "Allowed once", always: "Allowed, and always allowed from now on", deny: "Denied" }[decision];
  pending.card.replaceWith(h(`<div class="dmuted">${said}</div>`));
  const turn = pending.turn;
  turn.done = false;
  e.run = { id: null, turn };
  if (id === S.session) refreshHints();
  return turn;
}

async function answer(decision) {
  const id = S.session, e = live.get(id), pending = e?.approval;
  if (!pending) return;
  if (pending.grant && decision === "always") decision = "allow";
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
async function followAnswered(id, turn, runId) {
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

/// Hands a view to the command center window, which hides the bar.
function toWindow(target) {
  return invoke("center_open", { target }).catch(() => {});
}
/// Opens chat `id` (null for a new one) in the window.
const openInWindow = id => toWindow({ chat: { session: id ?? null } });

async function openSession(id) {
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
  loadAgents(id, body());
  loadSources(id, body());
  keepWaitingQuestions(id);
}

/// Puts the files sent in session `id` under their prompts, once the server
/// lists them. The chat may have been left or redrawn meanwhile.
async function loadAskFiles(id, b) {
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
async function loadSources(id, b) {
  const turns = await invoke("session_sources", { id }).catch(() => []);
  if (S.session !== id || body() !== b) return;
  for (const { message, sources } of turns) {
    const turn = b.querySelector(`.turn[data-at="${message}"]`);
    if (turn) sourceRow(turn, sources);
  }
}

/// The site a source is on, without "www.".
function sourceSite(url) {
  try { return new URL(url).hostname.replace(/^www\./, ""); } catch { return ""; }
}

/// The web pages a reply used, as chips under it: the site's initial and
/// name. Past three the rest fold into "+N", which shows them. A click opens
/// the page in the browser.
function sourceRow(turn, sources) {
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

/// Names the agent that answered each turn of session `id`, from the
/// server's list. General and turns without agents show nothing.
async function loadAgents(id, b) {
  const agents = await invoke("session_agents", { id }).catch(() => []);
  if (S.session !== id || body() !== b) return;
  for (const { message, agent } of agents) {
    const turn = agent && b.querySelector(`.turn[data-at="${message}"]`);
    if (turn) { turn.agent = agent; replyActs(turn); }
  }
}
/// The same for a turn that just finished live: it is the newest one listed.
async function loadTurnAgent(id, turn) {
  const agents = await invoke("session_agents", { id }).catch(() => []);
  const agent = agents.at(-1)?.agent;
  if (!agent || !turn.isConnected || turn !== [...body().querySelectorAll(".turn")].findLast(t => t.user)) return;
  turn.agent = agent;
  replyActs(turn);
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
    : `<span class="acts">${PANE ? "" : '<button class="dbtn" data-a="window">Open in window<kbd>⌘O</kbd></button>'}<button class="dbtn" data-a="rename">Rename<kbd>⌘R</kbd></button>${pin}<button class="dbtn" data-a="share">Share</button><button class="dbtn warn" data-a="delete">Delete<kbd>⌘⌫</kbd></button></span>`;
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
      ${c.pinned ? `<span class="pin" aria-label="Pinned">${ICON.pin}</span>` : ""}${c.kind === "group" ? `<span class="grp" aria-label="Group">${ICON.group}</span>` : ""}${RESEARCHES.has(c.id) ? `<span class="grp" aria-label="Research">${ICON.doc}</span>` : ""}${LIVE_CHATS.has(c.id) ? `<span class="grp vmic" aria-label="Spoken">${VOICE_ICON.mic}</span>` : ""}
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
  if (RESEARCHES.has(c.id)) return invoke("open_research", { session: c.id }).catch(() => openSession(c.id));
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
      case "window": if (c.kind === "chat") await openInWindow(c.id); break;
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

// ---------- Attached images and files ----------

// Pictures and files pasted or dropped into the prompt, sent with the next
// message. Each picture is shrunk and sent as JPEG, as the phone does; other
// files go as they are, up to MAX_UPLOAD in all.
const attached = { pics: [], files: [], error: "" };
const MAX_IMAGES = 8;
const MAX_UPLOAD = 5 * 1024 * 1024;
const PIC_SIDE = 1024;

const attachedCount = () => attached.pics.length + attached.files.length;

/// An attached file's chip: its name and size, with a remove button while
/// it waits in the prompt, as a button that opens it once it is kept.
function fileChip(f, i) {
  // A spoken message's recording, kept with it.
  if (i === undefined && f.mime?.startsWith("audio/")) {
    const tag = f.id ? "button" : "span";
    const about = ["Audio", fileSize(f.size)].filter(Boolean).join(", ");
    return `<${tag} class="dachip" aria-label="Open your recording"><span class="ic">${VOICE_ICON.mic}</span><span style="min-width:0"><span class="nm">Your recording</span><span class="sz">${esc(about)}</span></span></${tag}>`;
  }
  const about = [fileKind(f), fileSize(f.size)].filter(Boolean).join(", ");
  const remove = i === undefined ? "" : `<button aria-label="Remove ${esc(f.name)}" data-i="${i}">${ICON.x}</button>`;
  const tag = f.id ? "button" : "span";
  return `<${tag} class="dachip"><span class="ic">${ICON.doc}</span><span style="min-width:0"><span class="nm">${esc(f.name)}</span><span class="sz">${esc(about)}</span></span>${remove}</${tag}>`;
}

/// The files a prompt carried, under it. A kept one opens like a file Beep sent.
function askFiles(files) {
  const row = h(`<div class="dachips">${files.map(f => fileChip(f)).join("")}</div>`);
  [...row.children].forEach((chip, i) => {
    const file = files[i];
    if (!file.id) return;
    chip.addEventListener("click", async () => {
      if (chip.classList.contains("gone")) return;
      try { await invoke("open_file", { id: file.id, name: file.name }); }
      catch (e) {
        chip.querySelector(".sz").textContent = String(e) === GONE ? GONE : String(e);
        if (String(e) === GONE) chip.classList.add("gone");
      }
    });
  });
  return row;
}

/// The strip of attached pictures and files above the prompt, or the error
/// from the last paste, with the placeholder and hints to match.
function drawAttached() {
  const old = $("#thumbs"), input = $("#ask")?.closest(".dinput");
  if (!input) return;
  const pics = attached.pics.length, n = attachedCount();
  const el = n || attached.error ? h(`<div class="dthumbs" id="thumbs">${attached.pics.map((url, i) =>
    `<span class="dthumb"><img src="${esc(url)}" alt="Image ${i + 1}"><button aria-label="Remove image ${i + 1}" data-i="${i}">${ICON.x}</button></span>`).join("")}${attached.files.map((f, i) => fileChip(f, pics + i)).join("")}${attached.error ? `<span class="derr">${esc(attached.error)}</span>` : ""}</div>`) : null;
  el?.querySelectorAll("button").forEach(b => b.addEventListener("click", () => { unattach(+b.dataset.i); focusAsk(); }));
  if (old) el ? old.replaceWith(el) : old.remove();
  else if (el) input.before(el);
  const ask = $("#ask");
  if (n) {
    ask.dataset.ph ??= ask.placeholder;
    const what = attached.files.length ? (n > 1 ? "these" : "this file") : (n > 1 ? "these images" : "this image");
    ask.placeholder = `Ask about ${what}…`;
  } else if (ask.dataset.ph) {
    ask.placeholder = ask.dataset.ph;
    delete ask.dataset.ph;
  }
  refreshHints();
}

/// Removes the `i`th attachment, counting pictures first.
function unattach(i) {
  if (i < attached.pics.length) attached.pics.splice(i, 1);
  else attached.files.splice(i - attached.pics.length, 1);
  attached.error = "";
  drawAttached();
}

function attachError(text) {
  attached.error = text;
  drawAttached();
  clearTimeout(attached.timer);
  attached.timer = setTimeout(() => { attached.error = ""; drawAttached(); }, 4000);
}

/// Adds files from a paste or a drop: pictures shrunk, up to `MAX_IMAGES`;
/// other files, or pictures that cannot be drawn, as they are.
async function attach(files) {
  const all = [...files];
  if (!all.length) return false;
  for (const f of all) {
    if (attachedCount() >= MAX_IMAGES) { attachError(`Up to ${MAX_IMAGES} attachments in one message`); break; }
    if (f.type.startsWith("image/")) {
      try { attached.pics.push(await shrinkPic(f)); continue; } catch { /* sent as a file, such as a HEIC */ }
    }
    const used = attached.files.reduce((n, x) => n + x.size, 0);
    if (used + f.size > MAX_UPLOAD) { attachError(`Files up to ${MAX_UPLOAD >> 20} MB in one message`); continue; }
    try { attached.files.push({ name: f.name || "file", mime: f.type, size: f.size, data: await fileBase64(f) }); }
    catch { attachError(`${f.name || "That file"} could not be read`); }
  }
  drawAttached();
  return true;
}

/// `file`'s bytes in base64. A folder fails to read.
function fileBase64(file) {
  return new Promise((done, fail) => {
    const reader = new FileReader();
    reader.onload = () => done(String(reader.result).split(",")[1] ?? "");
    reader.onerror = () => fail(reader.error);
    reader.readAsDataURL(file);
  });
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
  if (S.view === "agent" && $("#agmsg")) {
    const files = [...(e.clipboardData?.items ?? [])].filter(i => i.kind === "file").map(i => i.getAsFile()).filter(Boolean);
    if (files.length) { e.preventDefault(); agentAttach(files); }
    return;
  }
  if (S.view !== "chat" || !$("#ask")) return;
  const files = [...(e.clipboardData?.items ?? [])].filter(i => i.kind === "file").map(i => i.getAsFile()).filter(Boolean);
  if (files.length) { e.preventDefault(); attach(files); }
});

// Dropping images and files on the chat attaches them too.
bar.addEventListener("dragover", e => {
  const agent = S.view === "agent" && $("#agmsg");
  if ((S.view !== "chat" && !agent) || !e.dataTransfer?.types.includes("Files")) return;
  e.preventDefault();
  if (!$("#ddrop")) bar.append(h(`<div class="ddrop" id="ddrop">${ICON.image}${agent ? "Drop images to send them to the agent" : "Drop images or files to ask about them"}</div>`));
});
bar.addEventListener("dragleave", e => { if (!bar.contains(e.relatedTarget)) $("#ddrop")?.remove(); });
bar.addEventListener("drop", e => {
  $("#ddrop")?.remove();
  if (S.view === "agent" && $("#agmsg") && e.dataTransfer?.files.length) {
    e.preventDefault();
    agentAttach(e.dataTransfer.files).then(() => $("#agmsg")?.focus());
    return;
  }
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

/// What can be said in live mode, and its keys: in the shortcuts and in the
/// sheet ⌘/ opens over live mode.
const LIVE_SAY = [["“send”", "Send now"], ["“stop”", "Send now, or stop Beep speaking"]];
const LIVE_TALK = [["tap ⌥", "Leave, or stop Beep"], ["hold ⌥", "Talk, or interrupt Beep"], ["esc", "Cancel the reply, or leave"], ["any key", "Leave and type instead"], ["⌥ ⌥", "Over the orb: bring Beep back. In Beep: leave"]];
const SHORTCUTS = [
  ["Anywhere", [["⌥ ⌥", "Open or hide Beep"], ["⌘N", "New chat"], ["⌘K", "Chats"], ["⌘G", "New group"], ["⌘M", "Memories"], ["⌘J", "Routines, then ⌘G for grants"], ["⌘L", "Skills"], ["⌘R", "Research"], ["⌘,", "Settings"], ["⌘Q", "Quit"]]],
  ["Talking", [["tap ⌥", "Live mode, with Beep open"], ["hold ⌥", "Talk while held, with Beep open"], ...LIVE_TALK]],
  ["Say in live mode", LIVE_SAY],
  ["In a chat", [["⌘E", "Model, for admins"], [ICON.ret, "Send"], [`⇧${ICON.ret}`, "New line"], ["↑", "Chats, when empty"], ["⌘.", "Stop"], ["⌘R", "Research what is typed, or the offered question"], [`⌘${ICON.ret}`, "Allow"], [`⌥⌘${ICON.ret}`, "Always allow"], ["⌘D", "Deny"]]],
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
  // The view may stay "group", so a new group is a change of its own.
  if (G.id !== id) { G.id = id; changed(); }
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

/// Settings, This Mac: the switch that lets Beep use this Mac, where its
/// link stands, and the two permissions desktop control needs.
function thisMac(mac) {
  const p = mac.permissions;
  const sub = !mac.enabled ? "Beep can see the screen, click, type and run commands here when you ask"
    : mac.status === "failed" ? `Couldn't connect: ${esc(mac.error || "unknown error")}. Beep tries again on its own.`
    : mac.status === "connected" ? (p.screen_recording && p.accessibility ? "Connected. Ask Beep to do something on this Mac" : "Connected. Allow both below so Beep can see and use the screen")
    : "Connecting…";
  const perm = (key, name, why, ok) => `<div class="dtoggle"><span><b>${name}</b><small>${why}</small></span>${ok
    ? '<span class="dtag ok">Allowed</span>'
    : `<button class="dbtn" data-allow="${key}">Allow…</button>`}</div>`;
  const rows = mac.enabled
    ? perm("ScreenRecording", "Screen Recording", "To see the screen", p.screen_recording)
      + perm("Accessibility", "Accessibility", "To click, type and read app controls", p.accessibility)
    : "";
  return `<div class="dsec flat" style="padding-top:6px">This Mac</div>
    <div class="dtoggle"><span><b>Let Beep use this Mac</b><small>${sub}</small></span>
      <button class="dswitch" id="usemac" role="switch" aria-checked="${mac.enabled}" aria-label="Let Beep use this Mac"></button></div>${rows}`;
}

/// Settings: what a new notice does while you are at this Mac.
const VOICE_NOTES = [["ask", "Ask first"], ["aloud", "Read aloud right away"], ["off", "Off"]];
const VOICE_NOTES_SAY = {
  ask: "New notices show by the orb with Play and Open when you are at this Mac and not on a call. Otherwise only a notification.",
  aloud: "New notices are read aloud when you are at this Mac and not on a call. Otherwise only a notification.",
  off: "New notices only show as a notification.",
};
function voiceNotesRow(cur) {
  return `<div class="dtoggle"><span><b>Voice notes</b><small>${VOICE_NOTES_SAY[cur] ?? ""}</small></span></div>
    <div class="dchips" role="radiogroup" aria-label="Voice notes" style="margin:6px 0 10px">${VOICE_NOTES.map(([k, label]) =>
      `<button class="dchip" role="radio" aria-checked="${cur === k}" aria-pressed="${cur === k}" data-vn="${k}">${label}</button>`).join("")}</div>`;
}

/// Settings, under Voice notes: what ends a spoken message in live mode.
function sendRow(onPause) {
  const why = onPause ? "Sends after a short pause. Saying “send” sends at once." : "Pauses never send. Say “send” or “stop” when you are done.";
  return `<div class="dtoggle"><span><b>When I stop talking</b><small>${why}</small></span></div>
    <div class="dchips" role="radiogroup" aria-label="When I stop talking" style="margin:6px 0 10px"><button class="dchip" role="radio" aria-checked="${onPause}" aria-pressed="${onPause}" data-pause="1">Send when I stop talking</button><button class="dchip" role="radio" aria-checked="${!onPause}" aria-pressed="${!onPause}" data-pause="0">Wait until I say “send”</button></div>`;
}

/// Settings: drawn at once from what this Mac knows (its own settings and
/// the last devices, apps and machines seen), then each server-backed
/// section fills in on its own as its call answers. The calls run side by
/// side: one after another they took as long as all four together. In the
/// bar it is the bar's page; in the window it is a sheet over the view
/// (`SET.root`), which `settingsSheet` opens.
const SET = { root: null, token: 0, devices: null, machines: undefined, confirm: false };
const SET_SECTIONS = [["devices", "Devices"], ["apps", "Apps"], ["mac", "This Mac"], ["machines", "Machines"], ["creds", "Credentials"], ["general", "General"]];

/// Settings, Credentials (`credentials.js` draws it). `list` is undefined
/// until the server answers and null when it failed; `edit` is the row
/// whose token is being typed.
/// `cli` is what gh and teacli on this Mac are signed in to, `cliBusy` the
/// sign-in being saved and `cliError` why one was refused. `ghFlow` is gh's
/// browser sign-in under way: { code, url, copied }.
/// `github` is the GitHub accounts, undefined when the server could not
/// list them; `ghAdd` whether "Add another GitHub account" is open.
const CRED = { mod: null, list: undefined, github: undefined, providers: [], edit: null, busy: false, error: "", cli: undefined, cliBusy: "", cliError: null, ghFlow: null, ghAdd: false };
const credModule = () => import("./credentials.js").then(m => (CRED.mod = m));

function setCredsHtml() {
  if (CRED.list === undefined || !CRED.mod) return setSkeleton(2);
  if (CRED.list === null) return `<div class="dmuted">Could not load your credentials.</div>`;
  // Which machines have gh and teacli, when the machines are known.
  const machines = Array.isArray(SET.machines) ? SET.machines : null;
  return CRED.mod.credentialsHtml(CRED, machines, esc, when);
}

/// Asks gh and teacli on this Mac what they are signed in to, then redraws.
async function loadCliLogins() {
  try {
    CRED.cli = await invoke("cli_logins");
  } catch (e) {
    console.warn(e);
    CRED.cli = null;
  }
  // Not while a token is typed: the redraw would drop it.
  if (setShown() && CRED.list && !setRoot().querySelector("#credSecret")?.value) fillSection("creds", setCredsHtml());
}

/// Lists the credentials again and redraws their section.
async function loadCreds() {
  loadCliLogins();
  try {
    // A server from before several GitHub accounts does not list them:
    // the credentials list then has the one GitHub token.
    const accounts = invoke("list_github_accounts").catch(e => { console.warn(e); return undefined; });
    const [, list, providers, github] = await Promise.all([credModule(), invoke("list_credentials"), invoke("credential_providers"), accounts]);
    CRED.list = list;
    CRED.github = github;
    CRED.providers = providers;
  } catch (e) {
    console.warn(e);
    CRED.list ??= null;
  }
  if (setShown()) fillSection("creds", setCredsHtml());
}

/// Runs gh's browser sign-in: Beep shows the code gh prints (and copies
/// it), GitHub opens in the browser, and once the code is entered there the
/// token is saved with no further click. Cancel or a failure shows why.
async function signInGh(redraw) {
  if (CRED.ghFlow) return;
  CRED.ghFlow = { code: "", url: "", copied: false };
  CRED.cliError = null;
  redraw();
  const onCode = new Channel();
  onCode.onmessage = async ({ code, url }) => {
    if (!CRED.ghFlow) return;
    CRED.ghFlow = { code, url, copied: false };
    redraw();
    await copy(code);
    if (CRED.ghFlow?.code === code) {
      CRED.ghFlow.copied = true;
      redraw();
    }
  };
  try {
    await invoke("gh_sign_in", { onCode });
  } catch (e) {
    CRED.cliError = { key: "github", message: String(e) };
  }
  CRED.ghFlow = null;
  CRED.ghAdd = !!CRED.cliError;
  await loadCreds();
}

/// Binds the Credentials controls inside `el`.
function bindCreds(el) {
  const $in = sel => el.querySelector(sel);
  const redraw = () => { if (setShown()) fillSection("creds", setCredsHtml()); };
  el.querySelectorAll("[data-cred-edit]").forEach(b => b.addEventListener("click", () => {
    CRED.edit = CRED.mod?.editFor(b.dataset.credEdit, CRED.list) ?? null;
    CRED.error = "";
    redraw();
    setRoot().querySelector(CRED.edit?.kind === "forgejo" && !CRED.edit.id ? "#credUrl" : "#credSecret")?.focus();
  }));
  el.querySelectorAll("[data-cred-remove]").forEach(b => b.addEventListener("click", async () => {
    b.disabled = true;
    try {
      await invoke("remove_credential", { id: b.dataset.credRemove });
    } catch (e) {
      console.warn(e);
    }
    await loadCreds();
  }));
  // Saves the token gh or teacli here holds; the token never reaches the page.
  el.querySelectorAll("[data-cred-cli]").forEach(b => b.addEventListener("click", async () => {
    const key = b.dataset.credCli;
    const host = CRED.mod?.cliHost(key);
    if (!host || CRED.cliBusy) return;
    CRED.cliBusy = key;
    CRED.cliError = null;
    redraw();
    try {
      await invoke("use_cli_login", { host });
    } catch (e) {
      CRED.cliError = { key, message: String(e) };
    }
    CRED.cliBusy = "";
    await loadCreds();
  }));
  el.querySelectorAll("[data-cred-gh]").forEach(b => b.addEventListener("click", () => signInGh(redraw)));
  // Opens or closes "Add another GitHub account".
  el.querySelectorAll("[data-cred-gh-add]").forEach(b => b.addEventListener("click", () => {
    CRED.ghAdd = !!b.dataset.credGhAdd;
    CRED.cliError = null;
    redraw();
  }));
  // Saves the token gh here holds for one of its accounts.
  el.querySelectorAll("[data-cred-gh-use]").forEach(b => b.addEventListener("click", async () => {
    const login = b.dataset.credGhUse;
    if (!login || CRED.cliBusy) return;
    CRED.cliBusy = `gh:${login}`;
    CRED.cliError = null;
    redraw();
    try {
      await invoke("use_gh_account", { login });
      CRED.ghAdd = false;
    } catch (e) {
      CRED.cliError = { key: "github", message: String(e) };
    }
    CRED.cliBusy = "";
    await loadCreds();
  }));
  el.querySelectorAll("[data-cred-gh-default]").forEach(b => b.addEventListener("click", async () => {
    b.disabled = true;
    try {
      await invoke("set_default_github_account", { id: b.dataset.credGhDefault });
    } catch (e) {
      console.warn(e);
    }
    await loadCreds();
  }));
  el.querySelectorAll("[data-cred-gh-cancel]").forEach(b => b.addEventListener("click", () => {
    b.disabled = true;
    invoke("cancel_gh_sign_in").catch(console.warn);
  }));
  el.querySelectorAll("[data-cred-open]").forEach(a => a.addEventListener("click", e => {
    e.preventDefault();
    e.stopPropagation();
    invoke("open_url", { url: a.dataset.credOpen }).catch(console.warn);
  }));
  $in("#credCancel")?.addEventListener("click", () => { CRED.edit = null; CRED.error = ""; redraw(); });
  const save = async () => {
    if (CRED.busy || !CRED.mod) return;
    const machines = $in("#credMachines");
    if (CRED.edit && machines) CRED.edit.toMachines = machines.checked;
    const url = $in("#credUrl")?.value;
    if (CRED.edit && url !== undefined) CRED.edit.url = url;
    const d = CRED.mod.draft(CRED.edit, url, $in("#credSecret")?.value, machines?.checked);
    if (d.error) {
      CRED.error = d.error;
      const err = $in("#credErr");
      if (err) err.textContent = d.error;
      return;
    }
    CRED.busy = true;
    CRED.error = "";
    redraw();
    try {
      await invoke("save_credential", { credential: d.credential });
      if (CRED.edit?.kind === "github") CRED.ghAdd = false;
      CRED.edit = null;
    } catch (e) {
      CRED.error = String(e);
    }
    CRED.busy = false;
    // The typed token is gone from the page either way.
    await loadCreds();
  };
  $in("#credSave")?.addEventListener("click", save);
  $in("#credSecret")?.addEventListener("keydown", e => { if (e.key === "Enter") { e.preventDefault(); save(); } });
}

const setRoot = () => SET.root ?? bar;
/// The sheet is open, or the bar still shows Settings.
const setShown = () => SET.root ? SET.root.isConnected : S.view === "settings";

// Permissions are granted in System Settings; coming back shows them.
// Not while a token is typed in Credentials: the redraw would drop it.
window.addEventListener("focus", () => { if (setShown() && !$("#unlinkYes") && !$("#credSecret")?.value) renderSettings(); });

/// Rows that stand in for a section until the server answers.
const setSkeleton = n => `<div class="dskel-rows">${[78, 56, 68, 48].slice(0, n).map(w => `<div class="dskel" style="width:${w}%"></div>`).join("")}</div>`;

function setDevicesHtml(linked, set) {
  if (!linked) return `<div class="dtoggle"><span><b>Not linked</b><small>Link to see your chats and memories here</small></span><button class="dbtn pri" id="link">Link to phone…</button></div>`;
  if (SET.devices === null) return setSkeleton(2);
  const devices = SET.devices;
  // Whether this Mac made the account, and so links and removes devices.
  const owner = devices.some(d => d.this_device && d.primary);
  const row = d => {
    const tags = [d.this_device ? "This Mac" : "", d.primary ? "Owner" : ""].filter(Boolean)
      .map(t => `<span class="dtag">${t}</span>`).join("");
    const remove = owner && !d.this_device ? `<button class="dbtn quiet" data-remove="${esc(d.id)}" aria-label="Remove ${esc(d.name)}">${ICON.x}</button>` : "";
    return `<div class="dtoggle"><span><b style="display:flex;gap:7px;align-items:center">${esc(d.name)}${tags}</b><small>${d.kind === "phone" ? "Phone" : "Computer"}</small></span>${remove}</div>`;
  };
  const ask = SET.confirm ? `<div class="dcard"><div style="padding:10px 12px;display:flex;flex-direction:column;gap:8px">
      <b style="font-size:13.5px">Unlink this Mac?</b><div class="dmuted" style="display:block">This Mac stops seeing your chats and memories. Your phone keeps them. To use Beep here again, start over and link it from the phone.</div>
      <div style="display:flex;gap:8px"><button class="dbtn warn" id="unlinkYes">Unlink</button><button class="dbtn" id="unlinkNo">Cancel</button></div>
      <div class="derr" id="unlinkErr"></div></div></div>` : "";
  return devices.map(row).join("") + (owner
    ? '<div class="dtoggle"><span><small>Scan its code from Beep on the phone to bring its chats here</small></span><button class="dbtn pri" id="invite">Link a phone…</button></div>'
    : SET.confirm ? "" : `<div class="dtoggle"><span><small>This Mac: ${esc(set.name)}</small></span><button class="dbtn" id="invite">Link a phone…</button><button class="dbtn warn" id="unlink">Unlink this Mac</button></div>`) + ask;
}

function setAppsHtml() {
  if (!A.apps) return setSkeleton(1);
  const on = A.apps.filter(a => a.connected).map(a => a.name);
  return `<div class="dtoggle"><span><b>Connected apps</b><small>${on.length ? `${on.length} connected: ${esc(on.join(", "))}` : "Gmail, Calendar, Notion and more"}</small></span><button class="dbtn" id="apps">Manage…</button></div>`;
}

/// Other machines; left out when the list could not be read.
function setMachinesHtml() {
  if (SET.machines === undefined) return setSkeleton(1);
  const list = SET.machines;
  if (!list) return "";
  return `<div class="dtoggle"><span><b>Other machines</b><small>${list.length ? esc(`${list.length} linked: ${list.map(m => m.name).join(", ")}`) : "None linked yet"}</small></span><button class="dbtn" id="machines">Show…</button></div>`;
}

/// Leaves Settings for another page: in the window the sheet closes and
/// the pane shows it.
function setAway(view, inBar) {
  if (SET.root && window.beepSettingsAway) return window.beepSettingsAway(view);
  inBar();
}

/// Binds the controls inside `el`, a section or the whole page.
function bindSettings(el) {
  const $in = sel => el.querySelector(sel);
  $in("#link")?.addEventListener("click", () => setAway("link", renderLink));
  $in("#invite")?.addEventListener("click", () => setAway("invite", renderInvitePhone));
  $in("#apps")?.addEventListener("click", () => setAway("apps", renderApps));
  $in("#machines")?.addEventListener("click", () => setAway("machines", renderMachines));
  el.querySelectorAll("[data-remove]").forEach(b => b.addEventListener("click", async () => {
    b.disabled = true;
    try {
      await invoke("remove_device", { id: b.dataset.remove });
    } catch (e) {
      console.warn(e);
    }
    if (setShown()) renderSettings();
  }));
  $in("#unlink")?.addEventListener("click", () => renderSettings(true));
  $in("#unlinkNo")?.addEventListener("click", () => renderSettings());
  $in("#unlinkYes")?.addEventListener("click", async () => {
    try {
      await invoke("unlink");
      if (SET.root) window.beepSettingsAway?.(null);
      show(null);
    } catch (e) {
      $("#unlinkErr").textContent = String(e);
    }
  });
  $in("#usemac")?.addEventListener("click", async e => {
    const sw = e.currentTarget;
    sw.disabled = true;
    try {
      await invoke("set_mac_control", { enabled: sw.getAttribute("aria-checked") !== "true" });
    } catch (err) {
      console.warn(err);
    }
    if (setShown()) renderSettings();
  });
  el.querySelectorAll("[data-allow]").forEach(b => b.addEventListener("click", () =>
    invoke("allow_mac_permission", { permission: b.dataset.allow }).catch(e => console.warn(e))));
  el.querySelectorAll("[data-vn]").forEach(b => b.addEventListener("click", async () => {
    await invoke("set_notice_voice", { choice: b.dataset.vn }).catch(e => console.warn(e));
    if (setShown()) renderSettings();
  }));
  el.querySelectorAll("[data-pause]").forEach(b => b.addEventListener("click", async () => {
    await invoke("set_send_on_pause", { enabled: b.dataset.pause === "1" }).catch(e => console.warn(e));
    if (setShown()) renderSettings();
  }));
  bindCreds(el);
  $in("#autostart")?.addEventListener("click", async e => {
    const sw = e.currentTarget, on = sw.getAttribute("aria-checked") !== "true";
    try {
      await invoke("set_autostart", { enabled: on });
      sw.setAttribute("aria-checked", on);
    } catch (err) {
      console.warn(err);
    }
  });
}

/// Redraws one section in place, once its data came.
function fillSection(id, html) {
  const box = setRoot().querySelector(`#set-${id}`);
  if (!box) return;
  const focus = focusKey(box);
  box.innerHTML = html;
  bindSettings(box);
  if (focus) refocus(box, focus);
}

// The data attributes that name a Settings control when it has no id.
const FOCUS_KEYS = ["remove", "vn", "pause", "allow", "goto"];

/// What has focus inside `box`, as a selector that finds it again after
/// a redraw; null when focus is elsewhere.
function focusKey(box) {
  const a = document.activeElement;
  // A button disabled while it waited drops focus to the body; in the
  // sheet that would let Tab reach the page behind.
  if (!a || a === document.body) return box.closest(".cc-set") ? "*" : null;
  if (a === box || !box.contains(a)) return null;
  if (a.id) return `#${CSS.escape(a.id)}`;
  const k = FOCUS_KEYS.find(k => k in a.dataset);
  return k ? `[data-${k}="${CSS.escape(a.dataset[k])}"]` : "*";
}

/// Puts focus back after a redraw, on the same control when it is still
/// there, else on the sheet, so Tab never lands on the page behind.
function refocus(box, key) {
  const again = key === "*" ? null : box.querySelector(key);
  if (again && !again.disabled) return again.focus();
  (box.closest(".cc-set") || box).focus();
}

/// Draws Settings into `root` (the window's sheet), or the bar when none.
async function renderSettings(confirm = false, root = undefined) {
  if (root !== undefined) SET.root = root;
  else if (SET.root && !SET.root.isConnected) SET.root = null;
  // The window shows Settings as its sheet, never as a page of the pane.
  if (PANE && !SET.root && window.beepOpenSettings) return window.beepOpenSettings();
  if (!SET.root) S.view = "settings";
  SET.confirm = confirm;
  const token = ++SET.token;
  const alive = () => SET.token === token && setShown();
  // Local: answered by this Mac, never the server.
  const [set, mac] = await Promise.all([
    invoke("settings"),
    invoke("mac_control").catch(e => { console.warn(e); return null; }),
  ]).catch(e => [e, null]);
  if (!alive()) return;
  const el = setRoot();
  if (set instanceof Error || typeof set !== "object") {
    el.innerHTML = `<div class="dwin"><h4>Settings</h4><p class="derr">${esc(set)}</p></div>`;
    return;
  }
  const linked = S.link === "linked";
  const sec = (id, title, body) => `<section id="set-${id}-sec" data-sec="${id}"><div class="dsec flat" style="padding-top:6px">${title}</div><div id="set-${id}">${body}</div></section>`;
  const sections = sec("devices", "Devices", setDevicesHtml(linked, set))
    + (linked ? sec("apps", "Apps", setAppsHtml()) : "")
    + (linked && mac ? `<section id="set-mac-sec" data-sec="mac">${thisMac(mac)}</section>` : "")
    + (linked ? sec("machines", "Machines", setMachinesHtml()) : "")
    + (linked ? sec("creds", "Credentials", setCredsHtml()) : "")
    + `<section id="set-general-sec" data-sec="general"><div class="dsec flat" style="padding-top:6px">General</div>
      <div class="dtoggle"><span><b>Launch at login</b><small>Keep Beep in the menu bar after a restart</small></span>
        <button class="dswitch" id="autostart" role="switch" aria-checked="${set.autostart}" aria-label="Launch at login"></button></div>
      ${voiceNotesRow(set.notice_voice)}${sendRow(set.send_on_pause)}</section>`;
  if (SET.root) {
    const shown = new Set([...sections.matchAll(/data-sec="(\w+)"/g)].map(m => m[1]));
    const nav = SET_SECTIONS.filter(([id]) => shown.has(id))
      .map(([id, label], i) => `<button class="cc-si" data-goto="${id}"${i === 0 ? ' aria-current="true"' : ""}>${label}</button>`).join("");
    const top = el.querySelector(".dset-body")?.scrollTop ?? 0;
    const focus = focusKey(el);
    el.innerHTML = `<nav class="dset-nav" aria-label="Settings sections"><b>Settings</b>${nav}</nav>
      <div class="dset-body"><div class="dset-top"><span class="tt"><b>Settings</b><small>Beep for macOS ${esc(set.version)}</small></span><kbd>esc</kbd></div>
      <div class="dform" style="display:flex;flex-direction:column">${sections}</div></div>`;
    const body = el.querySelector(".dset-body");
    body.scrollTop = top;
    el.querySelectorAll("[data-goto]").forEach(b => b.addEventListener("click", () => {
      el.querySelectorAll("[data-goto]").forEach(x => x.removeAttribute("aria-current"));
      b.setAttribute("aria-current", "true");
      el.querySelector(`#set-${b.dataset.goto}-sec`)?.scrollIntoView({ block: "start", behavior: "smooth" });
    }));
    bindSettings(el);
    if (focus) refocus(el, focus);
  } else {
    el.innerHTML = `<div class="dhead"><span class="tt"><b>Settings</b><small>Beep for macOS ${esc(set.version)}</small></span></div>
      <div class="dform" style="padding-top:12px;display:flex;flex-direction:column">${sections}</div>${hints([["esc", "close"]])}`;
    bindSettings(el);
  }
  if (!linked) return;
  // The server's parts, side by side; each fills its section as it answers.
  invoke("list_devices").then(d => { SET.devices = d; }, e => { console.warn(e); SET.devices ??= []; })
    .then(() => { if (alive()) fillSection("devices", setDevicesHtml(true, set)); });
  invoke("list_apps").then(a => { A.apps = a; }, () => { A.apps ??= []; })
    .then(() => { if (alive()) fillSection("apps", setAppsHtml()); });
  invoke("list_machines").then(m => { SET.machines = m; MC.list = m; }, () => { SET.machines = null; })
    .then(() => {
      if (!alive()) return;
      fillSection("machines", setMachinesHtml());
      // The git hosts say which machines have gh and teacli.
      if (CRED.list) fillSection("creds", setCredsHtml());
    });
  loadCreds();
}

// ---------- Apps ----------

/// The Apps view: every app, the one being connected and the one whose
/// permissions are open.
const A = { apps: null, error: "", connecting: null, poll: null, open: null, perms: null,
  // MCP servers added by URL: null until listed, the add form, and the one
  // whose sign-in is open in the browser.
  servers: null, adding: false, addError: "", waiting: null, busy: null,
  // The server whose tools are open, its tools once listed, the tool
  // waiting on the warning about going past the limit, and whether
  // Remove waits for a second click.
  server: null, tools: null, asking: null, unadd: false,
  // Fastmail's accounts by app slug, the agent's default first; missing
  // when the server is too old to list them. `removing` is the account
  // whose Remove waits for a second click.
  accounts: {}, removing: null };

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
  const [servers] = await Promise.all([
    invoke("list_mcp_servers").catch(() => A.servers),
    ...(A.apps ?? []).filter(a => a.kind === "fastmail").map(loadAccounts),
  ]);
  A.servers = servers;
  if (A.waiting && A.servers?.find(m => m.id === A.waiting)?.connected) A.waiting = null;
  // A Composio sign-in ends on the web page; the app shows up connected here.
  if (A.connecting && A.apps?.find(a => a.slug === A.connecting)?.connected) stopConnecting();
  if (S.view === "apps") drawApps();
}

async function loadAccounts(app) {
  const accounts = app.connected ? await invoke("list_app_accounts", { slug: app.slug }).catch(() => null) : [];
  if (accounts) A.accounts[app.slug] = accounts;
  else delete A.accounts[app.slug];
}

/// The addresses of a connected app's accounts, for its row in Apps.
function accountList(app) {
  const accounts = A.accounts[app.slug];
  return accounts?.length ? accounts.map(a => a.address ?? `${app.name} account`).join(", ") : null;
}

function drawApps() {
  const list = $("#alist");
  if (!list) return;
  const apps = A.apps ?? [];
  const row = app => {
    let end;
    if (A.connecting === app.slug) end = waitingFor("data-cancel");
    else if (app.connected) end = `<span class="dtag">Connected</span><span class="dmuted">${ICON.chev}</span>`;
    else if (!app.configured) end = '<span class="dmuted">Unavailable</span>';
    else end = `<button class="dbtn pri" data-connect="${esc(app.slug)}"${A.connecting ? " disabled" : ""}>Connect</button>`;
    return `<div class="dappr${app.connected ? " on" : ""}" ${app.connected ? `data-open="${esc(app.slug)}" role="button" tabindex="0"` : ""}>
      ${appLogo(app)}<span class="tt"><b>${esc(app.name)}</b><small>${esc((app.connected && accountList(app)) || app.description)}</small></span>${end}</div>`;
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

/// A sign-in open in the browser, with the Cancel that `attr` marks.
const waitingFor = attr => `<span class="dmuted"><span class="dspin"></span> Waiting for browser…</span><button class="dbtn quiet" ${attr}>Cancel</button>`;

/// The servers the user added, then the add form or its button.
function drawServers() {
  const row = m => {
    let end;
    if (A.waiting === m.id) end = waitingFor("data-mcancel");
    else if (A.busy === m.id) end = '<span class="dmuted"><span class="dspin"></span></span>';
    else if (m.connected) end = `<span class="dtag ok">${m.tools.length === 1 ? "1 tool" : `${m.tools.length} tools`}</span><span class="dmuted">${ICON.chev}</span>`;
    else end = `<span class="dtag ask">${m.sign_in ? "Sign-in expired" : "Not connected"}</span><button class="dbtn pri" data-mconnect="${esc(m.id)}">${m.sign_in ? "Sign in" : "Retry"}</button>`
      + `<button class="dbtn quiet" data-mremove="${esc(m.id)}">Remove</button>`;
    const open = m.connected && A.waiting !== m.id && A.busy !== m.id ? ` data-mopen="${esc(m.id)}" role="button" tabindex="0"` : "";
    return `<div class="dappr${open ? " on" : ""}"${open}><span class="dalogo">${esc(m.name.slice(0, 1).toUpperCase())}</span><span class="tt"><b>${esc(m.name)}</b><small>${esc(hostOf(m.url))}</small></span>${end}</div>`;
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
  const m = e.target.closest("[data-mnew],[data-mclose],[data-madd],[data-mcancel],[data-mconnect],[data-mremove],[data-mopen]");
  if (m) {
    const d = m.dataset;
    if (d.mnew != null) { A.adding = true; A.addError = ""; drawApps(); return $("#mname")?.focus(); }
    if (d.mclose != null) { A.adding = false; A.addError = ""; return drawApps(); }
    if (d.madd != null) return addServer();
    if (d.mcancel != null) { A.waiting = null; return drawApps(); }
    if (d.mconnect) return serverAction("connect", d.mconnect);
    if (d.mremove) return serverAction("remove", d.mremove);
    if (d.mopen) return renderMcpServer(A.servers.find(s => s.id === d.mopen));
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
    await invoke("connect_app", { slug: app.slug, kind: app.kind, add: false });
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

// ---------- An MCP server's tools ----------

/// Opened from a connected server in Apps: its tools, each switched on or
/// off, and Remove.
async function renderMcpServer(server) {
  if (!server) return;
  S.view = "mcp";
  Object.assign(A, { server, tools: null, asking: null, unadd: false, error: "" });
  bar.innerHTML = `<div class="dhead"><span class="dalogo">${esc(server.name.slice(0, 1).toUpperCase())}</span><span class="tt"><b>${esc(server.name)}</b><small class="dmono">${esc(server.url)}</small></span></div>
    <div class="dlist dapps" id="tlist"></div>${hints([["esc", "back"]])}`;
  $("#tlist").addEventListener("click", mcpClick);
  drawMcpTools();
  try {
    A.tools = await invoke("list_mcp_tools", { id: server.id });
  } catch (e) {
    A.error = String(e);
  }
  if (S.view === "mcp" && A.server === server) drawMcpTools();
}

/// "search_pages" reads as "Search pages", as on the phone.
const mcpToolLabel = name => name.replace(/[_-]/g, " ").trim().replace(/^./, c => c.toUpperCase());

function drawMcpTools() {
  const list = $("#tlist"), t = A.tools;
  if (!list) return;
  let html = A.error ? `<div class="derr" style="padding:8px 10px">${esc(A.error)}</div>` : "";
  if (!t && !A.error) html += '<div class="dmuted" style="padding:14px 10px"><span class="dspin"></span></div>';
  if (t) {
    const on = t.tools.filter(x => x.enabled).length;
    html += `<div class="dsec dsplit"><span>Tools</span><span${on > t.limit ? ' class="over"' : ""}>${on} of ${t.limit} on</span></div>` + t.tools.map(tool => {
      const name = esc(mcpToolLabel(tool.name)), title = tool.description ? ` title="${esc(tool.description)}"` : "";
      if (A.asking === tool.name) return `<div class="daction"><span>${name}<small>More than ${t.limit} tools makes replies slower and tool picks worse</small></span>`
        + `<button class="dbtn quiet" data-tno>Cancel</button><button class="dbtn pri" data-tyes>Turn on</button></div>`;
      if (!tool.usable) return `<div class="daction off"${title}><span>${name}<small>Too large for Beep to use</small></span><button class="dswitch" role="switch" aria-checked="false" aria-label="Use ${name}" disabled></button></div>`;
      return `<div class="daction"${title}><span>${name}</span><button class="dswitch" role="switch" aria-checked="${tool.enabled}" data-ttool="${esc(tool.name)}" aria-label="Use ${name}"></button></div>`;
    }).join("");
    html += `<div class="dmuted" style="padding:8px 10px;display:block">Beep uses up to ${t.limit} tools from a server, so turn on the ones you need. It asks before every tool on a server you add.</div>`;
  }
  html += `<div style="padding:6px 10px 2px;display:flex;gap:8px;align-items:center">${A.unadd
    ? '<span class="dmuted">Beep stops using this server</span><button class="dbtn quiet" data-unremove>Cancel</button><button class="dbtn warn" data-mremove>Remove</button>'
    : '<button class="dbtn warn" data-mremoving>Remove server</button>'}</div>`;
  list.innerHTML = html;
}

async function setTool(name, enabled) {
  const server = A.server;
  A.asking = null;
  try {
    A.tools = await invoke("set_mcp_tool", { id: server.id, tool: name, enabled });
    A.error = "";
  } catch (e) {
    A.error = String(e);
  }
  if (S.view === "mcp" && A.server === server) drawMcpTools();
}

async function mcpClick(e) {
  const d = e.target.closest("[data-ttool],[data-tyes],[data-tno],[data-mremoving],[data-unremove],[data-mremove]")?.dataset;
  if (!d || !A.tools && d.ttool) return;
  if (d.ttool) {
    const tool = A.tools.tools.find(x => x.name === d.ttool);
    // Past the limit, the switch asks first, as the phone's does.
    if (!tool.enabled && A.tools.tools.filter(x => x.enabled).length >= A.tools.limit) { A.asking = tool.name; return drawMcpTools(); }
    return setTool(tool.name, !tool.enabled);
  }
  if (d.tyes != null) return setTool(A.asking, true);
  if (d.tno != null) { A.asking = null; return drawMcpTools(); }
  if (d.mremoving != null || d.unremove != null) { A.unadd = d.mremoving != null; return drawMcpTools(); }
  if (d.mremove != null) {
    try {
      await invoke("remove_mcp_server", { id: A.server.id });
    } catch (e) {
      A.error = String(e);
      return drawMcpTools();
    }
    return renderApps();
  }
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
  A.removing = null;
  drawPerms();
  const accounts = app.kind === "fastmail" ? loadAccounts(app) : null;
  try {
    A.perms = await invoke("app_permissions", { slug: app.slug });
    A.error = "";
  } catch (e) {
    A.error = String(e);
  }
  await accounts;
  if (S.view === "appperms" && A.open === app) drawPerms();
}

/// Each connected account with Remove, and a button that signs in another.
function drawAccounts(app) {
  const accounts = A.accounts[app.slug];
  let html = '<div class="dsec">Accounts</div>' + accounts.map((a, i) => {
    const name = esc(a.address ?? `${app.name} account`);
    const first = i === 0 && accounts.length > 1 ? ' <small class="dmuted" style="display:inline">Default</small>' : "";
    const end = A.busy === "account:" + a.id ? '<span class="dmuted"><span class="dspin"></span></span>'
      : A.removing === a.id ? `<span class="dmuted">${accounts.length === 1 ? `Disconnects ${esc(app.name)}` : "Beep stops using this account"}</span>`
        + `<button class="dbtn quiet" data-unremove>Cancel</button><button class="dbtn warn" data-remove="${esc(a.id)}">Remove</button>`
      : `<button class="dbtn quiet" data-removing="${esc(a.id)}">Remove</button>`;
    return `<div class="daction"><span>${name}${first}</span>${end}</div>`;
  }).join("");
  html += A.connecting === app.slug
    ? `<div style="padding:4px 10px;display:flex;gap:8px;align-items:center">${waitingFor("data-cancel")}</div>`
    : '<div style="padding:4px 10px"><button class="dbtn" data-addaccount>Add another account</button></div>';
  return html + `<div class="dmuted" style="padding:6px 10px;display:block">Beep uses the first account unless you name another, like "check my work inbox". To add a different account, sign out of ${esc(app.name)} in the browser first.</div>`;
}

async function addAccount(app) {
  if (A.connecting) return;
  A.connecting = app.slug;
  A.error = "";
  drawPerms();
  try {
    await invoke("connect_app", { slug: app.slug, kind: app.kind, add: true });
  } catch (e) {
    if (A.connecting === app.slug && String(e) !== "cancelled") A.error = String(e);
  }
  A.connecting = null;
  await loadAccounts(app);
  if (S.view === "appperms" && A.open === app) drawPerms();
}

async function removeAccount(app, id) {
  A.busy = "account:" + id;
  A.removing = null;
  drawPerms();
  try {
    await invoke("remove_app_account", { slug: app.slug, id });
    A.error = "";
  } catch (e) {
    A.error = String(e);
  }
  A.busy = null;
  await loadAccounts(app);
  if (S.view !== "appperms" || A.open !== app) return;
  // The last account gone disconnects the app.
  if (A.accounts[app.slug]?.length === 0) { A.apps = null; return renderApps(); }
  drawPerms();
}

function drawPerms() {
  const list = $("#plist"), app = A.open, p = A.perms;
  if (!list || !app) return;
  let html = A.error ? `<div class="derr" style="padding:8px 10px">${esc(A.error)}</div>` : "";
  if (!p && !A.error) html += '<div class="dmuted" style="padding:14px 10px"><span class="dspin"></span></div>';
  if (p && A.accounts[app.slug]?.length) html += drawAccounts(app);
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
  const all = (A.accounts[app.slug]?.length ?? 0) > 1 ? " (all accounts)" : "";
  html += `<div style="padding:6px 10px 2px"><button class="dbtn warn" id="disconnect">Disconnect ${esc(app.name)}${all}</button></div>`;
  list.innerHTML = html;
}

async function permsClick(e) {
  const app = A.open, p = A.perms;
  if (e.target.closest("[data-cancel]")) {
    invoke("cancel_app_sign_in").catch(() => {});
    A.connecting = null;
    return drawPerms();
  }
  if (e.target.closest("[data-addaccount]")) return addAccount(app);
  const account = e.target.closest("[data-removing],[data-unremove],[data-remove]")?.dataset;
  if (account) {
    if (account.remove) return removeAccount(app, account.remove);
    A.removing = account.removing ?? null;
    return drawPerms();
  }
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
/// `list` names the agents set up, General first; empty when there are none.
const MODELS = { info: null, sel: 0, agents: false, list: [] };
const THINKING = ["off", "low", "medium", "high"];

/// Whether the server offers model choices (every user once it lists them).
function canPickModel() { return !!MODELS.info?.choices.length; }
async function loadModels() {
  try {
    MODELS.info = await invoke("get_models");
  } catch {
    return;
  }
  refreshHints();
  if (!canPickModel()) return;
  try {
    const agents = await invoke("get_agents");
    MODELS.agents = agents.enabled;
    MODELS.list = agents.agents.map(a => a.name);
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
  // Auto picks only the thinking: the model below always applies, and with
  // agents on the agent Jev picks brings its prompt and thinking.
  const autoSub = m.auto
    ? (agents ? "Jev picks an agent, which brings its prompt and thinking, on the model below" : "Jev picks the thinking for each prompt, on the model below")
    : (agents ? "Use the model below, with the agent's prompt" : "Off: every prompt uses the model below");
  const auto = m.auto_available
    ? `<div class="dtoggle"><span><b>Auto</b><small>${autoSub}</small></span><button class="dswitch" data-act="auto" role="switch" aria-checked="${m.auto}" aria-label="Auto"></button></div>`
    : "";
  const rows = m.choices.map((c, i) => `<button class="drt dmrow" role="option" data-pick="${i}" aria-selected="${i === MODELS.sel}"><span class="tt"><b>${esc(c.name)}</b><span>${esc(modelPrice(c))}</span></span>${c.id === m.selected ? `<span class="dok">${ICON.check}</span>` : ""}</button>`).join("");
  const picker = `<div class="dlist" style="padding:0" role="listbox" aria-label="Models">${rows}</div>
      <div class="dfield"><span>Thinking</span><div class="dchips">${THINKING.map(l => `<button class="dchip" data-level="${l}" aria-pressed="${l === m.thinking}">${cap(l)}</button>`).join("")}</div></div>`;
  // Agents apply to everyone, so they sit apart from the caller's own model.
  const agentSec = MODELS.list.length ? `<div class="dsec flat dagents">Agents <span class="dtag">Admin</span></div>
      <div class="dtoggle"><span><b>Agents</b><small>${agents ? "On for everyone: Jev picks an agent for each prompt" : "Off: everyone uses the general assistant"}</small></span><button class="dswitch" data-act="agents" role="switch" aria-checked="${agents}" aria-label="Agents"></button></div>
      <div class="dmuted dagentlist"${agents ? "" : ' style="opacity:.6"'}>${esc(MODELS.list.join(", "))}</div>` : "";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Model</b><small>${agents ? "Your own turns only." : "Your own turns only. Everyone else stays on the default."}</small></span></div>
    <div class="dform dmodelform" style="padding-top:12px">${auto}
      ${picker}
      ${agentSec}
      <div class="derr" id="modelErr"></div>
    </div>${hints([["↑↓", "move"], [ICON.ret, "use"], ["esc", "back"]])}`;
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
  if (e.target.closest('[data-act="agents"]')) return setAgents(!MODELS.agents);
}
async function setAgents(enabled) {
  try {
    const agents = await invoke("set_agents", { enabled });
    MODELS.agents = agents.enabled;
    MODELS.list = agents.agents.map(a => a.name);
  } catch (e) {
    const err = $("#modelErr");
    if (err) err.textContent = String(e);
    return;
  }
  if (S.view === "model") drawModel();
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

/// Copy, and for admins Rerun, under a finished reply, then the agent that
/// answered it when there was one.
function replyActs(turn) {
  turn.querySelector(".dreplyacts")?.remove();
  const text = turn.querySelector(".reply")?.innerText.trim();
  const rerun = canPickModel() && turn.user;
  if (!text && !rerun && !turn.agent) return;
  const agent = turn.agent ? `<span class="dagent" title="The agent that answered">${esc(turn.agent)}</span>` : "";
  const row = h(`<div class="dreplyacts">${text ? `<button class="dcopy" data-act="copyReply">${COPY_ICON}Copy</button>` : ""}${rerun ? `<span class="drerun"><button class="dcopy" data-act="rerunMenu" aria-haspopup="menu" aria-expanded="false">${PLAY_ICON}Rerun<span class="dchev">${ICON.chev}</span></button></span>` : ""}${agent}</div>`);
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

// mode: list | edit | clear. filter: all | you | scope:<key> | read:<id>
const M = { all: [], list: [], sel: 0, query: "", mode: "list", error: "", loaded: false, scopes: new Map(), reads: [], filter: "all" };
const SCOPE_ICON = {
  machine: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>',
  project: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linejoin="round"><path d="M3 6.5A1.5 1.5 0 014.5 5H9l2 2.5h8.5A1.5 1.5 0 0121 9v9.5a1.5 1.5 0 01-1.5 1.5h-15A1.5 1.5 0 013 18.5z"/></svg>',
  account: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="10" r="3"/><path d="M6.5 18.5a6.5 6.5 0 0111 0"/></svg>',
};
/// A subject as the phone writes it: the machine's name, "Project atlas", "Github account".
function scopeLabel(sc) {
  if (sc.kind === "machine") return sc.name || "A machine";
  return sc.kind === "project" ? `Project ${sc.name}` : `${cap(sc.app)} account`;
}
function scopeSubject(sc) {
  if (sc.kind === "machine") return sc.name ? `the machine ${sc.name}` : "a machine";
  return sc.kind === "project" ? `the project ${sc.name}` : `your ${cap(sc.app)} account`;
}
const scopeKey = sc => `scope:${sc.kind}:${sc.id ?? sc.name ?? sc.app}`;
const scopeTag = sc => `<span class="dscope">${SCOPE_ICON[sc.kind] ?? ""}${esc(scopeLabel(sc))}</span>`;
/// "today", "yesterday" or "28 Oct", for a read's chip.
function readDay(at) {
  const day = d => new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const ago = Math.round((day(new Date()) - day(at)) / 86400000);
  return ago === 0 ? "today" : ago === 1 ? "yesterday" : `${at.getDate()} ${MONTHS[at.getMonth()]}`;
}
/// The chips over the list: everything, the user, each subject by name, then
/// the latest reads that added or confirmed memories. Empty when there is
/// nothing to pick between.
function memFilters() {
  const subjects = new Map();
  for (const sc of M.scopes.values()) {
    const key = scopeKey(sc), f = subjects.get(key) ?? { key, label: scopeLabel(sc), about: `What Beep knows about ${scopeSubject(sc)}.`, ids: new Set() };
    subjects.set(key, f);
  }
  for (const m of M.all) { const sc = M.scopes.get(m.id); if (sc) subjects.get(scopeKey(sc)).ids.add(m.id); }
  const have = new Set(M.all.map(m => m.id));
  const reads = M.reads.filter(r => r.memories.some(id => have.has(id))).slice(0, 3).map(r => {
    const at = new Date(r.finished_at), day = readDay(at);
    const when = day === "today" || day === "yesterday" ? day : `on ${day}`;
    return { key: `read:${r.id}`, label: `${r.source}, ${day}`, read: true, ids: new Set(r.memories.filter(id => have.has(id))),
      about: `What Beep learned or confirmed reading ${r.source} ${when} at ${clock(at.getHours(), at.getMinutes())}.` };
  });
  const scoped = [...subjects.values()].filter(f => f.ids.size).sort((a, b) => a.label.toLowerCase() < b.label.toLowerCase() ? -1 : 1);
  if (!scoped.length && !reads.length) return [];
  const you = new Set(M.all.filter(m => !M.scopes.has(m.id)).map(m => m.id));
  return [{ key: "all", label: "Everything" }, { key: "you", label: "You", about: "What Beep knows about you.", ids: you }, ...scoped, ...reads];
}

/// "2 Sep" from `YYYY-MM-DD`.
function shortDate(day) {
  const [, m, d] = String(day ?? "").split("-").map(Number);
  return m && d ? `${d} ${MONTHS[m - 1]}` : "";
}

function renderMemories() {
  S.view = "memories";
  invoke("close_group").catch(() => {});
  Object.assign(M, { sel: 0, query: "", mode: "list", error: "", filter: "all" });
  bar.innerHTML = `<div class="dinput"><input id="mfind" placeholder="Search memories" autocomplete="off" spellcheck="false"><span class="dmuted" id="mcount"></span><kbd>⌘M</kbd></div>
    <div id="mchips" style="border-top:1px solid var(--border)"></div>
    <div class="dlist" id="mlist" role="listbox" aria-label="Memories" style="padding-top:2px"></div><div id="mhints"></div>`;
  const find = $("#mfind");
  find.focus();
  find.addEventListener("input", () => { M.query = find.value.trim(); M.sel = 0; M.mode = "list"; drawMemories(); });
  $("#mlist").addEventListener("click", memClick);
  $("#mchips").addEventListener("click", e => {
    const chip = e.target.closest("[data-mf]");
    if (chip) { M.filter = chip.dataset.mf; M.sel = 0; M.mode = "list"; drawMemories(); $("#mfind")?.focus(); }
  });
  drawMemories();
  loadMemories();
}

async function loadMemories() {
  try {
    // Subjects and reads only add the chips; a server without them still lists memories.
    const [all, scopes, reads] = await Promise.all([invoke("list_memories"), invoke("list_memory_scopes").catch(() => []), invoke("list_memory_reads").catch(() => [])]);
    M.all = all;
    M.scopes = new Map(scopes.map(x => [x.id, x.scope]));
    M.reads = reads;
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
  // A search looks through everything, so the chips go while it is on.
  const filters = q ? [] : memFilters(), picked = filters.find(f => f.key === M.filter) ?? filters[0];
  if (!picked || picked.key === "all") M.filter = "all";
  M.list = M.all.filter(m => (!q || [m.content, m.category, m.quote].some(t => t?.toLowerCase().includes(q))) && (!picked?.ids || picked.ids.has(m.id)));
  const chips = $("#mchips");
  if (chips) {
    const chip = f => `<button class="dchip" aria-pressed="${f === picked}" data-mf="${esc(f.key)}">${esc(f.label)}${f.ids ? ` <span style="color:var(--muted)">${f.ids.size}</span>` : ""}</button>`;
    const firstRead = filters.findIndex(f => f.read);
    chips.innerHTML = filters.length ? `<div class="dchips" style="padding:8px 16px 2px">${filters.map((f, i) => (i === firstRead ? '<span class="dsep"></span>' : "") + chip(f)).join("")}</div>`
      + (picked?.about ? `<div class="dmuted" style="padding:2px 18px 4px">${esc(picked.about)}</div>` : "") : "";
  }
  // Grouped by category; the server lists newest first, so each group keeps that order.
  const cats = [...new Set(M.list.map(m => m.category || "Other"))];
  M.list = cats.flatMap(c => M.list.filter(m => (m.category || "Other") === c));
  M.sel = Math.max(0, Math.min(M.sel, M.list.length - 1));
  $("#mcount").textContent = M.loaded ? (q || M.filter !== "all" ? `${M.list.length} of ${M.all.length}` : String(M.all.length)) : "";
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
        const sc = M.scopes.get(m.id), about = sc ? scopeTag(sc) + " · " : "";
        if (on && M.mode === "edit") {
          html += `<div class="dmem" aria-selected="true" data-i="${i}"><div class="top"><textarea id="medit" rows="2" aria-label="Memory">${esc(m.content)}</textarea>
            <span style="font-size:12px;color:var(--muted);flex:none;display:inline-flex;gap:4px;align-items:center">${ICON.ret} save</span></div><div class="m">${about}${esc(meta)}</div></div>`;
        } else {
          const acts = on ? `<span class="acts"><button class="dbtn" data-m="edit">Edit</button><button class="dbtn warn" data-m="forget">Forget<kbd>⌘⌫</kbd></button></span>` : "";
          html += `<div class="dmem${ended ? " ended" : ""}" role="option" aria-selected="${on}" data-i="${i}"><div class="top"><span class="c">${mark(m.content, M.query)}</span>${acts}</div>
            <div class="m">${about}${esc(meta)}</div>${m.quote ? `<q>${mark(m.quote, M.query)}</q>` : ""}</div>`;
      }
    });
    if (M.all.length && !M.query && M.filter === "all") html += `<div style="padding:6px 10px 4px"><button class="dbtn warn" data-m="clear">Forget all…</button></div>`;
  }
  list.innerHTML = html;
  list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  const edit = $("#medit");
  if (edit) { edit.focus(); edit.setSelectionRange(edit.value.length, edit.value.length); }
  $("#mhints").innerHTML = M.mode === "edit"
    ? hints([[ICON.ret, "save"], [`⇧${ICON.ret}`, "new line"], ["esc", "cancel"]])
    : hints([["↑↓", "move"], [ICON.ret, "edit"], ["⌘⌫", "forget"], ...(filters.length ? [["tab", "filter"]] : []), ["esc", "back"]]);
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
  if (e.key === "Tab") {
    e.preventDefault();
    const filters = M.query ? [] : memFilters();
    if (!filters.length) return;
    const at = Math.max(0, filters.findIndex(f => f.key === M.filter));
    M.filter = filters[(at + (e.shiftKey ? filters.length - 1 : 1)) % filters.length].key;
    M.sel = 0;
    drawMemories();
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
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
// The routine form, new or editing `edit` (a cron). repeat: day | weekdays |
// week | hour | once | cron, or place for a routine a place starts, which
// the form leaves as it is.
const NR = { edit: null, label: "", prompt: "", repeat: "day", days: [1], time: "08:00", date: "", cron: "", error: "", busy: false };
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
  // The header's Grants count; a failure leaves it out.
  invoke("list_grants").then(list => { GR.list = list; GR.loaded = true; if (S.view === "routines") drawRoutines(); }).catch(() => {});
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
  const head = `<div class="dhead"><span class="tt"><b>Routines</b><small>${sum}</small></span><button class="dbtn" data-r="grants">Grants${GR.list.length ? ` <span style="color:var(--muted)">${GR.list.length}</span>` : ""}<kbd>⌘G</kbd></button><button class="dbtn" data-r="new">${PLUS_ICON}New<kbd>⌘N</kbd></button></div>`;
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
        ? `<span class="acts"><button class="dbtn" data-r="run">${PLAY_ICON}Run now</button><button class="dbtn" data-r="edit">Edit<kbd>⌘E</kbd></button><button class="dbtn warn" data-r="delete">Delete<kbd>⌘⌫</kbd></button></span>`
        : `<button class="dswitch" role="switch" aria-checked="${c.enabled}" aria-label="${label} on" data-r="toggle" data-i="${i}"></button>`;
      return `<div class="drt${c.enabled ? "" : " off"}" role="option" aria-selected="${cur}" data-i="${i}"><span class="ic">${CLOCK_ICON}</span>
        <span class="tt"><b>${label}</b><span>${esc(c.prompt)}</span><small>${esc([routineWhen(c.schedule), next].filter(Boolean).join(" · "))}</small></span>${tail}</div>`;
    }).join("");
    bar.innerHTML = head + err + `<div class="dlist" id="rlist" role="listbox" aria-label="Routines" style="padding-top:8px">${rows}</div>`
      + hints(R.mode === "actions" ? [["⌘E", "edit"], ["tab", "hide actions"], ["esc", "back"]] : [["↑↓", "move"], ["space", "on or off"], ["tab", "actions"], ["⌘N", "new routine"], ["⌘G", "grants"], ["esc", "back"]]);
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
  if (a === "grants") return renderGrants();
  if (a === "edit") return c && renderRoutineEdit(c);
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
  Object.assign(NR, { edit: null, label: "", prompt: "", repeat: "day", days: [1], time: "08:00", cron: "", error: "", busy: false,
    date: `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}` });
  drawRoutineNew();
  $("#nrlabel").focus();
}

const pad2 = n => String(n).padStart(2, "0");
const dateValue = d => `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;

/// The form fields for `schedule`, the shapes routineSchedule writes read
/// back. A place trigger stays as it is, anything else shows as cron.
function routineFields(schedule) {
  if (/^(arrive|leave):/.test(schedule)) return { repeat: "place" };
  if (/^\d{4}-\d\d-\d\dT/.test(schedule)) {
    const at = new Date(schedule);
    if (!isNaN(at)) return { repeat: "once", date: dateValue(at), time: `${pad2(at.getHours())}:${pad2(at.getMinutes())}` };
  }
  const p = schedule.trim().split(/\s+/);
  const cron = { repeat: "cron", cron: schedule };
  if (p.length !== 5 || p[2] !== "*" || p[3] !== "*" || !/^\d+$/.test(p[0])) return cron;
  if (p[1] === "*") return p[4] === "*" ? { repeat: "hour", time: `00:${pad2(p[0])}` } : cron;
  if (!/^\d+$/.test(p[1])) return cron;
  const time = `${pad2(p[1])}:${pad2(p[0])}`;
  if (p[4] === "*") return { repeat: "day", time };
  if (p[4] === "1-5") return { repeat: "weekdays", time };
  if (/^[0-6](,[0-6])*$/.test(p[4])) return { repeat: "week", time, days: p[4].split(",").map(Number) };
  return cron;
}

/// Edits routine `c`'s name and schedule. What it does stays as it is.
function renderRoutineEdit(c) {
  S.view = "rnew";
  Object.assign(NR, { edit: c, label: c.label, prompt: c.prompt, repeat: "day", days: [1], time: "08:00", cron: "", error: "", busy: false,
    date: dateValue(new Date()) }, routineFields(c.schedule));
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
  const missing = NR.error === "prompt", edit = NR.edit;
  const text = t => `<div class="dmuted" style="display:block;font-size:13.5px;color:var(--fg)">${esc(t)}</div>`;
  const task = edit
    ? `<div class="dfield"><span>What Beep does</span>${text(NR.prompt)}</div>`
    : `<label class="dfield${missing ? " err" : ""}"><span>${missing ? "Tell Beep what to do" : "What should Beep do?"}</span>
        <textarea id="nrprompt" rows="2" placeholder="Summarise my calendar and the weather">${esc(NR.prompt)}</textarea></label>`;
  const schedule = NR.repeat === "place"
    ? `<div class="dfield"><span>Starts</span>${text(routineWhen(edit.schedule))}<span>Change the place in a chat.</span></div>`
    : `<div class="dfield"><span>Repeat</span><div class="dchips">${chip("day", "Every day")}${chip("weekdays", "Weekdays")}${chip("week", "Weekly")}${chip("hour", "Every hour")}${chip("once", "Once")}${chip("cron", "Cron")}</div></div>
      ${when}`;
  const go = edit ? "Save" : "Create";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>${edit ? "Edit routine" : "New routine"}</b><small>${edit ? esc(edit.label || "Reminder") : "Runs on this account, phone or computer off"}</small></span></div>
    <div class="dform" style="padding-top:12px;flex:1;min-height:0;overflow:auto;scrollbar-width:none;align-content:start">
      <label class="dfield"><span>Name</span><input id="nrlabel" placeholder="Morning briefing" value="${esc(NR.label)}"></label>
      ${task}
      ${schedule}
      ${NR.error && !missing ? `<div class="derr">${esc(NR.error)}</div>` : ""}
      <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri" id="nrgo"${NR.busy ? " disabled" : ""}>${go}<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn" id="nrno">Cancel</button>
        <span class="dmuted" style="margin-left:auto">${edit ? "To change what it does, ask Beep in a chat" : "Or just tell Beep in a chat"}</span></div>
    </div>${hints([["tab", "next field"], [`⌘${ICON.ret}`, go.toLowerCase()], ["esc", "cancel"]])}`;
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
  if (NR.edit) return saveRoutine();
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

/// Whether two schedules are the same, a one-off in the same minute
/// written another way included.
function sameSchedule(a, b) {
  if (a === b) return true;
  const iso = /^\d{4}-\d\d-\d\dT/;
  // The form keeps minutes only.
  const minute = s => Math.floor(new Date(s).getTime() / 60000);
  return iso.test(a) && iso.test(b) && minute(a) === minute(b);
}

/// Saves the name and schedule of the routine being edited, sending only
/// what changed.
async function saveRoutine() {
  const c = NR.edit;
  const schedule = NR.repeat === "place" ? c.schedule : routineSchedule();
  NR.error = !schedule ? (NR.repeat === "week" ? "Pick at least one day" : "Set when it runs") : "";
  if (NR.error) return drawRoutineNew();
  const label = NR.label.trim();
  const changed = { label: label && label !== c.label ? label : null, schedule: sameSchedule(schedule, c.schedule) ? null : schedule };
  if (changed.label == null && changed.schedule == null) return renderRoutines();
  NR.busy = true;
  try {
    await invoke("update_cron", { id: c.id, ...changed });
    if (S.view === "rnew") renderRoutines();
  } catch (e) {
    NR.error = String(e);
    NR.busy = false;
    if (S.view === "rnew") drawRoutineNew();
  }
}

// ---------- Routine grants ----------

// What routines may do without asking (ListGrants). mode: list | confirm
const GR = { list: [], sel: 0, mode: "list", error: "", loaded: false };
const SHIELD_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 3l8 3v6c0 4.5-3.4 8.3-8 9-4.6-.7-8-4.5-8-9V6z"/></svg>';

/// "40 min", "5 h" or "6 d".
function span(ms) {
  const min = Math.max(1, Math.round(Math.abs(ms) / 60000));
  return min < 60 ? `${min} min` : min < 48 * 60 ? `${Math.round(min / 60)} h` : `${Math.round(min / 1440)} d`;
}

function renderGrants() {
  S.view = "grants";
  invoke("close_group").catch(() => {});
  Object.assign(GR, { sel: 0, mode: "list", error: "" });
  drawGrants();
  loadGrants();
}

async function loadGrants() {
  try {
    GR.list = await invoke("list_grants");
    GR.loaded = true;
    GR.error = "";
  } catch (e) {
    GR.error = String(e);
  }
  if (S.view === "grants") drawGrants();
  else if (S.view === "routines") drawRoutines();
}

function drawGrants() {
  GR.sel = Math.max(0, Math.min(GR.sel, GR.list.length - 1));
  const head = `<div class="dhead"><span class="tt"><b>Routine grants</b><small>${GR.loaded && !GR.list.length ? "Nothing granted" : "What routines may do without asking. Each ends on its own."}</small></span></div>`;
  const err = GR.error ? `<div class="derr" style="padding:8px 18px 0">${esc(GR.error)}</div>` : "";
  if (GR.loaded && !GR.list.length) {
    bar.innerHTML = head + err + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${SHIELD_ICON}</span>
      <b style="font-size:14px">No routine grants.</b><span class="dmuted" style="display:block">When a routine needs to act without asking, Beep asks you once in a chat.</span></div>` + hints([["esc", "back"]]);
  } else {
    const now = Date.now();
    const rows = GR.list.map((g, i) => {
      const on = i === GR.sel, short = g.action.replace(/ for a routine$/, "");
      if (on && GR.mode === "confirm") {
        return `<div class="drt" aria-selected="true" data-i="${i}"><span class="tt"><span style="font-size:14px">Revoke? <b>${esc(g.routine)}</b> will ask before each ${esc(short)} again.</span></span>
          <span class="acts"><button class="dbtn warn" data-g="revoke-yes">Revoke</button><button class="dbtn" data-g="list">Cancel</button></span></div>`;
      }
      const per = g.per_run === 1 ? "1 each run" : `up to ${g.per_run} each run`;
      const ends = g.expires_at > now ? `Ends in ${span(g.expires_at - now)}` : "Ended";
      const used = g.last_used_at ? `Last used ${span(now - g.last_used_at)} ago` : "Not used yet";
      const acts = on ? `<span class="acts"><button class="dbtn warn" data-g="revoke">Revoke<kbd>⌘⌫</kbd></button></span>` : "";
      const kv = on && g.fields.length ? `<span class="dgkv">${g.fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</span>` : "";
      return `<div class="drt" role="option" aria-selected="${on}" data-i="${i}" style="align-items:flex-start"><span class="dapp" style="margin-top:2px">${esc((g.app || "?")[0].toUpperCase())}</span>
        <span class="tt"><b>${esc(g.routine)}</b><span>${esc(`${g.app}: ${short}`)}, ${per}</span><small>${ends} · ${used}</small>${kv}</span>${acts}</div>`;
    }).join("");
    bar.innerHTML = head + err + `<div class="dlist" id="glist" role="listbox" aria-label="Routine grants" style="padding-top:8px">${rows}</div>`
      + hints(GR.mode === "confirm" ? [[ICON.ret, "revoke"], ["esc", "cancel"]] : [["↑↓", "move"], ["⌘⌫", "revoke"], ["esc", "back"]]);
    bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }
  bar.onclick = grantClick;
}

function grantClick(e) {
  if (S.view !== "grants") return;
  const btn = e.target.closest("[data-g]"), row = e.target.closest(".drt[data-i]");
  if (row) GR.sel = Number(row.dataset.i);
  if (btn) return grantAction(btn.dataset.g);
  if (row && GR.mode === "list") drawGrants();
}

async function grantAction(a) {
  const g = GR.list[GR.sel];
  if (!g) return;
  if (a === "list" || a === "revoke") {
    GR.mode = a === "revoke" ? "confirm" : "list";
    return drawGrants();
  }
  if (a !== "revoke-yes") return;
  try {
    await invoke("revoke_grant", { id: g.id });
    GR.list = GR.list.filter(x => x.id !== g.id);
    GR.error = "";
  } catch (e) {
    GR.error = String(e);
  }
  GR.mode = "list";
  if (S.view === "grants") drawGrants();
}

function grantKeys(e, cmd) {
  if (GR.mode === "confirm") {
    if (e.key === "Enter") { e.preventDefault(); grantAction("revoke-yes"); }
    return;
  }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    GR.sel = Math.max(0, Math.min(GR.list.length - 1, GR.sel + (e.key === "ArrowDown" ? 1 : -1)));
    drawGrants();
  } else if (cmd && e.key === "Backspace" && GR.list.length) {
    e.preventDefault();
    grantAction("revoke");
  }
}

// ---------- Machines ----------

// The machines running beep-node (ListMachines), from Settings.
const MC = { list: null, error: "" };
const SERVER_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>';
/// The coding agents Beep can drive on a machine; its other tools follow them.
const CODERS = new Set(["claude", "codex", "gemini", "opencode", "cursor-agent", "amp", "aider"]);

async function renderMachines() {
  S.view = "machines";
  invoke("close_group").catch(() => {});
  drawMachines();
  try {
    MC.list = await invoke("list_machines");
    MC.error = "";
  } catch (e) {
    MC.error = String(e);
  }
  if (S.view === "machines") drawMachines();
}

function machineCard(m) {
  const env = m.env, now = Date.now();
  const tag = m.online ? '<span class="dtag ok">Online</span>' : `<span class="dtag">Seen ${span(now - m.seen_at)} ago</span>`;
  const version = /^\d/.test(m.version) ? `beep-node ${m.version}` : m.version;
  if (!env) return `<div class="dmach"><span class="nm">${esc(m.name)}${tag}</span><small>${esc(version)} · No report yet</small></div>`;
  const coders = env.tools.filter(t => CODERS.has(t.name)), rest = env.tools.filter(t => !CODERS.has(t.name)).map(t => t.name);
  const logins = env.logins.filter(l => l.signed_in).map(l => l.account ? `${l.tool} as ${l.account}` : `${l.tool} signed in`);
  const also = [rest.length ? `Also ${rest.join(", ")}` : "", logins.join(", ")].filter(Boolean).join(" · ");
  return `<div class="dmach"><span class="nm">${esc(m.name)}${tag}</span>
    <small>${esc(`${env.os_version}, ${env.arch} · ${env.cpus} CPUs, ${Math.round(env.memory_mb / 1024)} GB · ${version}`)}</small>
    <span class="cd">${coders.length ? esc("Coding: " + coders.map(t => t.version ? `${t.name} ${t.version}` : t.name).join(", ")) : '<span style="color:var(--muted)">No coding tools</span>'}</span>
    ${also ? `<small>${esc(also)}</small>` : ""}</div>`;
}

function drawMachines() {
  const list = MC.list ?? [], on = list.filter(m => m.online).length;
  const sum = !MC.list ? "Where Beep runs commands and coding agents" : [on && `${on} online`, list.length - on && `${list.length - on} offline`].filter(Boolean).join(", ");
  const head = `<div class="dhead"><span class="tt"><b>Machines</b><small>${esc(sum || "Where Beep runs commands and coding agents")}</small></span></div>`;
  const err = MC.error ? `<div class="derr" style="padding:8px 18px 0">${esc(MC.error)}</div>` : "";
  const setup = '<code>beep-node setup</code>';
  bar.innerHTML = head + err + (MC.list && !list.length
    ? `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${SERVER_ICON}</span><b style="font-size:14px">No machines yet.</b>
      <span class="dmuted" style="display:block">Press Add a machine in the Beep window and run the ${setup} command it shows on that computer, or turn on Let Beep use this Mac in Settings.</span></div>`
    : `<div class="dlist" style="padding-top:6px">${list.map(machineCard).join("")}</div>
      ${MC.list ? `<div class="dmuted" style="display:block;padding:0 18px 12px">To add one, press Add a machine in the Beep window and run the ${setup} command it shows.</div>` : ""}`)
    + hints([["esc", "back"]]);
}

// ---------- Coding agents ----------

// Start a coding agent on one of the user's machines, follow it live and
// answer it: `agentnew` is the start form, `agent` one session, `agents`
// every session. They run in the bar and in the window's pane alike.
const AG = {
  hosts: null, sessions: [], error: "", busy: false,
  form: { prompt: "", machine: "auto", agent: null, cwd: "", chat: null },
  open: null, page: null, why: "", back: "chat", timer: 0, inflight: null, note: "", sel: 0,
  // Polling: what the last answer looked like, when it last changed,
  // whether it stopped for quiet, and the question an answer is going to.
  sig: "", changed: 0, paused: false, answering: null, barHidden: false,
  // Images pasted or dropped into the session's field, the error from the
  // last one, and the session the server turned images down for.
  att: [], attError: "", noImages: null,
};
/// What a coding agent takes with a message, as the server checks it.
const AGENT_IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
const AGENT_MAX_IMAGES = 5;
const AGENT_MAX_IMAGE = 5 * 1024 * 1024;
const AGENT_MAX_IMAGES_TOTAL = 7 * 1024 * 1024;
/// Thumbnails of images sent from here, by session and name, so the
/// transcript's `[image: name]` lines show the picture.
const agentPics = new Map();
const AGENT_PICS_KEPT = 60;
let pastedCount = 0;
/// How often the session view asks for its session while on screen.
const AGENT_POLL_MS = 1500;
/// A quiet session (idle, or started outside Beep) unchanged this long is
/// asked every `AGENT_SLOW_MS`, and after `AGENT_PAUSE_AFTER` no more.
const AGENT_SLOW_AFTER = 2 * 60_000;
const AGENT_SLOW_MS = 10_000;
const AGENT_PAUSE_AFTER = 5 * 60_000;
const AGENT_ACTIVE = new Set(["asking", "working", "running", "starting"]);
const AGENT_LAST = 60;
const AGENT_KINDS = {
  claude: { ini: "CC", name: "Claude Code", cls: "claude" },
  codex: { ini: "CX", name: "Codex", cls: "codex" },
  gemini: { ini: "GE", name: "Gemini CLI", cls: "gemini" },
  pi: { ini: "PI", name: "pi", cls: "pi" },
  flsh: { ini: "FL", name: "flsh", cls: "flsh" },
};
const AGENT_ORDER = ["claude", "codex", "gemini", "pi", "flsh"];
const CAP_NAMES = { gpu: "GPU", cuda: "CUDA", blender: "Blender", unreal: "Unreal", xcode: "Xcode", android: "Android", docker: "Docker" };
const AGENT_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>';

const agentKind = a => AGENT_KINDS[a] ?? { ini: String(a || "?").slice(0, 2).toUpperCase(), name: a || "Agent", cls: "" };
const agentAvatar = a => `<span class="dapp dcoderav ${agentKind(a).cls}">${esc(agentKind(a).ini)}</span>`;
const folderName = p => String(p || "").split("/").filter(Boolean).pop() || p || "";
const byPreference = (a, b) => (AGENT_ORDER.indexOf(a) + 1 || 99) - (AGENT_ORDER.indexOf(b) + 1 || 99);

/// The machines that can run an agent; none for an account that may not use machines.
async function loadAgentHosts() {
  if (S.link !== "linked") return;
  try {
    AG.hosts = await invoke("agent_hosts");
  } catch {
    AG.hosts = [];
  }
  drawAgentButton();
  if (S.view === "agentnew") drawAgentNew();
}

/// The composer's Agent button, once some machine offers an agent.
function drawAgentButton() {
  const row = bar.querySelector(":scope > .dinput.dgrow");
  if (!row || S.view !== "chat" || $("#agentGo") || !AG.hosts?.length) return;
  const b = h(`<button class="dbtn quiet" id="agentGo" title="Start a coding agent on a machine (⌘⇧A)">${AGENT_ICON}Agent</button>`);
  b.onclick = () => renderAgentNew($("#ask")?.value.trim() ?? "");
  row.insertBefore(b, $("#grip"));
}

function stateTag(status) {
  if (status === "asking") return '<span class="dstate ask">Waiting for you</span>';
  if (status === "working" || status === "running" || status === "starting") return `<span class="dstate run">${status === "starting" ? "Starting" : "Working"}</span>`;
  if (status === "waiting") return '<span class="dstate ask">Waiting in terminal</span>';
  if (status === "ended") return '<span class="dstate">Ended</span>';
  return '<span class="dstate">Idle</span>';
}

// ----- Start form -----

/// The start form with `prompt`; `from` is where Esc goes back to.
/// `at` presets the machine and folder, as a project's "Start an agent here" does.
function renderAgentNew(prompt = "", from = "chat", at = null) {
  const chat = S.view === "chat" ? S.session : null;
  S.view = "agentnew";
  AG.back = from;
  AG.form = { prompt, machine: at?.machine || "auto", agent: null, cwd: at?.cwd || "", chat };
  AG.error = "";
  AG.busy = false;
  drawAgentNew();
  $("#agprompt")?.focus();
  loadAgentHosts();
  invoke("agent_sessions").then(list => { AG.sessions = list; if (S.view === "agentnew") drawFolders(); }).catch(() => {});
}

function formHost() { return AG.hosts?.find(m => m.machine_id === AG.form.machine) ?? null; }
/// The agents the chosen machine offers; for Auto, any machine's.
function formAgents() {
  const host = formHost();
  const all = host ? host.agents : [...new Set((AG.hosts ?? []).flatMap(m => m.agents))];
  return all.sort(byPreference);
}

function drawAgentNew() {
  const f = AG.form, hosts = AG.hosts ?? [], host = formHost();
  const agents = formAgents();
  if (f.agent && !agents.includes(f.agent)) f.agent = null;
  const chip = (attr, val, on, label, sub) => `<button class="dchip" aria-pressed="${on}" ${attr}="${esc(val)}"${sub ? ` title="${esc(sub)}"` : ""}>${label}</button>`;
  const caps = m => m.capabilities.map(c => CAP_NAMES[c] || c);
  const machines = [chip("data-agm", "auto", f.machine === "auto", "Auto", "Beep picks from what each machine has"),
    ...hosts.map(m => chip("data-agm", m.machine_id, f.machine === m.machine_id, esc(m.machine_name), caps(m).join(", ")))].join("");
  const machineNote = !AG.hosts ? '<span class="dspin"></span> Looking for machines…'
    : !hosts.length ? "None of your machines online offers a coding agent. Run beep-node on one with Claude Code, Codex, Gemini CLI, pi or flsh."
    : host ? (caps(host).length ? `Has ${esc(caps(host).join(", "))}` : "No special hardware or apps reported")
    : "Beep picks the machine with what the task needs, such as Xcode, a GPU or Blender, else your environment or the least busy one.";
  const agentChips = [chip("data-aga", "", !f.agent, "Any"), ...agents.map(a => chip("data-aga", a, f.agent === a, esc(agentKind(a).name)))].join("");
  const place = host ? (host.workspace || host.home || "Home folder") : "Workspace or home folder";
  const missing = AG.error === "prompt";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Start a coding agent</b><small>It runs on your machine; follow it and answer it here</small></span>
      <button class="dbtn quiet" id="agList">All sessions</button></div>
    <div class="dform" style="padding-top:12px;flex:1;min-height:0;overflow:auto;scrollbar-width:none;align-content:start">
      <label class="dfield${missing ? " err" : ""}"><span>${missing ? "Say what the agent should do" : "What should it do?"}</span>
        <textarea id="agprompt" rows="3" placeholder="Fix the failing login test">${esc(f.prompt)}</textarea></label>
      <div class="dfield"><span>Machine</span><div class="dchips">${machines}</div><span class="note">${machineNote}</span></div>
      <label class="dfield"><span>Folder</span><input id="agcwd" list="agfolders" spellcheck="false" placeholder="${esc(place)}" value="${esc(f.cwd)}" style="font-family:var(--mono);font-size:13px"><datalist id="agfolders"></datalist></label>
      <div class="dfield"><span>Agent</span><div class="dchips">${agentChips}</div></div>
      ${AG.error && !missing ? `<div class="derr">${esc(AG.error)}</div>` : ""}
      <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri" id="aggo"${AG.busy || !hosts.length ? " disabled" : ""}>${AG.busy ? '<span class="dspin"></span>Starting' : "Start"}<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn" id="agno">Cancel</button>
        <span class="dmuted" style="margin-left:auto">Or ask Beep in a chat to start one</span></div>
    </div>${hints([["tab", "next field"], [`⌘${ICON.ret}`, "start"], ["esc", "cancel"]])}`;
  drawFolders();
  $("#agprompt").addEventListener("input", e => { f.prompt = e.target.value; });
  $("#agcwd").addEventListener("input", e => { f.cwd = e.target.value; });
  bar.onclick = e => {
    if (S.view !== "agentnew") return;
    const m = e.target.closest("[data-agm]"), a = e.target.closest("[data-aga]");
    if (m) { f.machine = m.dataset.agm; drawAgentNew(); }
    else if (a) { f.agent = a.dataset.aga || null; drawAgentNew(); }
    else if (e.target.closest("#aggo")) startAgent();
    else if (e.target.closest("#agno")) back();
    else if (e.target.closest("#agList")) renderAgents();
  };
}

/// Folder suggestions: the machine's workspace and home, then where its sessions ran.
function drawFolders() {
  const list = $("#agfolders");
  if (!list) return;
  const host = formHost(), hosts = host ? [host] : AG.hosts ?? [];
  const ids = new Set(hosts.map(m => m.machine_id));
  const seen = [...hosts.flatMap(m => [m.workspace, m.home]),
    ...AG.sessions.filter(s => ids.has(s.machine_id)).sort((a, b) => b.last_activity - a.last_activity).map(s => s.cwd)];
  list.innerHTML = [...new Set(seen.filter(Boolean))].slice(0, 12).map(p => `<option value="${esc(p)}"></option>`).join("");
}

async function startAgent() {
  const f = AG.form;
  if (AG.busy || S.view !== "agentnew") return;
  if (!f.prompt.trim()) { AG.error = "prompt"; drawAgentNew(); return $("#agprompt")?.focus(); }
  AG.busy = true;
  AG.error = "";
  drawAgentNew();
  try {
    const started = await invoke("agent_start", {
      machineId: f.machine === "auto" ? null : f.machine,
      agent: f.agent,
      cwd: f.cwd.trim() || null,
      prompt: f.prompt.trim(),
      chat: f.chat,
    });
    AG.busy = false;
    if (S.view !== "agentnew") return;
    // Back from the session goes where the form came from.
    openAgent(started.session, { why: started.why, back: AG.back });
  } catch (e) {
    AG.busy = false;
    AG.error = String(e).replace(/^bad request: /, "");
    if (S.view === "agentnew") drawAgentNew();
  }
}

// ----- One session -----

/// Opens session `s` (a session row) and follows it while it is on screen.
function openAgent(s, { why = "", back: from } = {}) {
  if (from === undefined) from = S.view === "agents" ? "agents" : "chat";
  S.view = "agent";
  AG.open = { machine_id: s.machine_id, agent: s.agent, session: s.id };
  AG.page = { session: s, plan: [], files: [], entries: [], earlier: 0, last_message: "", error: null, queued: 0, turns: 0 };
  AG.why = why;
  AG.back = from;
  AG.note = "";
  AG.sig = "";
  AG.changed = Date.now();
  AG.paused = false;
  AG.answering = null;
  AG.att = [];
  AG.attError = "";
  AG.noImages = null;
  drawAgentFrame();
  pollAgent();
}

const isOpen = o => S.view === "agent" && AG.open && o === AG.open;

/// Whether the session view can be seen: the window is not hidden, and
/// the bar, when this is the bar, is up.
const agentOnScreen = () => document.visibilityState === "visible" && !AG.barHidden;

/// How long until the open session is asked again, or null to stop: an
/// ended session, or a quiet one that has not changed for a while.
function agentDelay() {
  const s = AG.page?.session;
  if (!s || s.status === "ended") return null;
  if (!s.external && AGENT_ACTIVE.has(s.status)) return AGENT_POLL_MS;
  const still = Date.now() - AG.changed;
  if (still >= AGENT_PAUSE_AFTER) return null;
  return still >= AGENT_SLOW_AFTER ? AGENT_SLOW_MS : AGENT_POLL_MS;
}

/// Asks for the open session, then again after `agentDelay()` while it
/// stays on screen. Never two calls at once for one session.
async function pollAgent() {
  clearTimeout(AG.timer);
  const o = AG.open;
  if (!isOpen(o) || AG.inflight === o || !agentOnScreen()) return;
  AG.inflight = o;
  try {
    const page = await invoke("agent_view", { machineId: o.machine_id, agent: o.agent, session: o.session, last: AGENT_LAST });
    if (isOpen(o)) {
      const sig = JSON.stringify(page);
      if (sig !== AG.sig) { AG.sig = sig; AG.changed = Date.now(); }
      if (page.session.question?.id !== AG.answering) AG.answering = null;
      AG.page = page;
      AG.note = AG.note === "gone" ? "" : AG.note;
      drawAgentBody();
    }
  } catch (e) {
    if (isOpen(o)) { AG.note = /not found/i.test(String(e)) ? "gone" : String(e); drawAgentBody(); }
  } finally {
    if (AG.inflight === o) AG.inflight = null;
  }
  // Another session opened while this one was asked: follow that one.
  if (AG.open !== o) { if (isOpen(AG.open) && !AG.inflight) pollAgent(); return; }
  if (!isOpen(o) || !agentOnScreen()) return;
  const delay = agentDelay();
  const paused = delay === null && AG.page?.session.status !== "ended";
  if (paused !== AG.paused) { AG.paused = paused; drawAgentBody(); }
  if (delay !== null) AG.timer = setTimeout(pollAgent, delay);
}

/// Follows the open session again after a pause or a message.
function resumeAgent() {
  AG.changed = Date.now();
  AG.paused = false;
  pollAgent();
}

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") { if (S.view === "agent") pollAgent(); } else clearTimeout(AG.timer);
});
if (!PANE) {
  listen("bar-hidden", () => { AG.barHidden = true; clearTimeout(AG.timer); });
  listen("bar-shown", () => { AG.barHidden = false; if (S.view === "agent") pollAgent(); });
}

/// Whether the server turned images down for the open session.
const noAgentImages = () => AG.noImages != null && AG.noImages === AG.open?.session;

function agentHints() {
  const s = AG.page?.session, q = s?.question;
  if (q) return hints([[`⌘${ICON.ret}`, "allow"], ["⌘D", "deny"], ["esc", "back"]]);
  if (s?.external || s?.status === "ended") return hints([["esc", "back"]]);
  // Once one is attached its chip says enough.
  const paste = !AG.att.length && !noAgentImages() ? [["⌘V", "paste an image"]] : [];
  return hints([...paste, [ICON.ret, "send"], ["⌘.", "stop the turn"], ["esc", "back"]]);
}

// ----- Images for the agent -----

/// A name for an image, unique among those waiting; a pasted one is
/// named "Pasted image 1.png" and so on rather than "image.png".
function agentImageName(f) {
  const ext = { "image/png": "png", "image/jpeg": "jpg", "image/gif": "gif", "image/webp": "webp" }[f.type] || "png";
  let name = f.name && f.name !== `image.${ext}` && f.name !== "image.png" ? f.name : `Pasted image ${++pastedCount}.${ext}`;
  const dot = name.lastIndexOf(".");
  const base = name;
  for (let n = 2; AG.att.some(a => a.name === name); n++) name = dot > 0 ? `${base.slice(0, dot)} ${n}${base.slice(dot)}` : `${base} ${n}`;
  return name;
}

function agentAttachError(text) {
  AG.attError = text;
  drawAgentAttached();
  clearTimeout(AG.attTimer);
  AG.attTimer = setTimeout(() => { AG.attError = ""; drawAgentAttached(); }, 5000);
}

/// Adds pasted or dropped images to the session's next message, refusing
/// what the agent cannot take rather than shrinking or dropping it.
async function agentAttach(files) {
  if (noAgentImages()) return agentAttachError("This Beep server does not take images yet.");
  for (const f of [...files]) {
    if (!AGENT_IMAGE_TYPES.includes(f.type)) { agentAttachError(`${f.name || "That file"} is not a PNG, JPEG, GIF or WebP image`); continue; }
    if (AG.att.length >= AGENT_MAX_IMAGES) { agentAttachError(`Up to ${AGENT_MAX_IMAGES} images in one message`); break; }
    if (f.size > AGENT_MAX_IMAGE) { agentAttachError(`${f.name || "That image"} is over ${AGENT_MAX_IMAGE >> 20} MB`); continue; }
    const used = AG.att.reduce((n, a) => n + a.size, 0);
    if (used + f.size > AGENT_MAX_IMAGES_TOTAL) { agentAttachError(`Images up to ${AGENT_MAX_IMAGES_TOTAL >> 20} MB in all in one message`); continue; }
    try {
      const data = await fileBase64(f);
      AG.att.push({ name: agentImageName(f), mime: f.type, size: f.size, data, url: `data:${f.type};base64,${data}` });
    } catch {
      agentAttachError(`${f.name || "That image"} could not be read`);
    }
  }
  drawAgentAttached();
}

/// The chips of waiting images above the field, as in the mock.
function drawAgentAttached() {
  const field = $("#agfield");
  if (!field) return;
  const old = $("#agatt");
  const el = AG.att.length || AG.attError ? h(`<div class="ag-att" id="agatt">${AG.att.map((a, i) =>
    `<span class="chip"><img src="${esc(a.url)}" alt=""><span class="nm">${esc(a.name)}</span><button aria-label="Remove ${esc(a.name)}" data-agatt="${i}">${ICON.x}</button></span>`).join("")}${AG.attError ? `<span class="derr">${esc(AG.attError)}</span>` : ""}</div>`) : null;
  el?.querySelectorAll("[data-agatt]").forEach(b => b.addEventListener("click", () => {
    AG.att.splice(Number(b.dataset.agatt), 1);
    drawAgentAttached();
    $("#agmsg")?.focus();
  }));
  if (old) el ? old.replaceWith(el) : old.remove();
  else if (el) field.before(el);
  const hint = bar.querySelector(":scope > .dhints");
  if (hint && S.view === "agent") hint.replaceWith(h(agentHints()));
}

/// Keeps the thumbnails of images just sent, for the transcript.
function keepAgentPics(o, images) {
  for (const a of images) {
    const key = `${o.session}\n${a.name}`;
    agentPics.delete(key);
    agentPics.set(key, a.url);
  }
  while (agentPics.size > AGENT_PICS_KEPT) agentPics.delete(agentPics.keys().next().value);
}

/// The parts that stay while the session updates: the head, the body and the field.
function drawAgentFrame() {
  const s = AG.page.session, k = agentKind(s.agent);
  const field = s.external
    ? `<div class="dmuted" style="display:block;padding:10px 16px;border-top:1px solid var(--border)">Started outside Beep. Beep follows it here; to steer it, ask Beep in a chat to take it over once it stops.</div>`
    : `<div class="dinput small" id="agfield"><input id="agmsg" autocomplete="off" spellcheck="false" placeholder="Message ${esc(k.name)}"><button class="dbtn" id="agstop" aria-label="Stop the turn"><span style="width:9px;height:9px;border-radius:2px;background:currentColor"></span>Stop<kbd>⌘.</kbd></button><button class="dbtn quiet" id="agclose">Close session</button></div>`;
  bar.innerHTML = `<div class="dhead">${agentAvatar(s.agent)}<span class="tt"><b>${esc(k.name)} on ${esc(s.machine_name)}</b><small style="font-family:var(--mono)">${esc(s.cwd)}</small></span><span id="agstate">${stateTag(s.status)}</span></div>
    <div class="dbody" id="agbody" style="padding-top:10px"></div>${field}${agentHints()}`;
  drawAgentAttached();
  const b = $("#agbody");
  b.stuck = true;
  b.onscroll = () => { b.stuck = b.scrollHeight - b.scrollTop - b.clientHeight < 40; };
  drawAgentBody();
  $("#agmsg")?.focus();
  bar.onclick = e => {
    if (S.view !== "agent") return;
    const opt = e.target.closest("[data-agopt]");
    if (opt) return answerAgentQuestion(Number(opt.dataset.agopt));
    if (e.target.closest("#agresume")) return resumeAgent();
    if (e.target.closest("#agstop")) return steerAgent({ cancel: null });
    if (e.target.closest("#agclose")) return steerAgent({ close: null });
  };
}

/// A user entry's text and the images it carried: the node writes each
/// as a line `[image: name]` after the text.
function userImages(text) {
  const lines = String(text).split("\n"), names = [];
  while (lines.length) {
    const m = /^\[image: (.+)\]$/.exec(lines[lines.length - 1]);
    if (!m) break;
    names.unshift(m[1]);
    lines.pop();
  }
  return { text: lines.join("\n"), names };
}

function entryRow(en) {
  const text = esc(en.text);
  if (en.kind === "user") {
    const { text: said, names } = userImages(en.text);
    if (!names.length) return `<div class="dcoderen user"><b>You</b><span>${text}</span></div>`;
    const pics = names.map(n => {
      const url = AG.open && agentPics.get(`${AG.open.session}\n${n}`);
      return url ? `<img class="pic" src="${esc(url)}" alt="${esc(n)}" title="${esc(n)}">` : `<span class="pic none" title="${esc(n)}">${ICON.image}${esc(n)}</span>`;
    }).join("");
    return `<div class="dcoderen user"><b>You</b>${said ? `<span>${esc(said)}</span>` : ""}<span class="pics">${pics}</span></div>`;
  }
  if (en.kind === "agent") return `<div class="dcoderen agent">${text}</div>`;
  if (en.kind === "tool") return `<div class="dcoderen tool">${text}</div>`;
  return `<div class="dcoderen other">${text}</div>`;
}

function drawAgentBody() {
  const b = $("#agbody"), p = AG.page;
  if (!b || !p) return;
  const s = p.session, q = s.question;
  const st = $("#agstate");
  if (st) st.innerHTML = stateTag(s.status);
  const mark = x => x === "completed" ? `<span class="dok">${ICON.check}</span>` : x === "in_progress" ? (s.status === "asking" ? '<span class="ddot" style="background:var(--coral)"></span>' : '<span class="dspin"></span>') : '<span class="ddot off"></span>';
  const doing = s.doing && s.status !== "ended" ? `<div class="row" style="padding:8px 12px 0;color:var(--muted)">${esc(s.doing)}</div>` : "";
  const question = q ? `<div class="q"><b style="font-weight:600">${esc(q.text)}</b>${q.detail ? `<code style="font-family:var(--mono);font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere">${esc(q.detail)}</code>` : ""}
      <div class="dups">${q.options.map((o, i) => `<button class="dbtn${o.allow && i === q.options.findIndex(x => x.allow) ? " pri" : !o.allow && i === q.options.findIndex(x => !x.allow) ? " warn" : ""}" data-agopt="${i}"${q.id === AG.answering ? " disabled" : ""}>${esc(o.name)}${o.allow && i === q.options.findIndex(x => x.allow) ? `<kbd>⌘${ICON.ret}</kbd>` : !o.allow && i === q.options.findIndex(x => !x.allow) ? "<kbd>⌘D</kbd>" : ""}</button>`).join("")}</div></div>` : "";
  const plan = p.plan.length ? `<div class="sec">Plan</div><div class="plan">${p.plan.map(x => `<div class="row"><span style="width:16px;display:grid;place-items:center">${mark(x.status)}</span><span style="${x.status === "pending" ? "color:var(--muted)" : ""}">${esc(x.content)}</span></div>`).join("")}</div>` : "";
  const files = p.files.length ? `<div class="sec">Files changed</div><div class="files">${p.files.slice(-12).map(f => `<span><b>${esc(f)}</b></span>`).join("")}${p.files.length > 12 ? `<span style="color:var(--muted)">and ${p.files.length - 12} more</span>` : ""}</div>` : "";
  const queued = p.queued ? `<div class="row" style="padding:8px 12px 0;color:var(--muted)">${p.queued === 1 ? "1 message waits" : `${p.queued} messages wait`} for the turn to end</div>` : "";
  const card = doing || question || plan || files || queued
    ? `<div class="dcard dcoder">${doing}${question}${plan}${files}${queued}<div style="height:12px"></div></div>` : "";
  const why = AG.why ? `<div class="dmuted">Auto picked ${esc(s.machine_name)}. ${esc(AG.why)}</div>` : "";
  const earlier = p.earlier ? `<div class="dmuted">${p.earlier} earlier ${p.earlier === 1 ? "entry" : "entries"} not shown</div>` : "";
  const entries = p.entries.length ? `<div class="dcoderlog">${earlier}${p.entries.map(entryRow).join("")}</div>`
    : s.status === "starting" ? '<div class="dmuted"><span class="dspin"></span> Starting the agent…</div>' : "";
  const err = p.error ? `<div class="derr">${esc(p.error)}</div>` : "";
  const note = AG.note === "gone" ? '<div class="dmuted">This session is no longer on the machine, or the machine went offline.</div>'
    : AG.note ? `<div class="derr">${esc(AG.note)}</div>` : "";
  const paused = AG.paused ? '<button class="dbtn quiet" id="agresume">Paused, click to refresh</button>' : "";
  b.innerHTML = why + card + entries + err + note + paused;
  const hint = bar.querySelector(":scope > .dhints");
  if (hint) hint.replaceWith(h(agentHints()));
  const field = $("#agfield");
  if (field) field.style.display = s.status === "ended" ? "none" : "";
  if (b.stuck !== false) b.scrollTop = b.scrollHeight;
}

/// Answers the open question once: its buttons stay disabled, and a poll
/// that still shows it does not bring them back, until the node moves on.
async function answerAgentQuestion(i) {
  const o = AG.open, q = AG.page?.session.question, opt = q?.options[i];
  if (!opt || AG.answering === q.id) return;
  AG.answering = q.id;
  drawAgentBody();
  try {
    await invoke("center_answer_agent", { machineId: o.machine_id, agent: o.agent, session: o.session, question: q.id, reply: opt.id });
    if (isOpen(o) && AG.page.session.question?.id === q.id) {
      AG.page.session.question = null;
      AG.page.session.status = "working";
    }
  } catch (e) {
    // A conflict means it was answered already, so it stays answered.
    const conflict = /^conflict/i.test(String(e));
    if (!conflict && AG.answering === q.id) AG.answering = null;
    if (isOpen(o)) AG.note = conflict ? "" : String(e);
  }
  if (isOpen(o)) { drawAgentBody(); resumeAgent(); }
}
/// The first option of the open question that allows, or that does not.
function answerFirst(allow) {
  const q = AG.page?.session.question;
  const i = q ? q.options.findIndex(o => o.allow === allow) : -1;
  if (i >= 0) answerAgentQuestion(i);
  return i >= 0;
}

/// Sends `steer` to the open session; true when the node took it.
async function steerAgent(steer) {
  const o = AG.open;
  if (!o || AG.page?.session.external) return false;
  let ok = false;
  try {
    await invoke("agent_steer", { machineId: o.machine_id, agent: o.agent, session: o.session, steer: "prompt" in steer ? steer : Object.keys(steer)[0] });
    AG.note = "";
    ok = true;
  } catch (e) {
    AG.note = String(e).replace(/^bad request: /, "");
  }
  if (isOpen(o)) { drawAgentBody(); resumeAgent(); }
  return ok;
}

async function sendAgentMessage() {
  const input = $("#agmsg"), text = input?.value.trim() ?? "", images = AG.att;
  if (!text && !images.length) return;
  const o = AG.open;
  input.value = "";
  if (!images.length) {
    AG.page.entries.push({ kind: "user", text });
    drawAgentBody();
    return steerAgent({ prompt: { text } });
  }
  AG.att = [];
  drawAgentAttached();
  keepAgentPics(o, images);
  const page = AG.page, mine = { kind: "user", text: [text, ...images.map(a => `[image: ${a.name}]`)].filter(Boolean).join("\n") };
  page.entries.push(mine);
  drawAgentBody();
  const sent = await steerAgent({ prompt: { text, images: images.map(({ name, mime, data }) => ({ name, mime, data })) } });
  if (sent || !isOpen(o)) return;
  // Nothing went: the message and its images wait in the field again.
  if (/does not take images yet/.test(AG.note)) AG.noImages = o.session;
  // A poll may have drawn more since: take out this message, not the last.
  const at = page.entries.indexOf(mine);
  if (at >= 0) page.entries.splice(at, 1);
  const field = $("#agmsg");
  if (field && !field.value) field.value = text;
  AG.att = images;
  drawAgentBody();
  drawAgentAttached();
}

// ----- Every session -----

async function renderAgents() {
  S.view = "agents";
  AG.sel = 0;
  drawAgents(true);
  try {
    AG.sessions = await invoke("agent_sessions");
    AG.error = "";
  } catch (e) {
    AG.error = String(e);
  }
  if (S.view === "agents") drawAgents(false);
}

function drawAgents(loading) {
  const list = AG.sessions;
  const machines = new Set(list.map(s => s.machine_id)).size;
  const sum = loading && !list.length ? "Looking at your machines…" : list.length ? `${list.length} on ${machines} ${machines === 1 ? "machine" : "machines"}` : "Nothing running";
  const row = (s, i) => {
    const k = agentKind(s.agent), where = folderName(s.cwd);
    const tail = s.status === "asking" ? '<span class="dstate ask">Needs you</span>'
      : s.status === "working" || s.status === "running" || s.status === "starting" ? `<span class="run"><span class="dspin"></span>${esc(s.doing || "Working")}</span>`
      : `<span class="w">${s.external ? "In terminal" : esc(cap(s.status))}</span>`;
    return `<button class="dchat" role="option" aria-selected="${i === AG.sel}" data-agi="${i}">${agentAvatar(s.agent)}
      <span class="t"><span>${esc(s.title || where || k.name)}</span><small>${esc(`${where ? where + ", " : ""}${k.name} on ${s.machine_name}`)}</small></span>${tail}</button>`;
  };
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Coding agents</b><small>${esc(sum)}</small></span><button class="dbtn" id="agNew">${AGENT_ICON}New<kbd>⌘⇧A</kbd></button></div>
    ${AG.error ? `<div class="derr" style="padding:8px 18px 0">${esc(AG.error)}</div>` : ""}
    ${list.length ? `<div class="dlist" role="listbox" style="padding-top:6px">${list.map(row).join("")}</div>`
      : loading ? "" : `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${AGENT_ICON}</span><b style="font-size:14px">No sessions running.</b>
        <span class="dmuted" style="display:block">Start one here, ask Beep in a chat, or run Claude Code in a terminal on a linked machine.</span></div>`}
    ${hints([[ICON.ret, "open"], ["⌘⇧A", "new"], ["esc", "back"]])}`;
  bar.onclick = e => {
    if (S.view !== "agents") return;
    const r = e.target.closest("[data-agi]");
    if (r) return openAgent(AG.sessions[Number(r.dataset.agi)], { back: "agents" });
    if (e.target.closest("#agNew")) renderAgentNew("", "agents");
  };
}

/// Keys of the agent views; true when one was taken.
function agentKeys(e, cmd) {
  if (S.view === "agentnew") {
    if (cmd && e.key === "Enter") { e.preventDefault(); startAgent(); return true; }
    return false;
  }
  if (S.view === "agent") {
    if (cmd && e.key === "Enter" && answerFirst(true)) { e.preventDefault(); return true; }
    if (cmd && e.key.toLowerCase() === "d" && answerFirst(false)) { e.preventDefault(); return true; }
    if (cmd && e.key === ".") { e.preventDefault(); steerAgent({ cancel: null }); return true; }
    if (e.key === "Enter" && !e.isComposing && e.target.id === "agmsg") { e.preventDefault(); sendAgentMessage(); return true; }
    return false;
  }
  if (S.view === "agents") {
    const n = AG.sessions.length;
    if (!n) return false;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      AG.sel = (AG.sel + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
      drawAgents(false);
      return true;
    }
    if (e.key === "Enter") { e.preventDefault(); openAgent(AG.sessions[AG.sel], { back: "agents" }); return true; }
  }
  return false;
}

/// Esc from an agent view.
function agentBack() {
  if (S.view === "agent" && AG.back === "agents") return renderAgents();
  if (S.view === "agentnew" && AG.back === "agents") return renderAgents();
  return S.session ? openSession(S.session) : renderChat();
}

// ---------- State ----------

async function refreshStatus() {
  const st = await invoke("status");
  S.link = st.link;
  // The bar's chat is not the window's.
  if (S.session === null && !PANE) S.session = st.session;
  return st;
}

async function show(view) {
  const st = await refreshStatus();
  loadResearches();
  loadLiveChats();
  if (S.link === "linked") { loadModels(); loadAgentHosts(); }
  if (S.link === "revoked") return renderRevoked();
  if (view === "settings") return renderSettings();
  if (S.link === "unlinked" || view === "link") return S.link === "linked" ? renderChat() : renderLink();
  if (view === "new") return newChat();
  if (view === "chats") return renderChats();
  if (view === "memories") return renderMemories();
  if (view === "routines") return renderRoutines();
  if (view === "skills") return renderSkills();
  if (view === "research") return renderResearches();
  if (view === "apps") return renderApps();
  // Back from the browser: look again at what is connected.
  if (S.view === "apps") return loadApps();
  // A chat picked from the tray's Recent menu, or the orb's approval card,
  // already made current in Rust. In live mode it opens under the dock.
  if (view === "open" && st.session) {
    if (!LV.on) return openSession(st.session);
    if (S.view !== "chat" || !$("#ldock") || LV.chat !== st.session) openLive(st.session);
    return drawDock(LV.view);
  }
  // A group stays up between showings, fetched again for what came in.
  if (GROUP_VIEWS.has(S.view)) {
    if (S.view === "group") { loadGroup(); $("#gask")?.focus(); }
    else if (S.view === "ginfo") loadGroup();
    else bar.querySelector("input")?.focus();
    return;
  }
  // Live mode runs in the bar until it ends.
  if (LV.on) return renderLive();
  if (S.view !== "chat" || !$("#ask")) {
    if (S.session) return openSession(S.session);
    return renderChat();
  }
  focusAsk();
}

// ---------- Live voice ----------

// Live mode keeps the chat on screen as it always is: turns stream in, it
// scrolls, and links, cards and the approval card work as usual. A dock takes
// the field's place: a small orb that follows the microphone while listening
// and Beep's voice while speaking, one status word, the caption, the hint
// and the way out. Rust runs the microphone, the server's live transcription
// and the player; this draws.
const VOICE_ICON = {
  speaker: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9h4l5-4v14l-5-4H4z"/><path d="M17 9a4 4 0 010 6M19.5 6.5a8 8 0 010 11"/></svg>',
  mic: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><rect x="9" y="3" width="6" height="12" rx="3"/><path d="M5 11a7 7 0 0014 0M12 18v3"/></svg>',
};
/// `drafts` holds the words live mode kept when it ended, by chat ("" for a
/// new one), until that chat takes them, and `note` says why it ended.
/// `typed` is a key pressed to leave live mode, for the prompt after the words.
const VN = { drafts: new Map(), note: "", typed: "" };
/// The live view: the last view Rust sent, a toast, the pause bar, the
/// message just sent, and the chat under the dock (null for a new one).
const LV = { on: false, view: null, toast: "", toastTimer: 0, soon: null, sent: "", chat: null };
const redrawLive = () => LV.on && renderLive();
/// Chats live mode talked in, marked with a mic in the chat list.
const LIVE_CHATS = new Set();
async function loadLiveChats() {
  const ids = await invoke("live_chats").catch(() => []);
  for (const id of ids) LIVE_CHATS.add(id);
}
const LIVE_KEYS = [["tap ⌥", "leave"], ["hold ⌥", "talk, let go to send"], ["esc", "leave live"], ["⌘/", "what to say"]];
/// Push to talk, between holds.
const LIVE_PTT_KEYS = [["hold ⌥", "reply"], ["tap ⌥", "leave"], ["esc", "leave live"], ["⌘/", "what to say"]];
/// The sheet of what to say, over the chat and the dock.
const liveSheet = () => `<div class="dover lsheet" role="dialog" aria-label="What to say"><div class="vt"><span class="tt"><b>What to say</b></span></div>
  <div class="dkeys">${[["Say", LIVE_SAY], ["Keys", LIVE_TALK]].map(([sec, rows]) => `<div class="kg"><div class="dsec">${sec}</div>${rows.map(([k, t]) => `<div class="kr"><span>${t}</span><kbd>${k}</kbd></div>`).join("")}</div>`).join("")}</div>
  ${hints([["esc", "close"], ["⌘/", "close"]])}</div>`;
/// Bars of a waveform, the same every time for a seed.
function wave(n, cls = "", seed = 7) {
  let a = seed;
  const bars = Array.from({ length: n }, (_, i) => {
    a = (a * 9301 + 49297) % 233280;
    const tall = 4 + Math.round((a / 233280) * 18 * (0.55 + 0.45 * Math.sin(i / 3)));
    return `<i style="height:${Math.max(3, tall)}px;animation-delay:${(i % 7) * -0.13}s"></i>`;
  });
  return `<span class="dwave ${cls}" aria-hidden="true">${bars.join("")}</span>`;
}
/// What the dock and the keys row say for view `v`.
function liveParts(v) {
  const mode = v.speaking || (v.phase === "speaking" && v.line) ? "speak" : v.phase === "thinking" ? "think" : "listen";
  const holding = v.phase === "holding";
  // A voice note is only read out; answering means opening Beep.
  const note = !!v.title && mode !== "listen";
  // The reply waits on the approval card in the chat.
  const e = here(), asked = !note && (e?.approval ?? (v.phase === "waiting" && !e?.run && waitingQuestion(S.session)));
  const waiting = !note && (v.phase === "waiting" || !!e?.approval);
  const word = waiting ? ["Waiting for you", ""]
    : { listen: [v.hold_only && !holding ? "Push to talk" : "Listening", "on"], think: ["Thinking", ""], speak: ["Speaking", "sp"] }[mode];
  const orb = waiting ? "idle" : mode === "listen" ? (v.words ? "" : "idle") : mode;
  const step = v.step ? `<span class="stepw"><span class="step"><span class="dspin"></span>${esc(v.step)}</span></span>` : "";
  const prev = t => t ? `<span class="prev">${esc(t)}</span>` : "";
  const idle = v.hold_only && !holding && !v.finishing ? "Hold Option to reply" : "Start talking…";
  const cap = waiting ? '<span class="ph">Allow or deny in the chat</span>'
    : mode === "listen" ? `${prev(v.line)}${v.words ? esc(v.words) : `<span class="ph">${idle}</span>`}`
    : mode === "think" ? (v.step ? `${prev(LV.sent)}${step}` : prev(LV.sent))
    : `${prev(v.prev ?? (v.phase === "thinking" ? LV.sent : ""))}${esc(v.line ?? "")}${v.phase === "thinking" ? step : ""}`;
  const hint = note ? "Tap Option to stop"
    : waiting ? "Beep goes on once you answer"
    : v.finishing ? "Sending…"
    : holding ? "Let go of Option to send"
    : mode === "listen" && v.hold_only ? "hold ⌥ to reply · tap ⌥ to leave"
    : mode === "listen" ? (v.pause_sends ? "Pause to send · tap ⌥ to leave" : "Say “send” or “stop” to send · tap ⌥ to leave")
    : mode === "think" ? "Option to skip the spoken answer"
    : v.phase === "thinking" ? "Beep reads the answer when it is ready · Option to stop"
    : "Say “stop” or tap Option to talk";
  const grant = asked && (e?.approval ? e.approval.grant : forRoutine(asked.action));
  const keys = note ? [["tap ⌥", "stop"], ["⌥ ⌥", "open the chat"], ["esc", "dismiss"]]
    : asked ? [[`⌘${ICON.ret}`, "allow"], ...(grant ? [] : [[`⌥⌘${ICON.ret}`, "always"]]), ["⌘D", "deny"], ["esc", "leave live"]]
    : v.finishing ? [["esc", "leave live"]]
    : holding ? [["let go of ⌥", "send"], ["any key", "cancel"]]
    : mode === "listen" ? (v.hold_only ? LIVE_PTT_KEYS : LIVE_KEYS)
    : mode === "think" ? [["tap ⌥", "skip speaking"], ["esc", "cancel reply"]]
    : v.phase === "listening" ? (v.hold_only ? LIVE_PTT_KEYS : LIVE_KEYS)
    : [["tap ⌥", "interrupt"], ["hold ⌥", "reply"], ["esc", "cancel reply"]];
  // While a reply is written or read out it can be cancelled, on the server too.
  const replying = !note && (v.phase === "thinking" || v.phase === "speaking");
  const label = mode === "speak" ? "Stop Beep speaking" : mode === "think" ? "Skip the spoken answer" : "Leave live mode";
  return { mode, word, orb, cap, hint, keys, replying, label, title: v.title ? esc(v.title) : "" };
}
/// Draws live mode: the live chat with the dock, built once, then only the
/// dock changes as Rust sends views.
function renderLive() {
  const v = LV.view;
  if (!v) return;
  if (S.view !== "chat" || !$("#ldock")) openLive(v.session ?? null);
  drawDock(v);
}
/// Shows chat `id` (null for a new one) with the dock in place of the field.
/// The chat the bar already shows stays as it is.
function openLive(id) {
  const same = S.view === "chat" && !!$("#ask") && (S.session ?? null) === id;
  LV.chat = id;
  if (!same) {
    if (id) openSession(id);
    else { S.session = null; renderChat(); }
  }
  mountDock();
}
/// Puts the dock over the chat. The field stays, out of sight, for after.
function mountDock() {
  const field = $(".dinput.dgrow");
  if (!field || $("#ldock")) return;
  field.classList.add("lhid");
  // Keys go to live mode, not to the hidden field.
  $("#ask")?.blur();
  field.after(h('<div class="ddock" id="ldock" role="region" aria-label="Live mode"></div>'));
}
function drawDock(v) {
  const dock = $("#ldock");
  if (!dock || !v) return;
  const p = liveParts(v);
  const lvl = $("#orb")?.style.getPropertyValue("--lvl");
  const left = LV.soon ? Math.max(0, LV.soon.until - Date.now()) : 0;
  const soon = LV.soon && p.mode === "listen"
    ? `<div class="dsoon" role="progressbar" aria-label="Sending after the pause"><i style="--from:${(left / LV.soon.total).toFixed(3)};animation-duration:${left}ms"></i></div>` : "";
  dock.innerHTML = `${LV.toast ? `<div class="ltoast" role="status">${esc(LV.toast)}</div>` : ""}
    <div class="drow1"><button class="dorb ${p.orb}" id="orb" aria-label="${p.label}"><span class="ring b"></span><span class="ring"></span><span class="core"></span></button>
      <div class="dtx"><div class="word ${p.word[1]}">${p.title ? `<span class="tt">${p.title}</span> · ` : ""}${p.word[0]}</div><div class="cap" aria-live="polite">${p.cap}</div></div>
      <button class="dleave" id="lleave" aria-label="${p.replying ? "Cancel the reply and leave live mode" : "Leave live mode"}">${ICON.x}</button></div>
    <div class="dfoot">${soon}<div class="hint">${p.hint}</div>${p.replying ? `<button class="dbtn lcancel" id="lcancel">Cancel<kbd>esc</kbd></button>` : ""}</div>`;
  if (lvl) $("#orb").style.setProperty("--lvl", lvl);
  const cap = dock.querySelector(".cap");
  cap.scrollTop = cap.scrollHeight;
  $("#orb").addEventListener("click", () => invoke("live_tap").catch(() => {}));
  $("#lleave").addEventListener("click", () => invoke("cancel_live").catch(() => {}));
  $("#lcancel")?.addEventListener("click", () => invoke("cancel_live").catch(() => {}));
  refreshHints();
  const sheet = bar.querySelector(".lsheet");
  if (v.sheet && !sheet) bar.append(h(liveSheet()));
  else if (!v.sheet) sheet?.remove();
}
/// Draws a message live mode sent into the chat under the dock and follows
/// its reply there, like one typed in the field. With no dock on screen the
/// chat comes from the server when it is next opened.
function liveTurn(id, text, runId) {
  if (S.view !== "chat" || !$("#ldock")) return live.delete(id);
  if (LV.chat !== id) {
    // A new chat. An empty bar becomes it; "new chat" said in another one
    // starts it afresh.
    const fresh = LV.chat === null;
    LV.chat = id;
    S.session = id;
    if (!fresh) {
      renderChat();
      mountDock();
      drawDock(LV.view);
    }
  }
  // Already followed: the chat was redrawn from the server as it changed.
  if (live.get(id)?.run) return;
  const e = entry(id);
  // A new message leaves an earlier question unanswered on the server.
  if (e.approval) {
    e.approval.card.replaceWith(h('<div class="dmuted">Not answered</div>'));
    e.approval = null;
  }
  body().querySelectorAll(".dreplies").forEach(r => r.remove());
  // The server's copy may have drawn the message already.
  const last = [...body().querySelectorAll(".turn")].at(-1);
  const turn = last?.user && !last.done && last.querySelector(".ask")?.textContent === text ? last : newTurn(text);
  turn.user = true;
  turn.steps ??= [];
  reply(turn).innerHTML = '<span class="caret"></span>';
  e.body = body();
  e.run = { id: runId, turn };
  refreshHints();
  scrollDown();
  followAnswered(id, turn, runId);
}
/// Live mode ended: the dock goes and the chat keeps its field, with the
/// words kept in it and a note on why it ended.
function renderLeft() {
  bar.querySelector(".lsheet")?.remove();
  const chat = LV.chat;
  LV.chat = null;
  if (S.view === "chat" && $("#ldock") && chat === (S.session ?? null)) {
    $("#ldock").remove();
    $(".dinput.dgrow")?.classList.remove("lhid");
    refreshHints();
    takeDraft($("#ask"));
    focusAsk();
    return;
  }
  if (S.session) openSession(S.session);
  else renderChat();
}
/// Puts a kept draft in the open chat's prompt, after anything typed there.
function takeDraft(ask) {
  // The field is out of sight under the dock until live mode ends.
  if (LV.on) return;
  const key = S.session ?? "";
  const text = VN.drafts.get(key);
  if (!ask) return;
  if (text) {
    VN.drafts.delete(key);
    ask.value = ask.value.trim() ? `${ask.value.trimEnd()} ${text}` : text;
  }
  if (VN.typed) {
    // No space before punctuation typed right after the words.
    const glue = !ask.value || /^[\s.,;:!?)]/.test(VN.typed) ? "" : " ";
    ask.value = `${ask.value.trimEnd()}${glue}${VN.typed}`;
    VN.typed = "";
  }
  if (text || VN.note) ask.dispatchEvent(new Event("input"));
  if (VN.note) { notice(VN.note); VN.note = ""; }
}
/// Shows what the user said in place of the waiting prompt.
function heard(turn, text) {
  const ask = turn.querySelector(".ask") ?? h('<div class="ask"></div>');
  if (!ask.isConnected) turn.prepend(ask);
  ask.textContent = text;
  ask.classList.remove("dmuted");
  touch(S.session, text);
}
// Live mode stays in the bar and the orb.
if (!PANE) listen("live", e => {
  const v = e.payload;
  switch (v.kind) {
    case "view":
      if (!LV.on) {
        barHeight = 0;
        // What was typed in the prompt comes back when live mode ends.
        const ask = S.view === "chat" ? $("#ask") : null, typed = ask?.value.trim();
        if (typed) {
          const key = S.session ?? "";
          const kept = VN.drafts.get(key);
          VN.drafts.set(key, kept ? `${typed} ${kept}` : typed);
          ask.value = "";
        }
      }
      LV.on = true;
      LV.view = v;
      return redrawLive();
    case "level":
      return $("#orb")?.style.setProperty("--lvl", Math.min(1, v.level).toFixed(3));
    case "toast":
      LV.toast = v.text;
      clearTimeout(LV.toastTimer);
      LV.toastTimer = setTimeout(() => { LV.toast = ""; redrawLive(); }, 1600);
      return redrawLive();
    case "soon":
      LV.soon = v.ms == null ? null : { until: Date.now() + v.ms, total: Math.max(1, v.ms) };
      return redrawLive();
    case "answered": {
      // Answered in the orb: the card here says so and follows the run.
      const e = live.get(v.session), pending = e?.approval;
      if (pending?.queryId !== v.query_id) return;
      return followAnswered(v.session, settle(v.session, e, pending, v.decision), v.run);
    }
    case "sent":
      LV.sent = v.text;
      LV.soon = null;
      LIVE_CHATS.add(v.session);
      touch(v.session, v.text);
      return liveTurn(v.session, v.text, v.run);
    case "left":
      LV.on = false;
      LV.view = null;
      LV.soon = null;
      LV.toast = "";
      // The bar, if Rust shows it next, opens on the chat live mode was in.
      if (v.session) S.session = v.session;
      {
        const key = v.session ?? "";
        if (v.draft) {
          // The chat takes the draft the next time it opens.
          // A second draft for the same chat joins the first instead of
          // replacing it.
          const kept = VN.drafts.get(key);
          VN.drafts.set(key, kept ? `${kept} ${v.draft}` : v.draft);
        }
        // A failure or the quiet timeout is said even with no words kept.
        const kept = v.draft ? " Your words are kept here." : "";
        const why = v.failure ?? (v.quiet ? "Live mode ended after 2 minutes quiet." : VN.typed ? "Left live mode to type." : v.draft ? "Left live mode." : "");
        if (why) VN.note = why + kept;
      }
      return renderLeft(v);
  }
});

// Only the Option double tap shows the bar without naming a view.
if (!PANE) listen("bar-shown", e => { barHeight = 0; if (e.payload == null) learnedHotkey(); show(e.payload); });
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

// ---------- Background work ----------

// Background sub-agents that need an OK ask through a notice, so the card
// sits at the end of its chat until it is answered.
const WORK = new Map(); // session id -> { questions: [{ query_id, app, action, fields, answer, withdrawn }] }
function workOf(id) {
  if (!WORK.has(id)) WORK.set(id, { questions: [] });
  return WORK.get(id);
}
const waitingQuestion = id => WORK.get(id)?.questions.find(q => !q.answer && !q.withdrawn);

/// The open chat's questions from background work, at the end of its body.
function drawWorkQuestions() {
  const b = body();
  if (S.view !== "chat" || !b) return;
  b.querySelector(".dworkq")?.remove();
  const qs = WORK.get(S.session)?.questions ?? [];
  if (!qs.length) return;
  const said = { allow: "Allowed once. The work goes on.", always: "Allowed, and always allowed from now on. The work goes on.", deny: "Denied. The work goes on without it.", grant: "Allowed for this routine. The work goes on." };
  const first = waitingQuestion(S.session);
  const card = q => q.withdrawn || q.answer
    ? `<div class="dmuted">${esc(q.withdrawn ? `${q.action} with ${q.app}: no longer waiting` : said[forRoutine(q.action) && q.answer === "allow" ? "grant" : q.answer])}</div>`
    : `<div class="dcard" data-q="${esc(q.query_id)}">
    <div class="head"><span class="dapp">${esc((q.app || "?")[0].toUpperCase())}</span><div style="flex:1;min-width:0;display:grid"><b>${esc(q.action)}?</b><small>Background work asks first</small></div></div>
    ${q.fields.length ? `<div class="kv">${q.fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</div>` : ""}
    <div class="acts">${[["allow", "Allow", "pri", `⌘${ICON.ret}`], ["always", "Always", "", `⌥⌘${ICON.ret}`], ["deny", "Deny", "warn", "⌘D"]].filter(([d]) => d !== "always" || !forRoutine(q.action)).map(([d, t, cls, k]) =>
      `<button class="dbtn ${cls}" data-d="${d}">${t}${q === first && !here()?.approval ? `<kbd>${k}</kbd>` : ""}</button>`).join("")}</div></div>`;
  const box = h(`<div class="dworkq" style="display:grid;gap:8px"><div class="dmuted" style="justify-content:center;font-size:11.5px;color:var(--dim)">${first ? "Background work is waiting for you" : "Background work"}</div>${qs.map(card).join("")}</div>`);
  box.querySelectorAll(".dcard").forEach(c => c.querySelectorAll("[data-d]").forEach(btn => btn.addEventListener("click", () =>
    answerWork(S.session, qs.find(q => q.query_id === c.dataset.q), btn.dataset.d))));
  b.append(box);
  keepDown();
}

async function answerWork(id, q, decision) {
  if (!q || q.answer || q.withdrawn) return;
  if (decision === "always" && forRoutine(q.action)) decision = "allow";
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
function keepWaitingQuestions(id) {
  const w = WORK.get(id);
  if (w) w.questions = w.questions.filter(q => !q.answer && !q.withdrawn);
  drawWorkQuestions();
}

/// Loads the questions chat `id`'s background work still waits on, so cards
/// outlive a restart and show on every device. Ones answered here stay to say
/// so; ones no longer listed went elsewhere and leave quietly.
async function loadQuestions(id) {
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
  if (S.view === "agent" || S.view === "agentnew" || S.view === "agents") return agentBack();
  if (S.view === "chats") {
    if (S.chats.mode !== "list") return chatMode("list");
    if (S.chats.back) return renderGroup(S.chats.back);
    return S.session ? openSession(S.session) : renderChat();
  }
  if (S.view === "group" && G.mention) return closeMention();
  if (S.view === "memories" && M.mode !== "list") { M.mode = "list"; drawMemories(); return $("#mfind")?.focus(); }
  if (S.view === "routines" && R.mode !== "list") return routineAction("list");
  if (S.view === "grants" && GR.mode !== "list") { GR.mode = "list"; return drawGrants(); }
  if (S.view === "grants") return renderRoutines();
  if (S.view === "research" && RL.mode !== "list") { RL.mode = "list"; return drawResearches(); }
  if (S.view === "skills" && K.mode !== "list") return skillAction(K.mode === "edit" || K.mode === "vdelete" ? "view" : "list");
  if (S.view === "rnew") return renderRoutines();
  if (S.view === "appperms") return renderApps();
  if (S.view === "mcp" && A.asking) { A.asking = null; return drawMcpTools(); }
  if (S.view === "mcp") return renderApps();
  if (S.view === "apps" && A.adding) { A.adding = false; A.addError = ""; return drawApps(); }
  if (S.view === "apps" || S.view === "machines") return renderSettings();
  if (S.view === "model") return S.session ? openSession(S.session) : renderChat();
  if ((S.view === "memories" || S.view === "routines" || S.view === "skills" || S.view === "research" || S.view === "settings") && S.link === "linked") return S.session ? openSession(S.session) : renderChat();
  if (S.view === "ginfo" || S.view === "invite") return renderGroup(G.id);
  if (S.view === "gnew") {
    if (J.mode === "join") return renderGroupNew("code");
    if (J.mode === "code") return renderGroupNew("new");
    return renderChats();
  }
  if (!PANE) invoke("hide_bar");
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
  } else if (!PANE && cmd && key === "o" && c?.kind === "chat") {
    e.preventDefault();
    chatAction("window");
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
  if (!PANE && e.key === "ArrowUp" && !e.target.value) { e.preventDefault(); return renderChats(); }
  if (e.key === "ArrowLeft" || e.key === "ArrowRight") setTimeout(findMention);
}

/// Keys while the bar shows live mode. ⌘/ opens what to say over it and esc
/// closes that first (Rust keeps the sheet with the view). Keys that would
/// open another view do nothing; a typed character leaves live mode, keeping
/// the words, and goes in the prompt after them. True when the key is taken.
function liveKeys(e, cmd) {
  if (cmd && e.key === "/") { e.preventDefault(); invoke("live_sheet", { open: null }).catch(() => {}); return true; }
  if (e.key === "Escape") { e.preventDefault(); invoke("cancel_live").catch(() => {}); return true; }
  if (cmd && e.key.toLowerCase() === "q") return false;
  // The approval card in the chat answers to its usual keys, and text read
  // in the chat can be copied.
  const key = e.key.toLowerCase();
  if (cmd && (e.key === "Enter" || key === "d") && (here()?.approval || waitingQuestion(S.session))) return false;
  if (cmd && key === "c") return false;
  if (cmd || e.ctrlKey) { e.preventDefault(); return true; }
  if (e.key.length === 1 && !e.altKey && !e.isComposing && !LV.view?.title) {
    e.preventDefault();
    VN.typed += e.key;
    invoke("leave_live").catch(() => {});
    return true;
  }
  return false;
}

/// In the window, whether the pane is the view on screen; Home takes the keys
/// while it shows.
let paneMounted = false;
/// Keys the window keeps for itself: its palette and its views.
const WINDOW_KEYS = new Set(["k", "m", "j", "l", ","]);

document.addEventListener("keydown", e => {
  const cmd = e.metaKey;
  if (PANE && !paneMounted) return;
  // A key meant for the sidebar (Enter on a row) is not the pane's.
  if (PANE && !cmd && e.target !== document.body && !bar.contains(e.target)) return;
  if (PANE && cmd && WINDOW_KEYS.has(e.key.toLowerCase())) return;
  if ($("#viewer")) return viewerKeys(e, cmd);
  if (LV.on && liveKeys(e, cmd)) return;
  if (cmd && e.key === "/") { e.preventDefault(); return toggleKeys(); }
  if (e.key === "Escape" && LV.on) { e.preventDefault(); return invoke("cancel_live").catch(() => {}); }
  if (e.key === "Escape" && $("#keys")) { e.preventDefault(); return toggleKeys(); }
  if (e.key === "Escape" && closeRerunMenu()) { e.preventDefault(); return; }
  if (e.key === "Escape") {
    e.preventDefault();
    return back();
  }
  if (S.view === "link" || S.view === "revoked") return;
  if (agentKeys(e, cmd)) return;
  const key = e.key.toLowerCase();
  if (cmd && e.shiftKey && key === "a" && S.link === "linked") {
    e.preventDefault();
    return renderAgentNew(S.view === "chat" ? $("#ask")?.value.trim() ?? "" : "", S.view === "agents" ? "agents" : "chat");
  }
  // These views live in the window now; before linking the bar keeps them.
  const linked = S.link === "linked";
  if (cmd && e.key === ",") { e.preventDefault(); return linked ? toWindow("settings") : renderSettings(); }
  if (cmd && key === "q") { e.preventDefault(); return invoke("quit"); }
  if (cmd && key === "m") { e.preventDefault(); return linked ? toWindow("memories") : renderMemories(); }
  if (cmd && key === "j") { e.preventDefault(); return linked ? toWindow("routines") : renderRoutines(); }
  if (cmd && key === "l") { e.preventDefault(); return linked ? toWindow("skills") : renderSkills(); }
  if (cmd && key === "r" && S.view !== "chats") { e.preventDefault(); return researchShortcut(); }
  if (cmd && e.key === "Enter" && S.view === "rnew") { e.preventDefault(); return createRoutine(); }
  if (cmd && key === "n" && S.view === "routines") { e.preventDefault(); return renderRoutineNew(); }
  if (cmd && key === "n") { e.preventDefault(); return newChat(); }
  if (cmd && key === "e" && S.view === "routines" && R.list.length) { e.preventDefault(); return routineAction("edit"); }
  if (cmd && key === "g" && S.view === "routines") { e.preventDefault(); return renderGrants(); }
  if (cmd && key === "g") { e.preventDefault(); J.error = ""; J.name = ""; return renderGroupNew("new"); }
  if (cmd && key === "k") { e.preventDefault(); return S.view === "chats" ? null : renderChats(); }
  if (cmd && key === "i" && (S.view === "group" || S.view === "invite")) { e.preventDefault(); return renderGroupInfo(); }
  if (cmd && e.key === ".") { e.preventDefault(); return stop(); }
  if (cmd && key === "e" && canPickModel()) { e.preventDefault(); return S.view === "model" ? back() : renderModel(); }
  if (S.view === "model") return modelKeys(e);
  if (S.view === "chat") {
    if (cmd && e.key === "Enter" && here()?.approval) { e.preventDefault(); return answer(e.altKey ? "always" : "allow"); }
    if (cmd && key === "d" && here()?.approval) { e.preventDefault(); return answer("deny"); }
    const asked = !here()?.approval && S.view === "chat" && waitingQuestion(S.session);
    if (cmd && e.key === "Enter" && asked) { e.preventDefault(); return answerWork(S.session, asked, e.altKey ? "always" : "allow"); }
    if (cmd && key === "d" && asked) { e.preventDefault(); return answerWork(S.session, asked, "deny"); }
    if (cmd && e.key === "Enter" && queued.get(S.session)?.held && !here()) { e.preventDefault(); return sendQueued(S.session); }
    if (!PANE && cmd && key === "o") { e.preventDefault(); return openInWindow(S.session); }
    if (cmd && e.shiftKey && e.key === "Enter" && e.target.id === "ask") { e.preventDefault(); return research(e.target.value); }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && e.target.id === "ask") { e.preventDefault(); return send(e.target.value); }
    if (!PANE && e.key === "ArrowUp" && e.target.id === "ask" && !e.target.value) { e.preventDefault(); return renderChats(); }
    if (e.key === "Backspace" && !cmd && e.target.id === "ask" && !e.target.value && attachedCount()) { e.preventDefault(); return unattach(attachedCount() - 1); }
  }
  if (S.view === "chats") return chatsKeys(e, cmd);
  if (S.view === "memories") return memKeys(e, cmd);
  if (S.view === "routines") return routineKeys(e, cmd);
  if (S.view === "grants") return grantKeys(e, cmd);
  if (S.view === "research") return researchKeys(e);
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
  if (e.target.closest('[data-act="openWindow"]')) return openInWindow(S.session);
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
  const img = e.target.closest(".reply img");
  if (img && !img.closest("a, .dprods, .drecs")) {
    return openViewer([{ url: img.currentSrc || img.src, caption: img.alt }], 0);
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

// An approval answered in another webview: its card here goes and the chat
// follows the run the answer resumed. The bar has usually settled it already
// from live mode's "answered" event, which comes first.
listen("approval-answered", e => {
  const { session, query_id } = e.payload ?? {};
  const en = live.get(session), pending = en?.approval;
  if (!pending || pending.queryId !== query_id) return;
  en.approval = null;
  pending.card.replaceWith(h('<div class="dmuted">Answered</div>'));
  pending.turn.done = false;
  if (!en.run) live.delete(session);
  if (session !== S.session || S.view !== "chat") return;
  refreshHints();
  refreshSession(session).then(() => watchRun(session));
});

if (PANE) {
  let started = null;
  // Once per page: what the bar loads each time it shows, less live mode.
  const start = () => started ??= refreshPaneStatus().catch(() => {});
  async function refreshPaneStatus() {
    const st = await invoke("status");
    S.link = st.link;
    const researches = loadResearches();
    if (S.link === "linked") { loadModels(); loadAgentHosts(); }
    await researches;
  }
  /// Back on the chat or group the pane still shows, made current again
  /// after `leave`.
  function backOn() {
    if (S.view === "group") return renderGroup(G.id);
    if (S.session) invoke("open_session", { id: S.session }).catch(() => {});
    focusAsk();
  }
  const VIEWS = {
    chats: renderChats,
    memories: renderMemories,
    routines: renderRoutines,
    skills: renderSkills,
    apps: renderApps,
    settings: renderSettings,
    // Pages Settings leads to, which the window shows in the pane.
    machines: renderMachines,
    link: renderLink,
    invite: renderInvitePhone,
    // The command center leads here when the server says this Mac was removed.
    revoked: renderRevoked,
  };
  window.beepPane = {
    /// A routine's schedule in words, as Routines shows it.
    when: routineWhen,
    /// Draws Settings into `el`, the window's sheet, leaving the pane as it is.
    async settings(el) {
      await start();
      return renderSettings(false, el);
    },
    /// Shows `target`, a CenterTarget in its JSON form.
    async open(target) {
      await start();
      if (target && typeof target === "object" && "chat" in target) {
        const id = target.chat?.session ?? null;
        if (!id) return newChat();
        if (id === S.session && S.view === "chat" && $("#ask")) return backOn();
        return openSession(id);
      }
      // Groups are not a CenterTarget: only the window's sidebar opens them.
      if (target && typeof target === "object" && "group" in target) return renderGroup(target.group.id);
      // Agent views are not a CenterTarget either: Home and the sidebar open them.
      if (target && typeof target === "object" && "agent" in target) return openAgent(target.agent, { back: "agents" });
      if (target && typeof target === "object" && "agentnew" in target) return renderAgentNew(target.agentnew.prompt ?? "", "agents", target.agentnew.at ?? null);
      if (target === "agents") return renderAgents();
      if (VIEWS[target]) return VIEWS[target]();
      if ((S.view === "chat" && $("#ask")) || (S.view === "group" && G.id)) return backOn();
      return S.session ? openSession(S.session) : renderChat();
    },
    /// The pane goes off screen: the window stops showing its chat or group,
    /// so their notices show again.
    leave() {
      if (S.view === "chat") invoke("new_chat").catch(() => {});
      else if (GROUP_VIEWS.has(S.view)) invoke("close_group").catch(() => {});
    },
    /// Starts a new chat with `text` and streams it here.
    async compose(text) {
      await start();
      await newChat();
      return send(text);
    },
    /// Opens chat `session` with its history and sends `text` in it,
    /// streaming the reply here.
    async ask(session, text) {
      await start();
      await openSession(session);
      if (S.session !== session || S.view !== "chat") return;
      return send(text);
    },
    /// Starts a new chat with `text` in the composer, not sent, for the
    /// user to send or change.
    async draft(text) {
      await start();
      await newChat();
      const ask = $("#ask");
      if (!ask) return;
      ask.value = text;
      grow(ask);
      refreshHints();
      focusAsk();
    },
    /// Chats and groups, newest first, pinned ones as the bar lists them.
    async chatRows() {
      // The research set loads with the pane, so rows know which open their own window.
      await start();
      const [sessions, groups] = await Promise.all([invoke("list_sessions"), invoke("list_groups").catch(() => [])]);
      return rows(sessions, groups).map(c => ({ ...c, research: RESEARCHES.has(c.id) }));
    },
    /// When the open chat's first message was sent, in milliseconds, or
    /// null before its history has loaded.
    startedAt() {
      if (S.view !== "chat") return null;
      const at = Number(body()?.querySelector("[data-at]")?.dataset.at);
      return at > 0 ? at : null;
    },
    current: () => ({ view: S.view, session: S.session, group: GROUP_VIEWS.has(S.view) ? G.id : null }),
    onChange(fn) { changeListeners.push(fn); },
    /// Whether the pane is on screen and takes the keys.
    mounted(on) {
      paneMounted = !!on;
      if (paneMounted && S.view === "chat") focusAsk();
    },
  };
} else {
  show(null);
}
