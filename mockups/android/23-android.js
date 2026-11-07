// ---- Android ----
// The same dock over ChatScreen, in place of the composer. The orb's tap, the close button
// and Cancel act as on the iPhone.
Object.assign(S, { alvL: "words", alvT: "think", alvS: "reply", alvA: "ask", alvLeft: "draft" });
const aOn = {
  orb: mode => mode === "speak" ? 'data-set="alvS:stopped"' : mode === "think" ? 'data-go="aLive" data-set="alvL:empty"' : 'data-go="aLiveLeft" data-set="alvLeft:draft"',
  leave: mid => `data-go="aLiveLeft" data-set="alvLeft:${mid ? "cancel" : "draft"}"`,
};
const aCancel = `<div class="lbtns"><button class="lbtn" data-go="aLiveLeft" data-set="alvLeft:cancel">Cancel</button></div>`;
const aChat = (now, comp = composer()) => topBar().replace("Boiler and Saturday weather", "Plumber and dentist") + `<div class="body">
    <div class="user">Did the plumber get back to me?</div>
    <div class="reply">Yes, Alex wrote at 08:12: the plumber can come Thursday between 9 and 12, and she asks if that works.</div>
    ${now}
  </div>` + comp;
const aLive = (now, o) => aChat(now) + lvDock(o, aOn);
const A_CARET = '<span class="caret"></span>';
Object.assign(R, {
  aLive: () => {
    const o = {
      words: { mode: "listen", cap: 'What’s on my calendar <span class="tent">tomorrow morning</span>' },
      soon: { mode: "listen", cap: "What’s on my calendar tomorrow morning", soon: true, hint: "Sending… keep talking to add more" },
      pause: { mode: "listen", cap: "Remind me to call the plumber at nine", hint: "Say “send” or “stop” when you’re done · Tap the orb to leave" },
      empty: { mode: "idle", cap: '<span class="ph">Go ahead, I’m listening</span>', says: true },
      hold: { mode: "hold", word: "Holding", cap: "Text Sam that I’m running ten minutes late", hint: "Let go to send · Slide away to cancel" },
      send: { mode: "listen", cap: 'Book a table for four at eight <span class="cmd">send</span>', toast: "Sending" },
      connecting: { mode: "idle", word: "Connecting", cap: '<span class="ph">Getting ready…</span>', hint: "" },
      off: { mode: "idle", word: "Voice unavailable", cap: '<span class="ph">Beep can’t hear you. Allow the microphone for Beep in Settings.</span>', hint: "",
        extra: `<div class="lbtns"><button class="lbtn pri" data-toast="Opens Settings, Beep">Open Settings</button></div>` },
    }[S.alvL];
    // Held from the composer's mic, the composer stays until let go, so the finger keeps the mic; the dock comes after.
    if (S.alvL === "mic") return aChat("", composer("Let go to send · Slide away to cancel"));
    return aLive("", o);
  },
  aLiveThink: () => {
    const v = S.alvT;
    if (v === "says") return aLive(`<div class="user">Is build-box running out of disk?</div>${toolChip("Running code", "S").replace(/<span class="st"[\s\S]*?<\/span><\/div>$/, '<span class="st"><span class="spin"></span></span></div>')}<div class="reply">Let me check build-box.${A_CARET}</div>`,
      { mode: "speak", cap: `<span class="prev">Is build-box running out of disk?</span>Let me check build-box.`, hint: "Tap the orb to stop", extra: aCancel });
    const chip = v === "tool" ? `<div class="chip" style="background:var(--m-t1);align-self:flex-start"><span class="ini wiggle">G</span>Checking Google Calendar<span class="st"><span class="spin"></span></span></div>` : "";
    return aLive(`<div class="user">${LV_CAL}</div>${chip}<div class="reply">${A_CARET}</div>`,
      { mode: "think", cap: v === "tool" ? `<span class="prev">${LV_CAL}</span><span class="step"><span class="spin"></span>Checking Google Calendar</span>` : `<span class="prev">${LV_CAL}</span><span class="ph">…</span>`, hint: "Tap the orb to stop the answer", extra: aCancel });
  },
  aLiveSpeak: () => {
    const now = `<div class="user">${LV_CAL}</div>${toolChip("Checked Google Calendar", "G")}<div class="reply">${LV_CAL_REPLY}</div>`;
    if (S.alvS === "stopped") return aLive(now, { mode: "listen", toast: "Stopped", cap: '<span class="prev">You have the dentist at 9:30,</span><span class="ph">Go ahead</span>' });
    return aLive(now, { mode: "speak", cap: '<span class="prev">You have the dentist at 9:30,</span>then a call with Alex at 11 about the plumber.', hint: "Tap the orb or say “stop” to interrupt", extra: aCancel });
  },
  aLiveAsk: () => {
    const ask = S.alvA === "ask";
    const card = `<div class="ask">
        <div class="head"><span class="logo" style="width:32px;height:32px;font-size:13px;background:var(--m-t0)">F</span>
          <div class="grow"><b style="font-weight:600;font-size:14px">Send this email?</b><span class="muted" style="font-size:12px">Fastmail asks before sending</span></div>${I.shield}</div>
        <div class="kv"><span>To</span><span>Alex Chen</span><span>Subject</span><span>Plumber on Thursday</span></div>
        <div class="draft">Hi Alex, Thursday morning is fine. The door code is the same as last time.</div>
        <div class="acts"><span style="flex:1"></span>
          <button class="pill" style="min-height:38px;padding:0 14px" data-set="alvA:done" data-toast="Denied">Deny</button>
          <button class="go" style="min-height:38px;box-shadow:0 3px 0 var(--m-ledge)" data-set="alvA:done" data-toast="Allowed once">Allow</button>
        </div></div>`;
    const now = `<div class="user">Tell Alex Thursday morning works</div>` + (ask ? card
      : `<div class="muted" style="font-size:12px">Allowed once</div><div class="chip" style="background:var(--m-t0);align-self:flex-start"><span class="ini wiggle">F</span>Sending email<span class="st"><span class="spin"></span></span></div><div class="reply">${A_CARET}</div>`);
    return aLive(now, ask ? { mode: "idle", word: "Waiting for you", card: true, hint: "Beep goes on once you answer" }
      : { mode: "think", cap: `<span class="prev">Tell Alex Thursday morning works</span><span class="step"><span class="spin"></span>Sending email</span>`, hint: "Tap the orb to stop the answer", extra: aCancel });
  },
  // Left: the chat with its composer back, the words kept in it and a line on why it ended.
  aLiveLeft: () => {
    const v = S.alvLeft, kept = v !== "empty" && v !== "failed";
    const text = v === "cancel" ? LV_CAL : kept ? "And move the call with Alex to" : "";
    const now = `<div class="user">${LV_CAL}</div>${toolChip("Checked Google Calendar", "G")}<div class="reply">${v === "cancel" ? 'You have the dentist at 9:30,</div><div class="muted" style="font-size:12px">Stopped' : LV_CAL_REPLY}</div>`;
    const note = `<div class="lvnote">${I.micS}<span>${LV_ENDED[v] || LV_ENDED.draft}</span><button aria-label="Dismiss" data-toast="Dismissed">${I.x}</button></div>`;
    const comp = text ? composer(text).replace('<span style="flex:1">', '<span style="flex:1;color:var(--m-ink)">') : composer();
    return aChat(now, note + comp);
  },
});
SCREENS.splice(SCREENS.findIndex(g => g.group === "Group chats"), 0, { group: "Live voice", items: [
  ["aLive", "Listening", "LiveDock over ChatScreen"],
  ["aLiveThink", "Thinking", "LiveDock, thinking"],
  ["aLiveSpeak", "Speaking", "LiveDock, speaking"],
  ["aLiveAsk", "Asks first", "LiveDock, ApprovalCard in the chat"],
  ["aLiveLeft", "Left live mode", "ChatScreen, draft"],
] });
Object.assign(VARIANTS, {
  aLive: vs("alvL", [["Words coming in", "words"], ["About to send", "soon"], ["Waiting for “send”", "pause"], ["Nothing said yet", "empty"], ["Holding the orb", "hold"], ["Holding the composer mic", "mic"], ["Heard “send”", "send"], ["Connecting", "connecting"], ["No microphone", "off"]]),
  aLiveThink: vs("alvT", [["Sent", "think"], ["Using a tool", "tool"], ["Says what it is doing", "says"]]),
  aLiveSpeak: vs("alvS", [["Reply", "reply"], ["Tapped the orb", "stopped"]]),
  aLiveAsk: vs("alvA", [["Asks first", "ask"], ["Allowed", "done"]]),
  aLiveLeft: vs("alvLeft", [["Left with words unsent", "draft"], ["Nothing unsent", "empty"], ["Cancelled a reply", "cancel"], ["2 minutes quiet", "quiet"], ["Voice failed", "failed"]]),
});
