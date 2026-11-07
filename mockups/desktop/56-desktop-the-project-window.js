// ---- Desktop: the project window ----
// Mirrors projectwindow.ts and start in center.js. Open in New Window, Cmd-click or middle-click on a
// project opens a window with that project only: no sidebar, the project's name as the title and a row
// of tabs for its sections (the board's tiles). What opens from the project, an agent session, a
// terminal, a PR thread or a run, opens in this window with a back button to the project (Esc too).
// Home, other projects, chats outside the project and Settings open in the main window. Cmd-K lists
// the project's hits first; Cmd-W closes the window.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .cc-win.pw { grid-template-columns: minmax(0, 1fr) }
.mac .cc-win.pw .cc-top { padding-left: 84px; position: relative }
.mac .cc-win.pw .pw-lights { position: absolute; left: 14px; top: 50%; transform: translateY(-50%); display: flex; gap: 8px }
.mac .cc-win.pw .pw-lights i { width: 11px; height: 11px; border-radius: 50%; background: #34363C }
.mac .pw-tabs { flex: none; display: flex; gap: 2px; padding: 4px 16px 0; border-bottom: 1px solid var(--d-border); overflow-x: auto; scrollbar-width: none }
.mac .pw-tabs button { font: inherit; font-size: 12px; color: var(--d-muted); background: none; border: 0; border-bottom: 2px solid transparent; padding: 6px 10px 7px; white-space: nowrap; cursor: pointer }
.mac .pw-tabs button:hover { color: var(--d-fg) }
.mac .pw-tabs button[aria-current="true"] { color: var(--d-fg); border-bottom-color: var(--d-emerald) }
.mac .cc-win.pw .pb-back { display: none }
.mac .pw-back { padding: 2px 8px }`;
  document.head.append(st);
})();

{
  // What a frame draws in its window's main column, without the sidebar.
  const mainOf = frame => {
    const keep = ccWin;
    let main = "";
    ccWin = (side, m) => { main = m; return ""; };
    try { frame(); } finally { ccWin = keep; }
    return main;
  };
  const lights = '<span class="pw-lights"><i></i><i></i><i></i></span>';
  const win = main => ccWin("", main).replace('class="cc-win" role="main" aria-label="Beep"', 'class="cc-win pw" role="main" aria-label="acme/widget"');
  // The board's tiles as tabs; Overview is the board itself.
  const TABS = [["", "Overview"], ["tasks", "Tasks"], ["workers", "Agents"], ["branches", "Branches"], ["prs", "Pull requests"], ["issues", "Issues"], ["machines", "Machines"], ["terms", "Terminals"], ["sync", "File sync"]];
  const tabs = on => `<nav class="pw-tabs" aria-label="acme/widget">${TABS.map(([k, l]) =>
    `<button${k === on ? ' aria-current="true"' : ""} data-dact="dset:pbo:${k}" data-dgo="dWinProjectWindow">${l}</button>`).join("")}</nav>`;
  D.dWinProjectWindow = () => {
    const keep = { pj: S.d.pj, pjv: S.d.pjv, pnw: S.d.pnw, pbo: S.d.pbo, ags: S.d.ags };
    try {
      if (S.d.pw === "session") {
        // An agent session started from the project: in this window, back to the project on Esc.
        S.d.ags = "working";
        const m = mainOf(() => D.dAgentSession()).replace('<div class="cc-top">',
          `<div class="cc-top">${lights}<a href="#" class="dbtn pw-back" data-dact="dset:pw:" data-dgo="dWinProjectWindow" title="Back to acme/widget (esc)">widget</a>`);
        return win(m);
      }
      Object.assign(S.d, { pj: "default", pjv: "board", pnw: "" });
      const on = S.d.pbo || "";
      const m = mainOf(() => D.dWinProject()).replace('<div class="cc-top">', `<div class="cc-top">${lights}`)
        ;
      return win(m);
    } finally {
      Object.assign(S.d, keep);
    }
  };
  VARIANTS.dWinProjectWindow = [
    ["Project", () => { S.d.pw = ""; S.d.pbo = ""; render(); }, () => !S.d.pw && !S.d.pbo],
    ["Terminals", () => { S.d.pw = ""; S.d.pbo = "terms"; S.d.psel = "terminals"; render(); }, () => !S.d.pw && S.d.pbo === "terms"],
    ["Session opened", () => { S.d.pw = "session"; render(); }, () => S.d.pw === "session"]];
  DESK_SCREENS.find(g => g.group === "Command center").items.push(["dWinProjectWindow", "Project window", "ProjectWindow, ProjectView"]);
}
