// ---- Desktop: folder groups on the Projects page, and removing a machine ----
// Mirrors groupOf, sideProjects and overviewGroups in projects-state.js, the group sheet in
// center.js, and the machine menu (center.js) and Remove in Settings > Machines (bar/machines.js).
// A group is a folder the user picks, matched on every machine with the home folder and a
// leading Projects/ stripped: ~/Projects/work on a Mac and ~/work on Linux
// are one group. Its heading takes the place of the git remote's org.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .pj-org h3 .gp-acts { margin-left: auto; display: inline-flex; gap: 6px }
.mac .pj-org h3 .gp-acts .dbtn { min-height: 22px; padding: 0 9px; font-size: 12px; gap: 4px }
.mac .pj-org h3 .gp-paths { font-weight: 400; font-size: 11px; color: var(--d-dim); font-family: ui-monospace, Menlo, monospace }
.mac .pj-org h3 input { all: unset; font: inherit; padding: 1px 8px; border: 1px solid var(--d-emerald); border-radius: 6px; background: #0f1011; min-width: 160px }
.mac .cc-org.gp { display: flex; align-items: center; gap: 6px }
.mac .cc-org.gp svg { color: var(--d-dim) }
.mac .cc-menu { position: absolute; z-index: 6; min-width: 150px; padding: 4px; border-radius: 9px; border: 1px solid var(--d-border); background: #1c1d1f; box-shadow: 0 12px 32px rgba(0,0,0,.5); display: grid }
.mac .cc-menu :is(button, [role="menuitem"]) { all: unset; padding: 5px 10px; border-radius: 6px; font-size: 12.5px; cursor: pointer }
.mac .cc-menu :is(button, [role="menuitem"]):hover, .mac .cc-menu :is(button, [role="menuitem"]).on { background: rgba(255,255,255,.07) }
.mac .cc-menu :is(button, [role="menuitem"]).warn { color: var(--d-err) }
.mac .cc-menu .lbl { padding: 4px 10px 3px; font-size: 11px; color: var(--d-dim) }
.mac .gp-path { display: flex; gap: 8px; align-items: center; padding: 6px 0; font-size: 12.5px; font-family: ui-monospace, Menlo, monospace; border-top: 1px solid rgba(255,255,255,.05) }
.mac .gp-path span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis }
.mac .dmach .nm .dbtn { margin-left: auto; padding: 1px 9px; font-size: 11.5px; min-height: 0; font-weight: 400 }
.mac .dmach .ask { display: grid; gap: 8px; margin-top: 6px; padding: 9px 10px; border-radius: 8px; border: 1px solid var(--d-border); background: rgba(255,255,255,.02) }`;
  document.head.append(st);
})();

const GP_ICON = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6.5A1.5 1.5 0 014.5 5H10l2 2.5h7.5A1.5 1.5 0 0121 9v9.5a1.5 1.5 0 01-1.5 1.5h-15A1.5 1.5 0 013 18.5z"/></svg>';
// Over a window view: a dimmed scrim and a centred sheet, as Add a machine draws it.
const ccSheetOver = (html, sheet) => html.replace(/<\/div><\/div>\s*$/, `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5">
    <div style="width:min(520px,86%);padding:18px 20px;border-radius:14px;border:1px solid var(--d-border);background:#151617;box-shadow:0 20px 60px rgba(0,0,0,.5)">${sheet}</div></div></div></div>`)
  .replace('class="cc-win"', 'class="cc-win" style="position:relative"');

{
  const base = D.dWinProjects;
  D.dWinProjects = () => {
    const v = S.d.pjs || "list";
    let html = base();
    if (v === "empty") return html.replace(/(<div class="cc-top"><h2>Projects<\/h2><small>[^<]*<\/small>)/, `$1<span class="tools"><button class="dbtn small" data-dgo="dAddProject">${CC_IC.plus} Add a project</button></span>`);
    // New group sits beside the heading on every list.
    html = html.replace(/(<div class="cc-top"><h2>Projects<\/h2><small>[^<]*<\/small>)/,
      `$1<span class="tools"><button class="dbtn small" data-dact="dset:pjs:newgroup" data-dgo="dWinProjects">${GP_ICON} New group</button><button class="dbtn small" data-dgo="dAddProject">${CC_IC.plus} Add a project</button></span>`);
    const grouped = ["groups", "rename", "newgroup", "groupmenu"].includes(v);
    if (!grouped) return html;
    // acme/widget lives in ~/work/widget, so the group holds it instead of acme/.
    const cards = html.match(/<button class="pj-card"[\s\S]*?<\/button>/g) || [];
    const widget = (cards.find(c => c.includes("<b>widget</b>")) || "").replace("<b>widget</b>", "<b>acme/widget</b>");
    const notes = `<button class="pj-card" data-dgo="dWinProject"><div class="h"><span class="cc-pdot" style="background:#5E6AD2"></span><b>field-notes</b><span class="pa" role="button" aria-label="Pin to top" data-toast="Pinned field-notes" style="display:grid;color:var(--d-dim)">${DI.pin}</span><small>1 agent</small></div>
      <span class="line">From running agents</span></button>`;
    const name = v === "rename"
      ? `<input value="work" aria-label="Group name"><span class="gp-acts"><button class="dbtn pri" data-dact="dset:pjs:groups" data-dgo="dWinProjects" data-toast="Renamed">Save</button><button class="dbtn quiet" data-dact="dset:pjs:groups" data-dgo="dWinProjects">Cancel</button></span>`
      : `${GP_ICON} work<small>2 repos</small><span class="gp-paths" title="Matched on every machine">~/work</span>
        <span class="gp-acts"><button class="dbtn quiet" data-dact="dset:pjs:rename" data-dgo="dWinProjects">Rename</button><button class="dbtn quiet" data-dact="dset:pjs:newgroup" data-dgo="dWinProjects">Folders…</button><button class="dbtn quiet warn" data-dact="dset:pjs:list" data-dgo="dWinProjects" data-toast="Group removed. Its repos go back under their orgs">Remove</button></span>`;
    const group = `<section class="pj-org"><h3 style="display:flex;align-items:center;gap:8px">${name}</h3><div class="pj-grid">${widget}${notes}</div></section>`;
    html = html.replace(widget.replace("<b>acme/widget</b>", "<b>widget</b>"), "")
      .replace(/(<section class="pj-org"><h3>acme\/<small>)2 repos/, "$11 repo")
      .replace('<section class="pj-org">', group + '<section class="pj-org">')
      .replace("4 projects in 2 orgs, 3 workers running", "5 projects in 1 group and 2 orgs, 3 workers running");
    // The sidebar: the group's heading, its repos by full name, then the orgs left.
    html = html.replace(/<button class="cc-org"[^>]*>(?:(?!<\/button>)[\s\S])*?acme\/<\/button>[\s\S]*?(?=<button class="cc-org")/,
      `<button class="cc-org gp" title="~/work on every machine" aria-expanded="true"><span class="twist open">${PJ_TWIST}</span>${GP_ICON}work</button>
      <button class="cc-si" data-dgo="dWinProject"><span class="cc-pdot" style="background:var(--d-coral)"></span><span class="tt">acme/widget</span><span class="n">2</span></button>
      <button class="cc-si" data-dgo="dWinProject"><span class="cc-pdot" style="background:#5E6AD2"></span><span class="tt">field-notes</span><span class="n">1</span></button>`);
    if (v === "groupmenu") {
      // Right-click on a group's heading in the sidebar or on the page.
      return html.replace('class="cc-win"', 'class="cc-win" style="position:relative"').replace(/<\/div><\/div>\s*$/,
        `<div class="cc-menu" style="top:120px;left:440px"><span class="lbl">work</span><button data-dact="dset:pjs:rename" data-dgo="dWinProjects">Rename…</button><button data-dact="dset:pjs:newgroup" data-dgo="dWinProjects">Folders…</button><button class="warn" data-dact="dset:pjs:list" data-dgo="dWinProjects" data-toast="Group removed">Remove group</button></div></div></div>`);
    }
    if (v !== "newgroup") return html;
    const paths = [["~/work", "work"]];
    const sheet = `<div style="display:flex;justify-content:space-between;margin-bottom:12px"><b style="font-size:15px">New group</b><kbd style="font-size:11px;opacity:.7">esc</kbd></div>
      <div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">Folder</div>
      <div style="display:flex;gap:8px;align-items:center;margin-bottom:6px"><div class="ip-field focus"><span>~/Projects/work</span><span class="caret"></span></div><button class="dbtn" data-toast="Added ~/Projects/work">Add</button></div>
      <div style="font-size:12px;color:var(--d-muted);line-height:17px;margin-bottom:12px">Repos in this folder on any machine go in the group. <code>~/Projects/work</code> and <code>~/work</code> count as one folder.</div>
      ${paths.map(([p, n]) => `<div class="gp-path"><span>${p}</span><small style="font-family:inherit;color:var(--d-dim)">${n}</small><button class="dbtn quiet warn" style="padding:1px 8px;font-size:11.5px;min-height:0" data-toast="Removed ${p}">Remove</button></div>`).join("")}
      <div style="font-size:12px;color:var(--d-muted);margin:10px 0 14px">Named <b style="font-weight:500;color:var(--d-fg)">work</b>. 2 projects match.</div>
      <div style="display:flex;gap:8px;justify-content:flex-end"><button class="dbtn quiet" data-dact="dset:pjs:list" data-dgo="dWinProjects">Cancel</button><button class="dbtn pri" data-dact="dset:pjs:groups" data-dgo="dWinProjects" data-toast="Group saved">Save group<kbd>⌘${DI.ret}</kbd></button></div>`;
    return ccSheetOver(html, sheet);
  };
  VARIANTS.dWinProjects.push(...[["Folder groups", "groups"], ["New group", "newgroup"], ["Rename a group", "rename"], ["Group menu", "groupmenu"]]
    .map(([l, k]) => [l, () => { S.d.pjs = k; render(); }, () => S.d.pjs === k]));
}

// Remove a machine: right-click its row on Home, in the sidebar or on a project's Machines tile,
// then Remove, then confirm. The mesh drops its key and its connection; the node then
// says on its own side that it was removed. This Mac is not offered.
{
  window.__mrmOver = html => {
    const v = S.d.mrm;
    if (v !== "menu" && v !== "confirm" && v !== "failed") return html;
    if (v === "menu") return html.replace('class="cc-win"', 'class="cc-win" style="position:relative"').replace(/<\/div><\/div>(\s*<\/div>)?\s*$/, m => `<div class="cc-menu" style="top:640px;left:420px" role="menu"><span class="lbl">laptop</span><button class="warn on" role="menuitem" data-dact="dset:mrm:confirm" data-dgo="dHome">Remove…</button></div>` + m);
    const err = v === "failed" ? `<div class="am-msg bad">The machine took too long. Try again.</div>` : "";
    return html.replace('class="cc-win"', 'class="cc-win" style="position:relative"').replace(/<\/div><\/div>(\s*<\/div>)?\s*$/, m => `<div class="am-scrim" style="position:absolute"><div class="am-sheet" role="dialog" aria-modal="true" aria-label="Remove laptop?"><div class="am-head"><b>Remove laptop?</b><kbd>esc</kbd></div>
      <div class="am-msg">It leaves the mesh and agents running there stop.</div><div class="am-sub">Its files stay on the machine.</div>${err}
      <div class="gp-foot"><button class="dbtn quiet" data-dact="dset:mrm:" data-dgo="dHome">Cancel</button><button class="dbtn warn" data-dact="dset:mrm:" data-dgo="dHome" data-toast="laptop removed">Remove</button></div></div></div>` + m);
  };
  VARIANTS.dHome.push(...[["Machine menu", "menu"], ["Remove a machine", "confirm"], ["Remove failed", "failed"]]
    .map(([l, k]) => [l, () => { S.d.mrm = k; S.d.cc = "default"; S.d.htab = "machines"; render(); }, () => S.d.mrm === k]));
}

VARIANTS.dMachines.push(["Remove, asking", () => { S.d.machines = "list"; S.d.mrmb = "gpu-box"; render(); }, () => S.d.mrmb === "gpu-box"]);
VARIANTS.dMachines[0][1] = () => { S.d.machines = "list"; S.d.mrmb = ""; render(); };
