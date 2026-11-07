// Project file-sync ownership is set per checkout, without pausing its share.
(() => {
  const control = () => {
    const enabled = S.d.stFiles !== 'off';
    return `<div class="am-label">Syncthing file sync</div><label class="am-line"><input type="checkbox" ${enabled ? "checked" : ""} data-dact="dset:stFiles:${enabled ? "off" : "on"}"> Keep this project synced by Syncthing</label><div class="am-sub">Turn off to let Beep sync this project. Other projects in the shared folder keep syncing.</div>`;
  };
  const base = D.dImportProject;
  D.dImportProject = (...args) => base(...args).replace('<div class="ip-foot">', control() + '<div class="ip-foot">');
  const folder = D.dAddFolder;
  D.dAddFolder = (...args) => folder(...args).replace('<div class="ip-foot">', control() + '<div class="ip-foot">');
  const found = D.dAddProject;
  D.dAddProject = (...args) => found(...args).replace('<div class="ap-ways">', control() + '<div class="ap-ways">');
  D.dProjectSyncSettings = () => {
    const enabled = S.d.stFiles !== 'off';
    const panel = `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:center;justify-content:center;z-index:5"><div class="am-sheet" role="dialog" aria-label="Project settings"><div class="am-head"><b>Project settings</b><button class="dbtn quiet" data-dgo="dWinProject">Close</button></div><div class="am-label">build-box</div><button class="dbtn" role="switch" aria-checked="${enabled}" data-dact="dset:stFiles:${enabled ? 'off' : 'on'}">Syncthing file sync: ${enabled ? 'On' : 'Off'}</button><div class="am-sub">${enabled ? 'Syncthing syncs this project. Beep leaves it alone.' : 'Beep syncs this project. Other projects in the shared folder keep syncing.'}</div></div></div>`;
    return D.dWinProject().replace(/<\/div><\/div>\s*$/, panel + '</div></div>').replace('class="cc-win"', 'class="cc-win" style="position:relative"');
  };
  DESK_SCREENS.find(g => g.group === 'Command center').items.push(['dProjectSyncSettings', 'Project settings: file sync', 'ProjectSettings']);
})();
