// ---- iPhone live voice: connecting, problems, Cancel, asking first and why it ended ----
// Live voice opens in a window of its own, over any sheet that is open. The mic starts at once
// and what is said while the session connects is kept for it; until then the word reads
// "Connecting". A problem offers a way on: Open Settings when the microphone is off for Beep,
// Try again otherwise. While Beep thinks or speaks, Cancel stops the reply on the server too and
// leaves with the question as the draft, as on the Mac; the close button does the same mid-reply.
// A reply that asks first shows its approval card on the live screen with Allow, Always and Deny
// (no spoken yes or no). A tap on the orb while listening leaves, so the hints say leave, and after
// the first turn a short line keeps the spoken way out in view. A call pauses live mode and it
// carries on after, or ends for the call. Back in the chat, a line over the composer says why
// live mode ended. When the phone is locked a short sound marks listening again and leaving.
(() => {
  const st = document.createElement("style");
  st.textContent = `.ilive .lbtns { display: flex; gap: 8px; justify-content: center }
.ilive .lbtn { min-height: 44px; padding: 0 22px; border-radius: 999px; border: 0; background: var(--lv-chip); color: var(--lv-ink); font-size: 15px; font-weight: 600 }
.ilive .lbtn.pri { background: var(--m-ok); color: #fff }
.ilive .again { font-size: 12px; color: var(--lv-dim); text-align: center }
.ilive .lask { width: 100%; max-height: 300px; overflow-y: auto; text-align: left; color: var(--m-ink); scrollbar-width: none }
.ilive .lask .head .grow b { font-size: 15px; font-weight: 600 } .ilive .lask .head .grow span { font-size: 12px; color: var(--m-muted) }
.lvnote { display: flex; align-items: center; gap: 8px; padding: 6px 12px 0 20px; font-size: 12px; color: var(--m-muted) }
.lvnote span { flex: 1; min-width: 0 }
.lvnote button { border: 0; background: transparent; color: var(--m-dim); width: 32px; height: 32px; display: grid; place-items: center; padding: 0 }`;
  document.head.append(st);
})();

const LV_HINT = "Pause to send · Tap the orb to leave · Hold it to talk";
const lvBtn = (label, attrs, pri) => `<button class="lbtn${pri ? " pri" : ""}" ${attrs}>${label}</button>`;
const lvCancel = lvBtn("Cancel", 'data-lvleft="cancel"', false);
const iLiveViewBase = iLiveView;
iLiveView = o => {
  let html = iLiveViewBase({ ...o, hint: o.hint === undefined ? LV_HINT : o.hint, cap: o.card ? "" : o.cap });
  html = html.replace("Pause to send · Tap the orb to stop · Hold it to talk", LV_HINT)
    .replace("Say “send” or “stop” when you’re done · Tap the orb to stop", "Say “send” or “stop” when you’re done · Tap the orb to leave")
    .replace('aria-label="Tap to stop, hold to talk"', 'aria-label="Tap to leave, hold to talk"')
    // Mid-reply the close button cancels the reply as well.
    .replace('data-iset="lvLeft:draft" aria-label="Leave live mode"', o.mode === "think" || o.mode === "speak"
      ? 'data-lvleft="cancel" aria-label="Cancel the reply and leave"' : 'data-lvleft="draft" aria-label="Leave live mode"');
  if (o.card) html = html.replace('<div class="cap" aria-live="polite"></div>', o.card);
  if (o.extra) html = html.replace(/(<div class="lbot">[\s\S]*?)(\s*<\/div>\s*<div class="lhome">)/, `$1${o.extra}$2`);
  return html;
};
document.getElementById("ios").addEventListener("click", e => {
  const el = e.target.closest("[data-lvleft]");
  if (!el) return;
  e.stopPropagation();
  S.i.lv.left = el.dataset.lvleft;
  go("iLiveLeft");
}, true);

const iLiveBase = P.iLive;
P.iLive = () => {
  const v = S.i.lv.listen;
  const o = {
    connecting: { mode: "idle", word: "Connecting", cap: '<span class="ph">Getting ready…</span>', hint: "" },
    again: { mode: "idle", cap: '<span class="prev">You have the dentist at 9:30, then a call with Alex at 11.</span><span class="ph">Go ahead</span>',
      hint: "Say “send” or “stop” when you’re done · Tap the orb to leave" },
    paused: { mode: "idle", word: "Paused", cap: '<span class="ph">Paused while another app uses the audio</span>', hint: "Beep carries on when it’s done" },
    off: { mode: "idle", word: "Voice unavailable", cap: '<span class="ph">Beep can’t hear you. Allow the microphone for Beep in Settings.</span>', hint: "",
      extra: `<div class="lbtns">${lvBtn("Open Settings", 'data-itoast="Opens Settings, Beep"', true)}</div>` },
    failed: { mode: "idle", word: "Voice unavailable", cap: '<span class="ph">Voice is not available right now</span>', hint: "",
      extra: `<div class="lbtns">${lvBtn("Try again", 'data-iset="lvListenAgain:1"', true)}</div>` },
  }[v];
  return o ? iLiveUnder() + iLiveView(o) : iLiveBase();
};
document.getElementById("ios").addEventListener("click", e => {
  if (!e.target.closest('[data-iset="lvListenAgain:1"]')) return;
  e.stopPropagation();
  S.i.lv.listen = "connecting";
  render();
}, true);

