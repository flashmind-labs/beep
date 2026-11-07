// ---- iPhone live voice (LiveView.swift, LiveSession in the bridge) ----
// Live mode on the phone, as on the Mac: Beep listens, the words show as they are said, a pause
// (or "send") sends, and the reply is read out sentence by sentence before it listens again.
// It opens full screen. In Japan on iOS 26.2+ a press and hold of the side button opens it at once
// (the "activate" assistant intent); elsewhere the chat bar's live button, the Action Button, a
// Control Center control or Shortcuts do. The launch: a soft wash in Beep's colors floods in from
// the side button for 0.25 s, then contracts into a glow hugging the rounded screen edge that keeps
// flowing while live mode is on and brightens with the voice. The orb comes out of the wash and
// settles in the middle. Reduce Motion: the scene fades in, the glow stands still.
(() => {
  const st = document.createElement("style");
  st.textContent = `@property --ga { syntax: '<angle>'; inherits: false; initial-value: 0deg }
@property --ew { syntax: '<length>'; inherits: false; initial-value: 0px }
@property --hole { syntax: '<percentage>'; inherits: false; initial-value: 0% }
.ilive { position: absolute; left: 0; right: 0; top: -50px; bottom: -22px; z-index: 12; display: flex; flex-direction: column; overflow: hidden; border-radius: 46px;
  --lv-bg: var(--m-page); --lv-ink: var(--m-ink); --lv-muted: var(--m-muted); --lv-dim: var(--m-dim); --lv-chip: #FFFFFF; --lv-line: var(--m-soft); --lvl: 0; --ox: 0%; --oy: 21%;
  background: var(--lv-bg); color: var(--lv-ink); font-family: var(--body) }
.ilive.dark { --lv-bg: #0A0E0C; --lv-ink: #E4EEE8; --lv-muted: #92A59A; --lv-dim: #5E6F66; --lv-chip: #1A231E; --lv-line: #243028 }
.ilive .lst { height: 50px; flex: none; display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; padding: 0 30px; font-size: 15px; font-weight: 600; position: relative; z-index: 2 }
.ilive .lst .isl { width: 110px; height: 32px; border-radius:var(--radius); background: #000 }
.ilive .lst .r { justify-self: end; font-size: 12px }
.ilive .lhome { height: 22px; flex: none; display: flex; justify-content: center; align-items: center; position: relative; z-index: 2 }
.ilive .lhome span { width: 130px; height: 5px; border-radius: 5px; background: var(--lv-ink) }
.ilive .ltop { flex: none; display: flex; align-items: center; gap: 8px; padding: 8px 18px 0; font-size: 13px; color: var(--lv-muted); position: relative; z-index: 2 }
.ilive .ltop .lg { width: 24px; height: 24px; border-radius: 7px; background: var(--m-accent); color: var(--m-ink); display: grid; place-items: center; font-family: var(--display); font-weight: 800; font-size: 13px }
.ilive .ltop .pill { margin-left: 6px; display: inline-flex; align-items: center; gap: 5px; min-height: 0; padding: 2px 8px; border: 0; border-radius: 999px; background: var(--lv-chip); font-size: 10.5px; font-weight: 600; letter-spacing: .06em; text-transform: uppercase; color: var(--m-ok) }
.ilive .ltop .pill i { width: 6px; height: 6px; border-radius: 50%; background: currentColor }
.ilive .ltop .x { margin-left: auto; width: 40px; height: 40px; border-radius: 50%; border: 0; background: var(--lv-chip); color: var(--lv-ink); display: grid; place-items: center }
.ilive .lmid { flex: 1; min-height: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; padding: 0 26px; position: relative; z-index: 2 }
.ilive .iorb { --c1: #A6EBCF; --c2: var(--m-ok); --c3: #0F3B2C; --glow: 59,155,122; width: 168px; height: 168px; position: relative; display: grid; place-items: center; border: 0; background: transparent; padding: 0 }
.ilive .iorb.speak { --c1: #FFD0C7; --c2: var(--m-accent); --c3: #5A231C; --glow: 240,122,106 }
.ilive .iorb.think { --c1: #7FBFA6; --c2: #2E6F59; --c3: #0D2A20 }
.ilive .iorb .core { width: 112px; height: 112px; border-radius: 50%; background: radial-gradient(circle at 36% 30%, var(--c1) 0%, var(--c2) 46%, var(--c3) 100%);
  transform: scale(calc(1 + var(--lvl) * .2)); box-shadow: 0 0 calc(22px + var(--lvl) * 50px) rgba(var(--glow), calc(.3 + var(--lvl) * .4)), inset 0 -8px 18px rgba(0,0,0,.3); transition: transform .08s linear, box-shadow .08s linear }
.ilive .iorb .ring { position: absolute; inset: 28px; border-radius: 50%; border: 2px solid rgba(var(--glow), calc(.14 + var(--lvl) * .35)); transform: scale(calc(1.05 + var(--lvl) * .4)); transition: transform .12s linear }
.ilive .iorb .ring.b { transform: scale(calc(1.18 + var(--lvl) * .65)); border-color: rgba(var(--glow), calc(.06 + var(--lvl) * .2)) }
.ilive .iorb.think .core, .ilive .iorb.think .ring { animation: dbreath 2.4s ease-in-out infinite }
.ilive .word { font-size: 12px; font-weight: 600; letter-spacing: .08em; text-transform: uppercase; color: var(--lv-muted); margin-top: 8px }
.ilive .word.on { color: var(--m-ok) } .ilive .word.sp { color: var(--m-accent) }
.ilive .cap { width: 100%; min-height: 96px; text-align: center; font-size: 24px; line-height: 31px; font-weight: 500; letter-spacing: -0.01em; text-wrap: balance }
.ilive .cap .prev { display: block; font-size: 15px; line-height: 21px; font-weight: 400; color: var(--lv-dim); margin-bottom: 6px }
.ilive .cap .tent { color: var(--lv-muted) } .ilive .cap .ph { color: var(--lv-dim); font-weight: 400 }
.ilive .cap .cmd { color: var(--m-ok); font-weight: 600 }
.ilive .step { display: inline-flex; align-items: center; gap: 8px; font-size: 14px; color: var(--lv-muted) }
.ilive .soon { width: 150px; height: 3px; border-radius: 2px; background: var(--lv-line); overflow: hidden }
.ilive .soon i { display: block; height: 100%; background: var(--m-ok); transform-origin: left; animation: dsoon 1.5s linear infinite }
.ilive .lbot { flex: none; display: grid; justify-items: center; gap: 12px; padding: 0 22px 18px; position: relative; z-index: 2 }
.ilive .hint { font-size: 13px; color: var(--lv-muted); text-align: center }
.ilive .says { display: flex; flex-wrap: wrap; justify-content: center; gap: 6px }
.ilive .says span { font-size: 12px; padding: 4px 10px; border-radius: 999px; background: var(--lv-chip); color: var(--lv-muted) }
.ilive .says b { font-weight: 500; color: var(--lv-ink) }
.ilive .ltoast { position: absolute; top: 112px; left: 50%; transform: translateX(-50%); z-index: 3; padding: 5px 12px; border-radius: 999px; background: var(--lv-ink); color: var(--lv-bg); font-size: 13px; font-weight: 500; white-space: nowrap; animation: pop .25s cubic-bezier(.3,1.4,.5,1) }
/* The edge glow: a ring in Beep's colors, flowing around the rounded screen edge. */
.ilive .edge { position: absolute; inset: 0; z-index: 1; pointer-events: none; border-radius: 46px; padding: calc(13px + var(--lvl) * 10px);
  background: conic-gradient(from var(--ga) at 50% 50%, var(--m-accent), #FFB59E 14%, #FFE08A 26%, #A6EBCF 42%, var(--m-ok) 58%, #6FC3D9 72%, #F59A8C 88%, var(--m-accent));
  -webkit-mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0); -webkit-mask-composite: xor; mask: linear-gradient(#000 0 0) content-box exclude, linear-gradient(#000 0 0);
  filter: blur(9px); opacity: calc(.8 + var(--lvl) * .2); animation: ga 7s linear infinite }
.ilive .edge.sharp { padding: calc(3px + var(--lvl) * 2px); filter: blur(1px); opacity: .9 }
.ilive.dark .edge { opacity: calc(.8 + var(--lvl) * .2) }
@keyframes ga { to { --ga: 360deg } }
/* Launch: the wash floods in from the side button, then the ring contracts to the edge. */
.ilive .wash { position: absolute; inset: 0; z-index: 1; pointer-events: none; opacity: 0;
  background: radial-gradient(120% 75% at var(--ox) var(--oy), rgba(255,214,200,.95) 0%, rgba(240,122,106,.7) 22%, rgba(255,224,138,.5) 40%, rgba(166,235,207,.55) 62%, rgba(59,155,122,.35) 82%, rgba(111,195,217,.2) 100%);
  -webkit-mask: radial-gradient(70% 60% at 50% 50%, transparent var(--hole), #000 calc(var(--hole) + 45%)); mask: radial-gradient(70% 60% at 50% 50%, transparent var(--hole), #000 calc(var(--hole) + 45%)) }
.ilive.dark .wash { mix-blend-mode: screen }
.ilive .spark { position: absolute; left: calc(var(--ox) - 36px); top: calc(var(--oy) - 60px); width: 72px; height: 120px; z-index: 3; pointer-events: none; border-radius: 50%; opacity: 0;
  background: radial-gradient(closest-side, #fff, #FFD0C7 45%, rgba(240,122,106,0) 100%); filter: blur(3px) }
.ilive.launch .spark { animation: lspark .32s ease-out both }
.ilive.launch .wash { animation: lwash .7s cubic-bezier(.2,.8,.2,1) both }
.ilive.launch .edge { animation: ga 7s linear infinite, lsettle .7s ease-out both }
.ilive.launch .lmid .iorb { animation: lorb .62s .14s cubic-bezier(.3,1.35,.5,1) both }
.ilive.launch .ltop, .ilive.launch .cap, .ilive.launch .word, .ilive.launch .lbot { animation: lfade .3s .38s ease-out both }
@keyframes lspark { 0% { opacity: 0; transform: scale(.4) } 25% { opacity: 1; transform: scale(1) } 100% { opacity: 0; transform: scale(1.6) } }
@keyframes lwash { 0% { opacity: 0; --hole: 0%; clip-path: circle(0% at var(--ox) var(--oy)) } 10% { opacity: 1 } 36% { opacity: 1; --hole: 0%; clip-path: circle(150% at var(--ox) var(--oy)) } 100% { opacity: 0; --hole: 75%; clip-path: circle(150% at var(--ox) var(--oy)) } }
@keyframes lsettle { 0%, 25% { opacity: 0 } 100% { opacity: calc(.8 + var(--lvl) * .2) } }
@keyframes lorb { from { transform: translate(var(--odx), var(--ody)) scale(.15); opacity: 0 } 40% { opacity: 1 } to { transform: none; opacity: 1 } }
@keyframes lfade { from { opacity: 0; transform: translateY(6px) } }
/* Reduce Motion: the scene fades in; the glow and orb stand still. */
.ilive.rm { animation: lfade .2s ease-out both }
.ilive.rm .edge { animation: none; --ga: 200deg; padding: 10px; opacity: .8 }
.ilive.rm .edge.sharp { padding: 2.5px }
.ilive.rm .wash, .ilive.rm .spark { display: none }
.ilive.rm .iorb .core, .ilive.rm .iorb .ring { transition: none; transform: none !important; animation: none !important }
.ilive.rm * { animation-duration: 0s !important }
.ilive .press { position: absolute; top: calc(var(--oy) - 26px); z-index: 4; padding: 4px 9px; border-radius: 999px; background: rgba(16,32,26,.78); color: #fff; font-size: 11px; font-weight: 500; animation: lpress 1.1s ease-out both }
@keyframes lpress { 0%, 60% { opacity: 1 } 100% { opacity: 0 } }
/* The phone's side button, pressed for the launch. */
.iphone .sidebtn { position: absolute; right: -3px; width: 4px; height: 76px; border-radius: 0 3px 3px 0; background: #2A332F }
.iphone .actbtn { position: absolute; left: -3px; width: 4px; height: 30px; border-radius: 3px 0 0 3px; background: #2A332F }
.iphone.withbtn.pressed.act .actbtn, .iphone.withbtn.pressed.side .sidebtn { animation: lbtn .3s ease-out; background: var(--m-accent) }
.iphone.withbtn { position: relative }
@keyframes lbtn { 50% { transform: scaleX(.5) } }
/* Control Center control. */
.icc { position: absolute; inset: 0; background: linear-gradient(170deg, rgba(40,52,47,.92), rgba(16,22,19,.96)); display: grid; grid-template-columns: repeat(4, 72px); grid-auto-rows: 72px; gap: 14px; justify-content: center; align-content: start; padding-top: 40px }
.icc .t { border-radius:var(--radius); background: rgba(255,255,255,.14); display: grid; place-items: center; color: #fff }
.icc .t.w { grid-column: span 2; border-radius:var(--radius); display: flex; align-items: center; gap: 10px; padding: 0 14px; justify-content: flex-start }
.icc .t.on { background: #fff; color: var(--m-ink) }
.icc .t .o { width: 40px; height: 40px; border-radius: 50%; background: radial-gradient(circle at 36% 30%, #A6EBCF, var(--m-ok) 46%, #0F3B2C); flex: none }
.icc .t small { display: grid; font-size: 12px; line-height: 15px } .icc .t small b { font-size: 13px; font-weight: 600 }`;
  document.head.append(st);
})();

