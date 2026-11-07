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
.ilive .lhome { height: 22px; flex: none; display: flex; justify-content: center; align-items: center; position: relative; z-index: 2 }
.ilive .lhome span { width: 130px; height: 5px; border-radius: 5px; background: var(--lv-ink) }
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
.ilive.dark .edge { opacity: calc(.8 + var(--lvl) * .2) }
@keyframes ga { to { --ga: 360deg } }
/* Launch: the wash floods in from the side button, then the ring contracts to the edge. */
.ilive .wash { position: absolute; inset: 0; z-index: 1; pointer-events: none; opacity: 0;
  background: radial-gradient(120% 75% at var(--ox) var(--oy), rgba(255,214,200,.95) 0%, rgba(240,122,106,.7) 22%, rgba(255,224,138,.5) 40%, rgba(166,235,207,.55) 62%, rgba(59,155,122,.35) 82%, rgba(111,195,217,.2) 100%);
  -webkit-mask: radial-gradient(70% 60% at 50% 50%, transparent var(--hole), #000 calc(var(--hole) + 45%)); mask: radial-gradient(70% 60% at 50% 50%, transparent var(--hole), #000 calc(var(--hole) + 45%)) }
.ilive.dark .wash { mix-blend-mode: screen }
.ilive.launch .wash { animation: lwash .7s cubic-bezier(.2,.8,.2,1) both }
.ilive.launch .edge { animation: ga 7s linear infinite, lsettle .7s ease-out both }
.ilive.launch .cap, .ilive.launch .word, .ilive.launch .lbot { animation: lfade .3s .38s ease-out both }
@keyframes lwash { 0% { opacity: 0; --hole: 0%; clip-path: circle(0% at var(--ox) var(--oy)) } 10% { opacity: 1 } 36% { opacity: 1; --hole: 0%; clip-path: circle(150% at var(--ox) var(--oy)) } 100% { opacity: 0; --hole: 75%; clip-path: circle(150% at var(--ox) var(--oy)) } }
@keyframes lsettle { 0%, 25% { opacity: 0 } 100% { opacity: calc(.8 + var(--lvl) * .2) } }
@keyframes lfade { from { opacity: 0; transform: translateY(6px) } }
/* Reduce Motion: the scene fades in; the glow and orb stand still. */
.ilive.rm { animation: lfade .2s ease-out both }
.ilive.rm .edge { animation: none; --ga: 200deg; padding: 10px; opacity: .8 }
.ilive.rm .wash { display: none }
.ilive.rm .iorb .core, .ilive.rm .iorb .ring { transition: none; transform: none !important; animation: none !important }
.ilive.rm * { animation-duration: 0s !important }
.ilive .press { position: absolute; top: calc(var(--oy) - 26px); z-index: 4; padding: 4px 9px; border-radius: 999px; background: rgba(16,32,26,.78); color: #fff; font-size: 11px; font-weight: 500; animation: lpress 1.1s ease-out both }
@keyframes lpress { 0%, 60% { opacity: 1 } 100% { opacity: 0 } }
/* The phone's side button, pressed for the launch. */
@keyframes lbtn { 50% { transform: scaleX(.5) } }
/* Control Center control. */`;
  document.head.append(st);
})();

IC.wave = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M4 10v4M8 7v10M12 4v16M16 7v10M20 10v4"/></svg>';
S.i.lv = { dark: false, rm: false, from: "action", listen: "words", think: "think", speak: "reply", left: "draft", start: "chat" };
const lvWord = { listen: ["Listening", "on"], hold: ["Listening", "on"], idle: ["Listening", "on"], think: ["Thinking", ""], speak: ["Speaking", "sp"] };
// The phones show no list of commands: the hint names "send" and "stop", as on the Mac.
const LV_SAYS = [];
