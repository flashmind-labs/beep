import { body, type Decision, focusAsk, followAnswered, forRoutine, newTurn, renderChat, reply, scrollDown, settle } from "./chat.ts";
import { touch } from "./chats.ts";
import { $, $$, bar, entry, esc, h, here, ICON, invoke, listen, live, PANE, S, type Turn } from "./core.ts";
import { hints, keyGroups, LIVE_SAY, LIVE_TALK, refreshHints } from "./hints.ts";
import { openSession } from "./history.ts";
import { notice } from "./media.ts";
import { forgetHeight } from "./size.ts";
import { type HeldQuestion, waitingQuestion } from "./work.ts";

// ---------- Live voice ----------

// Live mode keeps the chat on screen as it always is: turns stream in, it
// scrolls, and links, cards and the approval card work as usual. A dock takes
// the field's place: a small orb that follows the microphone while listening
// and Beep's voice while speaking, one status word, the caption, the hint
// and the way out. Rust runs the microphone, the server's live transcription
// and the player; this draws.
/// What live mode shows (LiveEvent::View).
export interface LiveView {
  kind: "view";
  phase: string;
  /// What the user said so far this turn.
  words: string;
  /// Beep's sentence on screen, and the one before it.
  line: string | null;
  prev: string | null;
  /// The tool step running.
  step: string | null;
  speaking: boolean;
  pause_sends: boolean;
  session: string | null;
  /// The voice note's title, when one started live mode.
  title: string | null;
  /// Push to talk: nothing listens between holds.
  hold_only: boolean;
  finishing: boolean;
  /// The sheet of what to say is open.
  sheet: boolean;
}

/// What the bar hears about live mode, as the `live` event (LiveEvent).
type LiveEvent =
  | LiveView
  | { kind: "level"; level: number }
  | { kind: "toast"; text: string }
  | { kind: "soon"; ms: number | null }
  | { kind: "sent"; session: string; text: string; run: string }
  | { kind: "left"; draft: string | null; session: string | null; quiet: boolean; failure: string | null }
  | { kind: "approval" }
  | { kind: "answered"; session: string; query_id: string; run: string; decision: Decision };

export const VOICE_ICON = {
  speaker: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9h4l5-4v14l-5-4H4z"/><path d="M17 9a4 4 0 010 6M19.5 6.5a8 8 0 010 11"/></svg>',
  mic: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><rect x="9" y="3" width="6" height="12" rx="3"/><path d="M5 11a7 7 0 0014 0M12 18v3"/></svg>',
};
/// `drafts` holds the words live mode kept when it ended, by chat ("" for a
/// new one), until that chat takes them, and `note` says why it ended.
/// `typed` is a key pressed to leave live mode, for the prompt after the words.
export const VN = { drafts: new Map<string, string>(), note: "", typed: "" };
/// The live view: the last view Rust sent, a toast, the pause bar, the
/// message just sent, and the chat under the dock (null for a new one).
export const LV: {
  on: boolean;
  view: LiveView | null;
  toast: string;
  toastTimer: ReturnType<typeof setTimeout> | undefined;
  /// When a pause sends, and how long that wait is in all.
  soon: { until: number; total: number } | null;
  sent: string;
  chat: string | null;
} = { on: false, view: null, toast: "", toastTimer: undefined, soon: null, sent: "", chat: null };
export const redrawLive = () => LV.on && renderLive();
/// Chats live mode talked in, marked with a mic in the chat list.
export const LIVE_CHATS = new Set<string>();
export async function loadLiveChats() {
  const ids = await invoke<string[]>("live_chats").catch((): string[] => []);
  for (const id of ids) LIVE_CHATS.add(id);
}
export const LIVE_KEYS = [["tap ⌥", "leave"], ["hold ⌥", "talk, let go to send"], ["esc", "leave live"], ["⌘/", "what to say"]];
/// Push to talk, between holds.
export const LIVE_PTT_KEYS = [["hold ⌥", "reply"], ["tap ⌥", "leave"], ["esc", "leave live"], ["⌘/", "what to say"]];
/// The sheet of what to say, over the chat and the dock.
export const liveSheet = () => `<div class="dover lsheet" role="dialog" aria-label="What to say"><div class="vt"><span class="tt"><b>What to say</b></span></div>
  ${keyGroups([["Say", LIVE_SAY], ["Keys", LIVE_TALK]])}
  ${hints([["esc", "close"], ["⌘/", "close"]])}</div>`;
