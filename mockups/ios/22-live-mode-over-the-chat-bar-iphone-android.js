// ---- Live mode over the chat (bar, iPhone, Android) ----
// Live mode no longer covers the chat. The chat stays exactly as it is: turns stream in,
// it scrolls, and links, cards, files and approval cards answer with a click or a tap.
// Voice mode does not change the conversation, so there is no live transcript of its own.
// The live controls become a dock where the composer sits (the field at the top of the
// bar, the composer at the bottom of a phone): a small orb with the same states and level
// (a tap does what it did before), the state word, the caption, the pause bar, the hint,
// Cancel during a reply and a way out. A toast floats over the dock. Typing a character in
// the bar still leaves live mode with that key in the field; scrolling and clicking in the
// chat do not leave. The floating orb window for a hidden bar is unchanged.
(() => {
  const st = document.createElement("style");
  st.textContent = `/* Phones: the dock stands where the composer was. */
.ilive.docked { background: transparent; pointer-events: none; z-index: 4 }
#ios .icomp:has(~ .ldock), #app .composer:has(~ .ldock) { display: none }
.ldock { flex: none; position: relative; z-index: 5; margin: 0 10px 10px; padding: 10px 10px 10px 8px; border-radius:var(--radius); background: var(--m-surface);
  border: 1.5px solid var(--m-ledge); box-shadow: 0 3px 0 var(--m-ledge); display: grid; gap: 4px; --lvl: 0; color: var(--m-ink); font-family: var(--body) }
.ldock .lrow { display: flex; align-items: center; gap: 10px; min-width: 0 }
.ldock .iorb { --c1: #A6EBCF; --c2: var(--m-ok); --c3: #0F3B2C; --glow: 59,155,122; width: 48px; height: 48px; position: relative; display: grid; place-items: center; border: 0; background: transparent; padding: 0; flex: none }
.ldock .iorb.speak { --c1: #FFD0C7; --c2: var(--m-accent); --c3: #5A231C; --glow: 240,122,106 }
.ldock .iorb.think { --c1: #7FBFA6; --c2: #2E6F59; --c3: #0D2A20 }
.ldock .iorb.idle { --c1: #9FC2B2; --c2: #4F6E61; --c3: #1E2E27; --glow: 146,165,154 }
.ldock .iorb .core { width: 32px; height: 32px; border-radius: 50%; background: radial-gradient(circle at 36% 30%, var(--c1) 0%, var(--c2) 46%, var(--c3) 100%);
  transform: scale(calc(1 + var(--lvl) * .22)); box-shadow: 0 0 calc(8px + var(--lvl) * 22px) rgba(var(--glow), calc(.3 + var(--lvl) * .4)), inset 0 -4px 8px rgba(0,0,0,.3); transition: transform .08s linear, box-shadow .08s linear }
.ldock .iorb .ring { position: absolute; inset: 8px; border-radius: 50%; border: 1.5px solid rgba(var(--glow), calc(.16 + var(--lvl) * .35)); transform: scale(calc(1.05 + var(--lvl) * .35)); transition: transform .12s linear }
.ldock .iorb .ring.b { transform: scale(calc(1.18 + var(--lvl) * .55)); border-color: rgba(var(--glow), calc(.07 + var(--lvl) * .2)) }
.ldock .iorb.think .core, .ldock .iorb.think .ring { animation: dbreath 2.4s ease-in-out infinite }
.ldock .ltx { flex: 1; min-width: 0; display: grid; gap: 1px }
.ldock .word { font-size: 11px; line-height: 15px; font-weight: 600; letter-spacing: .08em; text-transform: uppercase; color: var(--m-muted) }
.ldock .word.on { color: var(--m-ok) } .ldock .word.sp { color: #D9604F }
.ldock .cap { font-size: 15px; line-height: 20px; font-weight: 500; min-height: 20px }
.ldock .cap .prev { display: block; font-size: 12px; line-height: 16px; font-weight: 400; color: var(--m-dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.ldock .cap .tent { color: var(--m-muted) } .ldock .cap .ph { color: var(--m-dim); font-weight: 400 } .ldock .cap .cmd { color: var(--m-ok); font-weight: 600 }
.ldock .cap > span[style] { justify-content: flex-start !important; margin-top: 2px !important }
.ldock .step { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; font-weight: 400; color: var(--m-muted) }
.ldock .x { width: 36px; height: 36px; border-radius: 50%; border: 1.5px solid var(--m-soft); background: var(--m-page); color: var(--m-ink); display: grid; place-items: center; padding: 0; flex: none; align-self: flex-start }
.ldock .soon { height: 3px; border-radius: 2px; background: var(--m-soft); overflow: hidden; margin: 2px 0 0 58px }
.ldock .soon i { display: block; height: 100%; background: var(--m-ok); transform-origin: left; animation: dsoon 1.5s linear infinite }
.ldock .lbot { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 10px; padding-left: 58px }
.ldock .lbot:empty { display: none }
.ldock .hint { flex: 1; min-width: 0; font-size: 12px; line-height: 16px; color: var(--m-muted); text-align: left; padding: 0 }
.ldock .says { display: flex; flex-wrap: wrap; gap: 4px; width: 100% }
.ldock .says span { font-size: 11px; padding: 2px 8px; border-radius: 999px; background: var(--m-page); color: var(--m-muted) } .ldock .says b { font-weight: 500; color: var(--m-ink) }
.ldock .lbtns { display: flex; gap: 6px; flex: none }
.ldock .lbtn { min-height: 32px; padding: 0 14px; border-radius: 999px; border: 1.5px solid var(--m-ledge); background: var(--m-surface); color: var(--m-ink); font-size: 13px; font-weight: 600 }
.ldock .lbtn.pri { background: var(--m-ok); color: #fff }
.ldock .again { width: 100%; font-size: 11.5px; color: var(--m-dim) }
.ldock .ltoast { position: absolute; left: 50%; top: -40px; transform: translateX(-50%); padding: 5px 12px; border-radius: 999px; background: var(--m-ink); color: var(--m-page); font-size: 13px; font-weight: 500; white-space: nowrap; animation: pop .25s cubic-bezier(.3,1.4,.5,1) }
.ldock .lhome { display: none }
.ldock.launch { animation: ldockin .4s .12s cubic-bezier(.3,1.3,.5,1) both }
.ldock.launch .iorb { animation: lorbin .5s .2s cubic-bezier(.3,1.5,.5,1) both }
@keyframes ldockin { from { transform: translateY(28px); opacity: 0 } }
@keyframes lorbin { from { transform: scale(.2); opacity: 0 } }
.ldock.rm, .ldock.rm * { animation: none !important; transition: none !important }
.ldock.rm .iorb .core, .ldock.rm .iorb .ring { transform: none !important }`;
  document.head.append(st);
})();

