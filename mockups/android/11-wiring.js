// ---- Wiring ----
let toastTimer;
function toast(msg) {
  const t = document.getElementById("toast");
  t.textContent = msg; t.hidden = false;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => t.hidden = true, 1600);
}
function go(screen) {
  if (!["permissions", "typeCode", "joinConfirm", "linkConfirm"].includes(screen)) S.onb = false;
  if (screen === "callout") S.callout.shown = true;
  if (screen === "chat" && S.screen !== "chat" && S.screen !== "drawer" && S.screen !== "menu") S.tools.arrived = CALLS.length + 1;
  if (screen === "memories" && S.screen !== "memories") S.memories = "list";
  if (screen === "home" && S.screen !== screen) S.home = "list";
  if (screen === "camera" && S.screen !== screen) S.cam = "sheet";
  if (screen === "groupInfo" && S.screen !== screen) S.ginfo = "info";
  S.screen = screen;
  render();
}
function renderIndex() {
  const groups = SCREENS;
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
  document.getElementById("app").innerHTML = R[S.screen]();
  if (S.screen === "chat" || S.screen === "drawer") {
    syncTools();
    if (S.screen === "chat" && S.tools.arrived <= CALLS.length) startRun();
  }
  armCallout();
  renderIndex(); renderVariants();
  try { localStorage.setItem("beep-screens", S.screen); } catch (e) {}
}

document.getElementById("index").addEventListener("change", e => {
  if (!e.target.matches("select")) return;
  if (e.target.value === "chat") { S.screen = "chat"; replay(); return; }
  go(e.target.value);
});
document.getElementById("index").addEventListener("click", e => {
  const b = e.target.closest("[data-screen]");
  if (!b) return;
  if (b.dataset.screen === "chat") { S.screen = "chat"; replay(); return; }
  go(b.dataset.screen);
});
document.getElementById("variants").addEventListener("click", e => {
  const b = e.target.closest("[data-variant]");
  if (!b) return;
  VARIANTS[S.screen][+b.dataset.variant][1]();
  renderVariants();
});
// Callout: leaves on its own after six seconds, or when swiped toward the bubble's edge.
let calloutTimer;
function showCallout() {
  S.callout.shown = true;
  if (S.screen === "callout") render();
}
function armCallout() {
  clearTimeout(calloutTimer);
  if (S.screen !== "callout" || !S.callout.shown) return;
  calloutTimer = setTimeout(() => dismissCallout(0), 6000);
}
function dismissCallout(dx) {
  const el = document.getElementById("callout");
  clearTimeout(calloutTimer);
  if (!el) return;
  el.classList.add("leaving");
  el.style.transform = `translateX(${dx || 0}px) scale(.8)`;
  setTimeout(() => { S.callout.shown = false; if (S.screen === "callout") render(); }, 200);
}
let flick = null;

