// ---- Desktop: the sidebar, Settings and Cmd+K in the command center window (#130) ----
// The window is the main UI. The bar stays for quick access.
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
.mac .dhints .win { display: inline-flex; align-items: center; gap: 6px; border: 0; background: transparent; padding: 0; font: inherit; color: inherit; cursor: pointer }
.mac .dhints .win:hover { color: var(--d-fg) }
.mac .dcoderlog .tools > .dcoderen.tool.row { margin: 0 }
.mac .dcoderen.agent:not(:has(> div, > ul, > ol, > p, > pre, > math, > table)) { display: block }
.mac .dbody > .dcoderen.user, .mac .dbody > .dcoderturn { flex: none }
.mac .dchat .t { display: grid; flex: 1; min-width: 0; line-height: 18px }
.mac .dchat .t > span, .mac .dchat .t > small { overflow: hidden; white-space: nowrap; text-overflow: ellipsis }
.mac .dchat .t > small { font-size: 12px; color: var(--d-muted) }
.mac .turn.dcoderlog { display: flex; flex-direction: column; gap: 10px; min-width: 0 }
.mac .cc-chat .dbody > :is(p, ul, ol, blockquote, h1, h2, h3, h4), .mac .cc-chat .dbody > div:not([class]) { max-width: 52em }
.mac .cc-hints { flex: none; display: flex; justify-content: center; flex-wrap: wrap; gap: 4px 14px; margin: -6px 16px 10px; font-size: 11px; color: var(--d-dim) }
.mac .cc-hints kbd, .mac .cc-pal kbd { font: inherit; font-size: 10.5px; padding: 0 4px; margin-right: 4px; border-radius: 4px; border: 1px solid var(--d-border); background: var(--d-elev); color: var(--d-muted) }
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
  .mac .cc-hints { display: none }
}`;
  document.head.append(st);
})();

CC_IC.search = '<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/></svg>';
CC_IC.plus = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>';
CC_IC.gear = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 00.3 1.8l.1.1a2 2 0 11-2.8 2.8l-.1-.1a1.7 1.7 0 00-1.8-.3 1.7 1.7 0 00-1 1.5V21a2 2 0 11-4 0v-.1a1.7 1.7 0 00-1.1-1.5 1.7 1.7 0 00-1.8.3l-.1.1a2 2 0 11-2.8-2.8l.1-.1a1.7 1.7 0 00.3-1.8 1.7 1.7 0 00-1.5-1H3a2 2 0 110-4h.1a1.7 1.7 0 001.5-1.1 1.7 1.7 0 00-.3-1.8l-.1-.1a2 2 0 112.8-2.8l.1.1a1.7 1.7 0 001.8.3H9a1.7 1.7 0 001-1.5V3a2 2 0 114 0v.1a1.7 1.7 0 001 1.5 1.7 1.7 0 001.8-.3l.1-.1a2 2 0 112.8 2.8l-.1.1a1.7 1.7 0 00-.3 1.8V9a1.7 1.7 0 001.5 1H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1z"/></svg>';

// The sidebar every window view shares. cur: "home", "agents", "terminals", "sync", "machines" or a project.
function ccSide(cur, o = {}) {
  const si = (ic, label, n, on, hot, go) => `<button class="cc-si"${on ? ' aria-current="true"' : ""}${go ? ` data-dgo="${go}"` : ` data-toast="${label}"`}>${ic}<span class="tt">${label}</span>${n ? `<span class="n${hot ? " hot" : ""}">${n}</span>` : ""}</button>`;
  // Pinned projects sit above everything else, in the order they were pinned.
  const pinned = o.bare ? "" : PJ.filter(p => p.pin).map(p => pjRow(cur, p, true)).join("");
  return `<aside class="cc-side">
      <div class="cc-lights"><i></i><i></i><i></i></div>
      <div class="cc-brand"><b>Beep</b></div>
      <div class="cc-grp">
        ${si(CC_IC.search, "Search", "", false, false, "dWinPalette").replace("</button>", "<kbd>⌘K</kbd></button>")}
        ${si(CC_IC.home, "Home", o.bare ? "" : 3, cur === "home", true, "dHome")}
        ${si(CC_IC.run, "Running", o.bare ? "" : 4, cur === "agents", false, "dAgents")}
        ${si(CC_IC.term, "Terminals", o.bare ? "" : 3, cur === "terminals", false, "dTerminals")}
        ${o.bare || o.noSync ? "" : si(CC_IC.sync, "Sync", o.sync ?? 3, cur === "sync", true, "dSync")}
      </div>
      ${pinned ? `<div class="cc-grp pinned"><h3>Pinned</h3>${pinned}</div>` : ""}
      ${o.bare ? `<div class="cc-grp"><h3><a class="cc-h3l" href="#" data-dgo="dWinProjects" title="All projects">Projects</a><button aria-label="Add project" data-dgo="dWinProjects">${CC_IC.plus}</button></h3>
        <div class="cc-si none">No projects yet</div></div>` : `<div class="cc-grp"><h3><a class="cc-h3l" href="#" data-dgo="dWinProjects" title="All projects">Projects</a><button aria-label="Add a project" data-dgo="dAddProject">${CC_IC.plus}</button></h3>
        ${pjSideOrgs(cur)}
      </div>`}
      <div class="cc-grp"><h3>Beep</h3>
        <button class="cc-si" data-dgo="dWinSettings">${CC_IC.gear}<span class="tt">Settings</span><kbd style="margin-left:auto;font-size:10.5px;opacity:.7">⌘,</kbd></button>
      </div>
      <div class="cc-foot">${o.bare ? `<span>No machines linked</span><button class="dbtn quiet" style="margin-left:auto;padding:2px 6px" aria-label="Add a machine" title="Add a machine" data-dgo="dAddMachine">${CC_IC.plus}</button>` : `<span><span class="ddot"></span> <b>3</b> of 4 machines online</span><button class="dbtn quiet" style="margin-left:auto;padding:2px 6px" aria-label="Add a machine" title="Add a machine" data-dgo="dAddMachine">${CC_IC.plus}</button>`}</div>
    </aside>`;
}
let ccWin = (side, main) => mbar() + `<div class="cc-win" role="main" aria-label="Beep">${side}<div class="cc-main">${main}</div></div>`;
// Settings open over whatever the window shows (⌘, anywhere, or the sidebar).
// The page behind stays put; esc or a click outside closes it. Sections load
// from the local copy first, so the sheet is drawn at once and fills in.
D.dWinSettings = () => {
  const sheet = CC_SET.frame(S.d.set || "general");
  return D.dHome().replace(/<\/div><\/div>\s*$/, sheet + "</div></div>");
};
const amSshStatus = () => `<div style="display:flex;gap:8px;align-items:center;font-size:12.5px;color:var(--d-muted)"><span class="dspin"></span>Waiting for the machine · token works once, 9:41 left<span style="margin-left:auto"><button class="dbtn quiet" data-toast="Token revoked">Cancel</button></span></div>`;
// Add a machine: Beep makes a one-time token; the machine claims it with
// `beep-node setup <token>` and joins the mesh. Single use,
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
VARIANTS.dAddMachine = [["Waiting", "waiting"], ["Search in ~/Projects", "search"], ["Linked", "linked"], ["Expired", "expired"], ["Making a token", "making"], ["No token", "tokenfail"]].map(([l, k]) => [l, () => { S.d.am = k; render(); }, () => (S.d.am || "waiting") === k]);
// Cmd+K: one search for projects, coding agents, machines, settings and actions.
D.dWinPalette = () => {
  const v = S.d.pal || "empty";
  const it = (ic, t, sm, w, on) => `<button class="it" role="option" aria-selected="${!!on}" data-toast="Opens ${t.replace(/<[^>]+>/g, "")}">${ic}<span class="t"><span>${t}</span>${sm ? `<small style="white-space:${v === "worktree bar" ? "normal" : "nowrap"}">${sm}</small>` : ""}</span>${w ? `<span class="w">${w}</span>` : ""}</button>`;
  let res;
  const PR_IC = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="6" r="2"/><circle cx="6" cy="18" r="2"/><circle cx="18" cy="18" r="2"/><path d="M6 8v8M18 16V9a2 2 0 00-2-2h-4"/></svg>';
  const AG_IC = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M12 15h5"/></svg>';
  if (v === "empty") res = `<h5>Actions</h5>${it(CC_IC.plus, "Start a coding agent", "", "", true)}${["Home", "Projects", "Terminals", "Settings", "Coding agents"].map(t => it(CC_IC.plus, t)).join("")}
      <h5>Coding agents</h5>${["Fix the flaky login test", "Review the sync retry change", "Bump the toolchain"].map(t => it(AG_IC, t)).join("")}`;
  // One ranked list, grouped by kind: the top hit first, then each group's best.
  else if (v === "worktree bar") res = `<h5>Coding agents</h5>${it(AG_IC, "Improve session search", "Prompt: Improve the <mark>worktree</mark> search <mark>bar</mark> · Add <mark>worktree</mark> support · Update the search <mark>bar</mark>")}`;
  else if (v === "mac") res = `<h5>Top hit</h5>${it(CC_IC.gear, "Let Beep use this <mark>Mac</mark>", "Settings · This computer", "", true)}
      <h5>Projects and code</h5>${it(PR_IC, "acme/widget · PR #45 Swipeable approvals on i<mark>Mac</mark>", "PR · yours · github.com")}${it(PR_IC, "acme/widget · beep/<mark>mac</mark>-approvals", "Branch · 2 ahead · Swipeable approvals")}
      <h5>Machines</h5>${it('<span class="ddot"></span>', "<mark>mac</mark>-studio", "", "online")}`;
  else if (v === "key") res = `<h5>Settings</h5>${it(CC_IC.gear, "<mark>GitHub</mark> token", "Settings · Credentials", "", true)}`;
  else if (v === "none") res = `<div class="none">Nothing matches “quinoa”.</div>`;
  else res = `<h5>Top hit</h5>${it(AG_IC, "<mark>Flaky</mark> login test", "acme/api · build-box", "", true)}
      <h5>Coding agents</h5>${it(AG_IC, "Retry uploads when the token expires", "…the <mark>flaky</mark> upload test on build-box…")}
      <h5>Projects and code</h5>${it(PR_IC, "acme/api · PR #41 Fix the <mark>flaky</mark> cache test", "PR · sam · github.com")}`;
  const q = v === "empty" ? `<span class="ph">Search projects, coding agents, machines, settings, PRs…</span>` : v === "none" ? "quinoa" : v === "mac" ? "mac" : v === "key" ? "github" : v === "worktree bar" ? "worktree bar" : "flaky";
  const pal = `<div class="cc-scrim" data-toast="Closes"><div class="cc-pal" role="dialog" aria-label="Search" onclick="event.stopPropagation()">
      <div class="q">${CC_IC.search}<span style="flex:1">${q}<span class="caret" style="height:17px"></span></span></div>
      <div class="res" role="listbox">${res}</div>
      <div class="foot"><span><kbd>↑↓</kbd>move</span><span><kbd>↵</kbd>open</span><span><kbd>esc</kbd>close</span></div></div></div>`;
  return D.dHome().replace(/<\/div>$/, pal + "</div>");
};
VARIANTS.dWinPalette = [["Empty", "empty"], ["Query", "query"], ["Multiple terms", "worktree bar"], ["Anything: “mac”", "mac"], ["A setting: “github”", "key"], ["No results", "none"]].map(([l, k]) => [l, () => { S.d.pal = k; render(); }, () => (S.d.pal || "empty") === k]);

DESK_SCREENS.find(g => g.group === "Command center").items.push(
  ["dWinSettings", "Settings overlay", "CommandCenterWindow, SettingsSheet"],
  ["dAddMachine", "Add a machine", "CommandCenterWindow, AddMachineSheet"],
  ["dAgentAccounts", "Agent accounts", "CommandCenterWindow, SettingsSheet, AgentAccounts"],
  ["dApiKeys", "Credentials", "CommandCenterWindow, SettingsSheet, Credentials"],
  ["dWinPalette", "Cmd+K search", "CommandPalette"]);
// Direct entries for two Settings sections, so they are easy to find in the list.
D.dAgentAccounts = () => { const keep = S.d.set; S.d.set = S.d.acc || "acct"; const html = D.dWinSettings(); S.d.set = keep; return html; };
D.dApiKeys = () => { const keep = S.d.set; S.d.set = S.d.keysv || "keys"; const html = D.dWinSettings(); S.d.set = keep; return html; };
VARIANTS.dApiKeys = [["Keys", "keys"], ["Adding Forgejo", "keys-gh"]].map(([l, k]) => [l, () => { S.d.keysv = k; render(); }, () => (S.d.keysv || "keys") === k]);
