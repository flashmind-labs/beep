// ---- Desktop: a project in its own window ----
// Mirrors newWindowOn and start in center.js and open_project in center.rs. Right-click a project in
// the sidebar, on Home or on the Projects page for Open in New Window; Cmd-click and middle-click do
// the same. The new window is another command center window, the same size as the main one and a
// step down and right, titled with the project's name. It starts on the project and keeps its own
// history: Esc never leaves the project for Home. Opening the same project again brings its window
// up instead of a second one. Closing it leaves the main window and the app running.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .cc-win.pnw-back { filter: brightness(.72) saturate(.85); animation: none }
.mac .cc-win.pnw-front { top: 62px; left: 40px; right: -16px; bottom: -16px; z-index: 4; box-shadow: 0 34px 90px rgba(0,0,0,.7) }
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

{
  // The project's own window in front of the main one, which stays on the Projects page.
  const base = D.dWinProject;
  D.dWinProject = (...a) => {
    const html = base(...a);
    if (S.d.pnw !== "window") return html;
    const at = html.indexOf('<div class="cc-win"');
    const pjs = S.d.pjs;
    S.d.pjs = "list";
    const back = D.dWinProjects();
    S.d.pjs = pjs;
    const front = html.slice(at).replace('class="cc-win" role="main" aria-label="Beep"', 'class="cc-win pnw-front" role="main" aria-label="acme/widget"');
    return back.replace('class="cc-win"', 'class="cc-win pnw-back"') + front;
  };
  VARIANTS.dWinProject = VARIANTS.dWinProject.map(([l, f, on]) => [l, () => { S.d.pnw = ""; f(); }, () => !S.d.pnw && on()]);
  VARIANTS.dWinProject.push(["In its own window", () => { S.d.pj = "default"; S.d.pjv = "board"; S.d.pbo = ""; S.d.pnw = "window"; render(); }, () => S.d.pnw === "window"]);
}
