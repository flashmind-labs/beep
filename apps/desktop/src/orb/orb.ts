// orb/: the orb window. Entry: this file (dist/orb.js).
// The orb window: live mode while the bar is hidden.
import "../ui/loopclock.ts";
import { followSkin } from "../ui/skin.ts";
followSkin();
import { invoke, Channel, listen } from "../ui/tauri.ts";
import { OrbAsks } from "../chat/approvals.ts";
import { appTile, fillAppTiles } from "../chat/applogo.ts";
import { esc } from "../ui/format.ts";

/// An action the reply waits on an OK for (Rust's `Asking`).
interface Asking {
  query_id: string;
  app: string;
  action: string;
  fields: { label: string; value: string }[];
}

/// An approval waiting while live mode is on, and its chat (Rust's `Waiting`).
interface Waiting {
  session: string;
  approval: Asking;
  /// Background work asked it: answered with `answer_work`.
  work: boolean;
}

/// A new notice the card offers (Rust's `NoticeCard`).
interface NoticeCard {
  id: number;
  title: string;
  snippet: string;
}

/// What live mode shows, as its `view` event carries it.
interface LiveView {
  phase: string;
  words: string;
  line: string | null;
  speaking: boolean;
  title: string | null;
  finishing: boolean;
  /// Dictation: the words are typed where the cursor is, nothing is sent.
  dictating: boolean;
}

/// The `live` events the orb follows (Rust's `LiveEvent`), as far as it reads them.
type LiveEvent =
  | ({ kind: "view" } & LiveView)
  | { kind: "level"; level: number }
  | { kind: "toast"; text: string }
  | { kind: "soon"; ms: number | null }
  | { kind: "sent" }
  | { kind: "left" }
  | ({ kind: "approval" } & Waiting)
  | { kind: "answered"; query_id: string }
  | { kind: "withdrawn"; query_id: string };

const byId = (id: string) => document.getElementById(id)!;
const orb = byId("orb");
const x = byId("x");
const card = byId("ask");
const note = byId("toast");
const ring = byId("ring");
const mini = byId("mini");
const noticeEl = byId("notice");

/// The orb alone, and the window's width while a card shows.
const ORB = 112, WITH_CARD = 296, WITH_NOTE = 240;
/// How long a toast stays.
const TOAST_MS = 1600;
/// Fields the card shows; the rest are in the chat.
const FIELDS = 3;

const clip = (s: string, n: number) => s.length > n ? `${s.slice(0, n - 1).trimEnd()}…` : s;
/// A routine asking for a grant: no Always, as in the bar.
const forRoutine = (action: string | undefined) => /for a routine$/.test(action ?? "");
const ignore = () => {};

/// Whether live mode runs here, from its first view until it is left.
let live = false;

function draw(v: LiveView) {
  live = true;
  // The same states as the bar's orb: listening, thinking, speaking.
  const mode = v.speaking || (v.phase === "speaking" && v.line) ? "speak"
    : v.phase === "thinking" ? "think"
    : v.words || v.dictating ? "" : "idle";
  orb.className = `dorb ${mode}`;
  mini.classList.toggle("hold", v.phase === "holding");
  // While Option is held, or the last words come in, the note says so.
  state = v.dictating ? (v.phase === "thinking" ? "Transcribing…" : "Let go to type")
    : v.finishing ? "Sending…" : v.phase === "holding" ? "Let go to send" : "";
  if (v.phase !== "listening") soon(null);
  drawNote();
  const replying = !v.title && (v.phase === "thinking" || v.phase === "speaking");
  x.setAttribute("aria-label", v.dictating ? "Cancel dictation" : v.title ? "Stop the voice note" : replying ? "Cancel the reply" : "Leave live mode");
}

// ---------- Approval ----------

/// What waits, the first on the card.
const asks = new OrbAsks<Waiting>();
let failure = "";

// ---------- Notes ----------

/// The note for the state live mode is in, and a toast shown over it for a moment.
let state = "", toast: string | null = null, toastTimer: ReturnType<typeof setTimeout> | undefined;
/// Toasts that say something went wrong.
const BAD = /could not|not available|busy|failed/i;

function drawNote() {
  const text = toast ?? state;
  const bad = toast !== null && BAD.test(toast);
  if (note.textContent === text && note.classList.contains("bad") === bad && note.hidden === !text) return;
  note.textContent = text;
  note.classList.toggle("bad", bad);
  note.hidden = !text;
  fit();
}

function showToast(text: string) {
  toast = text;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { toast = null; drawNote(); }, TOAST_MS);
  drawNote();
}

/// A pause sends after `ms`: the ring runs down; null takes it away.
function soon(ms: number | null) {
  ring.hidden = true;
  if (ms == null) return;
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
    const waiting = asks.shown;
    const last = notice ? noticeEl : waiting ? card : note.hidden ? null : note;
    const height = last ? Math.ceil(last.getBoundingClientRect().bottom + parseFloat(getComputedStyle(last).marginBottom)) : ORB;
    const width = waiting || notice ? WITH_CARD : last ? WITH_NOTE : ORB;
    invoke("fit_orb", { width, height }).catch(ignore);
  });
}

/// Calls `act` with a button's `data-<key>` on its click, which goes no
/// further than the button.
function onButtons(el: HTMLElement, key: "d" | "n", act: (value: string) => void) {
  el.querySelectorAll<HTMLElement>(`[data-${key}]`).forEach(b => b.addEventListener("click", e => {
    e.stopPropagation();
    act(b.dataset[key]!);
  }));
}

/// Empties and hides `el`, or shows it; either way the window fits again.
function showCard(el: HTMLElement, shown: boolean) {
  if (!shown) el.replaceChildren();
  el.hidden = !shown;
  fit();
}

