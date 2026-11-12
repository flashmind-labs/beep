// ---- Desktop: Import a project, typing a folder ----
// Mirrors folderLine, lookFolders and onFolderKey in importproject.ts and foldercomplete.ts. The
// Folder field takes a typed path (absolute or ~/). As it is typed, a menu lists the folders in the
// typed path's parent whose names start with the last part, ignoring case, sorted, at most 12, read
// from the picked machine with ListMachineFolder. Up and Down move, Tab or Enter take a folder and
// list its own folders next, Esc closes the menu (a second Esc closes the sheet). An offline
// machine or an error shows no menu. Enter with no menu reads the folder and imports.
(() => {
  const st = document.createElement("style");
  st.textContent = `.ip-folder { position: relative; flex: 1; min-width: 0; display: flex }
.ip-folder .ip-path.focus { border-color: var(--d-emerald) }
.ip-folder .caret { display: inline-block; width: 1px; height: 14px; background: var(--d-emerald); vertical-align: -2px; margin-left: 1px }
.ip-menu { position: absolute; top: calc(100% + 4px); left: 0; right: 0; z-index: 6; padding: 4px 0; border: 1px solid var(--d-border); border-radius: 8px; background: #0f1011; box-shadow: 0 10px 30px rgba(0,0,0,.45) }
.ip-menu button { all: unset; box-sizing: border-box; display: block; width: 100%; padding: 4px 11px; font-family: ui-monospace, Menlo, monospace; font-size: 12.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; cursor: pointer }
.ip-menu button:hover { background: rgba(255,255,255,.04) }
.ip-menu button.on { background: rgba(94,106,210, .14) }`;
  document.head.append(st);

  const typedDir = "/srv/projects/", typedPrefix = "a";
  const found = ["acme", "admin-tools", "api-gateway", "archive", "atlas"];
  const field = (open) => `<div class="ip-folder"><div class="ip-path focus" title="${typedDir}${typedPrefix}">${typedDir}${typedPrefix}<span class="caret"></span></div>${open ? `<div class="ip-menu" role="listbox">${found.map((d, i) =>
    `<button type="button" role="option" aria-selected="${i === 1}" class="${i === 1 ? "on" : ""}" data-toast="Lists ${typedDir}${d}/">${d}</button>`).join("")}</div>` : ""}</div>`;

  const base = D.dImportProject;
  D.dImportProject = (...a) => {
    const v = S.d.ip;
    if (v !== "complete") return base(...a);
    // The "Filled in" sheet on build-box, with the folder typed and its menu open.
    S.d.ip = "typed";
    let html = base(...a);
    S.d.ip = v;
    html = html.replace(/<div class="ip-path"[^>]*>[^<]*<\/div>/, field(true))
      .replace('<div class="ip-field focus">', '<div class="ip-field">')
      .replace(/<span class="caret"><\/span><\/div><\/div>\s*<div style="font-size:13px;color:var\(--d-muted\);margin-bottom:8px">Machine/, '</div></div>\n    <div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">Machine');
    return html;
  };
  VARIANTS.dImportProject.splice(3, 0, ["Typing a folder on build-box", () => { S.d.ip = "complete"; S.d.pp = "list"; render(); }, () => S.d.ip === "complete"]);
})();