IC.wave = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M4 10v4M8 7v10M12 4v16M16 7v10M20 10v4"/></svg>';
S.i.lv = { dark: false, rm: false, from: "action", listen: "words", think: "think", speak: "reply", left: "draft", start: "chat" };
// Where the launch glow starts: the Action Button high on the left edge, the side button on the
// right (Japan), or the middle for a Control Center or Lock Screen control. --odx/--ody move the orb
// from there to the middle of the screen.
const LV_FROM = {
  action: { ox: "0%", oy: "21%", odx: "-150px", ody: "-170px", press: "Action Button", side: "left" },
  side: { ox: "100%", oy: "27%", odx: "150px", ody: "-120px", press: "Side button held", side: "right" },
  control: { ox: "50%", oy: "4%", odx: "0px", ody: "-300px", press: "", side: "" },
};
const lvWord = { listen: ["Listening", "on"], hold: ["Listening", "on"], idle: ["Listening", "on"], think: ["Thinking", ""], speak: ["Speaking", "sp"] };
// The phones show no list of commands: the hint names "send" and "stop", as on the Mac.
const LV_SAYS = [];
/// The live scene: status bar, chat name, orb, status word, caption and hint, inside the edge glow.
function iLiveView(o) {
  const lv = S.i.lv, w = lvWord[o.mode];
  const orb = { listen: "", hold: "", idle: "idle", think: "think", speak: "speak" }[o.mode];
  const cls = ["ilive", lv.dark ? "dark" : "", lv.rm ? "rm" : "", o.launch && !lv.rm ? "launch" : ""].join(" ");
  const f = LV_FROM[lv.from];
  const press = o.launch && !lv.rm && f.press ? `<div class="press" style="${f.side}:10px">${f.press}</div>` : "";
  return `<div class="${cls}" role="dialog" aria-label="Live voice" style="--ox:${f.ox};--oy:${f.oy};--odx:${f.odx};--ody:${f.ody}">
    <div class="edge"></div><div class="edge sharp"></div><div class="wash"></div><div class="spark"></div>
    ${press}
    <div class="lst"><span>9:41</span><span class="isl"></span><span class="r">5G 82%</span></div>
    <div class="ltop"><span class="lg">${BI(14)}</span>${o.chat || "New chat"}<span class="pill"><i></i>Live</span>
      <button class="x" data-igo="iLiveLeft" data-iset="lvLeft:draft" aria-label="Leave live mode">${I.x}</button></div>
    ${o.toast ? `<div class="ltoast" role="status">${o.toast}</div>` : ""}
    <div class="lmid">
      <button class="iorb ${orb}" data-iact="lvTap" aria-label="${o.mode === "speak" ? "Stop speaking" : "Tap to stop, hold to talk"}"><span class="ring b"></span><span class="ring"></span><span class="core"></span></button>
      <div class="word ${w[1]}">${o.word || w[0]}</div>
      <div class="cap" aria-live="polite">${o.cap}</div>
      ${o.soon ? '<div class="soon" role="progressbar" aria-label="Sending after the pause"><i></i></div>' : ""}
    </div>
    <div class="lbot">
      <div class="hint">${o.hint ?? "Pause to send · Tap the orb to stop · Hold it to talk"}</div>
      ${o.says && LV_SAYS.length ? `<div class="says">${LV_SAYS.map(([k, v]) => `<span><b>“${k}”</b>${v ? " " + v : ""}</span>`).join("")}</div>` : ""}
    </div>
    <div class="lhome"><span></span></div>
  </div>`;
}
// Under the scene, the chat it was opened from, so the wash brightens real content.
const iLiveUnder = () => iChatBar() + `<div class="body" style="padding:8px 18px 16px;gap:18px">
    <div class="user" style="border-width:2px;box-shadow:3px 3px 0 var(--m-ledge);border-radius:var(--radius) 18px 6px 18px;margin-left:48px">Did the plumber get back to me?</div>
    <div class="reply">Yes, Alex wrote at 08:12: the plumber can come Thursday between 9 and 12, and she asks if that works.</div>
  </div>` + iComposer();
