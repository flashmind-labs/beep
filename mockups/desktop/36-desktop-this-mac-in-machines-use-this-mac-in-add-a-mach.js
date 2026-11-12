// ---- Desktop: This computer in Machines, Use this computer in Add a machine, the Projects page and Import a project ----
// Mirrors thisMacRow in home.js and center.js, macOption in addmachine-state.js, projectTable in center.js
// and importproject.js. This computer shows while the server does not list it; S.d.tm picks its state.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .cc-mt .mac-state { grid-column: span 5 }
.mac .cc-mt .mac-state.bad, .mac .cc-si .mac-state.bad { color: #f2a65f }
.mac .cc-si .mac-state { max-width: 45%; overflow: hidden; text-overflow: ellipsis }
.mac .cc-si .mac-add { margin-left: auto; min-height: 18px; padding: 0 6px; font-size: 11px }
.mac .cc-sec .pl-filter { margin-left: auto; height: 26px; width: 220px; padding: 0 10px; border-radius: 6px; border: 1px solid var(--d-border); background: transparent; color: var(--d-fg); font: inherit; font-size: 12.5px }
.mac .pl { border: 1px solid var(--d-border); border-radius: 8px; overflow: hidden }
.mac .pl-row { all: unset; box-sizing: border-box; display: grid; grid-template-columns: minmax(180px, 1.4fr) 110px minmax(0, 2fr) 60px 36px 70px; gap: 12px; align-items: center; width: 100%; min-height: 40px; padding: 0 14px; border-top: 1px solid var(--d-border); font-size: 13px; cursor: pointer }
.mac .pl-row:hover { background: color-mix(in srgb, var(--d-fg) 4%, transparent) }
.mac .pl-head { min-height: 32px; border-top: 0; cursor: default; color: var(--d-muted); font-size: 11.5px }
.mac .pl-head:hover { background: none }
.mac .pl-grp { padding: 10px 14px 4px; border-top: 1px solid var(--d-border); font-size: 11px; color: var(--d-muted); text-transform: uppercase; letter-spacing: .04em }
.mac .pl-name { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pl-owner { color: var(--d-muted) }
.mac .pl-name b { font-weight: 500 }
.mac .pl-branch { font-family: var(--d-mono, ui-monospace, monospace); font-size: 11.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pl-mach { display: flex; gap: 6px; flex-wrap: nowrap; overflow: hidden; min-width: 0 }
.mac .pl-chip { display: inline-flex; align-items: center; gap: 5px; padding: 1px 8px; border: 1px solid var(--d-border); border-radius: 999px; font-size: 11.5px; white-space: nowrap }
.mac .pl-chip .ddot { width: 6px; height: 6px }
.mac .ddot.warn { background: #e0a43a }
.mac .pl-ag { display: flex; align-items: center; gap: 6px; color: var(--d-muted) }
.mac .pl-ci { width: 8px; height: 8px; border-radius: 50%; justify-self: center }
.mac .pl-ci.ok { background: var(--d-emerald) } .mac .pl-ci.bad { background: var(--d-err) } .mac .pl-ci.busy { background: #e0a43a } .mac .pl-ci.none { border: 1px solid var(--d-border) }
.mac .pl-last { color: var(--d-muted); text-align: right; font-size: 12px }
.mac .cc-mt.cc-pt { grid-template-columns: minmax(140px, 1fr) minmax(0, 2fr) 60px 100px }
.mac .cc-mt.cc-pt .nm b button { all: unset; cursor: pointer; max-width: 100%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-mt.cc-pt .nm b button:hover { color: var(--d-emerald) }
.mac .cc-mt.cc-pt .nm .os b { font-weight: 500; color: var(--d-fg); font-size: 10.5px }
.mac .cc-top .tools { margin-left: auto; display: flex; gap: 8px }
.ip-field { flex: 1; min-width: 0; padding: 8px 12px; border-radius: 8px; border: 1px solid var(--d-border); background: #0f1011; font-size: 13px; display: flex; align-items: center; white-space: nowrap; overflow: hidden }
.ip-field.focus { border-color: var(--d-emerald) }
.ip-field .ph { color: var(--d-dim) }
.ip-path { flex: 1; min-width: 0; padding: 7px 11px; border-radius: 8px; border: 1px solid var(--d-border); background: #0f1011; font-size: 12.5px; font-family: ui-monospace, Menlo, monospace; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.ip-path.dim { color: var(--d-muted); font-family: inherit }
.ip-browse { border: 1px solid var(--d-border); border-radius: 8px; background: #0f1011; margin-bottom: 14px; overflow: hidden }
.ip-browse .at { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-bottom: 1px solid var(--d-border); font-size: 12.5px; font-family: ui-monospace, Menlo, monospace }
.ip-browse .at span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.ip-browse .dirs { max-height: 168px; overflow: auto; padding: 4px 0 }
.ip-browse .dirs button { all: unset; box-sizing: border-box; display: flex; gap: 8px; width: 100%; padding: 5px 12px; font-size: 13px; cursor: pointer }
.ip-browse .dirs button:hover { background: rgba(255,255,255,.04) }
.ip-browse .dirs .git { margin-left: auto; color: var(--d-emerald); font-size: 11.5px }
.ip-browse .foot { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-top: 1px solid var(--d-border); font-size: 12px; color: var(--d-muted) }
.ip-found { display: flex; gap: 8px; align-items: center; font-size: 12.5px; margin: -6px 0 14px }
.mac .ip-sheet { box-sizing: border-box; width: min(560px, 86%); max-height: calc(100% - 48px); overflow: auto; padding: 18px 20px; border-radius: 8px; border: 1px solid var(--d-border); background: #151617; box-shadow: 0 20px 60px rgba(0,0,0,.5) }
.mac .ip-foot { position: sticky; bottom: -18px; display: flex; gap: 8px; align-items: center; margin: 12px -20px -18px; padding: 12px 20px 18px; background: #151617; border-top: 1px solid var(--d-border) }
.mac .ip-to { font-size: 12.5px; color: var(--d-muted); margin: -6px 0 8px; font-family: inherit }
.mac .ip-sub { font-size: 12.5px; color: var(--d-muted); margin: 8px 0 14px }
.mac .ip-label { font-size: 13px; color: var(--d-muted); margin-bottom: 8px }
.mac .ip-wait { display: flex; gap: 8px; align-items: center; font-size: 12.5px; color: var(--d-muted) }
.mac .ip-branches { display: flex; flex-direction: column; gap: 2px }
.mac .ip-branch { display: flex; flex-wrap: wrap; gap: 4px 8px; align-items: center; padding: 5px 0; font-size: 12.5px; border-bottom: 1px solid var(--d-border) }
.mac .ip-branch:last-child { border-bottom: 0 }
.mac .ip-branch code { font-family: var(--mono) }
.mac .ip-branch .m { flex: 1; min-width: 0; color: var(--d-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .ip-branch .lbl { margin-left: auto; color: var(--d-muted); font-size: 11.5px }
.mac .ip-branch .seg { display: inline-flex; border: 1px solid var(--d-border); border-radius: 7px; overflow: hidden }
.mac .ip-branch .seg button { all: unset; cursor: pointer; padding: 3px 9px; font-size: 12px }
.mac .ip-branch .seg button + button { border-left: 1px solid var(--d-border) }
.mac .ip-branch .seg button[aria-pressed=true] { background: rgba(94,106,210,.22); color: var(--d-fg) }`;
  document.head.append(st);
})();

const TM = {
  off: { state: "Off", title: "Let Beep use this computer is off. Add turns it on." },
  connecting: { state: "Connecting", title: "Joining your machines" },
  notallowed: { state: "This account can't add machines on this server", title: "This account can't add machines on this server. Beep tries again on its own.", bad: true },
  failed: { state: "Could not reach the server", title: "Could not reach the server. Beep tries again on its own.", bad: true },
};
// Only on Home with machines listed, and while the server does not list this computer.
const tmRow = () => S.d.cc === "empty" || S.d.cc === "old" ? null : TM[S.d.tm] || null;
const tmCells = () => {
  const r = tmRow();
  if (!r) return "";
  return `<span class="nm off" id="m-this-mac"><b><span class="ddot off" style="margin-right:6px"></span>This computer</b><span>MacBook Air</span></span>
      <span class="caps mac-state${r.bad ? " bad" : ""}" title="${r.title}">${r.state}</span>
      <span class="num">${S.d.tm === "off" ? `<button class="dbtn pri" style="padding:2px 10px;font-size:12px;min-height:0" data-dact="dset:tm:connecting">Add</button>` : "–"}</span>`;
};
const tmItem = () => {
  const r = tmRow();
  if (!r) return "";
  return S.d.tm === "off"
    ? `<div class="cc-si" title="This computer: ${r.title}"><span class="ddot off"></span><span class="tt">This computer</span><button class="dbtn quiet mac-add" data-dact="dset:tm:connecting">Add</button></div>`
    : `<button class="cc-si" title="This computer: ${r.title}"><span class="ddot off"></span><span class="tt">This computer</span><span class="n mac-state${r.bad ? " bad" : ""}">${r.state}</span></button>`;
};
VARIANTS.dHome.push(...[["This computer off", "off"], ["This computer connecting", "connecting"], ["This computer not allowed", "notallowed"], ["This computer failed", "failed"]]
  .map(([l, k]) => [l, () => { S.d.tm = k; S.d.mrm = ""; S.d.cc = "default"; S.d.htab = "machines"; render(); }, () => S.d.tm === k]));

// Add a machine, first option: this computer in one click, then the steps for another machine.
const amMac = () => {
  const k = S.d.amm || "off";
  if (k === "none") return "";
  const wrap = x => `<div style="padding-bottom:14px;margin-bottom:14px;border-bottom:1px solid var(--d-border)"><div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">This computer</div>${x}</div>
      <div style="font-size:13px;font-weight:500;margin-bottom:8px">Another machine</div>`;
  if (k === "connecting") return wrap(`<div style="display:flex;gap:8px;align-items:center;font-size:12.5px;color:var(--d-muted)"><span class="dspin"></span>Connecting MacBook Air</div>`);
  if (k === "linked") return wrap(`<div style="display:flex;gap:8px;align-items:center;font-size:13px"><span class="ddot"></span>MacBook Air is linked. It shows under Machines.</div>`);
  if (k === "notallowed") return wrap(`<div style="font-size:13px;color:var(--d-err)">Couldn't connect: this account can't add machines on this server. Beep tries again on its own.</div>`);
  if (k === "failed") return wrap(`<div style="font-size:13px;color:var(--d-err)">Couldn't connect: could not reach the server. Beep tries again on its own.</div>`);
  return wrap(`<div style="display:flex;gap:8px;align-items:center"><div style="flex:1;min-width:0;font-size:13px">Let Beep run commands and use the screen on MacBook Air.</div><button class="dbtn pri" data-dact="dset:amm:connecting">Use this computer</button></div>`);
};
VARIANTS.dAddMachine.push(...[["This computer: Use this computer", "off"], ["This computer: connecting", "connecting"], ["This computer: linked", "linked"], ["This computer: not allowed", "notallowed"], ["This computer: failed", "failed"], ["This computer: already a machine", "none"]]
  .map(([l, k]) => [l, () => { S.d.amm = k; S.d.am = "waiting"; render(); }, () => (S.d.amm || "off") === k]));

// Projects page: imported repositories and the folders agents run in, one row per project.
const PP = [
  { name: "acme/widget", host: "github.com", at: [["build-box", "/srv/projects/acme/widget", "ok"], ["MacBook Air", "~/Projects/acme/widget", "ok"]], agents: 2, ci: "fail", branch: "main", last: "6 min" },
  { name: "acme/relay", host: "github.com", at: [["build-box", "/srv/projects/acme/relay", "ok"], ["MacBook Air", "~/Projects/acme/relay", "sync"]], agents: 1, ci: "run", branch: "signup", last: "31 min" },
  { name: "acme/site", host: "github.com", at: [["build-box", "/srv/projects/acme/site", "ok"], ["gpu-box", "/srv/projects/acme/site", "ok"], ["MacBook Air", "~/Projects/acme/site", "ok"]], agents: 0, ci: "ok", branch: "main", last: "2 h" },
  { name: "northwind/pines", host: "git.example.com", at: [["gpu-box", "/srv/projects/northwind/pines", "off"]], agents: 0, ci: "", branch: "main", last: "3 d" },
  { name: "northwind/docs", host: "git.example.com", at: [["build-box", "/srv/projects/northwind/docs", "ok"], ["MacBook Air", "~/Projects/northwind/docs", "conflict"]], agents: 0, ci: "ok", branch: "main", last: "5 d" },
];
const ppSide = () => ccSidePj("projects");
D.dProjectsPage = () => {
  const v = S.d.pp || "list", one = v === "one", empty = v === "empty";
  const q = (S.d.ppq || "").toLowerCase();
  const rows = (one ? PP.slice(0, 1) : PP).filter(p => !q || p.name.includes(q));
  const SYNC = { ok: ["", "In step"], sync: ["busy", "Syncing"], conflict: ["warn", "Conflict"], off: ["off", "Offline"] };
  const CI = { ok: ["ok", "Checks passed"], fail: ["bad", "Checks failed"], run: ["busy", "Checks running"] };
  const row = p => {
    const [owner, repo] = p.name.split("/");
    const chips = p.at.map(([m, f, st]) => `<span class="pl-chip" title="${m}: ${f} · ${SYNC[st][1]}"><span class="ddot ${SYNC[st][0]}"></span>${m}</span>`).join("");
    const ci = p.ci ? `<span class="pl-ci ${CI[p.ci][0]}" title="${CI[p.ci][1]}"></span>` : `<span class="pl-ci none" title="No checks"></span>`;
    return `<button type="button" class="pl-row" data-dact="dset:pp:one" data-dgo="dProjectsPage">
      <span class="pl-name"><span class="pl-owner">${owner}/</span><b>${repo}</b></span>
      <span class="pl-branch">${p.branch}</span>
      <span class="pl-mach">${chips}</span>
      <span class="pl-ag">${p.agents ? `<span class="ddot busy"></span>${p.agents}` : ""}</span>
      ${ci}
      <span class="pl-last">${p.last}</span></button>`;
  };
  const active = rows.filter(p => p.agents), rest = rows.filter(p => !p.agents);
  const filter = `<input class="pl-filter" placeholder="Filter projects" value="${S.d.ppq || ""}" aria-label="Filter projects">`;
  const head = `<div class="pl-row pl-head"><span>Project</span><span>Branch</span><span>Machines</span><span>Agents</span><span>CI</span><span>Activity</span></div>`;
  const table = `<div class="pl">${head}${active.length ? `<div class="pl-grp">Working now</div>${active.map(row).join("")}` : ""}${rest.length ? `<div class="pl-grp">Others</div>${rest.map(row).join("")}` : ""}</div>`;
  const imp = `<button class="dbtn${empty ? " pri" : ""}" style="padding:3px 10px;font-size:12px" data-dgo="dAddProject">${CC_IC.plus} Add a project</button>`;
  const body = empty
    ? `<div class="cc-panel cc-empty"><b>No projects yet</b><span>A folder or a repository to clone.</span>${imp.replace('padding:3px 10px;font-size:12px', '')}</div>`
    : `${one && window.__pjOne ? window.__pjOne() : `<div class="cc-sec"><h4>Projects<span class="n">${PP.length}</span></h4>${filter}</div>${table}${apFoundSection()}`}`;
  const tools = `<div class="tools">${one ? `<button class="dbtn small" data-toast="Opens it on GitHub">GitHub</button><button class="dbtn quiet small" aria-label="More" data-dact="dset:pp:list" data-dgo="dProjectsPage">⋯</button>` : empty ? "" : imp.replace('style="padding:3px 10px;font-size:12px"', "").replace('class="dbtn', 'class="dbtn small')}</div>`;
  return ccWin(ppSide(), `<div class="cc-top"><h2>${one ? PP[0].name : "Projects"}</h2><small>${one ? "github.com · imported 3 d" : "Repositories on your machines"}</small>${tools}</div><div class="cc-view">${body}</div>`);
};
VARIANTS.dProjectsPage = [["Projects", "list"], ["One project", "one"], ["None yet", "empty"]].map(([l, k]) => [l, () => { S.d.pp = k; render(); }, () => (S.d.pp || "list") === k]);

// Import a project: a field for org/repo or a URL, a chip per online machine (Auto lets the server pick), then Import.
D.dImportProject = () => {
  const v = S.d.ip || "form";
  const typed = v !== "form", repo = "acme/widget", mac = v === "checkout";
  const field = typed ? `<span>${repo}</span>` : `<span class="ph">org/repo or https://github.com/org/repo</span>`;
  const picked = !typed ? 0 : mac ? 2 : 1;
  const chips = ["Auto", "build-box", "MacBook Air"].map((c, i) => `<button class="dchip" aria-pressed="${i === picked}" data-toast="${c === "Auto" ? "The server picks a machine" : c}">${c}</button>`).join("");
  // Folder: the parent the clone goes under as <parent>/<org>/<repo>, the machine's Projects in folder
  // until edited or Choose… picks one. This computer opens the macOS folder picker; another machine
  // opens the browser below, listing its folders from its home.
  const folder = !typed ? `<div class="ip-path dim">The default folder on the machine Beep picks</div>`
    : mac ? `<div class="ip-path" title="~/Documents/code/widget">~/Documents/code/widget</div>`
    : `<div class="ip-path" title="/srv/code">/srv/code</div>`;
  const choose = `<button class="dbtn quiet"${!typed ? ' disabled style="opacity:.5"' : mac ? ' data-toast="Opens the macOS folder picker"' : ' data-dact="dset:ip:browse"'}>Choose…</button>${mac ? `<button class="dbtn quiet" data-dact="dset:ip:typed">Default</button>` : ""}`;
  const browse = `<div class="ip-browse">
      <div class="at"><button class="dbtn quiet" style="padding:2px 8px;font-size:12px;min-height:0" data-toast="Up to /srv">Up</button><span title="/srv/projects">/srv/projects</span></div>
      <div class="dirs">${["acme", "northwind", "notes", "scratch"].map(d => `<button type="button" data-toast="Opens /srv/projects/${d}">${d}</button>`).join("")}</div>
      <div class="foot"><span style="flex:1;min-width:0">Has other files: it goes in /srv/projects/widget.</span><button class="dbtn quiet" style="padding:3px 10px;font-size:12px" data-dact="dset:ip:typed">Cancel</button><button class="dbtn pri" style="padding:3px 10px;font-size:12px" data-dact="dset:ip:typed">Use this folder</button></div>
    </div>`;
  // The checkouts found and the branches with work, each with where its worktree goes or Skip.
  // They sit in the sheet itself, which scrolls whole: no list of their own.
  const BRANCH_ROWS = [["main", "default branch"], ["fix-retry", "PR #52 Retry on a dropped connection · 3 ahead"], ["login-copy", "PR #41 Login screen copy · 2 ahead"], ["bump-gradle", "PR #38 Bump Gradle · 1 ahead · not in the checkout"], ["cache-warmup", "4 ahead"], ["docs-sync", "2 ahead · not in the checkout"], ["ios-sheet", "PR #36 Sheet on iOS · 6 ahead"], ["release-notes", "1 ahead"]];
  const manyBranches = v === "branches" ? [...BRANCH_ROWS, ...Array.from({ length: 26 }, (_, i) => [`feature-${i + 1}`, `${i % 5 + 1} ahead${i % 3 ? "" : " · not in the checkout"}`])] : BRANCH_ROWS;
  const branchRow = ([n, m], i) => `<div class="ip-branch" title="${n}  ${m}"><code>${n}</code><span class="m">${m}</span>${n === "main" ? "" : `<span class="lbl">Worktree</span><span class="seg"><button aria-pressed="${i < 3}" data-toast="A worktree on build-box">build-box</button><button aria-pressed="${i >= 3}" data-toast="No worktree">Skip</button></span>`}</div>`;
  const branches = v === "reading" ? `<div class="ip-label">Branches</div><div class="ip-wait" style="margin-bottom:14px"><span class="dspin"></span>Looking for checkouts and branches…</div>`
    : ["typed", "branches", "complete"].includes(v) ? `<div class="ip-label">Branches</div><div class="ip-sub">No checkout on your online machines yet; importing clones it.</div><div class="ip-branches">${manyBranches.map(branchRow).join("")}</div>` : "";
  const help = !typed ? "Beep picks a machine and its default folder. Private repositories use your tokens from Settings &gt; Credentials."
    : mac ? "Private repositories use your tokens from Settings &gt; Credentials."
    : "The default on build-box. Private repositories use your tokens from Settings &gt; Credentials.";
  const form = `<div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">Repository</div>
    <div style="display:flex;gap:8px;align-items:center;margin-bottom:14px"><div class="ip-field${mac ? "" : " focus"}">${field}${typed && !mac ? '<span class="caret"></span>' : ""}</div></div>
    <div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">Machine</div>
    <div style="display:flex;gap:8px;align-items:center;margin-bottom:14px"><div class="dchips">${chips}</div></div>
    <div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">${mac ? "Folder" : "Clone into"}</div>
    ${v === "browse" ? browse : `<div style="display:flex;gap:8px;align-items:center;margin-bottom:14px">${folder}${choose}</div>`}
    ${mac ? `<div class="ip-found"><span class="ddot"></span><span>A checkout of acme/widget, imported in place.</span></div>` : ""}
    ${v === "fork" ? `<div class="ip-found" style="font-size:12px;color:var(--d-muted);margin-bottom:8px"><span class="ddot"></span><span>Your fork octo/widget will be cloned, upstream added. Your PR branch fix-retry is made local.</span></div>` : ""}
    ${typed && !mac && v !== "browse" ? '<div class="ip-to">Goes in /srv/code/acme/widget.</div>' : ""}
    <div style="font-size:12.5px;color:var(--d-muted);margin:0 0 14px">${help}</div>
    ${branches}
    ${v === "error" ? `<div style="font-size:13px;color:var(--d-err);margin-bottom:12px">Repository not found, or your tokens cannot read it.</div>` : ""}
    <div class="ip-foot"><button class="dbtn pri"${typed ? ' data-dact="dset:ip:cloning"' : " disabled"} style="${typed ? "" : "opacity:.5"}">Import</button><button class="dbtn quiet" data-dgo="dProjectsPage">Cancel</button></div>`;
  const cloning = `<div style="display:flex;gap:8px;align-items:center;font-size:12.5px;color:var(--d-muted)"><span class="dspin"></span>Cloning ${repo} onto build-box…</div>
    <div style="font-size:12.5px;color:var(--d-muted);margin:8px 0 14px">A large repository takes a few minutes. You can close this; the project shows up once it is in.</div>
    <div style="display:flex"><span style="margin-left:auto"><button class="dbtn quiet" data-dact="dset:pp:list" data-dgo="dProjectsPage">Close</button></span></div>`;
  const done = (there) => `<div style="display:flex;gap:10px;align-items:center;font-size:14px"><span class="ddot"></span><b>${repo} imported</b></div>
    <div style="font-size:12.5px;color:var(--d-muted);margin:8px 0 14px">${there ? "Already there, now listed" : "Cloned"} on build-box in /srv/code/acme/widget</div>
    <button class="dbtn pri" data-dact="dset:pp:one" data-dgo="dProjectsPage">Open project</button>`;
  const body = v === "cloning" ? cloning : v === "done" ? done(false) : v === "there" ? done(true) : form;
  const sheet = `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5">
    <div class="am-sheet ip-sheet" role="dialog" aria-label="Import a project">
      <div style="display:flex;justify-content:space-between;margin-bottom:12px"><b style="font-size:15px">Import a project</b><kbd style="font-size:11px;opacity:.7">esc</kbd></div>${body}</div></div>`;
  S.d.pp = S.d.pp || "list";
  return D.dProjectsPage().replace(/<\/div><\/div>\s*$/, sheet + "</div></div>");
};
VARIANTS.dImportProject = [["Empty", "form"], ["Filled in", "typed"], ["Reading branches", "reading"], ["Many branches, the sheet scrolls", "branches"], ["Choosing a folder on build-box", "browse"], ["Existing checkout on this computer", "checkout"], ["Your fork", "fork"], ["Cloning", "cloning"], ["Done", "done"], ["Done, already there", "there"], ["Refused", "error"]]
  .map(([l, k]) => [l, () => { S.d.ip = k; S.d.pp = "list"; render(); }, () => (S.d.ip || "form") === k]);
DESK_SCREENS.find(g => g.group === "Command center").items.push(
  ["dProjectsPage", "Projects page, imported repositories", "CommandCenterWindow, projectTable"],
  ["dImportProject", "Import a project", "ImportProjectSheet"]);
