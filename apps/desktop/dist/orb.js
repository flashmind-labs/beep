// The orb window: live mode while the bar is hidden. It follows the same
// `live` events as the bar. A click brings the bar back; × cancels the
// reply, or leaves.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const orb = document.getElementById("orb");
const x = document.getElementById("x");

function draw(v) {
  // The same states as the bar's orb: listening, thinking, speaking.
  const mode = v.speaking || (v.phase === "speaking" && v.line) ? "speak"
    : v.phase === "thinking" ? "think"
    : v.words ? "" : "idle";
  orb.className = `dorb ${mode}`;
  const replying = !v.title && (v.phase === "thinking" || v.phase === "speaking");
  x.setAttribute("aria-label", v.title ? "Stop the voice note" : replying ? "Cancel the reply" : "Leave live mode");
}

listen("live", e => {
  const v = e.payload;
  if (v.kind === "view") draw(v);
  else if (v.kind === "level") orb.style.setProperty("--lvl", Math.min(1, v.level).toFixed(3));
});
document.getElementById("mini").addEventListener("click", () => invoke("show_bar").catch(() => {}));
x.addEventListener("click", e => {
  e.stopPropagation();
  invoke("cancel_live").catch(() => {});
});
