// ---- Desktop: Add a project to a machine ----
// From a "Not on <machine>" row of Where it lives. The folder starts at the machine's default, Choose... browses
// that machine. Add makes the folder and joins the project's sync; the files and .git follow.
D.dAddToMachine = () => {
  const v = S.d.pa || "form";
  const browse = `<div class="ip-browse">
      <div class="at"><button class="dbtn quiet" style="padding:2px 8px;font-size:12px;min-height:0" data-toast="Up to /path">Up</button><span title="/path/to/code">/path/to/code</span></div>
      <div class="dirs">${["acme", "notes", "scratch"].map(d => `<button type="button" data-toast="Opens /path/to/code/${d}">${d}</button>`).join("")}</div>
      <div class="foot"><span style="flex:1;min-width:0">It goes in /path/to/code/widget.</span><button class="dbtn quiet" style="padding:3px 10px;font-size:12px" data-dact="dset:pa:form">Cancel</button><button class="dbtn pri" style="padding:3px 10px;font-size:12px" data-dact="dset:pa:form">Use this folder</button></div>
    </div>`;
  const form = `<div class="ip-label">Folder on laptop</div>
    <div style="display:flex;gap:8px;margin-bottom:14px"><div class="ip-path" title="/path/to/code/widget">/path/to/code/widget</div><button class="dbtn quiet" data-dact="dset:pa:browse">Choose…</button></div>
    ${v === "browse" ? browse : ""}
    ${v === "failed" ? '<div style="color:var(--d-err);font-size:12.5px;margin-bottom:12px">/path/to/code/widget has other files. Choose another folder.</div>' : ""}
    <div style="display:flex;justify-content:flex-end;gap:8px"><button class="dbtn quiet" data-dgo="dProjectsPage">Cancel</button><button class="dbtn pri" data-dact="dset:pa:copying">Add</button></div>`;
  const copying = `<div class="ip-wait"><span class="dspin"></span>Copying to laptop: 12 MB of 25 MB · 1.2 MB/s · under a minute</div>
    <div class="fs-bar am-bar" style="margin-bottom:14px"><i style="width:48%"></i></div>
    <div style="display:flex;justify-content:flex-end"><button class="dbtn quiet" data-dgo="dProjectsPage">Close</button></div>`;
  const sheet = `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5">
    <div class="am-sheet ip-sheet" role="dialog" aria-label="Add to a machine">
      <div style="display:flex;justify-content:space-between;margin-bottom:12px"><b style="font-size:15px">Add widget to laptop</b><kbd style="font-size:11px;opacity:.7">esc</kbd></div>${v === "copying" ? copying : form}</div></div>`;
  S.d.pp = "one";
  return D.dProjectsPage().replace(/<\/div><\/div>\s*$/, sheet + "</div></div>");
};
VARIANTS.dAddToMachine = [["Folder", "form"], ["Choosing a folder", "browse"], ["Folder has other files", "failed"], ["Copying", "copying"]]
  .map(([l, k]) => [l, () => { S.d.pa = k; render(); }, () => (S.d.pa || "form") === k]);
DESK_SCREENS.find(g => g.group === "Command center").items.push(["dAddToMachine", "Add a project to a machine", "project/addmachine.ts"]);
