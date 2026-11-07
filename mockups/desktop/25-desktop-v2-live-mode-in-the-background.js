// ---- Desktop v2: live mode in the background ----
// Only Option Option opens Beep. With the bar open, a tap of Option starts live mode
// and holding it talks while held. Esc or Cancel stops a reply being written or read
// (CancelRun) and keeps what was said as the draft. Clicking elsewhere hides the bar
// and live mode goes on in a small orb at the top right of the menu bar's screen.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .dlv .lcancel { margin-top: 2px }
.mac .dmini { position: absolute; top: 34px; right: 12px; z-index: 5; width: 64px; height: 64px; border-radius: 50%; display: grid; place-items: center;
  background: rgba(21,22,23,.86); backdrop-filter: blur(20px); border: 1px solid var(--d-border); box-shadow: 0 10px 28px rgba(0,0,0,.5); cursor: pointer; animation: barIn .22s var(--ease) }
.mac .dmini .dorb { width: 56px; height: 56px; margin: 0 } .mac .dmini .dorb .core { width: 34px; height: 34px } .mac .dmini .dorb .ring { inset: 11px }
.mac .dmini .x { position: absolute; top: -4px; right: -4px; width: 22px; height: 22px; border-radius: 50%; border: 1px solid var(--d-border); background: var(--d-elev); color: var(--d-fg);
  display: none; place-items: center; padding: 0 }
.mac .dmini.hov .x, .mac .dmini:hover .x { display: grid }`;
  document.head.append(st);
})();

const MINI_X = `<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>`;
/// The orb window: no bar, just the orb in the top right. Clicking it opens the bar.
const dMini = (mode, hover, o = {}) => `<div class="dmini${hover ? " hov" : ""}${o.hold ? " hold" : ""}" data-dgo="${{ think: "dThink", speak: "dSpeak" }[mode] || "dLive"}" role="button" aria-label="Open Beep">
    ${dOrb({ listen: "", think: "think", speak: "speak", idle: "idle" }[mode])}
    ${o.ring ? '<svg class="oring" viewBox="0 0 70 70" aria-hidden="true"><circle cx="35" cy="35" r="33"/></svg>' : ""}
    <button class="x" ${o.xAct || 'data-dgo="dLeft"'} aria-label="${o.xLabel || (mode === "listen" ? "Leave live mode" : "Cancel the reply")}">${MINI_X}</button></div>`;
/// The short note under the orb: a toast, or what Option is doing.
const dOrbToast = (text, bad) => `<div class="dorbtoast${bad ? " bad" : ""}" role="status">${text}</div>`;

// Thinking and speaking: Esc or Cancel stops the reply on the server and keeps what was said.
const cancelBtn = `<button class="dbtn quiet lcancel" data-dgo="dLeft" data-dact="dset:le:cancel">Cancel<kbd>esc</kbd></button>`;
const withCancel = html => html.replace(/(<div class="hint">[^<]*<\/div>)/, `$1${cancelBtn}`);
const dThinkBase = D.dThink;
D.dThink = () => withCancel(dThinkBase()).replace(/<span><kbd>esc<\/kbd>leave live<\/span>/, "<span><kbd>esc</kbd>cancel reply</span>");
const dSpeakBase = D.dSpeak;
D.dSpeak = () => {
  if (S.d.ls === "note") {
    // A voice note plays in the orb window. It does not open the bar or listen afterwards.
    return deskScene(dMini("speak", false, { xLabel: "Stop the voice note", xAct: 'data-toast="Stopped the voice note"' }));
  }
  return withCancel(dSpeakBase()).replace(/<span><kbd>esc<\/kbd>leave live<\/span>/, "<span><kbd>esc</kbd>cancel reply</span>");
};
VARIANTS.dSpeak = dv("ls", [["Reply", "reply"], ["A voice note arrives", "note"]]);

// Left live mode after Cancel: the reply stopped and the words are back in the field.
const dLeftBase = D.dLeft;
D.dLeft = () => S.d.le !== "cancel" ? dLeftBase() : deskScene(dBar(`<div class="dinput multi dgrip"><span class="txt">Tell Alex Thursday works, and move the dentist to next week<span class="caret" style="height:20px"></span></span></div>
      <div class="dmuted" style="padding:0 16px 10px">${I.micS}Cancelled the reply. Your words are kept here.</div>` + dHints([[DI.ret, "send"], ["tap ⌥", "talk again"], ["esc", "to hide"]])));
VARIANTS.dLeft = dv("le", [["Tapped Option or esc", "draft"], ["Cancelled a reply", "cancel"], ["2 minutes quiet", "quiet"], ["Nothing said", "empty"]]);

// In the background: the bar lost focus during live mode, so it hides and the orb stays.
D.dLiveMini = () => {
  const m = S.d.lm || "listen";
  // Hover shows × to cancel the reply, or to leave while listening.
  if (m === "hold") return deskScene(dMini("listen", false, { hold: true }) + dOrbToast("Let go to send"));
  if (m === "sending") return deskScene(dMini("think", false) + dOrbToast("Sending…"));
  if (m === "dictate") return deskScene(dMini("listen", false, { hold: true }) + dOrbToast("Let go to type"));
  if (m === "typing") return deskScene(dMini("think", false) + dOrbToast("Transcribing…"));
  if (m === "dictate-bad") return deskScene(dMini("listen", false) + dOrbToast("Add an OpenAI key in Assistant first.", true));
  if (m === "soon") return deskScene(dMini("listen", false, { ring: true }));
  if (m === "toast") return deskScene(dMini("listen", false) + dOrbToast("Beep needs your OK"));
  if (m === "bad") return deskScene(dMini("listen", false) + dOrbToast("Could not send", true));
  return deskScene(m === "hover" ? dMini("speak", true) : dMini(m, false));
};
VARIANTS.dLiveMini = dv("lm", [["Listening", "listen"], ["Thinking", "think"], ["Speaking", "speak"], ["Hover", "hover"], ["Holding Option", "hold"], ["Sending", "sending"], ["Dictating", "dictate"], ["Dictation, transcribing", "typing"], ["Dictation, a failure", "dictate-bad"], ["About to send", "soon"], ["A note", "toast"], ["A failure", "bad"]]);
{
  const g = DESK_SCREENS.find(g => g.group === "Live voice").items;
  g.splice(g.findIndex(it => it[0] === "dInterrupt") + 1, 0, ["dLiveMini", "In the background", "OrbWindow"]);
}

// Settings: a voice note is read out and nothing listens afterwards.
const dLiveSettingsBase = D.dLiveSettings;
D.dLiveSettings = () => dLiveSettingsBase().replace(/Speak new notices when you are at this Mac and not on a call[^<]*/, "Speak new notices when you are at this Mac and not on a call. Tap Option twice to open Beep, then once more to talk live, or hold Option to talk while it is held.");
