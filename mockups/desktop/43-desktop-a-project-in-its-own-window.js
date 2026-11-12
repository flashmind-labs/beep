// ---- Desktop: a project in its own window ----
// Mirrors newWindowOn and start in center.js and open_project in center.rs. Right-click a project in
// the sidebar, on Home or on the Projects page for Open in New Window; Cmd-click and middle-click do
// the same. The new window is another command center window, the same size as the main one and a
// step down and right, titled with the project's name. It starts on the project and keeps its own
// history: Esc never leaves the project for Home. Opening the same project again brings its window
// up instead of a second one. Closing it leaves the main window and the app running.
(() => {
  const st = document.createElement("style");
  st.textContent = `
.mac .cc-si.pnw-on { background: rgba(227,228,230,.1); box-shadow: inset 0 0 0 1px var(--d-border) }
.mac .cc-side:has(.pnw-menu) { overflow: visible }
.mac .cc-menu.pnw-menu { top: 24px; left: 118px; width: max-content }
.mac .cc-menu.pnw-menu kbd { margin-left: 18px; opacity: .7 }`;
  document.head.append(st);
})();

{
  // The sidebar row for acme/widget, right-clicked: its menu opens where the row is.
  const base = D.dWinProjects;
  D.dWinProjects = (...a) => {
    const html = base(...a);
    if (S.d.pnw !== "menu") return html;
    const row = /<button class="cc-si pj-repo"((?:(?!<\/button>)[\s\S])*?<span class="tt">widget<\/span>[\s\S]*?<\/button>)/;
    const menu = `<div class="cc-menu pnw-menu" role="menu"><span class="lbl">acme/widget</span>
      <button class="on" role="menuitem" data-dact="dset:pnw:window" data-dgo="dWinProject">Open in New Window<kbd>⌘ click</kbd></button></div>`;
    return html.replace('class="cc-win"', 'class="cc-win" style="position:relative"')
      .replace(row, `<div style="position:relative"><button class="cc-si pj-repo pnw-on"$1${menu}</div>`);
  };
  VARIANTS.dWinProjects = VARIANTS.dWinProjects.map(([l, f, on]) => [l, () => { S.d.pnw = ""; f(); }, () => !S.d.pnw && on()]);
  VARIANTS.dWinProjects.push(["Project menu: new window", () => { S.d.pjs = "list"; S.d.pnw = "menu"; render(); }, () => S.d.pnw === "menu"]);
}

