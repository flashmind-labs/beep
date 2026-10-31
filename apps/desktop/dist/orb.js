// The orb window: live mode while the bar is hidden. It follows the same
// `live` events as the bar. A click brings the bar back; × cancels the
// reply, or leaves. When the reply waits for an OK, a card under the orb
// asks; clicking the card, not a button, opens the bar on that chat.
// Outside live mode, a new notice can show here too, when Voice notes asks
// first: a card with Play, Open and ×, which goes after 20 seconds alone.
const { invoke, Channel } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const orb = document.getElementById("orb");
const x = document.getElementById("x");
const card = document.getElementById("ask");
const note = document.getElementById("toast");
const ring = document.getElementById("ring");
const mini = document.getElementById("mini");
const noticeEl = document.getElementById("notice");

/// The orb alone, and the window's width while a card shows.
const ORB = 112, WITH_CARD = 296, WITH_NOTE = 240;
/// How long a toast stays.
const TOAST_MS = 1600;
/// Fields the card shows; the rest are in the chat.
const FIELDS = 3;

const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const clip = (s, n) => s.length > n ? `${s.slice(0, n - 1).trimEnd()}…` : s;
/// A routine asking for a grant: no Always, as in the bar.
const forRoutine = action => /for a routine$/.test(action ?? "");

/// Whether live mode runs here, from its first view until it is left.
let live = false;

function draw(v) {
  live = true;
  // The same states as the bar's orb: listening, thinking, speaking.
  const mode = v.speaking || (v.phase === "speaking" && v.line) ? "speak"
    : v.phase === "thinking" ? "think"
    : v.words ? "" : "idle";
  orb.className = `dorb ${mode}`;
  mini.classList.toggle("hold", v.phase === "holding");
  // While Option is held, or the last words come in, the note says so.
  state = v.finishing ? "Sending…" : v.phase === "holding" ? "Let go to send" : "";
  if (v.phase !== "listening") soon(null);
  drawNote();
  const replying = !v.title && (v.phase === "thinking" || v.phase === "speaking");
  x.setAttribute("aria-label", v.title ? "Stop the voice note" : replying ? "Cancel the reply" : "Leave live mode");
}

// ---------- Approval ----------

let waiting = null; // { session, approval: { query_id, app, action, fields } }
let failure = "";

// ---------- Notes ----------

/// The note for the state live mode is in, and a toast shown over it for a moment.
let state = "", toast = null, toastTimer = 0;
/// Toasts that say something went wrong.
const BAD = /could not|not available|busy|failed/i;

function drawNote() {
  const text = toast?.text ?? state;
  const bad = !!toast && BAD.test(toast.text);
  if (note.textContent === text && note.classList.contains("bad") === bad && note.hidden === !text) return;
  note.textContent = text;
  note.classList.toggle("bad", bad);
  note.hidden = !text;
  fit();
}

function showToast(text) {
  toast = { text };
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { toast = null; drawNote(); }, TOAST_MS);
  drawNote();
}

/// A pause sends after `ms`: the ring runs down; null takes it away.
function soon(ms) {
  if (ms == null) { ring.hidden = true; return; }
  ring.hidden = true;
  ring.style.setProperty("--ms", `${Math.max(1, ms)}ms`);
  // Hidden and shown again restarts the animation.
  void ring.getBoundingClientRect();
  ring.hidden = false;
}

/// Sizes the window to what shows, once the card is laid out.
function fit() {
  requestAnimationFrame(() => {
    // The card's bottom plus its margin: the body's own height is never
    // less than the window's, so it would not shrink for a shorter card.
    const last = notice ? noticeEl : waiting ? card : note.hidden ? null : note;
    const height = last ? Math.ceil(last.getBoundingClientRect().bottom + parseFloat(getComputedStyle(last).marginBottom)) : ORB;
    const width = waiting || notice ? WITH_CARD : last ? WITH_NOTE : ORB;
    invoke("fit_orb", { width, height }).catch(() => {});
  });
}

