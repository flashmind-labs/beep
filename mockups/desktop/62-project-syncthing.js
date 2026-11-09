// Project sync ownership is set across machines, without pausing its share.
(() => {
  const control = () => {
    const enabled = S.d.stFiles !== 'off';
    return `<div class="am-label">Syncthing file sync</div><label class="am-line"><input class="dcheck" type="checkbox" ${enabled ? "checked" : ""} data-dact="dset:stFiles:${enabled ? "off" : "on"}"> Keep Syncthing file sync for this project</label><div class="am-sub">Turn off to use Beep on every machine, including machines that reconnect later. Git projects exchange commits. Uncommitted edits need an explicit handoff.</div>`;
  };
  const base = D.dImportProject;
  D.dImportProject = (...args) => base(...args).replace('<div class="ip-foot">', control() + '<div class="ip-foot">');
  const folder = D.dAddFolder;
  D.dAddFolder = (...args) => folder(...args).replace('<div class="ip-foot">', control() + '<div class="ip-foot">');
  const found = D.dAddProject;
  D.dAddProject = (...args) => found(...args).replace('<div class="ap-footer"></div>', '<details class="ap-sync"><summary>Sync options</summary>' + control() + '</details>');
  D.dProjectSyncSettings = () => {
    const enabled = S.d.stFiles !== 'off';
    const menu = `<div class="cc-menu" role="menu" aria-label="Project menu" style="position:absolute;right:24px;top:65px;z-index:5"><span class="lbl">acme/widget</span><button role="menuitem">Pin</button><button role="menuitem" data-dact="dset:stFiles:${enabled ? 'off' : 'on'}">Syncthing file sync: ${enabled ? 'On' : 'Off'} (all machines)</button><button role="menuitem" class="warn">Remove project</button></div>`;
    return D.dWinProject().replace(/<\/div><\/div>\s*$/, menu + '</div></div>').replace('class="cc-win"', 'class="cc-win" style="position:relative"');
  };
  D.dProjectHandoff = () => D.dWinProject().replace(/<\/div><\/div>\s*$/, `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:center;justify-content:center;z-index:5"><div class="am-sheet" role="dialog" aria-label="Bring uncommitted work"><div class="am-head"><b>Bring uncommitted work</b></div><div class="am-sub">Destination: build-box. Both checkouts must be at the same commit and the destination must be clean. Edits arrive unstaged; the source copy stays intact.</div><button class="dbtn pri">Bring work from laptop</button></div></div></div></div>`).replace('class="cc-win"', 'class="cc-win" style="position:relative"');
  DESK_SCREENS.find(g => g.group === 'Command center').items.push(['dProjectHandoff', 'Project: bring uncommitted work', 'ProjectHandoff']);
  DESK_SCREENS.find(g => g.group === 'Command center').items.push(['dProjectSyncSettings', 'Project settings: file sync', 'ProjectSettings']);
  if (["dProjectSyncSettings", "dProjectHandoff"].includes(location.hash.slice(1))) { S.screen = location.hash.slice(1); render(); }
})();
