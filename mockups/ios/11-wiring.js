// ---- Wiring ----
let toastTimer;
function toast(msg) {
  const t = document.getElementById("itoast");
  t.textContent = msg; t.hidden = false;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => t.hidden = true, 1600);
}
function go(screen) {
  S.onb = false;
  if (screen === "iCamera" && S.screen !== screen) S.cam = "sheet";
  if (screen === "iGroupInfo" && S.screen !== screen) S.ginfo = "info";
  S.screen = screen;
  render();
}
function renderIndex() {
  const groups = IOS_SCREENS;
  const ids = groups.flatMap(g => g.items.map(([id]) => id)), at = ids.indexOf(S.screen);
  const step = (to, label, glyph) => `<button ${to ? `data-screen="${to}"` : "disabled"} aria-label="${label}">${glyph}</button>`;
  document.getElementById("index").innerHTML = `<div class="pick">${step(ids[at - 1], "Previous screen", "&lsaquo;")}
    <select aria-label="Screen">${groups.map(g => `<option disabled>${g.group.toUpperCase()}</option>${g.items.map(([id, name]) =>
      `<option value="${id}"${S.screen === id ? " selected" : ""}>&nbsp;&nbsp;&nbsp;${name}</option>`).join("")}`).join("")}</select>
    ${step(ids[at + 1], "Next screen", "&rsaquo;")}</div>` + groups.map(g =>
    `<div class="group"><h2>${g.group}</h2>${g.items.map(([id, name]) =>
      `<button data-screen="${id}" aria-current="${S.screen === id}">${name}</button>`).join("")}</div>`).join("");
}
function renderVariants() {
  const v = VARIANTS[S.screen] || [];
  document.getElementById("variants").innerHTML = v.map(([n, , on], i) =>
    `<button data-variant="${i}" aria-pressed="${on()}">${n}</button>`).join("");
}
function render() {
  document.getElementById("ios").innerHTML = P[S.screen]();
  renderIndex(); renderVariants();
  try { localStorage.setItem("beep-screens", S.screen); } catch (e) {}
}

document.getElementById("index").addEventListener("change", e => {
  if (!e.target.matches("select")) return;
  go(e.target.value);
});
document.getElementById("index").addEventListener("click", e => {
  const b = e.target.closest("[data-screen]");
  if (!b) return;
  go(b.dataset.screen);
});
document.getElementById("variants").addEventListener("click", e => {
  const b = e.target.closest("[data-variant]");
  if (!b) return;
  VARIANTS[S.screen][+b.dataset.variant][1]();
  renderVariants();
});

// Phone prompt map: the desktop's tick per prompt, on the chat's right edge once it scrolls.
// Touch has no hover, so pressing the strip shows the nearest prompt and lifting jumps to it; a tap jumps straight away.
(() => {
  const st = document.createElement("style");
  st.textContent = `.pmap { position: absolute; right: 0; top: 68px; bottom: 80px; width: 28px; z-index: 3; touch-action: none }
.pmap.held { background: linear-gradient(90deg, transparent, color-mix(in srgb, var(--m-soft) 70%, transparent)) }`;
  document.head.append(st);
  S.pmap = "read";
  const toTurn = (i, behavior) => {
    const body = document.getElementById("pbody"), t = document.getElementById("pturn" + i);
    if (body && t) body.scrollTo({ top: t.offsetTop - body.offsetTop - 8, behavior });
  };
  const base = render;
  render = function () { base(); if (S.screen === "promptMap" && S.pmap !== "short") toTurn(S.pmap === "read" ? 2 : 4, "instant"); };
})();