function drawAsk() {
  if (!waiting) {
    card.hidden = true;
    card.replaceChildren();
    return fit();
  }
  const { app, action, fields } = waiting.approval;
  const grant = forRoutine(action);
  const shown = fields.slice(0, FIELDS), more = fields.length - shown.length;
  card.innerHTML = `<div class="head"><span class="dapp">${esc((app || "?")[0].toUpperCase())}</span><div class="tt"><b title="${esc(action)}?">${esc(action)}?</b><small>${esc(app)} asks first</small></div></div>
    ${shown.length ? `<div class="kv">${shown.map(f => `<span>${esc(clip(f.label, 24))}</span><span title="${esc(f.value)}">${esc(clip(f.value.replace(/\s+/g, " "), 120))}</span>`).join("")}</div>` : ""}
    ${more > 0 ? `<div class="more">${more} more in the chat</div>` : ""}
    ${failure ? `<div class="err">${esc(failure)}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-d="allow">Allow</button>${grant ? "" : '<button class="dbtn" data-d="always">Always</button>'}<button class="dbtn warn" data-d="deny">Deny</button></div>`;
  card.querySelectorAll("[data-d]").forEach(b => b.addEventListener("click", e => {
    e.stopPropagation();
    answer(b.dataset.d);
  }));
  card.hidden = false;
  fit();
}

/// Answers from the orb. The run it resumes shows in the chat when it is
/// opened, so nothing here follows it. The bar hears of the answer from Rust.
async function answer(decision) {
  const asked = waiting;
  if (!asked) return;
  waiting = null;
  failure = "";
  drawAsk();
  try {
    await invoke("answer_approval", { session: asked.session, queryId: asked.approval.query_id, decision, onEvent: new Channel() });
  } catch (err) {
    // Still waiting, unless something newer took its place.
    if (waiting) return;
    waiting = asked;
    failure = String(err);
    drawAsk();
  }
}

function ask(w) {
  waiting = w;
  failure = "";
  drawAsk();
}

/// Drops the card for `queryId`, or any card when it is left out.
function clear(queryId) {
  if (!waiting || (queryId && waiting.approval.query_id !== queryId)) return;
  waiting = null;
  failure = "";
  drawAsk();
}

// ---------- A new notice ----------

let notice = null; // { id, title, snippet }
const X_ICON = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>';
const PLAY_ICON = '<svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>';

function drawNotice() {
  if (!notice) {
    noticeEl.hidden = true;
    noticeEl.replaceChildren();
    return fit();
  }
  if (!live) orb.className = "dorb idle";
  noticeEl.innerHTML = `<div class="head"><span class="dlogo">b</span><div class="tt"><b title="${esc(notice.title)}">${esc(notice.title)}</b><small>New message</small></div><button class="nx" data-n="dismiss" aria-label="Dismiss">${X_ICON}</button></div>
    ${notice.snippet ? `<div class="snip">${esc(notice.snippet)}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-n="play">${PLAY_ICON}Play</button><button class="dbtn" data-n="open">Open<kbd>⌥ ⌥</kbd></button></div>
    <div class="life"><i></i></div>`;
  noticeEl.querySelectorAll("[data-n]").forEach(b => b.addEventListener("click", e => {
    e.stopPropagation();
    noticeAct(b.dataset.n);
  }));
  noticeEl.hidden = false;
  fit();
}

/// Play, Open or ×. Rust drops the card; it is gone here at once.
function noticeAct(act) {
  if (!notice) return;
  const { id } = notice;
  notice = null;
  drawNotice();
  // The voice note takes a moment to come; the orb thinks meanwhile.
  if (act === "play") orb.className = "dorb think";
  invoke(`${act}_notice`, { id }).catch(() => {});
}

noticeEl.addEventListener("click", () => noticeAct("open"));
// The pointer on the card keeps it up; leaving it starts the 20 seconds again.
noticeEl.addEventListener("mouseenter", () => {
  if (notice) invoke("hold_notice", { id: notice.id, held: true }).catch(() => {});
});
noticeEl.addEventListener("mouseleave", () => {
  if (!notice) return;
  invoke("hold_notice", { id: notice.id, held: false }).catch(() => {});
  const life = noticeEl.querySelector(".life");
  if (life) life.innerHTML = "<i></i>";
});
listen("notice-card", e => {
  notice = e.payload;
  drawNotice();
});

card.addEventListener("click", () => {
  if (waiting) invoke("open_approval", { session: waiting.session }).catch(() => {});
});

listen("live", e => {
  const v = e.payload;
  if (v.kind === "view") draw(v);
  else if (v.kind === "level") orb.style.setProperty("--lvl", Math.min(1, v.level).toFixed(3));
  else if (v.kind === "approval") ask({ session: v.session, approval: v.approval });
  else if (v.kind === "answered") clear(v.query_id);
  else if (v.kind === "toast") showToast(v.text);
  else if (v.kind === "soon") soon(v.ms);
  // A new spoken message leaves the question unanswered; leaving ends it.
  else if (v.kind === "sent" || v.kind === "left") clear();
  if (v.kind === "sent") soon(null);
  if (v.kind === "left") {
    live = false;
    clearTimeout(toastTimer);
    toast = null;
    state = "";
    soon(null);
    mini.classList.remove("hold");
    drawNote();
  }
});
mini.addEventListener("click", () => {
  if (notice && !live) return noticeAct("open");
  invoke("show_bar").catch(() => {});
});
x.addEventListener("click", e => {
  e.stopPropagation();
  if (notice && !live) return noticeAct("dismiss");
  invoke("cancel_live").catch(() => {});
});
// An approval that came before this window was made.
invoke("live_approval").then(w => { if (w && !waiting) ask(w); }).catch(() => {});
// A notice card that came before this window was made.
invoke("notice_card").then(c => { if (c && !notice) { notice = c; drawNotice(); } }).catch(() => {});