Object.assign(P, {
  iLiveLaunch: () => iLiveUnder() + iLiveView({ mode: "idle", launch: true, cap: '<span class="ph">Go ahead, I’m listening</span>', says: true }),
  iLive: () => {
    const v = S.i.lv.listen;
    const o = {
      words: { mode: "listen", cap: 'What’s on my calendar <span class="tent">tomorrow morning</span>' },
      soon: { mode: "listen", cap: "What’s on my calendar tomorrow morning", soon: true, hint: "Sending… keep talking to add more" },
      pause: { mode: "listen", cap: "Remind me to call the plumber at nine", hint: "Say “send” or “stop” when you’re done · Tap the orb to stop" },
      empty: { mode: "idle", cap: '<span class="ph">Go ahead, I’m listening</span>', says: true },
      hold: { mode: "hold", word: "Holding", cap: "Text Sam that I’m running ten minutes late", hint: "Let go to send · Slide away to cancel" },
      // A press on the AirPods talks like holding the orb; the next press sends, a double press leaves.
      airpods: { mode: "hold", word: "Talking", cap: "Text Sam that I’m running ten minutes late", hint: "Press your AirPods to send · Press twice to leave" },
      send: { mode: "listen", cap: 'Book a table for four at eight <span class="cmd">send</span>', toast: "Sending" },
      off: { mode: "idle", word: "Voice unavailable", cap: '<span class="ph">Beep can’t hear you. Allow the microphone in Settings, Beep.</span>', hint: "" },
    }[v];
    return iLiveUnder() + iLiveView(o);
  },
  iLiveThink: () => {
    const v = S.i.lv.think;
    if (v === "tool") return iLiveUnder() + iLiveView({ mode: "think", cap: `<span class="prev">What’s on my calendar tomorrow morning?</span><span class="step"><span class="spin"></span>Checking Google Calendar</span>`, hint: "Tap the orb to stop the answer" });
    if (v === "says") return iLiveUnder() + iLiveView({ mode: "speak", cap: `<span class="prev">Is build-box running out of disk?</span>Let me check build-box.<span style="display:flex;justify-content:center;margin-top:8px"><span class="step"><span class="spin"></span>Running code</span></span>`, hint: "Tap the orb to stop" });
    return iLiveUnder() + iLiveView({ mode: "think", cap: `<span class="prev">What’s on my calendar tomorrow morning?</span><span class="ph">…</span>`, hint: "Tap the orb to stop the answer" });
  },
  iLiveSpeak: () => {
    const v = S.i.lv.speak;
    if (v === "stopped") return iLiveUnder() + iLiveView({ mode: "listen", toast: "Stopped", cap: '<span class="prev">You have the dentist at 9:30,</span><span class="ph">Go ahead</span>' });
    return iLiveUnder() + iLiveView({ mode: "speak", cap: '<span class="prev">You have the dentist at 9:30,</span>then a call with Alex at 11 about the plumber.', hint: "Tap the orb or say “stop” to interrupt" });
  },
  iLiveLeft: () => {
    const v = S.i.lv.left;
    const draft = v === "draft" ? "And move the call with Alex to" : "";
    return iChatBar().replace(">New conversation<", ">Tomorrow morning<") + `<div class="body" style="padding:8px 18px 16px;gap:18px">
      <div class="user" style="border-width:2px;box-shadow:3px 3px 0 var(--m-ledge);border-radius:var(--radius) 18px 6px 18px;margin-left:48px">What’s on my calendar tomorrow morning?</div>
      ${iChip("Checked Google Calendar", "G", "ok", 0)}
      <div class="reply">You have the dentist at 9:30, then a call with Alex at 11 about the plumber.</div>
    </div>` + iComposer({ text: draft });
  },
  iLiveStart: () => {
    if (S.i.lv.start === "lock") return `<div class="icc" style="background:linear-gradient(170deg,#2F4F43,#0E1A15);grid-template-columns:repeat(4,72px);align-content:space-between;padding:60px 0 40px">
      <div style="grid-column:1/-1;text-align:center;color:#fff;display:grid;gap:2px"><span style="font-size:16px;font-weight:500">Thursday 30 October</span><span style="font-family:var(--display);font-weight:800;font-size:88px;line-height:1">9:41</span></div>
      <div class="t" style="border-radius:50%;width:52px;height:52px;align-self:end;justify-self:center;background:rgba(255,255,255,.18)">${small(I.key, 20)}</div><div></div><div></div>
      <div class="t" style="border-radius:50%;width:52px;height:52px;align-self:end;justify-self:center;background:#fff"><span class="o" style="width:30px;height:30px;border-radius:50%;background:radial-gradient(circle at 36% 30%,#A6EBCF,var(--m-ok) 46%,#0F3B2C)"></span></div>
    </div>`;
    if (S.i.lv.start === "cc") return `<div class="icc">
      <div class="t">${small(I.bell, 22)}</div><div class="t">${small(I.pin, 22)}</div><div class="t w on"><span class="o"></span><small><b>Talk to Beep</b>Live voice</small></div>
      <div class="t w"><small><b>Focus</b>Off</small></div><div class="t">${small(I.plus, 22)}</div><div class="t">${small(I.key, 22)}</div>
    </div>`;
    return iLiveUnder().replace(`aria-label="Talk live"`, `aria-label="Talk live" style="color:var(--m-ink);background:var(--m-t1);border-radius:var(--radius)"`);
  },
});
IOS_SCREENS.splice(IOS_SCREENS.findIndex(g => g.group === "Group chats"), 0, { group: "Live voice", items: [
  ["iLiveLaunch", "Launch", "LiveView, EdgeGlow, TalkToBeepIntent"],
  ["iLive", "Listening", "LiveView, listening"],
  ["iLiveThink", "Thinking", "LiveView, thinking"],
  ["iLiveSpeak", "Speaking", "LiveView, speaking"],
  ["iLiveLeft", "Left live mode", "ChatView, draft"],
  ["iLiveStart", "Other ways in", "ChatView live button, TalkToBeepControl"],
  ["iLiveSetup", "Action Button hint", "SettingsView, live voice"],
] });
const lvSet = (key, list) => list.map(([label, val]) => [label, () => { S.i.lv[key] = val; render(); }, () => S.i.lv[key] === val]);
const lvLook = [["Dark", () => { S.i.lv.dark = !S.i.lv.dark; render(); }, () => S.i.lv.dark], ["Reduce Motion", () => { S.i.lv.rm = !S.i.lv.rm; render(); }, () => S.i.lv.rm]];
Object.assign(VARIANTS, {
  iLiveLaunch: [...lvSet("from", [["Action Button", "action"], ["Side button (Japan)", "side"], ["Control", "control"]]), ...lvLook],
  iLive: [...lvSet("listen", [["Words coming in", "words"], ["About to send", "soon"], ["Waiting for “send”", "pause"], ["Nothing said yet", "empty"], ["Holding the orb", "hold"], ["Talking from AirPods", "airpods"], ["Heard “send”", "send"], ["No microphone", "off"]]), ...lvLook],
  iLiveThink: [...lvSet("think", [["Sent", "think"], ["Using a tool", "tool"], ["Says what it is doing", "says"]]), ...lvLook],
  iLiveSpeak: [...lvSet("speak", [["Reply", "reply"], ["Tapped the orb", "stopped"]]), ...lvLook],
  iLiveLeft: lvSet("left", [["Left with words unsent", "draft"], ["Nothing unsent", "empty"]]),
  iLiveStart: lvSet("start", [["Chat bar button", "chat"], ["Control Center", "cc"], ["Lock Screen", "lock"]]),
});
document.getElementById("ios").addEventListener("click", e => {
  const el = e.target.closest('[data-iact="lvTap"]');
  if (!el) return;
  if (S.screen === "iLiveSpeak") { S.i.lv.speak = "stopped"; render(); }
  else if (S.screen === "iLiveThink") { go("iLive"); }
  else { S.i.lv.left = "draft"; go("iLiveLeft"); }
});
// The orb and the glow follow a made up level: speech while listening, playback while speaking.
let ilvRaf = 0;
function ilvLoop() {}
