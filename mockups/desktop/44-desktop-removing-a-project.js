// ---- Desktop: removing a project ----
// Mirrors openProjectMenu and removeProject in center.ts and remove_project in projects.rs. Every
// project right-click menu (sidebar, Projects page rows and cards, Home's list) gains
// "Remove project…" under Open in New Window, for an imported project only; the project page has a
// quiet "Remove project…" in its header. Both ask first. The mesh forgets the import, turns sync
// off on every machine, stops the project's workers and terminals and deletes its pins. No file is
// deleted on any machine. Once removed, an open project page goes back
// to the Projects list.
{
  const sheet = back => `<div style="display:flex;justify-content:space-between;margin-bottom:10px"><b style="font-size:15px">Remove acme/widget?</b><kbd style="font-size:11px;opacity:.7">esc</kbd></div>
      <div style="font-size:13px;line-height:19px;margin-bottom:6px">Beep stops syncing it and its workers.</div>
      <div style="font-size:12.5px;color:var(--d-muted);line-height:18px;margin-bottom:14px">Files on your machines stay.</div>
      <div style="display:flex;gap:8px;justify-content:flex-end"><button class="dbtn quiet" data-dact="dset:prm:" data-dgo="${back}">Cancel</button><button class="dbtn warn" data-dact="dset:prm:" data-dgo="dWinProjects" data-toast="acme/widget removed. Its files stay on your machines">Remove project</button></div>`;
  const removeItem = `<button class="warn" role="menuitem" data-dact="dset:prm:confirm" data-dgo="dWinProjects">Remove project…</button>`;

  // The project menu: Open in New Window, then Remove project…
  const basePjs = D.dWinProjects;
  D.dWinProjects = (...a) => {
    const v = S.d.prm;
    if (v === "confirm") {
      const keep = S.d.pnw; S.d.pnw = "";
      const html = basePjs(...a);
      S.d.pnw = keep;
      return ccSheetOver(html, sheet("dWinProjects"));
    }
    const html = basePjs(...a);
    if (S.d.pnw !== "menu") return html;
    return html.replace(/(Open in New Window<kbd>[^<]*<\/kbd><\/button>)/, `$1${removeItem}`);
  };
  VARIANTS.dWinProjects = VARIANTS.dWinProjects.map(([l, f, on]) => [l, () => { S.d.prm = ""; f(); }, () => !S.d.prm && on()]);
  VARIANTS.dWinProjects.push(
    ["Remove a project", () => { S.d.pjs = "list"; S.d.pnw = ""; S.d.prm = "confirm"; render(); }, () => S.d.prm === "confirm"]);
}