// One dock for both phones. `on` gives the attributes for the orb and the close button.
function lvDock(o, on) {
  const lv = S.i.lv, w = lvWord[o.mode];
  const orb = { listen: "", hold: "", idle: "idle", think: "think", speak: "speak" }[o.mode];
  const mid = o.mode === "think" || o.mode === "speak";
  let hint = o.hint === undefined ? LV_HINT : o.hint;
  hint = hint.replace("Tap the orb to stop · Hold it to talk", "Tap the orb to leave · Hold it to talk")
    .replace("Say “send” or “stop” when you’re done · Tap the orb to stop", "Say “send” or “stop” when you’re done · Tap the orb to leave");
  const cls = ["ldock", o.rm ? "rm" : "", o.launch ? "launch" : ""].join(" ");
  const cap = o.card ? '<span class="ph">Allow or deny in the chat</span>' : o.cap;
  return `<div class="${cls}" role="region" aria-label="Live voice">
    ${o.toast ? `<div class="ltoast" role="status">${o.toast}</div>` : ""}
    <div class="lrow"><button class="iorb ${orb}" ${on.orb(o.mode)} aria-label="${o.mode === "speak" ? "Stop speaking" : mid ? "Skip the spoken answer" : "Tap to leave, hold to talk"}"><span class="ring b"></span><span class="ring"></span><span class="core"></span></button>
      <div class="ltx"><div class="word ${w[1]}">${o.word || w[0]}</div><div class="cap" aria-live="polite">${cap}</div></div>
      <button class="x" ${on.leave(mid)} aria-label="${mid ? "Cancel the reply and leave" : "Leave live mode"}">${I.x}</button></div>
    ${o.soon ? '<div class="soon" role="progressbar" aria-label="Sending after the pause"><i></i></div>' : ""}
    <div class="lbot">${hint ? `<div class="hint">${hint}</div>` : ""}${o.says && LV_SAYS.length ? `<div class="says">${LV_SAYS.map(([k, v]) => `<span><b>“${k}”</b>${v ? " " + v : ""}</span>`).join("")}</div>` : ""}${o.extra || ""}</div><div class="lhome"></div>
  </div>`;
}
