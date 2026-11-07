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
  st.textContent = `.mac .dlv.ddock { display: flex; flex-direction: column; align-items: stretch; justify-items: stretch; gap: 4px; padding: 10px 12px; border-bottom: 1px solid var(--d-border); background: rgba(15,16,17,.28) }
.mac .ddock .drow1 { display: flex; align-items: center; gap: 12px; min-width: 0 }
.mac .ddock .dorb { width: 46px; height: 46px; margin: 0; flex: none; border: 0; background: transparent; padding: 0; cursor: pointer }
.mac .ddock .dorb .core { width: 28px; height: 28px }
.mac .ddock .dorb .ring { inset: 9px }
.mac .ddock .dtx { flex: 1; min-width: 0; display: grid; gap: 1px }
.mac .ddock .word { margin: 0; font-size: 10.5px; line-height: 14px }
.mac .ddock .cap { max-width: none; min-height: 22px; text-align: left; font-size: 15px; line-height: 22px; text-wrap: pretty }
.mac .ddock .cap .prev { font-size: 12.5px; line-height: 18px; margin: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .ddock .cap > span[style] { justify-content: flex-start !important; margin-top: 2px !important }
.mac .ddock .step { font-size: 13px }
.mac .ddock .dleave { flex: none; width: 28px; height: 28px; border-radius: 8px; border: 1px solid var(--d-border); background: var(--d-elev); color: var(--d-muted); display: grid; place-items: center; padding: 0; align-self: flex-start }
.mac .ddock .dleave:hover { color: var(--d-fg); border-color: #34363C }
.mac .ddock .dfoot { display: flex; align-items: center; gap: 10px; padding-left: 58px; min-height: 24px }
.mac .ddock .hint { flex: 1; min-width: 0; text-align: left; font-size: 11.5px }
.mac .ddock .dsoon { width: 90px; flex: none }
.mac .ddock .lcancel { margin: 0; min-height: 24px; flex: none }
.mac .ddock .ltoast { top: 8px; left: auto; right: 50px; transform: none; animation: ddockfade .2s ease-out }
@keyframes ddockfade { from { opacity: 0 } }
.mac .ddock + .dbody { padding-top: 10px }

/* Phones: the dock stands where the composer was. */
#app .composer:has(~ .ldock) { display: none }
.ldock { flex: none; position: relative; z-index: 5; margin: 0 10px 10px; padding: 10px 10px 10px 8px; border-radius: 24px; background: var(--m-surface);
  border: 1.5px solid var(--m-ledge); box-shadow: 0 3px 0 var(--m-ledge); display: grid; gap: 4px; --lvl: 0; color: var(--m-ink); font-family: var(--body) }
.ldock .word { font-size: 12px; line-height: 15px; font-weight: 600; color: var(--m-muted) }
.ldock .word.on { color: #5E6AD2 } .ldock .word.sp { color: #D9604F }
.ldock .cap { font-size: 15px; line-height: 20px; font-weight: 500; min-height: 20px }
.ldock .cap .prev { display: block; font-size: 12px; line-height: 16px; font-weight: 400; color: var(--m-dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.ldock .cap .tent { color: var(--m-muted) } .ldock .cap .ph { color: var(--m-dim); font-weight: 400 } .ldock .cap .cmd { color: #5E6AD2; font-weight: 600 }
.ldock .cap > span[style] { justify-content: flex-start !important; margin-top: 2px !important }
.ldock .step { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; font-weight: 400; color: var(--m-muted) }
.ldock .x { width: 36px; height: 36px; border-radius: 50%; border: 1.5px solid var(--m-soft); background: var(--m-page); color: var(--m-ink); display: grid; place-items: center; padding: 0; flex: none; align-self: flex-start }
.ldock .soon { height: 3px; border-radius: 2px; background: var(--m-soft); overflow: hidden; margin: 2px 0 0 58px }
.ldock .soon i { display: block; height: 100%; background: #5E6AD2; transform-origin: left; animation: none}
.ldock .hint { flex: 1; min-width: 0; font-size: 12px; line-height: 16px; color: var(--m-muted); text-align: left; padding: 0 }
.ldock .says { display: flex; flex-wrap: wrap; gap: 4px; width: 100% }
.ldock .says span { font-size: 11px; padding: 2px 8px; border-radius: 999px; background: var(--m-page); color: var(--m-muted) } .ldock .says b { font-weight: 500; color: var(--m-ink) }
.ldock .again { width: 100%; font-size: 11.5px; color: var(--m-dim) }
.ldock .ltoast { position: absolute; left: 50%; top: -40px; transform: translateX(-50%); padding: 5px 12px; border-radius: 999px; background: var(--m-ink); color: var(--m-page); font-size: 13px; font-weight: 500; white-space: nowrap; animation: pop .25s cubic-bezier(.3,1.4,.5,1) }
.ldock.launch { animation: ldockin .4s .12s cubic-bezier(.3,1.3,.5,1) both }
@keyframes ldockin { from { transform: translateY(28px); opacity: 0 } }
.ldock.rm, .ldock.rm * { animation: none !important; transition: none !important }`;
  document.head.append(st);
})();
