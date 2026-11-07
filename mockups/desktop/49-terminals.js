
// Terminals on every machine (#144). A PTY on the node outlives the window and the app, like a
// tmux session: open one anywhere, close the window, attach again from any device.
const CC_TERMS = [
  { name: "Build box", terms: [["dev-server", "acme/widget · .worktrees/login-copy", "run", "attached on phone"], ["logs", "~/srv/relay", "run", "idle 12 min"], ["cargo watch", "acme/relay", "run", ""]] },
  { name: "Mac Studio", terms: [["ios-sim", "acme/widget · apps/ios", "run", ""], ["shell", "~", "exit", "exited 0, 3 h ago"]] },
  { name: "MacBook Air", terms: [] },
];
CC_IC.term = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>';
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .tl { display: grid; grid-template-columns: 260px minmax(0,1fr); gap: 14px; min-height: 0; flex: 1 }
.mac .tl-list { display: grid; align-content: start; gap: 12px; overflow-y: auto }
.mac .tl-m h5 { margin: 0 0 4px; display: flex; align-items: center; gap: 6px; font-size: 12px; font-weight: 600; color: var(--d-muted) }
.mac .tl-m h5 .dbtn { margin-left: auto; padding: 1px 7px; font-size: 11.5px }
.mac .tl-it { display: grid; grid-template-columns: 10px minmax(0,1fr); gap: 2px 8px; align-items: center; width: 100%; text-align: left; padding: 7px 9px; border-radius: 8px; border: 1px solid transparent; background: none; color: var(--d-fg) }
.mac .tl-it[aria-current="true"] { background: var(--d-elev); border-color: var(--d-border) }
.mac .tl-it b { font-family: var(--mono, ui-monospace, monospace); font-weight: 500; font-size: 12.5px }
.mac .tl-it small { grid-column: 2; color: var(--d-muted); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .tl-empty { font-size: 12px; color: var(--d-muted); padding: 4px 9px }
.mac .tl-new { display: grid; gap: 10px; border: 1px solid var(--d-border); border-radius: 8px; padding: 14px 16px; background: var(--d-elev); align-content: start }
.mac .tl-new label { display: grid; grid-template-columns: 90px minmax(0,1fr); align-items: center; gap: 10px; font-size: 12.5px; color: var(--d-muted) }
.mac .tl-new .in { height: 30px; border: 1px solid var(--d-border); border-radius: 7px; padding: 0 10px; display: flex; align-items: center; font-family: var(--mono, ui-monospace, monospace); font-size: 12px; color: var(--d-fg); background: rgba(0,0,0,.2) }`;
  document.head.append(st);
})();
const TL_T = {
  cw: { n: "cargo watch", m: "build-box", cwd: "~/code/api", where: "acme/api · ~/code/api", st: "running", note: "3 h", age: "3 h" },
  tl: { n: "tail -f server.log", m: "build-box", cwd: "~/logs", where: "~/logs", st: "running", note: "2 d", age: "2 d" },
  nd: { n: "npm run dev", m: "studio", cwd: "~/code/site", where: "you/site · ~/code/site", st: "exited", note: "exited 0", age: "5 h" },
};
const TL_M = [["build-box", ["cw", "tl"]], ["studio", ["nd"]], ["laptop", []], ["phone", []]];
D.dTerminals = () => {
  const v = S.d.tl || "default";
  const empty = v === "empty", off = v === "offline", nw = v === "new" || v === "nomachine", none = v === "nomachine", old = v === "old", wait = v === "loading";
  const act = k => `data-tl="${k === "close" ? "pick" : k === "attached" ? "default" : k}"`;
  const OPEN = { default: ["cw", "attached"], watching: ["cw", "attached", { watching: true }], attaching: ["cw", "attaching"], reconnecting: ["cw", "attaching", { tries: 2 }], detached: ["cw", "detached"], error: ["cw", "detached"], gap: ["cw", "attached", { gap: true }], viewonly: ["cw", "view_only"], ended: ["nd", "ended"], offline: ["nd", "ended", { st: "offline" }] };
  const oz = OPEN[v];
  const openId = oz && (oz[0] === "cw" && S.d.tlo ? S.d.tlo : oz[0]);
  const online = n => n !== "phone" && !none && !(off && n === "studio");
  const groups = empty || wait ? TL_M.map(([n]) => [n, []]) : TL_M;
  const list = groups.map(([n, ids]) => {
    const up = online(n);
    const items = ids.map(id => {
      const t = TL_T[id], st = !up && (t.st === "running" || off) ? "offline" : t.st;
      return `<button class="tl-it" title="${t.m}: ${t.n} in ${t.cwd}" data-tl="${id === "nd" ? (off ? "offline" : "ended") : "default"}" data-dact="dset:tlo:${id === "nd" ? "" : id}"${openId === id ? ' aria-current="true"' : ""}><span class="ddot${st === "running" ? "" : " off"}"></span><b>${t.n}</b><small>${t.where} · ${st === "offline" ? "offline" : t.note}</small></button>`;
    }).join("");
    return `<div class="tl-m"><h5><span class="ddot${up ? "" : " off"}"></span>${up ? n : `${n} · offline`}<button class="dbtn"${up ? ` data-tl="new" data-dact="dset:tlm:${n}"` : " disabled"}>New</button></h5>${items || '<div class="tl-empty">No terminals.</div>'}${!up && ids.length ? '<div class="tl-empty">They keep running there and show again when it is back.</div>' : ""}</div>`;
  }).join("");
  let right;
  if (nw) {
    const places = TL_M.map(([n]) => n).filter(online), m = S.d.tlm || places[0];
    right = `<form class="cc-panel tm-form" onsubmit="return false"><b class="tm-title">New terminal</b><div class="tm-f"><span>Machine</span>${places.length ? tmChips(places, m, "tlm") : '<span class="dmuted">No machine is online</span>'}</div>
      ${tmField("Folder", "Empty for the workspace or home folder")}${tmField("Command", "Empty for your shell")}${tmField("Name", "The command, or shell, when empty")}
      <div class="in-acts"><button class="dbtn pri" type="button"${places.length ? ' data-tl="default"' : " disabled"}>Open</button><button class="dbtn quiet" type="button" data-tl="pick">Cancel</button></div></form>`;
  } else if (oz) right = tmOpen(TL_T[openId], oz[1], act, oz[2] || {});
  else right = `<div class="cc-conn"><b>${wait ? "Loading…" : empty ? "No terminals yet" : "Pick a terminal"}</b><button class="dbtn pri" data-tl="new" data-dact="dset:tlm:">New terminal</button></div>`;
  const head = `<div class="cc-top"><h2>Terminals</h2><small>${old || wait ? "" : empty ? "None open" : "3 on 2 machines"}</small>
      <span class="tools">${old ? "" : '<button class="dbtn pri small" data-tl="new" data-dact="dset:tlm:">New terminal</button>'}</span></div>`;
  const view = old ? '<div class="cc-panel cc-empty"><b>The server needs an update before it lists terminals on every machine.</b></div>'
    : `${v === "error" ? `<div class="cc-err">The terminal's stream keeps dropping.</div>` : ""}<div class="tl"><div class="tl-list">${list}</div>${right}</div>`;
  return ccWin(ccSide("terminals"), `${head}<div class="cc-view tl-view">${view}</div>`);
};
document.getElementById("desk").addEventListener("click", e => {
  const b = e.target.closest("[data-tl]");
  if (!b) return;
  S.d.tl = b.dataset.tl; render();
});
VARIANTS.dTerminals = [["Attached", "default"], ["Watching", "watching"], ["Attaching", "attaching"], ["Reconnecting", "reconnecting"], ["Detached", "detached"], ["Stream keeps dropping", "error"], ["Earlier output lost", "gap"], ["View only", "viewonly"], ["Ended", "ended"], ["Machine offline", "offline"], ["Pick a terminal", "pick"], ["New terminal", "new"], ["No machine online", "nomachine"], ["Empty", "empty"], ["Loading", "loading"], ["Server too old", "old"]].map(([l, k]) => [l, () => { S.d.tl = k; S.d.tlo = ""; S.d.tlm = ""; render(); }, () => (S.d.tl || "default") === k]);
DESK_SCREENS.find(g => g.group === "Command center").items.push(["dTerminals", "Terminals on every machine", "CommandCenterWindow, TerminalsPage"]);

// Sync (#195): every syncing project's machines and conflicts on one page. A conflict has Fix with an
// agent, which fills the project page's bar for the user to send.
CC_IC.sync = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 11a8 8 0 00-14-4L4 9M4 5v4h4M4 13a8 8 0 0014 4l2-2M20 19v-4h-4"/></svg>';
const fsCmp = `<div class="fs-cmp"><div class="fs-cmph"><span>Kept <b>notes.txt</b></span><span>Copy <b>notes (conflict 1a2b3c4d).txt</b></span><small>on build-box, studio</small></div><div class="dv-lines"><div class="dv-l hunk"><span class="dv-g"><span></span><span></span></span><span class="dv-c">@@ -1,3 +1,3 @@</span></div><div class="dv-l"><span class="dv-g"><span>1</span><span>1</span></span><span class="dv-c">Ship the sync page</span></div><div class="dv-l del"><span class="dv-g"><span>2</span><span></span></span><span class="dv-c">Review on Friday</span></div><div class="dv-l add"><span class="dv-g"><span></span><span>2</span></span><span class="dv-c">Review on Thursday</span></div><div class="dv-l"><span class="dv-g"><span>3</span><span>3</span></span><span class="dv-c">Release after</span></div></div></div>`;
const fsConflict = (code, note) => { const file = !code.startsWith(".git/"); return `<div class="fs-conflict"><span class="t"><code>${code}</code><span>${note}</span></span><span class="acts">${file ? `<button class="dbtn small" aria-expanded="false" onclick="const r=this.closest('.fs-conflict'),o=r.classList.toggle('open');this.setAttribute('aria-expanded',o);this.textContent=o?'Hide':'Compare';o?r.insertAdjacentHTML('beforeend',fsCmp):r.querySelector('.fs-cmp').remove()">Compare</button>` : ""}<button class="dbtn pri small" data-toast="The bar is filled for you to send">Fix with an agent</button><button class="dbtn small" data-toast="Keeping this copy">Keep this copy</button><button class="dbtn small" data-toast="Keeping the original">Keep the original</button></span></div>`; };
const fsMachine = (n, chip, meta, off, pct) => `<div class="fs-row"><div class="fs-h"><span class="ddot${off ? " off" : ""}"></span><b>${n}</b><span class="pb-chip${meta[2] === "wait" ? " wait" : ""}">${chip}</span></div>${pct ? `<div class="fs-bar"><i style="width:${pct}%"></i></div>` : ""}<div class="fs-m">${meta[0].map(x => `<span>${x}</span>`).join("")}${meta[1] ? `<code>${meta[1]}</code>` : ""}</div></div>`;
D.dSync = () => {
  const v = S.d.sy || "default";
  const none = v === "none", fine = v === "insync";
  const proj = (name, sub, machines, conflicts) => `<div class="cc-sec"><h4>${name}</h4><small>${sub}</small><span class="fs-tools"><button class="dbtn quiet small" data-toast="Paused on every machine">Pause</button></span></div>
    <div class="cc-panel fs-list">${machines}</div>${conflicts ? `<div class="cc-sec"><h4>Conflicts</h4><small>edited on two machines at once</small></div><div class="cc-panel fs-list">${conflicts}</div>` : ""}`;
  const widget = fine
    ? proj("acme/widget", "in sync", fsMachine("build-box", "in sync", [["1,880 files", "284 MB"], "/srv/project/widget"]) + fsMachine("studio", "in sync", [["1,880 files", "284 MB"], "/srv/widget"]) + fsMachine("laptop", "offline", [["Seen 2 h ago"], ""], true), "")
    : proj("acme/widget", "2 conflicts", fsMachine("build-box", "in sync", [["1,880 files", "284 MB"], "/srv/project/widget", "wait"]) + fsMachine("studio", "syncing 23%", [["142 of 610 files", "48 of 210 MB", "2.1 MB/s", "about 1 min"], "/srv/widget"], false, 23) + fsMachine("laptop", "offline", [["Seen 2 h ago"], ""], true),
      fsConflict("notes (conflict 1a2b3c4d).txt", "Another copy of notes.txt · on build-box, studio") + fsConflict(".git/refs/beep-conflicts/heads/main-5e6f7a8b", "Another commit of main · on build-box"));
  const relay = fine
    ? proj("acme/platform/mesh-relay", "in sync", fsMachine("build-box", "in sync", [["412 files", "9 MB"], "/srv/project/mesh-relay"]), "")
    : proj("acme/platform/mesh-relay", "1 conflict", fsMachine("build-box", "in sync", [["412 files", "9 MB"], "/srv/project/mesh-relay", "wait"]) + fsMachine("mac-box", "in sync", [["412 files", "9 MB"], "/srv/mesh-relay", "wait"]),
      fsConflict("docs/plan (conflict 9fbd5479).md", "Another copy of docs/plan.md · on build-box, mac-box"));
  const sub = none ? "" : fine ? "2 projects, all in sync" : "3 conflicts in 2 projects";
  const view = none
    ? '<div class="cc-panel cc-empty"><b>No project syncs yet</b><span>Add a project on two machines and its files sync here.</span></div>'
    : `<div class="fs-page">${widget}${relay}<div class="dmuted fs-note">Builds, ignored files and links stay on each machine.</div></div>`;
  return ccWin(ccSide("sync", none ? { noSync: true } : fine ? { sync: 0 } : {}), `<div class="cc-top"><h2>Sync</h2><small>${sub}</small></div><div class="cc-view">${view}</div>`);
};
VARIANTS.dSync = [["Conflicts", "default"], ["All in sync", "insync"], ["Nothing synced", "none"]].map(([l, k]) => [l, () => { S.d.sy = k; render(); }, () => (S.d.sy || "default") === k]);
DESK_SCREENS.find(g => g.group === "Command center").items.push(["dSync", "Sync across projects", "CommandCenterWindow, SyncPage"]);
