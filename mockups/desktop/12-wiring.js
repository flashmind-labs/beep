// ---- Wiring ----
let toastTimer;
function toast(msg) {
  const t = document.getElementById("dtoast");
  t.textContent = msg; t.hidden = false;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => t.hidden = true, 1600);
}
function go(screen) {
  if (S.d.helpm) S.d.helpm = "";
  S.screen = screen;
  render();
}
// The hierarchical menu installs its renderer after all screens register.
function renderIndex() {}
function renderVariants() {
  const v = VARIANTS[S.screen] || [];
  document.getElementById("variants").innerHTML = v.map(([n, , on], i) =>
    `<button data-variant="${i}" aria-pressed="${on()}">${n}</button>`).join("");
}
// Fades a scrolling list at each edge that has more above or below it.
function fadeEdges(el) {
  const up = el.scrollTop > 2, down = el.scrollHeight - el.scrollTop - el.clientHeight > 2;
  el.classList.toggle("dfade", up || down);
  el.style.setProperty("--ft", up ? "28px" : "0px");
  el.style.setProperty("--fb", down ? "28px" : "0px");
}
document.getElementById("desk").addEventListener("scroll", e => { if (e.target.matches?.(".dbody, .dlist")) fadeEdges(e.target); }, true);
function render() {
  document.getElementById("desk").innerHTML = D[S.screen]();
  document.querySelectorAll("#desk .dbody, #desk .dlist").forEach(fadeEdges);
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
document.getElementById("desk").addEventListener("click", e => {
  const el = e.target.closest("[data-dgo],[data-dact],[data-toast]");
  if (!el) return;
  const d = el.dataset, a = d.dact || "";
  if (d.toast) toast(d.toast);
  if (a.startsWith("dset:")) { const [, k, v] = a.split(":"); S.d[k] = v; if (d.dgo) go(d.dgo); else render(); return; }
  if (a === "mac") { S.d.mac = (S.d.mac || "off") === "off" ? "perm" : "off"; toast(S.d.mac === "off" ? "Beep no longer uses this Mac" : "Beep can use this Mac"); render(); return; }
  if (a === "login") { S.d.login = S.d.login === false; toast(S.d.login ? "Launch at login on" : "Launch at login off"); render(); return; }
  if (d.dgo) go(d.dgo);
});
document.addEventListener("keydown", e => {
  if (e.key !== "Escape") return;
  if (S.screen === "dAddTicket") return go("dHome");
  if (S.screen === "dMachines") return go("dSettings");
});
document.getElementById("variants").addEventListener("click", e => {
  const b = e.target.closest("[data-variant]");
  if (!b) return;
  VARIANTS[S.screen][+b.dataset.variant][1]();
  renderVariants();
});
