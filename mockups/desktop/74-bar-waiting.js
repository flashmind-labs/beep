// ---- Bar: sessions waiting for you ----
// A double tap of Option opens the bar on the sessions that wait for the user: questions and
// approvals first, then idle sessions with an unread finished turn, then running ones (a dot and
// what they do), then other recent idle sessions. Newest first in each, at most ten rows. The list
// has the keys: arrows move, Return opens the session in the bar with its composer focused, typing
// filters by title or project, Esc clears the filter, then hides. Esc in a session comes back
// here. With nothing recent the bar opens on the new agent form, as before.
(() => {
  const { av } = AGENT_MOCK;
  const ROWS = [
    ["claude", "Fix the flaky login test", "acme/api", "build-box", "2 min", "ask", "asks"],
    ["codex", "Bump the toolchain", "acme/widget", "gpu-box", "6 min", "done", "idle"],
    ["claude", "Review the sync retry change", "acme/api", "this Mac", "1 min", "run", "working", "Running cargo test"],
    ["claude", "Draft the release notes", "acme/site", "this Mac", "1 h", "idle", "idle"],
    ["flsh", "Rename the settings pages", "acme/widget", "build-box", "3 h", "idle", "idle"],
  ];
  const tail = ([, , , , , st, , doing]) => st === "ask" ? '<span class="dstate ask">Needs you</span>'
    : st === "done" ? '<span class="dstate run">Done</span>'
    : st === "run" ? `<span class="run" title="${doing}"><span class="dspin"></span><span class="tx">${doing}</span></span>`
    : '<span class="w">Idle</span>';
  D.dWaiting = () => {
    const v = S.d.wt || "list";
    if (v === "empty") return D.dAgentNew();
    const q = v === "filter" ? "api" : "";
    const rows = ROWS.filter(r => !q || `${r[1]} ${r[2]}`.toLowerCase().includes(q));
    const sel = Math.min(S.d.sel || 0, rows.length - 1);
    const mark = s => q ? s.replace(new RegExp(q, "i"), m => `<mark>${m}</mark>`) : s;
    const need = ROWS.filter(r => r[5] === "ask" || r[5] === "done").length;
    const list = rows.map((r, i) => `<button class="dchat" role="option" aria-selected="${i === sel}" data-dact="dset:ags:${r[6]}" data-dgo="dBarSession">${av(r[0])}<span class="t"><span>${mark(r[1])}</span><small>${[mark(r[2]), r[3], r[4]].join(" · ")}</small></span>${tail(r)}</button>`).join("");
    return deskScene(dBar(`<div class="dhead"><span class="tt"><b>Sessions</b><small>${q ? `Filter: ${q}` : `${need} waiting for you`}</small></span><button class="dbtn" data-dgo="dAgentNew">New<kbd>⌘N</kbd></button></div>
      <div class="dlist top" role="listbox" aria-label="Sessions" tabindex="0">${list}</div>`
      + dHints([[DI.ret, "open"], ["↑↓", "move"], ["⌘N", "new agent"], ["esc", q ? "clear" : "to hide"]])));
  };
  VARIANTS.dWaiting = dv("wt", [["Waiting list", "list"], ["Filtering", "filter"], ["Nothing recent", "empty"]]);
  // A session opened in the bar starts with its pane folded to a strip under
  // the conversation; the strip or ⌘2 ⌘3 ⌘4 unfold it for this visit. ⌘O, or
  // the button in the key row, hands the session to the main window.
  D.dBarSession = () => {
    const open = S.d.bs === "unfolded", saved = S.d.ags;
    S.d.ags = "idle";
    const t = document.createElement("template");
    t.innerHTML = D.dAgentSession();
    S.d.ags = saved;
    const head = t.content.querySelector("#aguse").closest(".dhead"), shell = t.content.querySelector(".ag-shell");
    head.querySelector(".agedit")?.remove();
    shell.classList.add("dock-bottom");
    if (!open) {
      shell.querySelectorAll(".ag-pane, .ag-div").forEach(el => el.remove());
      shell.insertAdjacentHTML("beforeend", '<button class="ag-pstrip" data-dact="dset:bs:unfolded" title="Show the pane"><span>Files</span><small>Changes 3</small></button>');
    }
    shell.querySelector(".dhints.slim").insertAdjacentHTML("beforeend", '<button class="win" data-dgo="dAgentSession"><kbd>⌘O</kbd>open in window</button>');
    return deskScene(dBar(`${head.outerHTML}<div class="dbarag" style="container-type:inline-size;display:flex;flex-direction:column;height:560px">${shell.outerHTML}</div>`));
  };
  VARIANTS.dBarSession = dv("bs", [["Pane folded", "folded"], ["Pane unfolded", "unfolded"]]);
  DESK_SCREENS.find(g => g.group === "Floating bar").items.unshift(["dWaiting", "Waiting for you", "Bar on Option double tap"], ["dBarSession", "Session in the bar", "Pane folded, ⌘O to the window"]);
})();