function drawAsk() {
  const waiting = asks.shown;
  if (!waiting) return showCard(card, false);
  const { app, action, fields } = waiting.approval;
  const grant = forRoutine(action);
  const shown = fields.slice(0, FIELDS), more = fields.length - shown.length, behind = asks.count - 1;
  const from = waiting.work ? "Background work asks first" : `${esc(app)} asks first`;
  card.innerHTML = `<div class="head">${appTile(app)}<div class="tt"><b title="${esc(action)}?">${esc(action)}?</b><small>${from}</small></div></div>
    ${shown.length ? `<div class="kv">${shown.map(f => `<span>${esc(clip(f.label, 24))}</span><span title="${esc(f.value)}">${esc(clip(f.value.replace(/\s+/g, " "), 120))}</span>`).join("")}</div>` : ""}
    ${more > 0 ? `<div class="more">${more} more in the chat</div>` : ""}
    ${behind > 0 ? `<div class="more">${behind} more waiting after this</div>` : ""}
    ${failure ? `<div class="err">${esc(failure)}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-d="allow">Allow</button>${grant ? "" : '<button class="dbtn" data-d="always">Always</button>'}<button class="dbtn warn" data-d="deny">Deny</button>${grant ? "" : '<button class="dbtn wide" data-d="session">Allow for this session</button>'}</div>`;
  onButtons(card, "d", answer);
  fillAppTiles(card).then(fit);
  showCard(card, true);
}

/// Shows `w`'s card, or counts it behind the one up, unless it is there
/// already or was answered here.
function ask(w: Waiting) {
  if (!asks.add(w)) return;
  if (asks.count === 1) failure = "";
  drawAsk();
}

/// Answers the card from the orb.
async function answer(decision: string) {
  const asked = asks.answer();
  if (!asked) return;
  failure = "";
  drawAsk();
  const queryId = asked.approval.query_id;
  try {
    if (asked.work) await invoke("answer_work", { session: asked.session, queryId, decision });
    else await invoke("answer_approval", { session: asked.session, queryId, decision, onEvent: new Channel() });
  } catch (err) {
    // Back on the card, with why.
    asks.failed(asked);
    failure = String(err);
    drawAsk();
  }
}

/// Drops the card for `queryId`, answered or withdrawn elsewhere.
function clear(queryId: string) {
  const first = asks.shown?.approval.query_id === queryId;
  if (!asks.remove(queryId)) return;
  if (first) failure = "";
  drawAsk();
}

// ---------- A new notice ----------

let notice: NoticeCard | null = null;
const X_ICON = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>';
const PLAY_ICON = '<svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>';

function drawNotice() {
  if (!notice) return showCard(noticeEl, false);
  if (!live) orb.className = "dorb idle";
  noticeEl.innerHTML = `<div class="head"><div class="tt"><b title="${esc(notice.title)}">${esc(notice.title)}</b><small>New message</small></div><button class="nx" data-n="dismiss" aria-label="Dismiss">${X_ICON}</button></div>
    ${notice.snippet ? `<div class="snip">${esc(notice.snippet)}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-n="play">${PLAY_ICON}Play</button><button class="dbtn" data-n="open">Open<kbd>⌥ ⌥</kbd></button></div>
    <div class="life"><i></i></div>`;
  onButtons(noticeEl, "n", noticeAct);
  showCard(noticeEl, true);
}

/// Play, Open or ×. Rust drops the card; it is gone here at once.
function noticeAct(act: string) {
  if (!notice) return;
  const { id } = notice;
  notice = null;
  drawNotice();
  // The voice note takes a moment to come; the orb thinks meanwhile.
  if (act === "play") orb.className = "dorb think";
  invoke(`${act}_notice`, { id }).catch(ignore);
}

noticeEl.addEventListener("click", () => noticeAct("open"));
// The pointer on the card keeps it up; leaving it starts the 20 seconds again.
noticeEl.addEventListener("mouseenter", () => { if (notice) invoke("hold_notice", { id: notice.id, held: true }).catch(ignore); });
noticeEl.addEventListener("mouseleave", () => {
  if (!notice) return;
  invoke("hold_notice", { id: notice.id, held: false }).catch(ignore);
  const life = noticeEl.querySelector(".life");
  if (life) life.innerHTML = "<i></i>";
});
listen<NoticeCard>("notice-card", e => { notice = e.payload; drawNotice(); });

card.addEventListener("click", () => { if (asks.shown) invoke("open_approval", { session: asks.shown.session }).catch(ignore); });

listen<LiveEvent>("live", e => {
  const v = e.payload;
  if (v.kind === "view") draw(v);
  else if (v.kind === "level") orb.style.setProperty("--lvl", Math.min(1, v.level).toFixed(3));
  else if (v.kind === "approval") ask({ session: v.session, approval: v.approval, work: v.work });
  else if (v.kind === "answered" || v.kind === "withdrawn") clear(v.query_id);
  else if (v.kind === "toast") showToast(v.text);
  else if (v.kind === "soon") soon(v.ms);
  if (v.kind === "sent") soon(null);
  // Leaving ends every question here; Rust shows those still waiting as
  // notifications.
  if (v.kind === "left") {
    asks.clear();
    failure = "";
    drawAsk();
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
  invoke("show_bar").catch(ignore);
});
x.addEventListener("click", e => {
  e.stopPropagation();
  if (notice && !live) return noticeAct("dismiss");
  invoke("cancel_live").catch(ignore);
});
// Approvals that came before this window was made.
invoke<Waiting[]>("live_approval").then(ws => { for (const w of ws ?? []) ask(w); }).catch(ignore);
// A notice card that came before this window was made.
invoke<NoticeCard | null>("notice_card").then(c => { if (c && !notice) { notice = c; drawNotice(); } }).catch(ignore);
