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
.lvnote { display: flex; align-items: center; gap: 8px; padding: 6px 12px 0 20px; font-size: 12px; color: var(--m-muted) }
.lvnote span { flex: 1; min-width: 0 }
.lvnote button { border: 0; background: transparent; color: var(--m-dim); width: 32px; height: 32px; display: grid; place-items: center; padding: 0 }`;
  document.head.append(st);
})();

const LV_HINT = "Pause to send · Tap the orb to leave · Hold it to talk";

// Back in the chat: why live mode ended, over the composer, until the next message or ×.
const LV_ENDED = {
  draft: "Left live mode. Your words are kept here.",
  empty: "Left live mode.",
  quiet: "Live mode ended after 2 minutes quiet. Your words are kept here.",
  call: "Live mode ended for a call. Your words are kept here.",
  cancel: "Cancelled the reply. Your words are kept here.",
  failed: "Voice is not available right now.",
};
