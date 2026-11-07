// ---- Wiring ----
let toastTimer;
function toast(msg) {
  const t = document.getElementById("dtoast");
  t.textContent = msg; t.hidden = false;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => t.hidden = true, 1600);
}
function go(screen) {
  if (S.d.helpm) S.d.helpm = "";
  if (screen === "dStream" && S.screen !== "dStream") { S.d.t = 0; S.d.stopped = false; S.d.failed = false; S.d.sfold = ""; }
  if (screen === "dTray" && S.screen !== "dTray") S.d.recent = false;
  if (screen === "dChats" && S.screen !== "dChats") S.d.chats = "list";
  if (screen === "dMemories" && S.screen !== "dMemories") S.d.mem = "list";
  if (screen === "dRoutines" && S.screen !== "dRoutines") S.d.rt = "list";
  if (screen === "dSkills" && S.screen !== "dSkills") S.d.sk = "list";
  if (screen === "dGroupInfo" && S.screen !== "dGroupInfo") S.d.leave = false;
  if (screen === "dGroupNew" && S.screen !== "dGroupNew") S.d.gnew = "new";
  if (screen === "dLink" && S.screen !== "dLink" && S.d.link !== "linked") { S.d.link = "waiting"; S.d.left = 272; }
  S.screen = screen;
  render();
}
function renderIndex() {
  const groups = DESK_SCREENS;
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
// Fades a scrolling list at each edge that has more above or below it.
function fadeEdges(el) {
  const up = el.scrollTop > 2, down = el.scrollHeight - el.scrollTop - el.clientHeight > 2;
  el.classList.toggle("dfade", up || down);
  el.style.setProperty("--ft", up ? "28px" : "0px");
  el.style.setProperty("--fb", down ? "28px" : "0px");
}
document.getElementById("desk").addEventListener("scroll", e => { if (e.target.matches?.(".dbody, .dlist")) fadeEdges(e.target); }, true);
function stopDesk() { clearInterval(deskTimer); deskTimer = null; }
function startDesk() {
  if (S.screen === "dStream") {
    syncStream();
    if (!S.d.stopped && !S.d.failed && S.d.t < STREAM_END) deskTimer = setInterval(() => {
      S.d.t++; syncStream();
      if (S.d.t >= STREAM_END) stopDesk();
    }, 100);
  }
  if (S.screen === "dLink" && (S.d.link === "waiting" || S.d.link === "join")) deskTimer = setInterval(() => {
    S.d.left = Math.max(0, S.d.left - 1);
    const c = document.getElementById("dcount");
    if (c) c.textContent = mmss(S.d.left);
    if (!S.d.left) { S.d.link = "expired"; render(); }
  }, 1000);
}
// "Wide window": the desk at 2000x1250, as on a big display, scaled down to fit the page.
function setWide() {
  const m = document.querySelector(".mac");
  const on = !!S.d.wide && ["dHome", "dWinChat"].includes(S.screen);
  m.classList.toggle("wide", on);
  if (on) m.style.setProperty("--k", ((m.clientWidth - 16) / 2000).toFixed(4));
}
addEventListener("resize", setWide);
function render() {
  stopDesk();
  document.getElementById("desk").innerHTML = D[S.screen]();
  startDesk();
  orbLoop(); captionLoop();
  document.querySelectorAll("#desk .dbody, #desk .dlist").forEach(fadeEdges);
  setWide();
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
  const el = e.target.closest("[data-dgo],[data-dact],[data-toast],[data-dpick]");
  if (!el) return;
  const d = el.dataset, a = d.dact || "";
  if (d.toast) toast(d.toast);
  if (a.startsWith("dset:")) { const [, k, v] = a.split(":"); S.d[k] = v; if (d.dgo) go(d.dgo); else render(); return; }
  if (d.dpick) { S.d.sel = +d.dpick; render(); setTimeout(() => { if (S.screen === "dChats") go(d.dgroup ? "dGroup" : "dDone"); }, 300); return; }
  if (a && a.startsWith("jump:")) {
    const i = +a.slice(5);
    document.getElementById("dturn" + i)?.scrollIntoView({ block: "start", behavior: "smooth" });
    document.querySelectorAll(".dmap .tk").forEach((t, j) => t.classList.toggle("on", j === i));
    return;
  }
  if (a && a.startsWith("rpic:")) {
    const [, k, i] = a.split(":");
    if (k === "viewer") { S.d.rpic = "viewer"; S.d.rpicAt = +i; }
    else if (k === "prev") S.d.rpicAt = Math.max(0, S.d.rpicAt - 1);
    else if (k === "next") S.d.rpicAt = Math.min(RPICS.length - 1, S.d.rpicAt + 1);
    else S.d.rpic = k;
    render(); return;
  }
  if (a && a.startsWith("chats:")) { S.d.chats = a.slice(6); render(); return; }
  if (a && a.startsWith("gnew:")) { S.d.gnew = a.slice(5); render(); return; }
  if (a && a.startsWith("mem:")) { S.d.mem = a.slice(4); render(); return; }
  if (a && a.startsWith("rt:")) { S.d.rt = a.slice(3); render(); return; }
  if (a && a.startsWith("sk:")) { S.d.sk = a.slice(3); render(); return; }
  if (a && a.startsWith("nr:")) { const k = a.slice(3); S.d.nr = ["day", "weekdays", "week", "hour", "once", "cron"].includes(k) ? k : "day"; render(); return; }
  if (a === "group:mention") { S.d.group = "mention"; render(); return; }
  if (a === "leaveAsk") { if (d.dgo) go(d.dgo); S.d.leave = true; render(); return; }
  if (a === "leaveCancel") { S.d.leave = false; render(); return; }
  if (a === "shareApps") { S.d.shareApps = !S.d.shareApps; toast(S.d.shareApps ? "Beep can use your apps here" : "Your apps stay out of this group"); render(); return; }
  if (a === "inviteReset") { S.d.invite = "reset"; toast("New code made"); render(); return; }
  if (a === "gAllow" || a === "gDeny") { toast(a === "gAllow" ? "Allowed" : "Denied"); go("dGroup"); return; }
  if (a === "allow" || a === "always" || a === "deny") { S.d.approve = { allow: "allowed", always: "always", deny: "denied" }[a]; render(); return; }
  if (a === "nAllow" || a === "nDeny") { S.d.approve = a === "nAllow" ? "allowed" : "denied"; toast(a === "nAllow" ? "Allowed" : "Denied"); go("dApprove"); return; }
  if (a === "stop") { S.d.stopped = true; render(); return; }
  if (a === "retry") { S.d.stopped = false; S.d.failed = false; S.d.sfold = ""; S.d.t = 0; render(); return; }
  if (a === "sfold") { const b = document.querySelector("#dheadl")?.closest("button"); S.d.sfold = b && b.getAttribute("aria-expanded") === "true" ? "fold" : "open"; render(); return; }
  if (a === "think") { S.d.think = !S.d.think; render(); return; }
  if (a === "qsend") { S.d.stopped = false; S.d.failed = false; S.d.sfold = ""; S.d.t = 0; S.d.queued = Math.max(0, (S.d.queued || 0) - 1); render(); return; }
  if (a && a.startsWith("qedit:")) { S.d.queued = Math.max(0, (S.d.queued || 0) - 1); render(); return; }
  if (a === "latest") { S.d.up = false; render(); return; }
  if (a.startsWith("copy:")) { S.d.copied = a.slice(5); render(); return; }
  if (a.startsWith("model:")) { S.d.model = a.slice(6); render(); return; }
  if (a === "rerun:open" || a === "rerun:close") { S.d.rerun = a === "rerun:open" ? "open" : "closed"; render(); return; }
  if (a.startsWith("rerun:")) { S.d.rerunModel = a.slice(6); S.d.rerun = "running"; render(); setTimeout(() => { if (S.screen === "dRerun" && S.d.rerun === "running") { S.d.rerun = "done"; render(); } }, 1400); return; }
  if (a.startsWith("rthink:")) { S.d.rerunThink = a.slice(7); render(); return; }
  if (a === "chartRange") { S.d.chart = "loading"; render(); setTimeout(() => { if (S.screen === "dChart" && S.d.chart === "loading") { S.d.chart = "stock"; render(); } }, 900); return; }
  if (a.startsWith("chartOff:")) { const n = a.slice(9); S.d.chartOff = S.d.chartOff === n ? "" : n; render(); return; }
  if (a === "steps") { S.d.steps = !S.d.steps; render(); return; }
  if (a === "recent") { S.d.recent = !S.d.recent; render(); return; }
  if (a && a.startsWith("set:")) { S.d.set = a.slice(4); render(); return; }
  if (a && a.startsWith("appConnect:")) { S.d.apps = "waiting"; S.d.appWait = a.slice(11); render(); return; }
  if (a === "appCancel") { S.d.apps = "list"; render(); return; }
  if (a === "mcpAdd") { S.d.apps = "mcpAdd"; render(); return; }
  if (a === "mcpCancel") { S.d.apps = "list"; render(); return; }
  if (a && a.startsWith("mcpTool:")) {
    const t = a.slice(8), on = S.d.mcpOn, tools = S.d.mcpMany ? MCP_MANY : MCP_FEW;
    if (!on[t] && tools.filter(x => on[x]).length >= MCP_LIMIT) S.d.mcpAsk = t; else on[t] = !on[t];
    render(); return;
  }
  if (a === "mcpYes") { S.d.mcpOn[S.d.mcpAsk] = true; S.d.mcpAsk = null; render(); return; }
  if (a === "mcpNo") { S.d.mcpAsk = null; render(); return; }
  if (a && a.startsWith("appLevel:")) { S.d.appLevel = a.slice(9); render(); return; }
  if (a && a.startsWith("fm:")) { S.d.fm = a.slice(3); render(); return; }
  if (a && a.startsWith("appAllow:")) { const k = a.slice(9); S.d.appAllowed[k] = !S.d.appAllowed[k]; toast(S.d.appAllowed[k] ? `${k}: always allowed` : `${k}: asks first`); render(); return; }
  if (a === "mac") { S.d.mac = (S.d.mac || "off") === "off" ? "perm" : "off"; toast(S.d.mac === "off" ? "Beep no longer uses this Mac" : "Beep can use this Mac"); render(); return; }
  if (a === "login") { S.d.login = S.d.login === false; toast(S.d.login ? "Launch at login on" : "Launch at login off"); render(); return; }
  if (a === "newCode") { S.d.link = "waiting"; S.d.left = 300; render(); return; }
  if (a === "joinOpen") { S.d.link = "join"; render(); return; }
  if (a === "ownMac") { S.d.set = "owner"; toast("Started an account on this Mac"); go("dSettings"); return; }
  if (d.dgo) {
    if (d.dgo === "dApprove" && el.closest(".nmenu")) S.d.approve = "ask";
    go(d.dgo);
  }
});
document.addEventListener("keydown", e => {
  if (e.metaKey && e.key === "/") { e.preventDefault(); return go(S.screen === "dKeys" ? "dEmpty" : "dKeys"); }
  if (e.key !== "Escape" || S.screen === "dEmpty") return;
  if (S.screen === "dKeys") return go("dEmpty");
  if (S.screen === "dChats") return go("dDone");
  if (S.screen === "dGroupInfo" || S.screen === "dGroupInvite") return go("dGroup");
  if (S.screen === "dInvite" || S.screen === "dApps" || S.screen === "dMachines") return go("dSettings");
  if (S.screen === "dAppPerm" || S.screen === "dMcp") return go("dApps");
  if (S.screen === "dNewRoutine") return go("dRoutines");
  if (S.screen === "dSkills" && S.d.sk !== "list" && S.d.sk !== "empty") { S.d.sk = ["edit", "delete"].includes(S.d.sk) ? "view" : "list"; return render(); }
  if (S.screen === "dMemories" || S.screen === "dRoutines" || S.screen === "dSkills") return go("dEmpty");
  toast("Hidden. Double-tap Option to bring it back");
  go("dEmpty");
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
.pmap .tk { position: absolute; right: 2px; width: 26px; height: 16px; margin-top: -8px; border: 0; background: transparent; padding: 0; display: flex; align-items: center; justify-content: flex-end }
.pmap .tk i { display: block; width: 10px; height: 3px; border-radius: 3px; background: var(--m-dim); transition: width .15s, background .15s }
.pmap .tk.on i { background: var(--m-ink); width: 14px }
.pmap .tk.hov i { background: var(--m-brand); width: 16px }
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
