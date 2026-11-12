// ---- Desktop: Add a project ----
// One sheet for every way in: folders your coding agents worked in (Claude Code, Codex, pi and flsh
// sessions each node finds), a folder you pick, or a repository to clone. Already imported folders are
// left out. Importing a found folder links it in place and brings its sessions along; the same remote on
// another machine is linked too, and a machine without it gets a clone plus the synced ignored files.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .ap-row { display: grid; grid-template-columns: minmax(0,1fr) auto; gap: 4px 12px; align-items: center; padding: 9px 0; border-top: 1px solid var(--d-border) }
.mac .ap-row:first-child { border-top: 0 }
.mac .ap-row .n { font-size: 13.5px; font-weight: 600; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .ap-row .n small { font-weight: 400; color: var(--d-muted); font-size: 12px; margin-left: 6px }
.mac .ap-row .m { grid-column: 1; display: flex; flex-wrap: wrap; gap: 4px 10px; font-size: 12px; color: var(--d-muted); min-width: 0 }
.mac .ap-row .m code { font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100% }
.mac .ap-row .acts { grid-row: 1 / span 4; grid-column: 2; display: flex; gap: 6px }
.mac .ap-ag { display: inline-flex; gap: 4px; align-items: center; padding: 0 6px; border-radius: 6px; border: 1px solid var(--d-border); font-size: 11px; line-height: 17px; color: var(--d-fg) }
.mac .ap-ways { display: grid; grid-template-columns: 1fr 1fr; gap: 8px }
.mac .ap-ways button { display: grid; gap: 2px; text-align: left; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--d-border); background: var(--d-elev); color: var(--d-fg); font: inherit; cursor: pointer }
.mac .ap-ways button span { font-size: 12px; color: var(--d-muted) }
.mac .ap-q { display: flex; align-items: center; gap: 8px; padding: 6px 10px; border-radius: 8px; border: 1px solid var(--d-border); background: var(--d-elev); font-size: 13px }
.mac .ap-q .ph { color: var(--d-dim) }
.mac .ap-q kbd { margin-left: auto; font-size: 11px; opacity: .7 }
.mac .ap-bar { display: flex; align-items: center; gap: 10px; font-size: 12.5px; color: var(--d-muted) }
.mac .ap-bar label { display: inline-flex; align-items: center; gap: 6px; cursor: pointer }
.mac .ap-bar .dbtn { margin-left: auto }
.mac .ap-row.ap-pick { grid-template-columns: 18px minmax(0,1fr) auto }
.mac .ap-row.ap-pick > input { grid-row: 1 / span 4; grid-column: 1; align-self: start; margin: 3px 0 0 }
.mac .ap-row.ap-pick .n, .mac .ap-row.ap-pick .m { grid-column: 2 }
.mac .ap-row.ap-pick .acts { grid-column: 3; align-items: center }
.mac .ap-row.ap-focus { outline: 1px solid var(--d-accent, #4cb782); outline-offset: 2px; border-radius: 6px }
.mac .ap-st { font-size: 12px; color: var(--d-muted); display: inline-flex; gap: 6px; align-items: center }
.mac .ap-st.ok { color: #4cb782 }
.mac .ap-st.bad { color: #eb5757; max-width: 220px; white-space: normal; text-align: right }
.mac .ap-rm { display: flex; align-items: center; gap: 10px; padding: 7px 0; border-top: 1px solid var(--d-border); font-size: 13px }
.mac .ap-rm:first-child { border-top: 0 }
.mac .ap-rm b { flex: 1; min-width: 0; font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .ap-rm span { font-size: 12px; color: var(--d-muted) }
.mac .ap-q .ph { display: none }
.mac .ap-sheet .am-label { margin: 18px 0 8px }
.mac .ap-sheet .am-head + .am-label { margin-top: 4px }
.mac .ap-q { padding: 8px 12px }
.mac .ap-bar { margin: 10px 0 }
.mac .ap-bar .dbtn { padding: 6px 12px }
.mac .ap-list { padding: 4px 14px }
.mac .ap-row { gap: 6px 14px; padding: 12px 0 }
.mac .ap-row .m { gap: 6px 12px }
.mac .ap-row.ap-pick { column-gap: 12px }
.mac .ap-row .acts { gap: 8px; padding-left: 4px }
.mac .ap-ag { padding: 1px 8px }
.mac .ap-rm { gap: 14px; padding: 9px 0 }
.mac .ap-remote .ap-list { max-height: 220px }
.mac .ap-ways { gap: 10px; margin-top: 18px; grid-template-columns: repeat(3, minmax(0, 1fr)) }
.mac .agmode.on { color: var(--d-fg); border-color: var(--d-emerald) }
.mac .ag-plan { display: grid; gap: 10px; margin: 10px 16px; padding: 12px 14px; border: 1px solid var(--d-border); border-radius: var(--radius); background: var(--d-surface) }
.mac .ag-plan .h { display: flex; gap: 8px; align-items: baseline }
.mac .ag-plan .h small { color: var(--d-muted); font-size: 12px }
.mac .ag-plan .acts { display: flex; gap: 8px }
.mac .ap-ways button { padding: 12px 14px }
.mac .ap-sheet { width: min(640px, 88vw) }
.mac .ap-sheet .am-sub { margin-top: 0 }
.mac .ap-list { border: 1px solid var(--d-border); border-radius: 10px; background: var(--d-elev); padding: 2px 12px; max-height: 300px; overflow: auto }
.mac .ap-row { display: grid; grid-template-columns: minmax(0,1fr) auto; gap: 4px 12px; align-items: center; padding: 9px 0; border-top: 1px solid var(--d-border) }
.mac .ap-row:first-child { border-top: 0 }
.mac .ap-row .n { font-size: 13.5px; font-weight: 600; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .ap-row .m { grid-column: 1; display: flex; flex-wrap: wrap; gap: 4px 10px; font-size: 12px; color: var(--d-muted); min-width: 0 }
.mac .ap-row .m > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .ap-row .m b { font-weight: 500; color: var(--d-fg) }
.mac .ap-row .m code { font-size: 11.5px }
.mac .ap-row .m.bad { color: var(--d-err); white-space: normal }
.mac .ap-row .acts { grid-row: 1 / span 4; grid-column: 2; display: flex; gap: 6px }
.mac .ap-ag { display: inline-flex; gap: 4px; align-items: center; padding: 0 6px; border-radius: 6px; border: 1px solid var(--d-border); font-size: 11px; line-height: 17px; color: var(--d-fg) }
.mac .ap-q { width: 100%; box-sizing: border-box; padding: 6px 10px; border-radius: 8px; border: 1px solid var(--d-border); background: rgba(255,255,255,.03); color: var(--d-fg); font: inherit; font-size: 13px }
.mac .ap-q[hidden], .mac .ap-bar[hidden] { display: none }
.mac .ap-bar { display: flex; flex: none; align-items: center; gap: 10px; min-height: 30px; margin-bottom: 6px; font-size: 12.5px; color: var(--d-muted) }
.mac .ap-bar label { display: inline-flex; align-items: center; gap: 6px; cursor: pointer }
.mac .ap-bar .dbtn { margin-left: auto; flex: none; display: inline-flex; align-items: center; white-space: nowrap }
.mac .ap-bar .dbtn kbd { margin-left: 6px; padding: 0 4px; font-size: 10.5px; line-height: 15px; border: 0; background: none; opacity: .7 }
.mac .ap-row.ap-pick { grid-template-columns: 18px minmax(0,1fr) auto }
.mac .ap-row.ap-pick > input { grid-row: 1 / span 4; grid-column: 1; align-self: start; margin: 3px 0 0 }
.mac .ap-row.ap-pick .n, .mac .ap-row.ap-pick .m { grid-column: 2 }
.mac .ap-row.ap-pick .acts { grid-column: 3; align-items: center }
.mac .ap-row:focus { outline: none }
.mac .ap-row:focus-visible { outline: 1px solid var(--d-emerald); outline-offset: 2px; border-radius: 6px }
.mac .ap-st { font-size: 12px; color: var(--d-muted); display: inline-flex; gap: 6px; align-items: center }
.mac .ap-st.ok { color: #4cb782 }
.mac .ap-sheet { max-height: calc(100vh - 80px); overflow: auto }
.mac .ap-remote .ap-list { max-height: 180px }
.mac .ap-rm { display: flex; align-items: center; gap: 10px; padding: 7px 0; border-top: 1px solid var(--d-border); font-size: 13px }
.mac .ap-rm:first-child { border-top: 0 }
.mac .ap-rm .n { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .ap-rm .m { font-size: 12px; color: var(--d-muted) }
.mac .ap-ways { display: grid; grid-template-columns: 1fr 1fr; gap: 8px }
.mac .ap-ways button { display: grid; gap: 2px; text-align: left; padding: 10px 12px; border-radius: 10px; border: 1px solid var(--d-border); background: rgba(255,255,255,.03); color: var(--d-fg); font: inherit; cursor: pointer }
.mac .ap-ways button > span { display: grid; gap: 2px }
.mac .ap-ways button span span { font-size: 12px; color: var(--d-muted) }
.mac .am-sheet { width: 560px; max-width: calc(100% - 40px); padding: 18px 20px; border-radius: var(--radius); border: 1px solid var(--d-border); background: var(--d-surface); box-shadow: 0 20px 60px color-mix(in srgb, #000 50%, transparent); color: var(--d-fg) }
.mac .am-head { display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px }
.mac .am-head b { font-size: 15px; font-weight: 600 }
.mac .am-head kbd { font-size: 11px; opacity: .7 }
.mac .am-label { font-size: 13px; color: var(--d-muted); margin-bottom: 8px }
.mac .am-wait { display: flex; gap: 8px; align-items: center; font-size: 12.5px; color: var(--d-muted) }
.mac .am-wait .left { font-variant-numeric: tabular-nums }
.mac .am-wait .end { margin-left: auto }
.mac .am-sub { font-size: 12.5px; color: var(--d-muted); margin: 8px 0 14px }
.mac .ap-q { display: block; margin-bottom: 8px }
.mac .ap-box { margin-bottom: 12px }
.mac .ap-box:empty { display: none }
.mac .ap-remote .ap-list { margin-bottom: 12px }
.mac .ap-ways button > span { color: var(--d-fg); font-size: inherit }
.mac .ap-ways button span span { color: var(--d-muted); font-size: 12px }
.mac .ap-rm .n { color: var(--d-fg); font-size: 13px }`;
  document.head.append(st);
})();
// What the nodes found: a folder per row, its remote when it is a git checkout, and the sessions per agent.
const AP_FOUND = [
  { name: "acme/app", on: [["MacBook Air", "~/Projects/acme/app"], ["build-box", "/path/to/acme/app"]], ag: [["Claude Code", 41], ["Codex", 6], ["flsh", 3]], last: "12 min" },
  { name: "acme/billing", on: [["MacBook Air", "~/Documents/work/billing"]], ag: [["Claude Code", 9], ["pi", 2]], last: "1 d" },
  { name: "tools-org/uploader", on: [["build-box", "/srv/uploader"]], ag: [["Codex", 4]], last: "3 d" },
];
// In the sheet a row has a checkbox and, while a batch imports, its state:
// pick = { on, focus, st: "importing" | "done" | "failed", why }.
const apAct = (f, compact, pick) => !pick || !pick.st ? `<button class="dbtn${compact ? "" : " pri"} small" title="Import ${f.name} with ${f.ag.reduce((s, [, n]) => s + n, 0)} sessions" data-toast="Imported ${f.name}">Import</button>`
  : pick.st === "importing" ? `<span class="ap-st"><span class="dspin"></span>Importing…</span>`
  : pick.st === "done" ? `<span class="ap-st ok">Imported</span>`
  : `<button class="dbtn pri small" data-toast="Importing ${f.name} again">Retry</button>`;
const apRow = (f, compact, pick) => `<div class="ap-row${pick ? " ap-pick" : ""}${pick?.focus ? " ap-focus" : ""}">${pick ? `<input class="dcheck" type="checkbox" aria-label="Select ${f.name}"${pick.on ? " checked" : ""}${pick.st ? " disabled" : ""}>` : ""}<span class="n">${f.name}${f.git === false ? "<small>no git, syncs in full</small>" : f.note ? `<small>${f.note}</small>` : ""}</span>
    ${f.on.map(([m, p]) => `<span class="m"><span title="${m}: ${p}" style="min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap"><b style="font-weight:500;color:var(--d-fg)">${m}</b> <code>${p}</code></span></span>`).join("")}
    <span class="m">${compact ? "" : f.ag.map(([a, n]) => `<span class="ap-ag">${a} ${n}</span>`).join("")}<span>${f.last}</span></span>
    ${pick?.st === "failed" ? `<span class="m bad">Failed: ${pick.why}</span>` : ""}<span class="acts">${apAct(f, compact, pick)}</span></div>`;
const apFoundSection = () => `<div id="sec-found"><div class="cc-sec"><h4>Found on your machines</h4>${AP_FOUND.length > 3 ? `<button class="dbtn quiet small" data-dgo="dAddProject">All ${AP_FOUND.length}</button>` : ""}</div>
    <div class="cc-panel ap-list" style="padding:2px 12px;max-height:none;overflow:visible">${AP_FOUND.slice(0, 3).map(f => apRow(f, false)).join("")}</div></div>`;
// The search filters by repo, folder, machine and agent; typing goes there. Space ticks the focused row,
// ⌘↵ imports the ticked ones, three at a time; a failure leaves the rest going.
const AP_PICKS = {
  found: { q: "", picks: [{ focus: true }, {}, {}] },
  picked: { q: "", picks: [{ on: true }, { on: true, focus: true }, {}] },
  filter: { q: "acme", picks: [{}, { on: true, focus: true }, {}] },
  nomatch: { q: "zzz", picks: [{}, {}, {}] },
  importing: { q: "", picks: [{ on: true, st: "done" }, { on: true, st: "importing" }, { on: true, st: "failed", why: "build-box is offline" }] },
};
// The user's repositories on each git host that no machine has yet, newest update first, at most 30 per
// host. Read on a machine with gh or teacli and kept a few minutes. The filter applies here too.
const AP_REMOTE = [
  ["GitHub", [["acme/site", "2 h"], ["acme/docs", "1 d"], ["user/dotfiles", "4 d"], ["acme/sdk-swift", "14 d"]]],
  ["git.example.com", [["team/infra", "5 h"], ["team/handbook", "7 d"]]],
];
const apRemote = (q, v) => v === "looking" ? "" : AP_REMOTE.map(([host, repos]) => {
  const shown = repos.filter(([n]) => !q || n.toLowerCase().includes(q));
  if (!shown.length) return "";
  return `<div class="am-label">On ${host}</div><div class="ap-list">${shown.map(([n, at]) => `<div class="ap-rm"><span class="n">${n}</span><span class="m">${at}</span><button class="dbtn small" data-dgo="dImportProject">Clone</button></div>`).join("")}</div>`;
}).join("");
// A machine whose search failed or timed out, named instead of "No other folders yet.", and under the
// list when other machines answered.
const AP_UNREAD = "Not read on gpu-box: still looking for checkouts on this machine.";
const apMatches = (f, q) => !q || [f.name, ...f.on.flat(), ...f.ag.map(([a]) => a)].some(t => t.toLowerCase().includes(q));
D.dAddProject = () => {
  const v = S.d.ap || "found";
  const pk = AP_PICKS[v];
  const shown = pk ? AP_FOUND.map((f, i) => [f, pk.picks[i]]).filter(([f]) => apMatches(f, pk.q)) : [];
  const ticked = shown.filter(([, p]) => p.on).length, busy = shown.some(([, p]) => p.st === "importing");
  const all = shown.length > 0 && ticked === shown.length;
  const search = pk || v === "none" ? `<input class="ap-q" type="search" placeholder="Filter by repo, folder, machine or agent" aria-label="Filter found folders" value="${pk?.q || ""}">` : "";
  const q = (pk?.q || "").trim();
  const bar = pk ? `<div class="ap-bar"><label><input class="dcheck" type="checkbox"${all ? " checked" : ""}${busy ? " disabled" : ""}> Select all shown</label><span>${q ? `${shown.length} of ${AP_FOUND.length} shown` : `${AP_FOUND.length} found`}</span>
      <button class="dbtn pri small"${ticked && !busy ? "" : " disabled"} data-toast="Importing ${ticked} projects, three at a time">${busy ? "Importing…" : `Import ${ticked}`}<kbd>⌘↵</kbd></button></div>` : "";
  const found = pk ? `${search}${bar}<div class="ap-box">${shown.length ? `<div class="ap-list">${shown.map(([f, p]) => apRow(f, false, p)).join("")}</div>` : `<div class="am-sub">Nothing found matches “${q}”.</div>`}</div>`
    : v === "looking" ? `<div class="ap-box"><div class="am-wait"><span class="dspin"></span>Looking through Claude Code, Codex, pi, flsh and OpenCode sessions on 3 machines…</div></div>`
    : v === "failed" ? `<div class="ap-box"><div class="am-sub">${AP_UNREAD}</div></div>`
    : `${search}<div class="ap-box"><div class="am-sub">No other folders yet.</div></div>`;
  const ways = `<div class="ap-ways">
      <button class="dbtn small" data-dact="dset:af:mac" data-dgo="dAddFolder">Choose folder</button>
      <button class="dbtn small" data-dgo="dImportProject">Clone repository</button>
      <button class="dbtn small" data-dact="dset:np:form" data-dgo="dNewProject">New project</button></div>`;
  const sheet = `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:70px;justify-content:center;z-index:5">
    <div class="am-sheet ap-sheet" role="dialog" aria-label="Add a project" style="max-height:calc(100% - 90px)">
      <div class="am-head"><b>Add a project</b><kbd>esc</kbd></div>
      ${ways}<div class="am-label">On your machines</div>${found.replace('<div class="ap-box">', '<div class="ap-results"><div class="ap-box">')}
      <div class="ap-remote"><details${q ? " open" : ""}><summary>Browse repositories on connected hosts</summary>${apRemote(pk?.q || "", v)}</details></div></div><div class="ap-footer"></div>
      </div></div>`;
  S.d.pp = S.d.pp || "list";
  return D.dProjectsPage().replace(/<\/div><\/div>\s*$/, sheet + "</div></div>");
};
// Choose a folder: an existing checkout on a machine, added in place with no clone. This Mac is picked
// first; Choose… opens the macOS folder picker only when clicked, another machine opens the folder
// browser. What the folder is (checkout, origin, branch) shows before Add project.
D.dAddFolder = () => {
  const v = S.d.af || "mac";
  const mac = !["browse", "remote"].includes(v);
  const chips = ["MacBook Air", "build-box"].map((c, i) => `<button class="dchip" aria-pressed="${mac ? i === 0 : i === 1}" data-dact="dset:af:${i ? "browse" : "mac"}">${c}</button>`).join("");
  const path = { mac: "", checkout: "~/code/widget", plain: "~/Documents/notes", noorigin: "~/code/scratch", adding: "~/code/widget", done: "~/code/widget", remote: "/srv/projects/acme/api" }[v] || "";
  const field = path ? `<div class="ip-path" title="${path}">${path}</div>` : `<div class="ip-path dim">No folder chosen</div>`;
  const browse = `<div class="ip-browse">
      <div class="at"><button class="dbtn quiet" style="padding:2px 8px;font-size:12px;min-height:0" data-toast="Up to /srv">Up</button><span title="/srv/projects/acme">/srv/projects/acme</span></div>
      <div class="dirs">${["api", "site", "tools"].map(d => `<button type="button" data-toast="Opens /srv/projects/acme/${d}">${d}</button>`).join("")}</div>
      <div class="foot"><span style="flex:1;min-width:0">Not a git checkout.</span><button class="dbtn quiet" style="padding:3px 10px;font-size:12px" data-dact="dset:af:mac">Cancel</button><button class="dbtn pri" style="padding:3px 10px;font-size:12px" data-dact="dset:af:remote">Use this folder</button></div>
    </div>`;
  const what = {
    checkout: ["acme/widget", "github.com/acme/widget", "main"],
    remote: ["acme/api", "git.example.com/acme/api", "fix-retry"],
    adding: ["acme/widget", "github.com/acme/widget", "main"],
  }[v];
  const found = what ? `<div class="ip-found"><span class="ddot"></span><span>A checkout of <b>${what[0]}</b>, added in place.</span></div>
    <div class="am-sub" style="margin-bottom:14px">Remote <code>${what[1]}</code> · Branch <code>${what[2]}</code></div>`
    : v === "plain" ? `<div class="am-sub" style="margin-bottom:14px">Not a git checkout. Choose the folder that holds <code>.git</code>.</div>`
    : v === "noorigin" ? `<div class="am-sub" style="margin-bottom:14px">A checkout with no origin remote · Branch <code>main</code>. Add an origin remote to add it.</div>` : "";
  const can = !!what && v !== "adding";
  const form = `<div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">Machine</div>
    <div style="display:flex;gap:8px;align-items:center;margin-bottom:14px"><div class="dchips">${chips}</div></div>
    <div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">Folder</div>
    ${v === "browse" ? browse : `<div style="display:flex;gap:8px;align-items:center;margin-bottom:14px">${field}<button class="dbtn quiet"${mac ? ' data-dact="dset:af:checkout" data-toast="Opens the macOS folder picker"' : ' data-dact="dset:af:browse"'}>Choose…</button></div>`}
    ${found}
    <div class="ip-foot"><button class="dbtn pri"${can ? ' data-dact="dset:af:adding"' : ' disabled style="opacity:.5"'}>${v === "adding" ? "Adding…" : "Add project"}</button><button class="dbtn quiet" data-dgo="dAddProject">Back</button></div>`;
  const done = `<div style="display:flex;gap:10px;align-items:center;font-size:14px"><span class="ddot"></span><b>acme/widget added</b></div>
    <div style="font-size:12.5px;color:var(--d-muted);margin:8px 0 14px">In place on MacBook Air in ~/code/widget</div>
    <button class="dbtn pri" data-dact="dset:pp:one" data-dgo="dProjectsPage">Open project</button>`;
  const sheet = `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5">
    <div class="am-sheet ip-sheet" role="dialog" aria-label="Add a folder">
      <div style="display:flex;justify-content:space-between;margin-bottom:12px"><b style="font-size:15px">Add a folder</b><kbd style="font-size:11px;opacity:.7">esc</kbd></div>${v === "done" ? done : form}</div></div>`;
  S.d.pp = S.d.pp || "list";
  return D.dProjectsPage().replace(/<\/div><\/div>\s*$/, sheet + "</div></div>");
};
VARIANTS.dAddFolder = [["This Mac, nothing chosen", "mac"], ["A checkout on this Mac", "checkout"], ["Not a checkout", "plain"], ["No origin remote", "noorigin"], ["Browsing build-box", "browse"], ["A checkout on build-box", "remote"], ["Adding", "adding"], ["Added", "done"]]
  .map(([l, k]) => [l, () => { S.d.af = k; S.d.pp = "list"; render(); }, () => (S.d.af || "mac") === k]);
DESK_SCREENS.find(g => g.group === "Command center").items.push(["dAddFolder", "Add a folder", "AddFolderSheet"]);
// A new project: where its repository lives, which machines get a checkout, and the folder on each.
// A machine's folder starts at its Projects in folder, takes typing with the folder menu, and the
// project goes in <folder>/<name>.
D.dNewProject = () => {
  const v = S.d.np || "form", local = v === "local", typed = v !== "form";
  const hosts = [["GitHub", "acme"], ["forge.example.com", "acme"], ["Local only", ""]];
  const host = hosts.map(([n], i) => `<button class="dbtn dchip" aria-pressed="${local ? i === 2 : i === 1}" data-dact="dset:np:${i === 2 ? "local" : "typed"}">${n}</button>`).join("");
  const chips = [["build-box", true], ["studio", !local], ["MacBook Air", false]].map(([n, on]) => `<button class="dbtn dchip" aria-pressed="${on}" data-toast="${n}">${n}</button>`).join("");
  const label = t => `<div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">${t}</div>`;
  const row = x => `<div style="display:flex;gap:8px;align-items:center;margin-bottom:14px">${x}</div>`;
  const form = `${label("Name")}${row(`<div class="ip-field focus">${typed ? "<span>widget</span>" : '<span class="ph">my-project</span>'}</div>`)}
    <div class="am-sub">Create with a connected GitHub, Forgejo or Gitea account, or keep it local.</div>${label("Repository on")}${row(`<div class="dchips">${host}</div>`)}
    ${local ? "" : `${label("Owner and visibility")}${row(`<button class="dbtn" data-toast="Your account or an organization">acme</button><div class="dchips"><button class="dbtn dchip" aria-pressed="true" data-toast="Private">Private</button><button class="dbtn dchip" aria-pressed="false" data-toast="Public">Public</button></div>`)}`}
    ${label("Machines")}${row(`<div class="dchips">${chips}</div>`)}
    ${label("Folders")}${(local ? [["build-box", "~/code"]] : [["build-box", "~/code"], ["studio", "~/Projects"]]).map(([m, f]) => row(`<span class="np-m">${m}</span><div class="ip-path" title="${f}">${f}</div><span class="np-to">/${typed ? "widget" : "my-project"}</span>`)).join("")}
    ${typed && !local ? `<div class="ip-to">Each machine clones it and keeps it in sync.</div>` : ""}
    ${v === "error" ? `<div style="font-size:13px;color:var(--d-err);margin-bottom:12px">acme/widget already exists on forge.example.com.</div>` : ""}
    <div class="ip-foot"><button class="dbtn pri"${typed ? ' data-dact="dset:np:creating"' : " disabled"} style="${typed ? "" : "opacity:.5"}">Create</button><button class="dbtn quiet" data-dgo="dAddProject">Back</button></div>`;
  const creating = `<div style="display:flex;gap:8px;align-items:center;font-size:12.5px;color:var(--d-muted)"><span class="dspin"></span>Creating acme/widget, then cloning onto build-box and studio…</div>`;
  const done = `<div style="display:flex;gap:10px;align-items:center;font-size:14px"><span class="ddot"></span><b>acme/widget created</b></div>
    <div style="font-size:12.5px;color:var(--d-muted);margin:8px 0 14px">On build-box in ~/code/widget</div><div style="font-size:12.5px;color:var(--d-muted);margin:0 0 14px">On studio in ~/Projects/widget</div><button class="dbtn pri" data-dact="dset:pp:one" data-dgo="dProjectsPage">Open project</button>`;
  const sheet = `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5">
    <div class="am-sheet ip-sheet np-sheet" role="dialog" aria-label="New project">
      <div style="display:flex;justify-content:space-between;margin-bottom:12px"><b style="font-size:15px">New project</b><kbd style="font-size:11px;opacity:.7">esc</kbd></div>${v === "creating" ? creating : v === "done" ? done : form}</div></div>`;
  S.d.pp = S.d.pp || "list";
  return D.dProjectsPage().replace(/<\/div><\/div>\s*$/, sheet + "</div></div>");
};
VARIANTS.dNewProject = [["Empty", "form"], ["Filled in", "typed"], ["Local only", "local"], ["Name taken", "error"], ["Creating", "creating"], ["Created", "done"]]
  .map(([l, k]) => [l, () => { S.d.np = k; S.d.pp = "list"; render(); }, () => (S.d.np || "form") === k]);
DESK_SCREENS.find(g => g.group === "Command center").items.push(["dNewProject", "New project", "NewProjectSheet"]);
VARIANTS.dAddProject = [["Found folders", "found"], ["Two ticked", "picked"], ["Filtered", "filter"], ["No match", "nomatch"], ["Importing several", "importing"], ["Still looking", "looking"], ["Nothing found", "none"], ["A machine could not look", "failed"]]
  .map(([l, k]) => [l, () => { S.d.ap = k; S.d.pp = "list"; render(); }, () => (S.d.ap || "found") === k]);
DESK_SCREENS.find(g => g.group === "Command center").items.push(["dAddProject", "Add a project", "AddProjectSheet"]);
// The sheet opens on the list each machine named last; every online machine looks again at once
// (a quick, shallow walk of its search folder) and its rows come in as it answers. A hint names the
// machines still looking and goes when they are done.
{
  const base = D.dAddProject;
  D.dAddProject = (...a) => {
    if (S.d.ap !== "rescan") return base(...a);
    S.d.ap = "found";
    const html = base(...a);
    S.d.ap = "rescan";
    return html.replace('<div class="ap-box">', '<div class="am-wait ap-look"><span class="dspin"></span>Looking on gpu-box, build-box…</div><div class="ap-box">');
  };
  VARIANTS.dAddProject.push(["Looking again on two machines", () => { S.d.ap = "rescan"; S.d.pp = "list"; render(); }, () => S.d.ap === "rescan"]);
}
// Persistent terminals per project: a PTY on the node that outlives the window and the app; any device can reattach or watch.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .tm-tabs { display: flex; gap: 4px; padding: 0 0 8px; flex-wrap: wrap }
.mac .tm-tab { padding: 4px 10px; border-radius: 7px; border: 1px solid var(--d-border); background: transparent; color: var(--d-muted); font-size: 12px; display: inline-flex; gap: 6px; align-items: center }
.mac .tm-tab[aria-current="true"] { background: var(--d-elev); color: var(--d-fg) }
.mac .tm-pane { flex: 1; min-height: 0; border-radius: 8px; border: 1px solid var(--d-border); background: #0F1011; padding: 10px 12px; font-family: var(--mono, ui-monospace, monospace); font-size: 12px; line-height: 17px; color: #E3E4E6; white-space: pre; overflow: hidden }
.mac .tm-pane .g { color: #4CB782 } .mac .tm-pane .d { color: #8A8F98 } .mac .tm-pane .y { color: #F2C94C }
.mac .cc-top h2, .mac .cc-top .dbtn { white-space: nowrap }
.mac .cc-top small { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0 }
.mac .tm-bar { display: flex; gap: 8px; align-items: center; font-size: 11.5px; color: var(--d-muted); padding-top: 8px }`;
  document.head.append(st);
})();
const TM_OUT = {
  "cargo watch": `[Running 'cargo check']
    Checking api v0.4.0 (/srv/project/api)
    Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 1.84s
<span class="g">[Finished running. Exit status: 0]</span>
<span class="d">[Waiting for changes]</span>
`,
  "tail -f server.log": `<span class="d">10:42:07</span> GET /health 200 2ms
<span class="d">10:42:37</span> GET /health 200 1ms
<span class="d">10:43:02</span> <span class="y">slow query</span> 412ms
<span class="d">10:43:07</span> GET /health 200 2ms
`,
  "npm run dev": `
  VITE v6.2.0  ready in 412 ms

  Local:   http://localhost:5173/

<span class="d">10:51:30</span> page reload src/login/copy.ts
<span class="d">10:58:12</span> server stopped
`,
};
// linkLabel in terminals.ts.
const tmLabel = (link, machine, o) => link === "attaching" ? (o.tries ? `Reconnecting to ${machine}…` : "Attaching…")
  : link === "attached" ? (o.watching ? "Watching: typing is off" : `Attached: what you type goes to ${machine}`)
  : link === "detached" ? "Detached: it keeps running"
  : link === "ended" ? "Ended" : "View only: the last 100 lines, read every 2 seconds";
const tmOpen = (t, link, act, o = {}) => {
  const st = o.st || t.st, b = (l, cls, a) => `<button class="dbtn small${cls}" ${a}>${l}</button>`, acts = [];
  if (link === "attached" || link === "attaching") acts.push(
    b(o.watching ? "Take control" : "View only", "", `title="${o.watching ? "Type into the terminal again" : "Stop typing into it, keep watching"}" ${act(o.watching ? "attached" : "watching")}`),
    b("Detach", "", `title="Stop following it here; it keeps running" ${act("detached")}`));
  else if (link === "detached" || (link === "ended" && st === "offline")) acts.push(b("Attach", " pri", act("attached")));
  acts.push(b(st === "running" ? "Kill…" : "Close", " warn", st === "running" ? `data-toast="Stop ${t.n} on ${t.m}?"` : act("close")));
  const body = link === "view_only"
    ? `<pre class="tm-screen"><span class="tm-cmd">${t.m} ${t.cwd} $ ${t.n}</span>\n…\n${TM_OUT[t.n]}</pre>`
    : `<div class="tm-xterm"><pre class="tm-term">${link === "attaching" && !o.tries ? "" : (o.gap ? '<span class="d">… earlier output was not kept</span>\n' : "") + TM_OUT[t.n]}${link === "attached" && st === "running" && !o.watching ? '<i class="cur"></i>' : ""}</pre></div>`;
  return `<div class="cc-panel tm-panel"><div class="tm-head"><span><b>${t.n}</b> ${t.m} · ${t.cwd}</span><span class="tm-st">${st === "exited" ? "done" : st}</span>${acts.join("")}</div>${body}
    <div class="tm-foot"><span><span class="ddot${link === "attached" && st === "running" ? "" : " off"}"></span> ${tmLabel(link, t.m, o)}</span><span>started ${t.age}</span></div></div>`;
};
const tmField = (label, ph, value = "") => `<label class="tm-f"><span>${label}</span><input aria-label="${label}" placeholder="${ph}" value="${value}" spellcheck="false"></label>`;
const tmChips = (names, cur, key) => `<div class="tm-chips">${names.map(n => `<button class="dchip" type="button" aria-pressed="${n === cur}" data-dact="dset:${key}:${n}">${n}</button>`).join("")}</div>`;
// The project's Terminals pane: panel() in terminals.ts. S.d.ptm picks its state.
const tmPane = () => {
  const v = S.d.ptm || "pick", act = k => `data-dact="dset:ptm:${k === "close" ? "none" : k}"`;
  const T = { n: "cargo watch", m: "build-box", cwd: "~/code/widget", st: "running", age: "3 h" };
  const open = ["open", "watching", "attached", "detached"].includes(v);
  const has = v !== "none" && v !== "noplace";
  const tabs = `<div class="in-tabs tm-tabs">${has ? `<button class="tm-tab" ${act("attached")}${open ? ' aria-current="true"' : ""}><span class="ddot"></span>${T.n}</button>` : ""}<button class="tm-tab" aria-label="New terminal" title="New terminal" ${act("form")}>+</button></div>`;
  if (v === "form" || v === "noplace") {
    const none = v === "noplace", m = S.d.ptmm || "build-box";
    return `${tabs}<form class="cc-panel tm-form" onsubmit="return false">${tmField("Command", "Empty for a shell, or npm run dev")}${tmField("Name", "The command, or shell, when empty")}
      <div class="tm-f"><span>Machine</span>${none ? '<span class="dmuted">No machine has this project yet</span>' : tmChips(["build-box", "studio"], m, "ptmm")}</div>
      ${tmField("Folder", "The project's folder", none ? "" : "~/code/widget")}
      <div class="in-acts"><button class="dbtn pri" type="button"${none ? " disabled" : ` ${act("attached")}`}>Start</button><button class="dbtn quiet" type="button" ${act(none ? "none" : "pick")}>Cancel</button></div></form>`;
  }
  if (open) return tabs + tmOpen(T, v === "detached" ? "detached" : "attached", act, { watching: v === "watching" });
  return `${tabs}<div class="cc-panel cc-empty"><b>${v === "none" ? "No terminals yet" : "Pick a terminal"}</b><button class="dbtn pri" ${act("form")}>New terminal</button></div>`;
};
// There is no terminal window in the app: this is the project page with its Terminals pane open.
D.dWinTerminal = () => {
  const keep = S.d.psel, keepTm = S.d.ptm;
  S.d.psel = "terminals"; S.d.ptm = keepTm || "list";
  try { return D.dWinProject(); } finally { S.d.psel = keep; S.d.ptm = keepTm; }
};
VARIANTS.dWinTerminal = [["Running terminals", "list"], ["No terminals", "none"]].map(([l, k]) => [l, () => { S.d.ptm = k; S.d.ptmm = ""; render(); }, () => (S.d.ptm || "list") === k]);
DESK_SCREENS.find(g => g.group === "Command center").items.push(["dWinTerminal", "Project terminals", "CommandCenterWindow, TerminalList"]);

// Project setup uses the shared controls with compact spacing.
(() => { const style = document.createElement("style"); style.textContent = `
.mac .ap-sheet { width: min(640px, 88vw); overflow: auto; padding: 16px }
.mac .ap-sheet, .mac .np-sheet, .mac .ap-sheet .ap-list, .mac .ap-sheet .ap-q,
.mac .ap-sheet .dbtn, .mac .np-sheet .dbtn { border-radius: 4px }
.np-sheet .np-m { width: 88px; flex: none; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.np-sheet .np-to { flex: none; max-width: 40%; font-family: ui-monospace, Menlo, monospace; font-size: 12.5px; color: var(--d-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .ap-sheet .am-head { margin-bottom: 12px }
.mac .ap-sheet .am-label { margin: 12px 0 8px }
.mac .ap-ways { display: flex; flex-wrap: wrap; margin: 0; gap: 8px }
.mac .ap-ways .dbtn { display: inline-flex; align-items: center; padding: 0 12px; min-height: 28px; font-size: 12px; font-weight: 500 }
.mac .ap-sheet .ap-bar { margin: 8px 0; min-height: 28px }
.mac .ap-sheet .ap-bar .dbtn { padding: 0 8px; min-height: 24px }
.mac .ap-results { max-height: 320px; overflow: auto; border: 1px solid var(--d-border); border-radius: 4px; background: var(--d-elev) }
.mac .ap-sheet .ap-list { border: 0; padding: 0 12px; max-height: none; overflow: visible; border-radius: 0 }
.mac .ap-sheet .ap-remote .am-label { margin: 0; padding: 8px 12px; background: var(--d-surface) }
.mac .ap-sheet .ap-row { padding: 8px 0; gap: 2px 12px }
.mac .ap-sheet .ap-row .m { gap: 4px 12px }
.mac .ap-sheet .ap-ag { padding: 0; border: 0; border-radius: 0; color: var(--d-muted) }
.mac .ap-sheet .ap-row.ap-focus { outline: none }
.mac .ap-sheet .ap-row:focus-visible { outline: 1px solid var(--d-accent); outline-offset: -1px; border-radius: 0 }
.mac .ap-sheet .ap-row .acts .dbtn { background: var(--d-surface); border-color: var(--d-border); color: var(--d-fg) }
.mac .ap-sheet .ap-remote summary { padding: 8px 12px; cursor: pointer; font-size: 12px; color: var(--d-muted) }
.mac .ap-sync { margin-top: 12px; font-size: 12px; color: var(--d-muted) }
.mac .ap-sync summary { cursor: pointer; padding: 4px 0 }
.mac .ap-sync .am-line { margin: 8px 0; color: var(--d-fg) }
.mac .ap-sync .am-sub { font-size: 12px; line-height: 16px }
`; document.head.append(style); })();