/// What the dock and the keys row say for view `v`.
export function liveParts(v: LiveView) {
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
  const prev = (t: string | null | undefined) => t ? `<span class="prev">${esc(t)}</span>` : "";
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
  const grant = asked && (e?.approval ? e.approval.grant : forRoutine((asked as HeldQuestion).action));
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
export function renderLive() {
  const v = LV.view;
  if (!v) return;
  if (S.view !== "chat" || !$("#ldock")) openLive(v.session ?? null);
  drawDock(v);
}
/// Shows chat `id` (null for a new one) with the dock in place of the field.
/// The chat the bar already shows stays as it is.
export function openLive(id: string | null) {
  const same = S.view === "chat" && !!$("#ask") && (S.session ?? null) === id;
  LV.chat = id;
  if (!same) {
    if (id) openSession(id);
    else { S.session = null; renderChat(); }
  }
  mountDock();
}
/// Puts the dock over the chat. The field stays, out of sight, for after.
export function mountDock() {
  const field = $(".dinput.dgrow");
  if (!field || $("#ldock")) return;
  field.classList.add("lhid");
  // Keys go to live mode, not to the hidden field.
  $("#ask")?.blur();
  field.after(h('<div class="ddock" id="ldock" role="region" aria-label="Live mode"></div>'));
}
export function drawDock(v: LiveView | null) {
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
  if (lvl) $$("#orb").style.setProperty("--lvl", lvl);
  const cap = dock.querySelector(".cap")!;
  cap.scrollTop = cap.scrollHeight;
  $$("#orb").addEventListener("click", () => invoke("live_tap").catch(() => {}));
  $$("#lleave").addEventListener("click", () => invoke("cancel_live").catch(() => {}));
  $("#lcancel")?.addEventListener("click", () => invoke("cancel_live").catch(() => {}));
  refreshHints();
  const sheet = bar.querySelector(".lsheet");
  if (v.sheet && !sheet) bar.append(h(liveSheet()));
  else if (!v.sheet) sheet?.remove();
}
/// Draws a message live mode sent into the chat under the dock and follows
/// its reply there, like one typed in the field. With no dock on screen the
/// chat comes from the server when it is next opened.
export function liveTurn(id: string, text: string, runId: string) {
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
  const b = body()!;
  b.querySelectorAll(".dreplies").forEach(r => r.remove());
  // The server's copy may have drawn the message already.
  const last = [...b.querySelectorAll<Turn>(".turn")].at(-1);
  const turn = last?.user && !last.done && last.querySelector(".ask")?.textContent === text ? last : newTurn(text);
  turn.user = true;
  turn.steps ??= [];
  reply(turn).innerHTML = '<span class="caret"></span>';
  e.body = b;
  e.run = { id: runId, turn };
  refreshHints();
  scrollDown();
  followAnswered(id, turn, runId);
}
/// Live mode ended: the dock goes and the chat keeps its field, with the
/// words kept in it and a note on why it ended.
export function renderLeft() {
  bar.querySelector(".lsheet")?.remove();
  const chat = LV.chat;
  LV.chat = null;
  if (S.view === "chat" && $("#ldock") && chat === (S.session ?? null)) {
    $$("#ldock").remove();
    $(".dinput.dgrow")?.classList.remove("lhid");
    refreshHints();
    takeDraft($<HTMLTextAreaElement>("#ask"));
    focusAsk();
    return;
  }
  if (S.session) openSession(S.session);
  else renderChat();
}
/// Puts a kept draft in the open chat's prompt, after anything typed there.
export function takeDraft(ask: HTMLTextAreaElement | null) {
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
export function heard(turn: Turn, text: string) {
  const ask = turn.querySelector<HTMLElement>(".ask") ?? h('<div class="ask"></div>');
  if (!ask.isConnected) turn.prepend(ask);
  ask.textContent = text;
  ask.classList.remove("dmuted");
  touch(S.session!, text);
}
// Live mode stays in the bar and the orb.
if (!PANE) listen<LiveEvent>("live", e => {
  const v = e.payload;
  switch (v.kind) {
    case "view":
      if (!LV.on) {
        forgetHeight();
        // What was typed in the prompt comes back when live mode ends.
        const ask = S.view === "chat" ? $<HTMLTextAreaElement>("#ask") : null, typed = ask?.value.trim();
        if (typed) {
          const key = S.session ?? "";
          const kept = VN.drafts.get(key);
          VN.drafts.set(key, kept ? `${typed} ${kept}` : typed);
          ask!.value = "";
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
      if (!e || pending?.queryId !== v.query_id) return;
      return followAnswered(v.session, settle(v.session, e, pending!, v.decision), v.run);
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
      return renderLeft();
  }
});