// Routines: drag a card right to run it, left to delete it.
let drag = null, swallowClick = false, snackTimer;
const app = document.getElementById("app");
let holdTimer;
app.addEventListener("pointerdown", e => {
  const h = e.target.closest("[data-hold]");
  const rh = e.target.closest("[data-rerun-hold]");
  if (rh) { clearTimeout(holdTimer); holdTimer = setTimeout(() => { swallowClick = true; S.rerun = "thinking"; S.rerunHeld = rh.dataset.rerunHold; render(); }, 450); }
  if (h) { clearTimeout(holdTimer); holdTimer = setTimeout(() => { swallowClick = true; S.held = S.held === +h.dataset.hold ? null : +h.dataset.hold; render(); }, 450); }
  const co = e.target.closest(".callout");
  if (co) { flick = { el: co, x: e.clientX, dx: 0 }; return; }
  const box = e.target.closest(".swipe");
  if (!box || e.target.closest(".switch")) return;
  drag = { box, card: box.querySelector(".card"), x: e.clientX, dx: 0, moved: false };
});
app.addEventListener("pointermove", e => {
  if (flick) {
    const dx = e.clientX - flick.x, toEdge = S.callout.side === "right" ? dx > 0 : dx < 0;
    if (Math.abs(dx) > 6) { flick.el.style.animation = "none"; flick.el.style.transform = `translateX(${toEdge ? dx : dx / 4}px)`; flick.el.style.opacity = toEdge ? Math.max(.2, 1 - Math.abs(dx) / 160) : 1; }
    flick.dx = dx; return;
  }
  if (!drag) return;
  const dx = e.clientX - drag.x;
  if (!drag.moved && Math.abs(dx) < 6) return;
  if (!drag.moved) { drag.moved = true; drag.box.setPointerCapture(e.pointerId); drag.box.classList.add("dragging"); }
  drag.dx = dx;
  drag.card.style.transform = `translateX(${dx}px)`;
  drag.box.classList.toggle("to-del", dx < 0);
  drag.box.classList.toggle("to-run", dx > 0);
  drag.box.classList.toggle("armed", Math.abs(dx) > drag.box.offsetWidth * 0.35);
});
function endDrag() {
  if (flick) {
    const { el, dx } = flick; flick = null;
    if (Math.abs(dx) <= 6) return;
    swallowClick = true; setTimeout(() => swallowClick = false, 0);
    const toEdge = S.callout.side === "right" ? dx > 0 : dx < 0;
    if (toEdge && Math.abs(dx) > 50) dismissCallout(dx * 2);
    else { el.style.transition = "transform .25s cubic-bezier(.2,1.3,.4,1), opacity .2s"; el.style.transform = ""; el.style.opacity = 1; }
    return;
  }
  if (!drag) return;
  const { box, card, dx, moved } = drag;
  drag = null;
  if (!moved) return;
  swallowClick = true; setTimeout(() => swallowClick = false, 0);
  box.classList.remove("dragging");
  const armed = box.classList.contains("armed");
  if (armed && dx < 0) {
    box.classList.add("leaving");
    card.style.transform = "translateX(-110%)";
    setTimeout(() => {
      box.style.height = box.offsetHeight + "px";
      box.offsetHeight;
      box.classList.add("closing"); box.style.height = "0px";
      setTimeout(() => {
        S.deleted = +box.dataset.swipe; S.snack = true; render();
        clearTimeout(snackTimer); snackTimer = setTimeout(() => { S.snack = false; render(); }, 4000);
      }, 220);
    }, 200);
    return;
  }
  card.style.transform = "";
  if (armed) {
    toast(`Running ${box.dataset.label}`);
    setTimeout(() => go("chat"), 320);
  }
  setTimeout(() => box.classList.remove("to-del", "to-run", "armed"), 320);
}
app.addEventListener("pointerup", e => { clearTimeout(holdTimer); if (swallowClick) setTimeout(() => swallowClick = false, 0); endDrag(e); });
app.addEventListener("pointercancel", endDrag);

