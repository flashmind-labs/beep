// ---- Desktop: chats, memories, routines and Cmd+K in the command center window (#130) ----
// The window is the main UI. Its sidebar lists chats; chats, memories and routines open in the pane with
// everything the bar has. The bar stays for quick access and can hand a chat to the window.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .cc-main { position: relative }
.mac .cc-grp h3 { display: flex; align-items: center; justify-content: space-between }
.mac .cc-grp h3 .cc-h3l { color: inherit; text-decoration: none; cursor: pointer; border-radius: 4px; padding: 1px 3px; margin: -1px -3px }
.mac .cc-grp h3 .cc-h3l:hover { color: var(--d-fg); background: rgba(255,255,255,.06) }
.mac .cc-grp h3 button { border: 0; background: transparent; color: var(--d-muted); padding: 0 2px; font-size: 14px; line-height: 1 }
.mac .cc-si .tt { min-width: 0; overflow: hidden; text-overflow: ellipsis }
.mac .cc-si .pin { color: var(--d-coral); display: grid }
.mac .cc-si .pa { margin-left: auto; display: grid; color: var(--d-muted); opacity: 0 }
.mac .cc-si:hover .pa { opacity: 1 }
.mac .cc-si .pa:hover { color: var(--d-coral) }
.mac .cc-si .dspin { width: 10px; height: 10px; margin-left: auto }
.mac .cc-si.more { color: var(--d-muted); font-size: 12px }
.mac .cc-si.none { color: var(--d-dim); font-size: 12px; cursor: default }
.mac .cc-top .tools { margin-left: auto; display: flex; gap: 6px }
.mac .cc-chat { flex: 1; min-height: 0; overflow: auto; padding: 16px 0 10px; scrollbar-width: thin }
.mac .cc-chat > .in { max-width: var(--colw); margin: 0 auto; padding: 0 22px; display: flex; flex-direction: column; gap: 4px }
.mac .cc-chat .dbody { flex: none; overflow: visible; padding: 0 }
.mac .cc-chat .dturn + .dturn { padding-top: 18px }
.mac .cc-chat > .in { gap: 14px }
.mac .dhints .win { display: inline-flex; align-items: center; gap: 6px; border: 0; background: transparent; padding: 0; font: inherit; color: inherit; cursor: pointer }
.mac .dhints .win:hover { color: var(--d-fg) }
.mac .dkeys kbd { white-space: nowrap; flex: none }
.mac .dover .dkeys { min-height: 0; overflow-y: auto }
.mac .dover { display: flex; flex-direction: column; min-height: 0; flex: 1 }
.mac .dover .vt { flex: none; display: flex; align-items: center; gap: 8px; padding: 12px 14px; border-bottom: 1px solid var(--d-border) }
.mac .dover .vt .tt { flex: 1; min-width: 0; display: grid; line-height: 18px }
.mac .dover .vt b { font-size: 14px; font-weight: 600 }
.mac .dover .dhints { margin-top: auto }
.mac .dcoderlog .tools > .dcoderen.tool.row { margin: 0 }
.mac .dcoderen.agent:not(:has(> div, > ul, > ol, > p, > pre, > math, > table)) { display: block }
.mac .dbody > .dcoderen.user, .mac .dbody > .dcoderturn { flex: none }
.mac .dchat .t { display: grid; flex: 1; min-width: 0; line-height: 18px }
.mac .dchat .t > span, .mac .dchat .t > small { overflow: hidden; white-space: nowrap; text-overflow: ellipsis }
.mac .dchat .t > small { font-size: 12px; color: var(--d-muted) }
.mac .dcard.dshare { flex: none; display: grid; gap: 8px; margin: 2px 8px 6px; padding: 10px 12px }
.mac .dcard.dshare .code { font-size: 12px; letter-spacing: 0; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .dsystem { align-self: center; justify-self: center; text-align: center; font-size: 12px; line-height: 18px; color: var(--d-muted) }
.mac .turn.dcoderlog { display: flex; flex-direction: column; gap: 10px; min-width: 0 }
.mac .cc-hints .dmodel { display: inline-flex; align-items: center; gap: 6px }
.mac .cc-chat .dbody > :is(p, ul, ol, blockquote, h1, h2, h3, h4), .mac .cc-chat .dbody > div:not([class]) { max-width: 52em }
.mac .cc-chat ~ .cc-comp, .mac .cc-chat ~ .cc-hints { margin-inline: max(16px, calc((100% - var(--colw)) / 2 + 16px)) }
.mac .cc-comp .model { flex: none; font-size: 11.5px; color: var(--d-muted); border: 1px solid var(--d-border); border-radius: 999px; padding: 1px 9px; background: var(--d-elev) }
.mac .cc-hints { flex: none; display: flex; justify-content: center; flex-wrap: wrap; gap: 4px 14px; margin: -6px 16px 10px; font-size: 11px; color: var(--d-dim) }
.mac .cc-hints kbd, .mac .cc-pal kbd { font: inherit; font-size: 10.5px; padding: 0 4px; margin-right: 4px; border-radius: 4px; border: 1px solid var(--d-border); background: var(--d-elev); color: var(--d-muted) }
.mac .cc-pane { flex: 1; min-height: 0; overflow: auto; padding: 14px 16px 16px; scrollbar-width: thin }
.mac .cc-pane > .cc-panel { max-width: 760px; margin: 0 auto; overflow: hidden }
.mac .cc-pane .dinput { font-size: 15px; padding: 10px 14px }
.mac .cc-main > .dviewer { border-radius: 0 }
.mac .cc-scrim { position: absolute; inset: 0; z-index: 8; background: rgba(8,8,9,.5); display: grid; justify-items: center; align-content: start; padding-top: 70px }
.mac .cc-pal { width: 560px; max-width: calc(100% - 40px); border-radius: 14px; border: 1px solid var(--d-border); background: rgba(21,22,23,.97); box-shadow: 0 28px 70px rgba(0,0,0,.6); overflow: hidden; animation: barIn .18s var(--ease) }
.mac .cc-pal .q { display: flex; align-items: center; gap: 10px; padding: 12px 14px; font-size: 16px; border-bottom: 1px solid var(--d-border) }
.mac .cc-pal .q .ph { color: var(--d-dim) }
.mac .cc-pal .q svg { color: var(--d-muted); flex: none }
.mac .cc-pal .res { max-height: 340px; overflow: auto; padding: 6px; display: grid; gap: 1px; scrollbar-width: thin }
.mac .cc-pal h5 { margin: 6px 8px 3px; font-size: 12px; font-weight: 600; color: var(--d-dim) }
.mac .cc-pal .it { display: flex; align-items: center; gap: 10px; padding: 6px 8px; border-radius: 7px; border: 0; background: transparent; text-align: left; width: 100%; color: var(--d-fg); font-size: 13px }
.mac .cc-pal .it[aria-selected="true"] { background: rgba(94,106,210,.26) }
.mac .cc-pal .it svg { flex: none; color: var(--d-muted) }
.mac .cc-pal .it .t { flex: 1; min-width: 0; display: grid; line-height: 17px }
.mac .cc-pal .it .t span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-pal .it .t small { font-size: 11.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-pal .it .w { font-size: 11px; color: var(--d-muted); flex: none }
.mac .cc-pal mark { background: rgba(242,153,74,.3); color: inherit; border-radius: 3px; padding: 0 1px }
.mac .cc-pal .none { padding: 22px 14px; text-align: center; font-size: 13px; color: var(--d-muted) }
.mac .cc-pal .foot { display: flex; gap: 14px; padding: 7px 14px; border-top: 1px solid var(--d-border); font-size: 11px; color: var(--d-dim) }
@media (max-width: 600px) {
  .mac .cc-grp.chats .cc-si:not([aria-current="true"]) { display: none }
  .mac .cc-chat > .in { padding: 0 12px }
  .mac .cc-hints { display: none }
}`;
  document.head.append(st);
})();

CC_IC.search = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/></svg>';
CC_IC.loop = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M17 2l4 4-4 4"/><path d="M3 11V9a3 3 0 013-3h15M7 22l-4-4 4-4"/><path d="M21 13v2a3 3 0 01-3 3H3"/></svg>';
CC_IC.plus = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>';
CC_IC.gear = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z"/></svg>';

// t, when, pinned, group, answering
const CC_CHATS = [
  ["Saturday plan", "now"],
  ["Flat viewing questions", "29 Sep", true],
  ["Tokyo trip", "Sam: find a ryokan", false, true],
  ["Lunch with Sam", "1 h ago"],
  ["Train to Girona", "3 Oct"],
];
// The sidebar every window view shares. cur: "home", "chats", "memories", "routines" or a chat title.
function ccSide(cur, o = {}) {
  const si = (ic, label, n, on, hot, go) => `<button class="cc-si"${on ? ' aria-current="true"' : ""}${go ? ` data-dgo="${go}"` : ` data-toast="${label}"`}>${ic}<span class="tt">${label}</span>${n ? `<span class="n${hot ? " hot" : ""}">${n}</span>` : ""}</button>`;
  const chats = o.noChats ? [] : (o.newChat ? [[o.newChat, "", false, false, true], ...CC_CHATS.slice(0, 4)] : CC_CHATS);
  const row = ([t, w, pin, group, run]) => `<button class="cc-si"${t === cur ? ' aria-current="true"' : ""} data-dgo="dWinChat">${group ? DI.group : CC_IC.chat}<span class="tt">${t}</span>${run ? '<span class="dspin"></span>' : `<span class="pa" role="button" aria-label="${pin ? "Unpin" : "Pin to top"}" data-toast="${pin ? "Unpinned " : "Pinned "}${t}">${DI.pin}</span>`}</button>`;
  // Pinned chats and projects sit above everything else, in the order they were pinned.
  const pinned = chats.filter(c => c[2]).map(row).join("") + (o.old || o.bare ? "" : PJ.filter(p => p.pin).map(p => pjRow(cur, p, true)).join(""));
  return `<aside class="cc-side">
      <div class="cc-lights"><i></i><i></i><i></i></div>
      <div class="cc-brand"><b>Beep</b></div>
      <div class="cc-grp">
        ${si(CC_IC.search, "Search", "", false, false, "dWinPalette").replace("</button>", "<kbd>⌘K</kbd></button>")}
        ${si(CC_IC.home, "Home", o.old || o.bare ? "" : 3, cur === "home", true, "dHome")}
        ${o.old ? "" : si(CC_IC.run, "Running", o.bare ? "" : 4, cur === "agents", false, "dAgents")}
        ${o.old ? "" : si(CC_IC.term, "Terminals", o.bare ? "" : 3, cur === "terminals", false, "dTerminals")}
        ${o.old || o.bare || o.noSync ? "" : si(CC_IC.sync, "Sync", o.sync ?? 3, cur === "sync", true, "dSync")}
        ${si(CC_IC.cal, "Coming up", o.bare ? "" : 3)}
      </div>
      ${pinned ? `<div class="cc-grp pinned"><h3>Pinned</h3>${pinned}</div>` : ""}
      <div class="cc-grp chats"><h3><span style="cursor:pointer" data-dgo="dWinChats" title="All chats">Chats</span><button aria-label="New chat" data-toast="New chat">${CC_IC.plus}</button></h3>
        ${chats.length ? chats.filter(c => !c[2]).map(row).join("") : '<span class="cc-si none">No chats yet</span>'}
      </div>
      ${o.old ? "" : o.bare ? `<div class="cc-grp"><h3><a class="cc-h3l" href="#" data-dgo="dWinProjects" title="All projects">Projects</a><button aria-label="Add project" data-dgo="dWinProjects">${CC_IC.plus}</button></h3>
        <div class="cc-si none">No projects yet</div></div>` : `<div class="cc-grp"><h3><a class="cc-h3l" href="#" data-dgo="dWinProjects" title="All projects">Projects</a><button aria-label="Add a project" data-dgo="dAddProject">${CC_IC.plus}</button></h3>
        ${pjSideOrgs(cur)}
      </div>`}
      <div class="cc-grp"><h3>Beep</h3>
        ${si(CC_IC.mem, "Memories", "", cur === "memories", false, "dWinMemories")}
        ${si(CC_IC.loop, "Routines", "", cur === "routines", false, "dWinRoutines")}
        <button class="cc-si" data-dgo="dWinSettings">${CC_IC.gear}<span class="tt">Settings</span><kbd style="margin-left:auto;font-size:10.5px;opacity:.7">⌘,</kbd></button>
      </div>
      <div class="cc-foot">${o.old ? "<span>Machines unavailable</span>" : o.bare ? `<span>No machines linked</span><button class="dbtn quiet" style="margin-left:auto;padding:2px 6px" aria-label="Add a machine" title="Add a machine" data-dgo="dAddMachine">${CC_IC.plus}</button>` : `<span><span class="ddot"></span> <b>3</b> of 4 machines online</span><button class="dbtn quiet" style="margin-left:auto;padding:2px 6px" aria-label="Add a machine" title="Add a machine" data-dgo="dAddMachine">${CC_IC.plus}</button>`}</div>
    </aside>`;
}
// Draws a bar screen with some state and gives back its pieces, so the window shows exactly what the bar does.
function ccFromBar(screen, setup = {}) {
  const keep = {};
  for (const k of Object.keys(setup)) keep[k] = S.d[k];
  Object.assign(S.d, setup);
  let html;
  try { html = D[screen](); } finally { Object.assign(S.d, keep); }
  const t = document.createElement("template");
  t.innerHTML = html;
  const bar = t.content.querySelector(".dbar");
  bar.querySelectorAll(".dhints").forEach(n => n.remove());
  const body = bar.querySelector(".dbody");
  // A bar chat drawn as a log carries its prompt in "You": that is the ask, and the body keeps the rest.
  const you = body && body.querySelector(".dcoderlog > .dcoderen.user");
  const ask = you ? you.querySelector("span") : bar.querySelector(".dinput.dgrip .txt");
  const askHtml = ask ? ask.innerHTML : "";
  if (you) you.remove();
  const viewer = bar.querySelector(".dviewer");
  return { bar, ask: askHtml, body: body ? body.innerHTML : "", viewer: viewer ? viewer.outerHTML : "" };
}
let ccWin = (side, main) => mbar() + `<div class="cc-win" role="main" aria-label="Beep">${side}<div class="cc-main">${main}</div></div>`;
const ccComp = (ph, streaming) => `<div class="cc-comp"><span class="in"><span class="caret"></span><span class="ph">${ph}</span></span>
    <button class="dbtn quiet" data-dgo="dAgentNew" title="Start a coding agent on a machine (⌘⇧A)"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>Agent</button>${streaming ? `<button class="dbtn" aria-label="Stop the reply" data-toast="Stopped"><span style="width:9px;height:9px;border-radius:2px;background:currentColor"></span>Stop</button>` : ""}</div>
  <div class="cc-hints">${streaming ? `<span><kbd>${DI.ret}</kbd>queue</span><span><kbd>⌘.</kbd>stop</span><span><kbd>⌘K</kbd>search</span>` : `<span><kbd>⌘K</kbd>search</span><span><kbd>⌘N</kbd>new agent</span><span><kbd>⇧${DI.ret}</kbd>new line</span><span><kbd>⌥ ⌥</kbd>quick chat</span>`}<button class="dmodel" data-dgo="dModel">Auto<kbd>⌘E</kbd></button></div>`;
// One turn as the bar and the window both draw it: "You", then the work and the answer.
const ccTurn = (ask, body) => {
  const you = ask ? dYou(ask) : "", open = /^\s*<div class="(?:turn )?dcoderlog">/.exec(body);
  return open ? `<div class="turn dcoderlog">${you}${body.slice(open[0].length)}` : `<div class="turn dcoderlog">${you}${body}</div>`;
};

D.dWinChat = () => {
  const v = S.d.wc || "reply";
  const src = ccFromBar("dSources"), chart = ccFromBar("dChart", { chart: "stock" });
  const turns = [ccTurn(src.ask, src.body), ccTurn(chart.ask, chart.body)];
  let title = "Saturday plan", sub = "Started 09:12", streaming = false, viewer = "", newChat = "";
  if (v === "stream" || v === "home") {
    streaming = true;
    const ask = v === "home" ? "Find a time with Alex next week for the flat viewing" : "And will it rain for the 10:00 walk?";
    const live = ccTurn(ask, `${dHead("Working for 4s", { id: "", stats: "1 tool call" })}<div class="tools">${dTool(v === "home" ? "Checking Alex's free times" : "Checking the forecast", "run", "web")}</div>${dLive("Working", "4s")}
      <div class="dcoderen agent">${v === "home" ? "You are both free on <b>Thursday morning</b>" : "Light rain until about 10:30, so the walk"}<span class="caret"></span></div>`);
    if (v === "home") { turns.length = 0; title = newChat = "Flat viewing with Alex"; sub = "Started from Home just now"; }
    turns.push(live);
  }
  if (v === "update") {
    turns.length = 0;
    title = "Dentist moved to Thursday";
    sub = `From ${UPDATE_CHAT[0]} · ${UPDATE_CHAT[1]}`;
    turns.push(`<div class="turn dcoderlog"><div class="dsystem">${UPDATE_CHAT[0]}</div><div class="dcoderen agent">${UPDATE_CHAT[2]}</div></div>`);
  }
  if (v === "agentdone" || v === "agentask") {
    const q = v === "agentask";
    turns.length = 1;
    turns.push(`<div class="turn dcoderlog"><div class="dsystem">${q ? "Claude Code has a question" : "Claude Code finished"}</div>${dHead("Worked for 6s", { stats: "1 tool call", act: "toast" , when: "09:44"})}<div class="dcoderen agent">${q ? "It asks which error text the login screen should show. Want the one the web app uses?" : "The login test passes now on <b>fix-login</b>, and 2 files changed. The other tests still pass."}</div></div>`);
  }
  if (v === "approve") {
    const ap = ccFromBar("dApprove", { approve: S.d.approve || "ask" });
    turns.push(ccTurn(ap.ask, ap.body));
  }
  if (v === "picture") {
    const p = ccFromBar("dReplyPics", { rpic: S.d.rpic || "viewer", rpicAt: S.d.rpicAt || 0 });
    turns.push(ccTurn(p.ask, p.body));
    viewer = p.viewer;
  }
  // Every chat says where it belongs: General, or the project it is about.
  const scope = v === "project"
    ? `<button class="cc-tag" style="cursor:pointer" title="acme/widget: open the project" data-dgo="dProjectsPage"><span class="cc-pdot" style="width:6px;height:6px;background:var(--d-coral)"></span>widget</button>`
    : '<span class="cc-tag">General</span>';
  const top = `<div class="cc-top"><h2>${title}</h2>${scope}<small>${sub}</small><span class="tools">
      ${CC_CHATS.some(c => c[0] === title && c[2]) ? '<button class="dbtn" data-toast="Unpinned">Unpin</button>' : '<button class="dbtn" data-toast="Pinned to the top of the sidebar">Pin</button>'}</span></div>`;
  return ccWin(ccSide(title, { newChat, noChats: v === "nochats" }), v === "nochats"
    ? `<div class="cc-top"><h2>New chat</h2><span class="cc-tag">General</span></div><div class="cc-chat"><div class="in"></div></div>${ccComp("Ask Beep…")}`
    : `${top}<div class="cc-chat"><div class="in">${turns.join("")}</div></div>${ccComp("Ask a follow-up…", streaming)}${viewer}`);
};
VARIANTS.dWinChat = [["Reply with sources and chart", "reply"], ["Streaming", "stream"], ["Approval waiting", "approve"], ["Picture viewer", "picture"], ["Sent from Home", "home"], ["From an update", "update"], ["Agent finished", "agentdone"], ["Agent has a question", "agentask"], ["Project chat", "project"], ["No chats", "nochats"]]
  .map(([l, k]) => [l, () => { S.d.wc = k; if (k === "approve") S.d.approve = "ask"; if (k === "picture") S.d.rpic = "viewer"; render(); }, () => (S.d.wc || "reply") === k]);
VARIANTS.dWinChat.push(["Wide window", () => { S.d.wide = !S.d.wide; render(); }, () => !!S.d.wide]);
// Right-click on a chat row in the sidebar: Pin and Delete.
VARIANTS.dWinChat.push(["Chat row menu", () => { S.d.wc = "rowmenu"; render(); }, () => S.d.wc === "rowmenu"]);
{
  const rowBase = D.dWinChat;
  D.dWinChat = () => {
    const html = rowBase();
    if (S.d.wc !== "rowmenu") return html;
    const menu = `<div class="cc-menu" role="menu" style="top:440px;left:60px"><span class="lbl">Lunch with Sam</span><button role="menuitem" data-toast="Pinned Lunch with Sam">Pin to top</button><button class="warn on" role="menuitem" data-toast="Chat deleted">Delete\u2026</button></div>`;
    return html.replace('class="cc-win"', 'class="cc-win" style="position:relative"').replace("</aside>", "</aside>" + menu);
  };
}

// Lists from the bar, widened into the pane.
const ccPane = (cur, title, sub, screen, setup) => {
  const { bar } = ccFromBar(screen, setup);
  bar.querySelectorAll(".dhead").forEach(n => n.remove());
  return ccWin(ccSide(cur), `<div class="cc-top"><h2>${title}</h2><small>${sub}</small></div><div class="cc-pane"><div class="cc-panel">${bar.innerHTML}</div></div>`);
};
// Settings open over whatever the window shows (⌘, anywhere, or the sidebar).
// The page behind stays put; esc or a click outside closes it. Sections load
// from the local copy first, so the sheet is drawn at once and fills in.
D.dWinSettings = () => {
  const sheet = CC_SET.frame(S.d.set || "general");
  return D.dWinChat().replace(/<\/div><\/div>\s*$/, sheet + "</div></div>").replace('class="cc-win"', 'class="cc-win" style="position:relative"');
};
const amSshStatus = () => `<div style="display:flex;gap:8px;align-items:center;font-size:12.5px;color:var(--d-muted)"><span class="dspin"></span>Waiting for the machine · token works once, 9:41 left<span style="margin-left:auto"><button class="dbtn quiet" data-toast="Token revoked">Cancel</button></span></div>`;
// Add a machine: Beep makes a one-time token; the machine claims it with
// `beep-node setup <token>` and links itself to this account. Single use,
// 10 minutes, revocable. The sheet waits and flips to Linked by itself.
D.dAddMachine = () => {
  const v = S.d.am || "waiting";
  // Where the node looks for git checkouts; anything but ~/ goes on the command.
  const search = v === "search" ? "~/Projects" : "~/";
  const cmd = "beep-node setup bk1-7QF3-M9XA-2KDP-WT4H" + (search === "~/" ? "" : ` --search ${search}`);
  const searchRow = `<div style="display:flex;gap:10px;align-items:center;margin-bottom:14px"><span style="font-size:13px;color:var(--d-muted)">Search for projects in</span><input aria-label="Search for projects in" value="${search}" style="flex:1;font-family:var(--mono,monospace);font-size:12.5px;padding:6px 10px;border-radius:8px;background:#0f1011;border:1px solid var(--d-border);color:var(--d-fg)"></div>`;
  const body = v === "linked"
    ? `<div style="display:flex;gap:10px;align-items:center;font-size:14px"><span class="ddot"></span><b>build-box linked</b></div>
       <div style="font-size:12.5px;color:var(--d-muted);margin:8px 0 14px">Linux, Docker, Claude Code, Codex. It shows under Machines and can run agents now.</div>
       <button class="dbtn pri" data-dgo="dHome">Done</button>`
    : v === "expired"
    ? `<div style="font-size:13px;margin-bottom:12px">This token expired. Tokens last 10 minutes and work once.</div><button class="dbtn pri" data-dact="dset:am:waiting" data-dgo="dAddMachine">New token</button>`
    : v === "making" ? `<div class="am-wait"><span class="dspin"></span>Making a setup token</div>`
    : v === "outdated" ? `<div style="font-size:13px;margin-bottom:12px">The server needs an update before machines can be added from here.</div><button class="dbtn" data-dgo="dHome">Close</button>`
    : v === "tokenfail" ? `<div style="font-size:13px;margin-bottom:12px;color:var(--d-err)">Could not make a setup token.</div><div style="display:flex;gap:8px"><button class="dbtn pri" data-dact="dset:am:waiting" data-dgo="dAddMachine">Try again</button><button class="dbtn quiet" data-dgo="dHome">Close</button></div>`
    : `${searchRow}
       <div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">1. Install beep-node on the machine</div>
       <div style="font-size:12px;color:var(--d-muted);margin-bottom:4px">macOS or Linux</div>
       <div style="display:flex;gap:8px;align-items:center;margin-bottom:14px"><div style="flex:1;min-width:0;font-family:var(--mono,monospace);font-size:12px;padding:9px 11px;border-radius:8px;background:#0f1011;border:1px solid var(--d-border)">curl -fsSL https://releases.example.com/beep/node/install.sh | sh</div><button class="dbtn" data-toast="Copied" style="flex:none">Copy</button></div>
       <div style="font-size:12px;color:var(--d-muted);margin-bottom:4px">Windows, in PowerShell</div>
       <div style="display:flex;gap:8px;align-items:center;margin-bottom:14px"><div style="flex:1;min-width:0;font-family:var(--mono,monospace);font-size:12px;padding:9px 11px;border-radius:8px;background:#0f1011;border:1px solid var(--d-border)">irm https://releases.example.com/beep/node/install.ps1 | iex</div><button class="dbtn" data-toast="Copied" style="flex:none">Copy</button></div>
       <div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">2. Run this there</div>
       <div style="display:flex;gap:8px;align-items:center;margin-bottom:12px"><div style="flex:1;font-family:var(--mono,monospace);font-size:12.5px;padding:9px 11px;border-radius:8px;background:#0f1011;border:1px solid var(--d-border)">${cmd}</div><button class="dbtn" data-toast="Copied">Copy</button></div>
       ${amSshStatus(v)}`;
  const sheet = `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5">
    <div style="width:min(560px,86%);padding:18px 20px;border-radius:14px;border:1px solid var(--d-border);background:#151617;box-shadow:0 20px 60px rgba(0,0,0,.5)">
      <div style="display:flex;justify-content:space-between;margin-bottom:12px"><b style="font-size:15px">Add a machine</b><kbd style="font-size:11px;opacity:.7">esc</kbd></div>${v === "linked" ? "" : amMac()}${body}</div></div>`;
  return D.dHome().replace(/<\/div><\/div>\s*$/, sheet + "</div></div>").replace('class="cc-win"', 'class="cc-win" style="position:relative"');
};
VARIANTS.dAddMachine = [["Waiting", "waiting"], ["Search in ~/Projects", "search"], ["Linked", "linked"], ["Expired", "expired"], ["Making a token", "making"], ["Server too old", "outdated"], ["No token", "tokenfail"]].map(([l, k]) => [l, () => { S.d.am = k; render(); }, () => (S.d.am || "waiting") === k]);
VARIANTS.dWinSettings = [];
D.dWinChats = () => ccPane("chats", "Chats", "Every chat, from the bar, this window and your phone", "dChats", { chats: S.d.chats === "search" ? "search" : "list", sel: S.d.sel ?? 0 });
// #pane-box > .dbar: the bar's page, its head and hints included, in a panel that fills the pane.
const ccListPane = (cur, title, sub, screen) => {
  const t = document.createElement("template");
  t.innerHTML = D[screen]();
  return ccWin(ccSide(cur), `<div class="cc-top"><h2>${title}</h2><small>${sub}</small></div><div class="cc-pane lp"><div class="cc-panel">${t.content.querySelector(".dbar").innerHTML}</div></div>`);
};
D.dWinMemories = () => ccListPane("memories", "Memories", "What Beep knows about you", "dMemories");
D.dWinRoutines = () => ccListPane("routines", "Routines", "What Beep does on its own", "dRoutines");
D.dWinSkills = () => ccListPane("skills", "Skills", "What Beep knows how to do", "dSkills");
VARIANTS.dWinChats = [["List", "list"], ["Search", "search"]].map(([l, k]) => [l, () => { S.d.chats = k; render(); }, () => (S.d.chats === "search" ? "search" : "list") === k]);

// Cmd+K: one search for chats (titles and message text), memories, routines, machines, projects and actions.
D.dWinPalette = () => {
  const v = S.d.pal || "empty";
  const old = v === "old";
  const it = (ic, t, sm, w, on) => `<button class="it" role="option" aria-selected="${!!on}" data-toast="Opens ${t.replace(/<[^>]+>/g, "")}">${ic}<span class="t"><span>${t}</span>${sm ? `<small style="white-space:${v === "worktree bar" ? "normal" : "nowrap"}">${sm}</small>` : ""}</span>${w ? `<span class="w">${w}</span>` : ""}</button>`;
  let res;
  const PR_IC = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="18" r="2"/><path d="M6 8v8M18 16V9a2 2 0 00-2-2h-4"/></svg>';
  if (v === "empty") res = `<h5>Actions</h5>${it(CC_IC.plus, "New chat", "", "", true)}${["Home", "All chats", "Memories", "Routines", "Skills", "Apps", "Settings", "Coding agents"].map(t => it(CC_IC.plus, t)).join("")}
      <h5>Recent chats</h5>${CC_CHATS.slice(0, 5).map(([t]) => it(CC_IC.chat, t)).join("")}`;
  // One ranked list, grouped by kind: the top hit first, then each group's best.
  else if (v === "worktree bar") res = `<h5>Coding agents</h5>${it(CC_IC.chat, "Improve session search", "Prompt: Improve the <mark>worktree</mark> search <mark>bar</mark> · Add <mark>worktree</mark> support · Update the search <mark>bar</mark>")}`;
  else if (v === "mac") res = `<h5>Top hit</h5>${it(CC_IC.gear, "Let Beep use this <mark>Mac</mark>", "Settings · This Mac", "", true)}
      <h5>Settings</h5>${it(CC_IC.gear, "Unlink this <mark>Mac</mark>", "Settings · Account")}
      <h5>Projects and code</h5>${it(PR_IC, "acme/widget · PR #45 Swipeable approvals on i<mark>Mac</mark>", "PR · yours · github.com")}${it(PR_IC, "acme/widget · beep/<mark>mac</mark>-approvals", "Branch · 2 ahead · Swipeable approvals")}
      <h5>Chats</h5>${it(CC_IC.chat, "Troubleshooting F3D on <mark>mac</mark>OS")}
      <h5>Memories</h5>${it(CC_IC.mem, "Uses a <mark>Mac</mark>Book Air for travel and a Mac Studio at home", "", "devices")}
      <h5>Machines</h5>${it('<span class="ddot"></span>', "<mark>mac</mark>-studio", "", "online")}`;
  else if (v === "key") res = `<h5>Settings</h5>${it(CC_IC.gear, "<mark>GitHub</mark> token", "Settings · Credentials", "", true)}
      <h5>Chats</h5>${it(CC_IC.chat, "Which <mark>GitHub</mark> org for the new repo?")}`;
  else if (v === "none") res = `<div class="none">Nothing matches “quinoa”.<br><button class="dbtn" data-toast="Sent as a new chat">Ask Beep “quinoa”<kbd>⌘↵</kbd></button></div>`;
  else res = `<h5>Top hit</h5>${it(CC_IC.chat, "<mark>Flat</mark> viewing questions", "", "", true)}
      <h5>Chats</h5>${it(CC_IC.chat, "Saturday plan", "…before the <mark>flat</mark> viewing on Thursday at 10:00…")}
      <h5>Memories</h5>${it(CC_IC.mem, "Looking for a two-bedroom <mark>flat</mark> near the station", "", "home")}
      <h5>Routines</h5>${it(CC_IC.loop, "Check new <mark>flat</mark> listings", "Weekdays at 08:00")}`;
  const q = v === "empty" ? `<span class="ph">Search anything: chats, projects, coding agents, machines, settings, PRs…</span>` : v === "none" ? "quinoa" : v === "mac" ? "mac" : v === "key" ? "github" : v === "worktree bar" ? "worktree bar" : "flat";
  const pal = `<div class="cc-scrim" data-toast="Closes"><div class="cc-pal" role="dialog" aria-label="Search" onclick="event.stopPropagation()">
      <div class="q">${CC_IC.search}<span style="flex:1">${q}<span class="caret" style="height:17px"></span></span></div>
      <div class="res" role="listbox">${res}</div>
      <div class="foot"><span><kbd>↑↓</kbd>move</span><span><kbd>↵</kbd>open</span><span><kbd>⌘↵</kbd>ask Beep</span><span><kbd>esc</kbd>close</span></div></div></div>`;
  return D.dHome().replace(/<\/div>$/, pal + "</div>");
};
VARIANTS.dWinPalette = [["Empty", "empty"], ["Query", "query"], ["Multiple terms", "worktree bar"], ["Anything: “mac”", "mac"], ["A setting: “github”", "key"], ["No results", "none"], ["Old server", "old"]].map(([l, k]) => [l, () => { S.d.pal = k; render(); }, () => (S.d.pal || "empty") === k]);

// The bar hands a chat to the window: a chip under the reply, and a row action in the chat list.
D.dWinHandoff = () => {
  if (S.d.hand === "list") return D.dChats();
  // The hand-off is a shortcut in the bottom bar, first in the row, not a button over the reply.
  return D.dDone().replace('<div class="dhints">', `<div class="dhints"><button style="all:unset;cursor:pointer;display:inline-flex;gap:6px;align-items:center" data-dgo="dWinChat"><kbd>⌘O</kbd>open in window</button>`);
};
VARIANTS.dWinHandoff = [["Chat", "chat"], ["Chat list row", "list"]].map(([l, k]) => [l, () => { S.d.hand = k; if (k === "list") { S.d.chats = "actions"; } render(); }, () => (S.d.hand || "chat") === k]);

// A chat in the window opens at its newest turn.
{
  const base = render;
  render = function () {
    base();
    document.querySelectorAll("#desk .cc-chat").forEach(c => { c.scrollTop = c.scrollHeight; });
  };
}
// Home uses the same sidebar.
{
  const base = D.dHome;
  D.dHome = () => ["connecting", "stuck"].includes(S.d.cc) ? base() : base().replace(/<aside class="cc-side">[\s\S]*?<\/aside>/, ccSide("home", { old: S.d.cc === "old" }));
}
DESK_SCREENS.find(g => g.group === "Command center").items.push(
  ["dWinChat", "Chat in the window", "CommandCenterWindow, ChatPane"],
  ["dWinChats", "All chats", "CommandCenterWindow, ChatList"],
  ["dWinSettings", "Settings overlay", "CommandCenterWindow, SettingsSheet"],
  ["dAddMachine", "Add a machine", "CommandCenterWindow, AddMachineSheet"],
  ["dAgentAccounts", "Agent accounts", "CommandCenterWindow, SettingsSheet, AgentAccounts"],
  ["dApiKeys", "Credentials", "CommandCenterWindow, SettingsSheet, Credentials"],
  ["dWinMemories", "Memories", "CommandCenterWindow, MemoriesPane"],
  ["dWinRoutines", "Routines", "CommandCenterWindow, RoutinesPane"],
  ["dWinSkills", "Skills", "CommandCenterWindow, SkillsPane"],
  ["dWinPalette", "Cmd+K search", "CommandPalette"],
  ["dWinHandoff", "Bar hands a chat to the window", "PromptBar, Open in window"]);
// Direct entries for two Settings sections, so they are easy to find in the list.
D.dAgentAccounts = () => { const keep = S.d.set; S.d.set = S.d.acc || "acct"; const html = D.dWinSettings(); S.d.set = keep; return html; };
VARIANTS.dAgentAccounts = [["Signed in", "acct"], ["Claude not signed in", "acct-none"], ["Codex device code", "acct-code"]].map(([l, k]) => [l, () => { S.d.acc = k; render(); }, () => (S.d.acc || "acct") === k]);
D.dApiKeys = () => { const keep = S.d.set; S.d.set = S.d.keysv || "keys"; const html = D.dWinSettings(); S.d.set = keep; return html; };
VARIANTS.dApiKeys = [["Keys", "keys"], ["Adding Forgejo", "keys-gh"]].map(([l, k]) => [l, () => { S.d.keysv = k; render(); }, () => (S.d.keysv || "keys") === k]);
