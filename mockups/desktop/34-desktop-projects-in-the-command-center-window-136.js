// ---- Desktop: projects in the command center window (#136) ----
// The overview lists projects; a project shows its coding agents, machines, PRs and issues.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .pj-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(230px, 1fr)); gap: 10px }
.mac .pj-card { min-width: 0; padding: 12px; display: grid; grid-template-columns: minmax(0, 1fr); gap: 8px; text-align: left; border: 1px solid var(--d-border); border-radius: 8px; background: rgba(21,22,23,.82); color: var(--d-fg) }
.mac .pj-card:hover { border-color: #34363C }
.mac .pj-card .h { display: flex; align-items: center; gap: 8px; min-width: 0 }
.mac .pj-card .h b { min-width: 0; flex: 0 1 auto; font-weight: 600; font-size: 14px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pj-card .h .pa, .mac .pj-card .h .cc-pdot { flex: none }
.mac .pj-card .h small { flex: none; margin-left: auto; font-size: 11px; color: var(--d-muted); white-space: nowrap }
.mac .pj-card .line { font-size: 12px; color: var(--d-muted); line-height: 17px; overflow-wrap: anywhere }
.mac .pj-card .line b { color: var(--d-fg); font-weight: 500 }
.mac .pj-card .line.sync { font-size: 11px; color: var(--d-dim) }
.mac .pj-cols { display: grid; grid-template-columns: minmax(0, 1.5fr) minmax(0, 1fr); gap: 14px; align-items: start }
.mac .pj-task { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 3px 10px; padding: 8px 12px; align-items: center }
.mac .pj-task + .pj-task { border-top: 1px solid var(--d-border) }
.mac .pj-task .t { font-size: 13px; font-weight: 500; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pj-task .m { grid-column: 1 / -1; display: flex; flex-wrap: wrap; gap: 4px 10px; font-size: 11.5px; color: var(--d-muted) }
.mac .pj-task .m a { color: #8A9CF0; text-decoration: none }
.mac .pj-task .acts { grid-column: 1 / -1; display: flex; gap: 6px; flex-wrap: wrap }
.mac .pj-task.ask { background: rgba(242,201,76,.06) }
.mac .pj-labels { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; padding: 7px 11px; border-bottom: 1px solid var(--d-border) }
.mac .pj-labels .lbl, .mac .pj-labels small { font-size: 11px; color: var(--d-muted) }
.mac .pj-st { font-size: 10.5px; font-weight: 600; padding: 0 7px; line-height: 17px; border-radius: 999px; border: 1px solid var(--d-border); color: var(--d-muted); white-space: nowrap; display: inline-flex; gap: 6px; align-items: center }
.mac .pj-st.running { color: var(--d-emerald); border-color: rgba(94,106,210,.45) }
.mac .pj-st.review { color: #8A9CF0; border-color: rgba(138,156,240,.45) }
.mac .pj-st.blocked { color: #F2C94C; border-color: rgba(242,201,76,.45) }
.mac .pj-st.done { color: var(--d-dim) }
.mac .pj-st.dropped { color: var(--d-dim); text-decoration: line-through }
.mac .pj-st .dspin { width: 9px; height: 9px }
.mac .pj-start { grid-column: 1 / -1; display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 6px 10px; align-items: center; margin-top: 4px; font-size: 12px }
.mac .pj-start .lbl { color: var(--d-muted) }
.mac .pj-start .seg { display: flex; flex-wrap: wrap; gap: 4px }
.mac .pj-start .seg button { padding: 2px 9px; border-radius: 999px; border: 1px solid var(--d-border); background: transparent; color: var(--d-fg); font-size: 12px }
.mac .pj-start .seg button[aria-pressed="true"] { background: rgba(94,106,210,.26); border-color: rgba(94,106,210,.55) }
.mac .pj-start code { justify-self: start }
.mac .pj-start .acts { grid-column: 1 / -1 }
.mac .pj-run { grid-column: 1 / -1; display: grid; gap: 8px; margin-top: 6px; padding: 9px 10px; border-radius: 9px; border: 1px solid var(--d-border); background: var(--d-elev) }
.mac .pj-jobs { display: grid; grid-template-columns: auto 1fr; gap: 6px 14px; font-size: 12px; align-items: baseline }
.mac .pj-jobs b { font-weight: 500 } .mac .pj-jobs b.bad { color: #F2C94C } .mac .pj-jobs small { color: var(--d-muted) }
.mac .pj-steps { display: grid; gap: 2px; font-size: 12px }
.mac .pj-step { display: grid; grid-template-columns: 10px 1fr auto; gap: 8px; align-items: center }
.mac .pj-step small { color: var(--d-muted) } .mac .pj-step.bad { color: #F2C94C } .mac .pj-step.skip { color: var(--d-dim) }
.mac .pj-log small { color: var(--d-muted); font-size: 11px }
.mac .pj-log pre { margin: 4px 0 0; padding: 8px; border-radius: 7px; background: rgba(0,0,0,.25); font-size: 11.5px; line-height: 16px; white-space: pre-wrap; max-height: 220px; overflow: auto }
.mac .cc-org { display: flex; align-items: center; gap: 6px; width: 100%; margin-top: 2px; padding: 3px 8px; border: 0; border-radius: 6px; background: none; font: inherit; font-size: 11.5px; text-align: left; color: var(--d-muted); cursor: pointer }
.mac .cc-org:hover { color: var(--d-fg) }
.mac .cc-org .twist { flex: none; width: 10px; display: grid; place-items: center; color: var(--d-dim) }
.mac .cc-org .twist.open svg { transform: rotate(90deg) }
.mac .cc-org[aria-current="true"] { background: rgba(94,106,210,.26); color: var(--d-fg) }
.mac .cc-si.pj-repo { padding-left: 18px }
.mac .pj-toolbar { display: flex; align-items: center; gap: 12px; justify-content: space-between }
.mac .pj-toolbar .pj-find { flex: 1; min-width: 0 }
.mac .pj-sort { color: var(--d-fg); background: var(--d-bg); font: inherit; padding: 5px 8px; border: 1px solid var(--d-border); border-radius: 6px }
.mac .pj-find { display: block; max-width: 320px }
.mac .pj-find input { display: block; width: 100%; box-sizing: border-box; padding: 6px 10px; border: 1px solid var(--d-border); border-radius: 6px; background: #0f1011; color: var(--d-fg); font: inherit; font-size: 13px }
.mac .pj-find input::placeholder { color: var(--d-dim) }
.mac .pj-org { display: grid; gap: 8px }
.mac .pj-org + .pj-org { margin-top: 6px }
.mac .pj-org h3 { margin: 0; display: flex; align-items: baseline; gap: 8px; font-size: 13px; font-weight: 600; color: var(--d-fg) }
.mac .pj-org h3 small { font-size: 11px; font-weight: 400; color: var(--d-muted) }
/* A group heading folds its cards; the fold is kept on this computer. A search shows every match. */
.mac .pj-fold { all: unset; display: inline-flex; align-items: baseline; gap: 8px; cursor: default }
.mac .pj-fold .twist { width: 10px; display: inline-grid; place-items: center; color: var(--d-dim) }
.mac .pj-fold .twist.open svg { transform: rotate(90deg) }
.mac .pj-cols.pj-half { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); margin-top: 14px }
@container (max-width: 640px) { .mac .pj-cols, .mac .pj-cols.pj-half { grid-template-columns: minmax(0, 1fr) } }
/* A workflow run in two lines at half width: title and chip, then the meta on one line (the branch
   shortens first) beside Details and Open. */
.mac .pj-task.pj-runrow .m { grid-column: 1; flex-wrap: nowrap; white-space: nowrap; overflow: hidden }
.mac .pj-task.pj-runrow .m > span { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis }
.mac .pj-task.pj-runrow .m > span:first-child { flex: 0 0 auto; max-width: 40% }
.mac .pj-task.pj-runrow > .pj-st { justify-self: end }
/* Below 420px the meta and the buttons take a line each. */
.mac .pj-runbox { container-type: inline-size }
@container (max-width: 520px) { .mac .pj-task.pj-runrow .m > span:nth-child(2), .mac .pj-task.pj-runrow .m > span:nth-child(3) { display: none } }
@container (max-width: 420px) { .mac .pj-task.pj-runrow .m, .mac .pj-task.pj-runrow .acts { grid-column: 1 / -1 } }
.mac .pj-task.pj-runrow .acts { grid-column: 2; flex-wrap: nowrap; justify-content: flex-end }`;
  document.head.append(st);
})();

const PJ = [
  { name: "acme/widget", color: "var(--d-coral)", issues: 3, workers: 2, at: 60, machines: "build-box, mac-box", sync: "Beep sync", last: "Codex: bump the sync crate<small> · 1 h</small>" },
  { name: "acme/relay", pin: true, color: "#6FA8DC", issues: 1, workers: 1, at: 12, machines: "build-box", sync: "Syncthing", last: "Claude Code on fix-clippy needs you<small> · 12 min</small>" },
  { name: "northwind/pines", color: "#B18CE0", workers: 0, at: 2880, machines: "gpu-box", last: "Claude Code rebuilt the index on gpu-box<small> · 2 d</small>" },
  { name: "northwind/a-very-long-project-name-that-needs-more-room-than-a-card", color: "#7BC47F", workers: 0, at: 30, machines: "gpu-box", sync: "Beep sync", last: "No agents yet" },
];
// Projects are grouped by GitHub org (or user); inside a group only the repo name shows.
const PJ_ORGS = () => [...new Set(PJ.map(p => p.name.split("/")[0]))].map(org => ({ org, repos: PJ.filter(p => p.name.startsWith(org + "/")) }));
const pjRepo = p => p.name.split("/")[1];
const PJ_TWIST = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>';
// A heading folds its group; folded, it lights up while it holds the open project.
const pjHead = (key, label, cur, repos, cls = "") => {
  const folded = S.d.pjfold === key;
  return `<button class="cc-org${cls}" aria-expanded="${!folded}"${folded && repos.some(p => p.name === cur) ? ' aria-current="true"' : ""} data-dact="dset:pjfold:${folded ? "" : key}"><span class="twist${folded ? "" : " open"}">${PJ_TWIST}</span>${label}</button>`;
};
const pjSideOrgs = cur => PJ_ORGS().map(g => ({ ...g, repos: g.repos.filter(p => !p.pin) })).filter(g => g.repos.length).map(g => pjHead(g.org, `${g.org}/`, cur, g.repos) + (S.d.pjfold === g.org ? "" : g.repos.map(p => pjRow(cur, p)).join(""))).join("");

// The sidebar's Projects group lists the projects of the mesh.
const pjRow = (cur, p, full) => `<button class="cc-si${full ? "" : " pj-repo"}"${cur === p.name ? ' aria-current="true"' : ""} data-dgo="dWinProject"><span class="cc-pdot" style="background:${p.color}"></span><span class="tt">${full ? p.name : pjRepo(p)}</span>${p.workers ? `<span class="n">${p.workers}</span>` : ""}<span class="pa" role="button" aria-label="${p.pin ? "Unpin" : "Pin to top"}" data-toast="${p.pin ? "Unpinned " : "Pinned "}${p.name}">${DI.pin}</span></button>`;
const ccSidePj = (cur, o = {}) => ccSide(cur, o).replace(/<div class="cc-grp"><h3>Projects<\/h3>[\s\S]*?<\/div>/,
  `<div class="cc-grp"><h3><a class="cc-h3l" href="#" data-dgo="dWinProjects" title="All projects">Projects</a><button aria-label="Add a project" data-dgo="dAddProject">${CC_IC.plus}</button></h3>
    ${pjSideOrgs(cur)}
</div>`);

D.dWinProjects = () => {
  const empty = S.d.pjs === "empty";
  const card = p => {
    return `<button class="pj-card" data-dgo="dWinProject"><div class="h"><span class="cc-pdot" style="background:${p.color}"></span><b title="${p.name}">${pjRepo(p)}</b><span class="pa" role="button" aria-label="${p.pin ? "Unpin" : "Pin to top"}" data-toast="${p.pin ? "Unpinned " : "Pinned "}${p.name}" style="display:grid;color:${p.pin ? "var(--d-coral)" : "var(--d-dim)"}">${DI.pin}</span><small>${p.workers ? p.workers + (p.workers === 1 ? " agent" : " agents") : "idle"}</small></div>
      <span class="line">${p.workers ? "Last activity 12 min ago" : "No recent activity"}</span><span class="line sync">on ${p.machines}</span>
      ${p.sync ? `<span class="line sync">${p.sync}</span>` : ""}</button>`;
  };
  // Search filters by name, folder and remote across groups; inside a group,
  // pinned first, then the most recent activity (agent sessions, visits).
  const q = S.d.pjs === "search" ? "relay" : S.d.pjs === "nomatch" ? "zzz" : "";
  const find = empty ? "" : `<div class="pj-toolbar"><label class="pj-find"><input type="search" placeholder="Search projects" aria-label="Search projects" value="${q}"></label><select class="pj-sort" aria-label="Sort projects"><option>Default</option><option>Recent activity</option><option>Last opened</option></select></div>`;
  const orgs = PJ_ORGS().map(g => ({ ...g, repos: g.repos.filter(p => p.name.includes(q)) })).filter(g => g.repos.length);
  const body = empty
    ? `<div class="cc-panel cc-empty"><b>No projects yet</b><span>A folder or a repository to clone.</span><button class="dbtn pri" data-dgo="dAddProject">${CC_IC.plus} Add a project</button></div>`
    : find + (orgs.length ? "" : `<div class="cc-panel cc-empty"><span>No projects match.</span></div>`) + orgs.map(g => { const folded = !q && S.d.pjovfold === g.org; return `<section class="pj-org"><h3><button class="pj-fold" aria-expanded="${!folded}" data-dact="dset:pjovfold:${folded ? "" : g.org}"><span class="twist${folded ? "" : " open"}">${PJ_TWIST}</span>${g.org}/<small>${g.repos.length} ${g.repos.length === 1 ? "repo" : "repos"}</small></button></h3>${folded ? "" : `<div class="pj-grid">${[...g.repos].sort((x, y) => (!!y.pin - !!x.pin) || (x.at - y.at)).map(card).join("")}</div>`}</section>`; }).join("") + `
       <div class="cc-panel cc-row pj-add"><span class="t"><b>Add a project</b><span>A folder or a repository to clone.</span></span><button class="dbtn" data-dgo="dAddProject">Add</button></div>`;
  const imp = S.d.pjs === "import";
  const PR_IC = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="5" r="2.2"/><circle cx="6" cy="19" r="2.2"/><circle cx="18" cy="19" r="2.2"/><path d="M6 7.2v9.6M18 16.8V9a3 3 0 0 0-3-3h-3"/></svg>';
  const ISS_IC = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="1.5"/></svg>';
  const outHead = (repo, age, pri) => `<div class="cc-row cc-line cc-gr bare" data-toast="Opens ${repo} on its host"><button class="t" title="${repo}"><b>${repo}</b><span>${(on => on ? `on ${on.on.map(([m]) => m).join(", ")}` : "github.com")(AP_FOUND.find(f => f.name === repo))}</span></button><span class="r"><span class="age">${age}</span><button class="dbtn${pri ? " pri" : ""} small" ${AP_FOUND.some(f => f.name === repo) ? `data-toast="Imported ${repo}"` : 'data-dgo="dImportProject"'}>Import project</button></span></div>`;
  const outItem = (ic, n, t, st, age) => `<div class="cc-row cc-line cc-gr it" data-toast="Opens #${n} on its host"><span class="ic">${ic}</span><button class="t" title="#${n}"><span class="num">#${n}</span><b>${t}</b></button><span class="r">${st}<span class="age">${age}</span></span></div>`;
  const outside = empty ? "" : `<div id="sec-elsewhere"><div class="cc-sec"><h4>Your open PRs and issues in repos without a project</h4></div><div class="cc-panel cc-open">
      ${outHead("tools-org/uploader", "2 h", true)}${outItem(PR_IC, 812, "Add a retry flag to the uploader", '<span class="cc-tag wait">your review</span>', "2 h")}
      ${outHead("big-org/parser", "4 d", false)}${outItem(ISS_IC, 2210, "Crash when the config file is empty", "", "4 d")}</div></div>`;
  const importCard = `<div class="cc-panel" style="padding:12px;display:grid;gap:10px"><b style="font-size:14px">Import tools-org/uploader</b>
      <span style="font-size:12px;color:var(--d-muted)">Found a checkout on build-box at <code>/srv/uploader</code> (fork remote: you). 3 branches have work; pick where each one lives.</span>
      ${[["retry-flag", "PR #812 · needs reply", "build-box", ".worktrees/retry-flag", 1], ["fix-windows-paths", "pushed 2 weeks ago, no PR", "", "", 0], ["main", "default branch", "build-box", "/srv/uploader", 1]].map(([b, meta, m, wt, has]) =>
        `<div class="pj-task" style="padding:6px 0"><span class="t"><code>${b}</code></span>${has ? `<span class="pj-st running" style="color:var(--d-muted);border-color:var(--d-border)">${m}</span>` : ""}<span class="m"><span>${meta}</span>${has ? `<span>${wt}</span>` : ""}</span>${has ? "" : `<span class="pj-start"><span class="lbl">Worktree</span><span class="seg"><button aria-pressed="true">build-box</button><button>mac-box</button><button>Skip</button></span></span>`}</div>`).join("")}
      <span class="acts" style="display:flex;gap:6px"><button class="dbtn pri" data-toast="Imported; worktree for fix-windows-paths created on build-box">Import<kbd>⌘${DI.ret}</kbd></button><button class="dbtn quiet" data-toast="Cancelled">Cancel</button></span></div>`;
  return ccWin(ccSidePj("projects"), `<div class="cc-top"><h2>Projects</h2><small>${empty ? "" : "4 projects in 2 orgs, 3 agents running"}</small></div><div class="cc-view">${imp ? importCard : ""}${body}${empty ? "" : apFoundSection()}${outside}</div>`);
};
VARIANTS.dWinProjects = [["Projects", "list"], ["None yet", "empty"], ["Import with branches", "import"], ["Search", "search"], ["Search without a match", "nomatch"]].map(([l, k]) => [l, () => { S.d.pjs = k; render(); }, () => (S.d.pjs || "list") === k]);