document.getElementById("app").addEventListener("click", e => {
  if (swallowClick) return;
  const el = e.target.closest("[data-go],[data-set],[data-act],[data-toast],[data-routine],[data-repeat],[data-day],[data-perm],[data-bubble],[data-level],[data-allow],[data-mtool],[data-mtoolok],[data-mtoolno],[data-model],[data-thinking],[data-rerun],[data-rerun-think]");
  if (!el) return;
  const d = el.dataset;
  if (d.toast) toast(d.toast);
  if (d.set) { const [k, v] = d.set.split(":"); S[k] = v === "true" ? true : v === "false" ? false : v; if (d.go) go(d.go); else render(); return; }
  if (d.routine) { S.enabled[+d.routine] = !S.enabled[+d.routine]; render(); return; }
  if (d.repeat) { S.repeat = d.repeat; render(); return; }
  if (d.day) { const n = +d.day; S.days = S.days.includes(n) ? S.days.filter(x => x !== n) : [...S.days, n]; render(); return; }
  if (d.perm) { S.perms[d.perm] = true; render(); return; }
  if (d.level) { S.level = d.level; render(); return; }
  if (d.allow) { S.allowed[d.allow] = !S.allowed[d.allow]; render(); return; }
  if (d.mtool || d.mtoolok) {
    const tools = S.mcpDetail === "many" ? MCP_MANY : MCP_FEW, on = { ...mcpOn(tools) }, t = d.mtool || S.mcpAsk;
    // Turning on the 26th tool asks first.
    if (d.mtool && !on[t] && tools.filter(x => on[x]).length >= MCP_LIMIT) { S.mcpAsk = t; render(); return; }
    on[t] = !on[t] || !!d.mtoolok;
    S.mcpOn = on; S.mcpAsk = null; render(); return;
  }
  if (d.mtoolno) { S.mcpAsk = null; render(); return; }
  if (d.act === "always") { S.always = !S.always; render(); return; }
  if (d.act === "allow") { if (S.always) S.allowed["Send email"] = true; S.approve = "allowed"; render(); return; }
  if (d.act === "deny") { S.approve = "denied"; render(); return; }
  if (d.act === "queueAdd") { S.queue = "queued"; S.queued = [...S.queued, "Add it to my calendar too"]; render(); return; }
  if (d.act && d.act.startsWith("queueEdit:")) { S.queued = S.queued.filter((_, i) => i !== +d.act.slice(10)); S.queue = S.queue === "stopped" && S.queued.length ? "stopped" : "typing"; render(); return; }
  if (d.act === "queueSend") { S.queue = "queued"; S.queued = S.queued.slice(1); toast("Sent the first, the rest follow"); render(); return; }
  if (d.act === "importKey") { S.keyImported = true; toast("Key imported"); render(); return; }
  if (d.thinking) { S.thinking = d.thinking; toast("Thinking set for your turns"); render(); return; }
  if (d.model) { S.model = d.model; toast("Model set for your turns"); render(); return; }
  if (d.bubble) { S.bubbleStyle = d.bubble; render(); return; }
  if (d.act === "thoughts") { S.thoughts = !S.thoughts; render(); return; }
  if (d.act === "viewer") { S.viewer = true; render(); return; }
  if (d.act === "closeViewer") { S.viewer = false; render(); return; }
  if (d.act === "openFileView") { S.fileView = true; render(); return; }
  if (d.act === "closeFileView") { S.fileView = false; render(); return; }
  if (d.act === "toggleTools") { S.tools.open = !S.tools.open; syncTools(); renderVariants(); return; }
  if (d.act === "reconnect") { S.connected = true; toast("Connected"); render(); return; }
  if (d.act === "time") { S.hour = (S.hour + 1) % 24; render(); return; }
  if (d.act === "edit") { S.memories = "edit"; render(); return; }
  if (d.act === "forget") { S.memories = "forget"; render(); return; }
  if (d.act === "memList") { S.memories = "list"; render(); return; }
  if (d.act === "memRead") { S.memories = "read"; render(); return; }
  if (d.act === "memEmpty") { S.memories = "empty"; render(); return; }
  if (d.act === "undo") { S.deleted = null; S.snack = false; clearTimeout(snackTimer); render(); return; }
  if (d.act === "retryRun") { S.ended = null; replay(); return; }
  if (d.act === "newIdeas") { S.starters = "loading"; render(); setTimeout(() => { if (S.starters === "loading") { S.starters = "server"; render(); } }, 1500); return; }
  if (d.act === "rerunMenu") { S.rerun = S.rerun === "open" ? "closed" : "open"; render(); return; }
  if (d.rerunThink) { S.rerunModel = S.rerunHeld; S.rerunThinking = d.rerunThink; S.rerun = "running"; render(); setTimeout(() => { if (S.screen === "rerun" && S.rerun === "running") { S.rerun = "done"; render(); } }, 1600); return; }
  if (d.rerun) { S.rerunModel = d.rerun; S.rerunThinking = null; S.rerun = "running"; render(); setTimeout(() => { if (S.screen === "rerun" && S.rerun === "running") { S.rerun = "done"; render(); } }, 1600); return; }
  if (d.act === "rideMenu") { S.place = { grab: "grabMenu", grabMenu: "grab", local: "localMenu", localMenu: "local" }[S.place]; render(); return; }
  if (d.act === "recallToggle") { S.recall = S.recall === "open" ? "closed" : "open"; render(); return; }
  if (d.act === "deleteTurn") { S.menu = "deleted"; render(); return; }
  if (d.act === "removeDesk") { S.devices = "removing"; render(); return; }
  if (d.act === "devCancel") { S.devices = "linked"; render(); return; }
  if (d.act === "devRemove") { S.devices = "none"; toast("MacBook removed"); render(); return; }
  if (d.act === "linkCancel") { go(S.onb ? "permissions" : "devices"); return; }
  if (d.act === "scanGroup") { toast("Camera opens to scan"); go("groupJoin"); return; }
  if (d.act === "drawerSelect") { S.drawerMode = "select"; render(); return; }
  if (d.act === "drawerBulk") { S.drawerMode = "bulk"; render(); return; }
  if (d.act === "drawerDone") { S.drawerMode = "list"; render(); return; }
  if (d.act === "shareClose") { S.drawerMode = "list"; go("drawer"); return; }
  if (d.act === "shareStop") { S.share = "stopped"; render(); return; }
  if (d.act === "linkDo") { S.devices = "linked"; toast("MacBook linked"); go(S.onb ? "permissions" : "devices"); return; }
  if (d.act === "joinDo") { S.devices = "joined"; toast("Joined MacBook's account"); if (S.onb) { S.computer = "joined"; go("permissions"); } else go("devices"); return; }
  if (d.act === "shareApps") { S.shareApps = !S.shareApps; render(); return; }
  if (d.act === "gRemDel") { S.gRem = "deleted"; render(); return; }
  if (d.act === "groupAllow") { S.groupAsk = false; toast("Done: create event in Dani's Google Calendar"); render(); return; }
  if (d.act === "groupDeny") { S.groupAsk = false; toast("Dani didn't allow it"); render(); return; }
  if (d.act === "inviteReset") { S.invite = "reset"; toast("Old code stopped working"); render(); return; }
  if (d.act === "joinDo") { toast("Joined Tokyo trip"); S.group = "replied"; go("groupChat"); return; }
  if (d.act === "notifOpen") { S.groupNotif = false; S.group = "replied"; render(); return; }
  if (d.act === "agents") { S.agents = !S.agents; toast(S.agents ? "Agents on for everyone" : "Agents off"); render(); return; }
  if (d.act === "auto") { S.auto = !S.auto; toast(S.auto ? "Jev picks per prompt" : "Auto off"); render(); return; }
  if (d.go) go(d.go);
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
.pmap.held { background: linear-gradient(90deg, transparent, color-mix(in srgb, var(--m-soft) 70%, transparent)) }
.ptip { position: absolute; right: 30px; z-index: 4; max-width: 250px; padding: 8px 12px; border-radius:var(--radius); background: var(--m-surface); border: 1.5px solid var(--m-ledge);
  box-shadow: 0 3px 0 var(--m-ledge); font-size: 13px; line-height: 18px; transform: translateY(-50%); pointer-events: none; animation: pop .15s }
.ptip small { display: block; font-size: 11px; font-weight: 600; color: var(--m-muted) }
.ptip span { display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden }`;
  document.head.append(st);
  const TURNS = [
    ["Can you check the bins are out Thursday?", "Yes, collection is Thursday morning. I'll remind you Wednesday evening."],
    ["What's on my calendar tomorrow?", "Three things: standup at 9:30, lunch with Sam at 13:00 at Fismuler, and the dentist at 17:15. The dentist is across town, so leave by 16:40."],
    ["Move lunch with Sam to Friday", "Done. Lunch with Sam is now Friday at 13:00, same place. I sent Sam the new time."],
    ["Did the landlord reply about the boiler?", "Yes, Alex replied this morning: the plumber can come Thursday between 9 and 12, and she asked if that works for you. You are free then, so I can tell her yes."],
    ["Find a quiet cordless vacuum under 300 euros", "Three fit: the Dreame R10 (€249, the quietest), the Bosch Unlimited 7 (€289) and the Rowenta X-Force (€269). All have a HEPA filter."],
    ["What's the weather like for Saturday?", "Dry after 10:00, around 19°C, with some rain early in the morning. Good for the market."],
  ];
  S.pmap = "read";
  // Ticks sit at the prompt's place in the chat: its message index over the message count, as the app has no heights for rows off screen.
  const at = i => Math.min(96, Math.max(4, (i * 2) / (TURNS.length * 2) * 100 + 2));
  R.promptMap = () => {
    const cur = S.pmap === "read" ? 2 : 4, hov = S.pmap === "scrub" ? 4 : -1;
    const turns = TURNS.map(([q, a], i) => `<div class="user" id="pturn${i}" style="scroll-margin-top:8px">${esc(q)}</div><div class="reply">${a}</div>`).join("");
    const map = S.pmap === "short" ? "" : `<div class="pmap${hov >= 0 ? " held" : ""}" aria-label="Prompts in this chat">${TURNS.map(([q], i) =>
      `<button class="tk${i === cur ? " on" : ""}${i === hov ? " hov" : ""}" style="top:${at(i)}%" data-pjump="${i}" aria-label="${esc(q)}"><i></i></button>`).join("")}</div>
      ${hov >= 0 ? `<div class="ptip" style="top:calc(68px + (100% - 148px) * ${at(hov) / 100})"><small>Prompt ${hov + 1} of ${TURNS.length}</small><span>${esc(TURNS[hov][0])}</span></div>` : ""}`;
    const body = S.pmap === "short" ? TURNS.slice(0, 1).map(([q, a]) => `<div class="user">${esc(q)}</div><div class="reply">${a}</div>`).join("") : turns;
    return topBar() + `<div class="body" id="pbody">${body}</div>${map}` + composer();
  };
  SCREENS.find(g => g.group === "Chat").items.splice(2, 0, ["promptMap", "Jump to a prompt", "ChatScreen, PromptMap"]);
  VARIANTS.promptMap = vs("pmap", [["Reading back", "read"], ["Finger on the strip", "scrub"], ["Short chat, no strip", "short"]]);
  const toTurn = (i, behavior) => {
    const body = document.getElementById("pbody"), t = document.getElementById("pturn" + i);
    if (body && t) body.scrollTo({ top: t.offsetTop - body.offsetTop - 8, behavior });
  };
  const base = render;
  render = function () { base(); if (S.screen === "promptMap" && S.pmap !== "short") toTurn(S.pmap === "read" ? 2 : 4, "instant"); };
  document.getElementById("app").addEventListener("click", e => {
    const b = e.target.closest("[data-pjump]");
    if (!b) return;
    const i = +b.dataset.pjump;
    toTurn(i, "smooth");
    document.querySelectorAll(".pmap .tk").forEach((t, j) => t.classList.toggle("on", j === i));
  });
})();