// Thinking and speaking: Cancel under the hint.
for (const id of ["iLiveThink", "iLiveSpeak"]) {
  const base = P[id];
  P[id] = () => {
    const html = base();
    if (id === "iLiveSpeak" && S.i.lv.speak === "stopped") return html;
    return html.replace(/(<div class="lbot">[\s\S]*?)(\s*<\/div>\s*<div class="lhome">)/, `$1<div class="lbtns">${lvCancel}</div>$2`);
  };
}

// A reply that asks first: the approval card on the live screen, answered with a tap.
const lvAsk = `<div class="ask lask">
  <div class="head"><span class="logo" style="width:32px;height:32px;font-size:14px;background:var(--m-surface)">F</span>
    <div class="grow"><b>Send email with Fastmail?</b><span>Fastmail asks before it does this</span></div></div>
  <div class="kv"><span>To</span><span>Alex Chen</span><span>Subject</span><span>Plumber on Thursday, 9 to 12 works for me</span>
    <span>Body</span><span>Hi Alex, Thursday morning is fine. The door code is the same as last time.</span></div>
  <div class="acts"><button class="go" style="min-height:44px;background:var(--m-ok);color:#fff" data-iset="lvAsk:done" data-itoast="Allowed once">Allow</button>
    <button class="pill" data-iset="lvAsk:done" data-itoast="Always allowed for Fastmail">Always</button><span style="flex:1"></span>
    <button class="text-btn" style="color:var(--m-error)" data-iset="lvAsk:done" data-itoast="Denied">Deny</button></div>
</div>`;
P.iLiveAsk = () => S.i.lvAsk === "done"
  ? iLiveUnder() + iLiveView({ mode: "think", cap: `<span class="prev">Tell Alex Thursday morning works</span><span class="step"><span class="spin"></span>Sending email</span>`, hint: "Tap the orb to stop the answer", extra: `<div class="lbtns">${lvCancel}</div>` })
  : iLiveUnder() + iLiveView({ mode: "idle", word: "Waiting for you", card: lvAsk, hint: "Beep goes on once you answer" });
VARIANTS.iLiveAsk = [["Asks first", () => { S.i.lvAsk = "ask"; render(); }, () => S.i.lvAsk !== "done"], ["Allowed", () => { S.i.lvAsk = "done"; render(); }, () => S.i.lvAsk === "done"], ...lvLook];
{
  const g = IOS_SCREENS.find(g => g.group === "Live voice").items;
  g.splice(g.findIndex(it => it[0] === "iLiveSpeak") + 1, 0, ["iLiveAsk", "Asks first", "LiveScreen, ApprovalCard"]);
}
VARIANTS.iLive.splice(0, 0, ...lvSet("listen", [["Connecting", "connecting"]]));
VARIANTS.iLive.splice(VARIANTS.iLive.findIndex(v => v[0] === "No microphone") + 1, 0,
  ...lvSet("listen", [["Couldn’t connect", "failed"], ["After a reply", "again"], ["Paused for a call", "paused"]]));

// Back in the chat: why live mode ended, over the composer, until the next message or ×.
const LV_ENDED = {
  draft: "Left live mode. Your words are kept here.",
  empty: "Left live mode.",
  quiet: "Live mode ended after 2 minutes quiet. Your words are kept here.",
  call: "Live mode ended for a call. Your words are kept here.",
  cancel: "Cancelled the reply. Your words are kept here.",
  failed: "Voice is not available right now.",
};
const iLiveLeftBase = P.iLiveLeft;
P.iLiveLeft = () => {
  const v = S.i.lv.left;
  const kept = v !== "empty" && v !== "failed";
  S.i.lv.left = kept ? "draft" : "empty";
  let html = iLiveLeftBase();
  S.i.lv.left = v;
  if (v === "cancel") html = html.replace("And move the call with Alex to", "What’s on my calendar tomorrow morning?")
    .replace("You have the dentist at 9:30, then a call with Alex at 11 about the plumber.", "You have the dentist at 9:30,");
  const note = `<div class="lvnote">${I.micS}<span>${LV_ENDED[v] || LV_ENDED.draft}</span><button aria-label="Dismiss" data-itoast="Dismissed">${I.x}</button></div>`;
  const at = html.lastIndexOf('<div class="icomp">');
  return at < 0 ? html : html.slice(0, at) + note + html.slice(at);
};
VARIANTS.iLiveLeft = lvSet("left", [["Left with words unsent", "draft"], ["Nothing unsent", "empty"], ["Cancelled a reply", "cancel"],
  ["2 minutes quiet", "quiet"], ["Ended for a call", "call"], ["Voice failed", "failed"]]);

// The server could not voice a line: it shows as Beep's line for a moment, with a note.
{
  const base = P.iLiveSpeak;
  P.iLiveSpeak = () => S.i.lv.speak !== "novoice" ? base()
    : iLiveUnder() + iLiveView({ mode: "speak", cap: '<span class="prev">You have the dentist at 9:30,</span>then a call with Alex at 11 about the plumber.<span style="display:block;margin-top:8px;font-size:13px;line-height:18px;font-weight:400;color:var(--lv-muted)">Beep’s voice is unavailable</span>',
        hint: "Tap the orb or say “stop” to interrupt", extra: `<div class="lbtns">${lvCancel}</div>` });
  VARIANTS.iLiveSpeak.splice(2, 0, ...lvSet("speak", [["Voice unavailable", "novoice"]]));
}
