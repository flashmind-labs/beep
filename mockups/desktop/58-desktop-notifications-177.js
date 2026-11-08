// ---- Desktop: notifications (#177) ----
// The activity log's pane: a button at the top right of every command center page, with the unread
// count, opens the entries, newest first. A row opens the page the entry names and counts as read:
// CI failed opens that run; a PR comment opens the PR in its project, scrolled to the comment. The
// round control completes an entry, for the user or the agent; completed entries move under Completed
// and say who did it. Nothing here animates.
// The assistant's alerts (a mail digest, an arrival, a review request) are notices here, not chats:
// they are not in the sidebar's Chats, the bar's Recent or search, and nothing opens by itself. A
// macOS notification goes out with each, and a click opens it in this pane. A row shows the source,
// the time and an unread dot; the cross dismisses it and Clear dismisses them all. Opening one reads
// it and shows its text with a box to reply; the first reply turns it into a normal chat that is in
// Recent from then on. A permission ask still goes to every desktop and phone; a notice that asks a
// question is answered from that box. A server without notices, or none, shows no notice rows.
// From the mesh, with no server (#324), kept on this Mac (the last 200, 30 days): a coding agent
// finished or asks (Allow and Deny on the row; Answered once answered here or anywhere), sync
// conflicts (Settled once settled), a session move that failed, and a machine offline for two
// minutes (Back online when it returns). A row opens its session, the Sync page or Machines; a skill
// conflict opens in the pane with its copies (#358); Keep this one makes that copy the skill everywhere (#359).
// Builds (#338): a run the user started (a push or a pull request) on a pinned project that passed,
// failed or was cancelled, "Build failed on <branch>" with the project under it. One machine with
// the checkout watches each project and tells every desktop once. A row opens the project on its
// Actions tile with that run open; a macOS notification says the same.
// mock-229
{
  const st = document.createElement("style");
  st.textContent = `.mac .nf-bell { display: inline-flex; align-items: center; gap: 6px; height: 28px; padding: 0 9px; margin-left: auto; flex: none; border-radius: 8px; border: 1px solid var(--d-border); background: transparent; color: var(--d-fg); font-size: 12px; cursor: pointer }
.mac .cc-top .tools ~ .nf-bell { margin-left: 10px }
.mac .nf-bell[aria-expanded="true"] { background: var(--d-elev) }
.mac .nf-n { min-width: 16px; height: 16px; padding: 0 4px; border-radius: 8px; display: grid; place-items: center; background: var(--d-emerald); color: #fff; font-size: 11px; font-weight: 600; font-variant-numeric: tabular-nums }
.mac .nf { position: absolute; top: 46px; right: 12px; width: 460px; max-width: calc(100% - 24px); max-height: calc(100% - 58px); z-index: 8; display: flex; flex-direction: column; border: 1px solid var(--d-border); border-radius: 10px; background: var(--d-surface); color: var(--d-fg); box-shadow: 0 18px 50px rgba(0,0,0,.5); overflow: hidden }
.mac .nf-h { flex: none; display: flex; align-items: center; gap: 8px; padding: 10px 14px; border-bottom: 1px solid var(--d-border); font-size: 12px; color: var(--d-muted) }
.mac .nf-h b { font-size: 14px; font-weight: 600; color: var(--d-fg) }
.mac .nf-h .dbtn { margin-left: auto; padding: 3px 10px; font-size: 12px; min-height: 24px }
.mac .nf-list { overflow: auto; scrollbar-width: thin; min-height: 0 }
.mac .nf-row { display: grid; grid-template-columns: 20px minmax(0, 1fr) auto 22px; gap: 10px; align-items: center; padding: 9px 14px; border-top: 1px solid var(--d-border); cursor: pointer; font-size: 13px }
.mac .nf-list > .nf-row:first-child { border-top: 0 }
.mac .nf-row:hover { background: var(--d-elev) }
.mac .nf-row:focus-visible, .mac .nf-ck:focus-visible { outline: 2px solid var(--d-emerald); outline-offset: -2px }
.mac .nf-ic { display: grid; place-items: center; color: var(--d-muted) }
.mac .nf-ic.bad { color: var(--d-err) } .mac .nf-ic.ok { color: #4cb782 } .mac .nf-ic.ac { color: var(--d-emerald) }
.mac .nf-ic .cc-ag { width: 20px; height: 20px; border-radius: 5px; font-size: 8.5px }
.mac .nf-row .t { white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .nf-row.unread .t { font-weight: 600 }
.mac .nf-row .sub { display: flex; gap: 6px; align-items: center; font-size: 11.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden }
.mac .nf-when { display: flex; gap: 6px; align-items: center; font-size: 11.5px; color: var(--d-muted); white-space: nowrap; font-variant-numeric: tabular-nums }
.mac .nf-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--d-emerald) }
.mac .nf-ck { width: 20px; height: 20px; padding: 0; border-radius: 50%; border: 1px solid var(--d-border); background: transparent; color: var(--d-muted); display: grid; place-items: center; cursor: pointer }
.mac .nf-ck:hover { border-color: var(--d-muted); color: var(--d-fg) }
.mac .nf-row.done .nf-ck { background: var(--d-elev); border-color: var(--d-muted); color: var(--d-fg) }
.mac .nf-row.done :is(.nf-ic, .t, .nf-when) { opacity: .55 }
.mac .nf-by { border: 1px solid var(--d-border); border-radius: 8px; padding: 0 6px; font-size: 10.5px; flex: none }
.mac .nf-empty { padding: 26px 14px; text-align: center; font-size: 13px; color: var(--d-muted) }
.mac .nf .nb-lg { border-top: 1px solid var(--d-border) }
.mac .nb-db { grid-auto-rows: max-content }
.mac .nb-post.nf-on { border-color: var(--d-emerald); box-shadow: inset 2px 0 var(--d-emerald) }
.mac .nb-post.nf-on > div:first-child { display: flex; gap: 6px; align-items: center; background: var(--d-elev) }
.mac .nb-post.nf-on > div:first-child .dbtn { margin-left: auto }
.mac .nf-h .dbtn + .dbtn, .mac .nf-h .dbtn.quiet:first-child { margin-left: 0 }
.mac .nf-note { padding: 14px; display: grid; gap: 8px; align-content: start }
.mac .nf-note b { font-size: 14px; font-weight: 600; line-height: 20px }
.mac .nf-note p { margin: 0; font-size: 13.5px; line-height: 20px }
.mac .nf-rep { flex: none; display: flex; gap: 8px; align-items: center; padding: 10px 14px; border-top: 1px solid var(--d-border) }
.mac .nf-rep .in { flex: 1; min-width: 0; display: flex; align-items: center; gap: 2px; height: 30px; padding: 0 10px; border: 1px solid var(--d-border); border-radius: 8px; color: var(--d-muted); font-size: 13px }
.mac .nf-rep .caret { width: 1px; height: 14px; background: var(--d-fg) }
.mac .nf-acts { display: flex; gap: 6px; margin-top: 6px }
.mac .nf-acts .dbtn { min-height: 22px; padding: 0 9px; font-size: 12px; margin-left: 0 }
.mac .nf-row.said :is(.nf-ic, .t) { opacity: .55 }`;
  document.head.append(st);

  const svg = (d, size = 14) => `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;
  const BELL = svg('<path d="M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 21h4"/>');
  const CHECK = svg('<path d="M5 12.5l4.5 4.5L19 7.5"/>', 12);
  const NOTICES = [
    { id: "m1", k: "notice", notice: true, t: "3 new emails: a lease renewal from Ana and two newsletters.", w: "Fastmail", at: "4 min",
      text: "3 new emails: a lease renewal from Ana and two newsletters. Archive the newsletters and draft a reply to Ana?" },
    { id: "m2", k: "notice", notice: true, t: "Sam asked for your review on acme/widget #412.", w: "GitHub", at: "20 min",
      text: "Sam asked for your review on acme/widget #412, a fix for the retry loop in the sync client. Want me to review it?" },
    { id: "m3", k: "notice", notice: true, read: true, t: "Arrived in Girona. Your hotel check-in opens at 15:00.", w: "Places", at: "4 h",
      text: "Arrived in Girona. Your hotel check-in opens at 15:00, and the train back is at 18:40 on Sunday." },
  ];
  const live = n => !n.gone && !(S.d.nfnonote && n.notice) && !(S.d.nfmesh && !n.mesh);
  const KIND = {
    notice: `<span class="nf-ic ac" title="Beep">${svg('<circle cx="12" cy="12" r="9"/><path d="M12 7.5v5.5M12 16.5v.01"/>')}</span>`,
    ci: `<span class="nf-ic bad" title="CI">${svg('<circle cx="12" cy="12" r="9"/><path d="M9 9l6 6M15 9l-6 6"/>')}</span>`,
    comment: `<span class="nf-ic ac" title="Comment">${svg('<path d="M4 5h16v11H9l-5 4z"/>')}</span>`,
    claude: '<span class="nf-ic" title="Coding agent"><span class="cc-ag claude">CC</span></span>',
    codex: '<span class="nf-ic" title="Coding agent"><span class="cc-ag codex">CX</span></span>',
    merged: `<span class="nf-ic ok" title="Merged">${svg('<circle cx="6" cy="5" r="2.2"/><circle cx="6" cy="19" r="2.2"/><circle cx="18" cy="13" r="2.2"/><path d="M6 7.2v9.6M6 7.2c0 3.5 3 5.8 9.8 5.8"/>')}</span>`,
    sync: `<span class="nf-ic ok" title="File sync">${svg('<path d="M20 11a8 8 0 0 0-14-4.5L4 9M4 4v5h5M4 13a8 8 0 0 0 14 4.5l2-2.5M20 20v-5h-5"/>')}</span>`,
    conflict: `<span class="nf-ic bad" title="Sync conflict">${svg('<path d="M20 11a8 8 0 0 0-14-4.5L4 9M4 4v5h5M4 13a8 8 0 0 0 14 4.5l2-2.5M20 20v-5h-5"/>')}</span>`,
    skill: `<span class="nf-ic bad" title="Skill conflict">${svg('<path d="M5 19l9-9"/><path d="M14 4l1.2 2.8L18 8l-2.8 1.2L14 12l-1.2-2.8L10 8l2.8-1.2z"/>')}</span>`,
    move: `<span class="nf-ic bad" title="Session move">${svg('<path d="M5 12h14M13 6l6 6-6 6"/>')}</span>`,
    build: `<span class="nf-ic bad" title="Build">${svg('<circle cx="12" cy="12" r="9"/><path d="M9 9l6 6M15 9l-6 6"/>')}</span>`,
    buildok: `<span class="nf-ic ok" title="Build">${svg('<circle cx="12" cy="12" r="9"/><path d="M8 12.5l3 3 5-6"/>')}</span>`,
    machine: `<span class="nf-ic" title="Machine">${svg('<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>')}</span>`,
  };
  // From the mesh: kept on this Mac, shown with or without a server.
  const MESH = [
    { id: "a1", k: "claude", mesh: true, ask: true, t: "Claude Code on build-box asks", w: "Run cargo test -p beep-node?", at: "2 min", ags: "asking" },
    { id: "c1", k: "conflict", mesh: true, t: "2 sync conflicts in acme/widget", w: "studio", at: "15 min", sync: true },
    { id: "a2", k: "codex", mesh: true, read: true, said: "Answered", t: "Codex on studio asks", w: "Delete the old fixtures?", at: "25 min", ags: "idle" },
    { id: "v1", k: "move", mesh: true, t: "Moving Fix the flaky login test to studio failed", w: "No checkout of acme/widget there", at: "50 min", ags: "idle" },
    { id: "o1", k: "machine", mesh: true, t: "gpu-box went offline", w: "gpu-box", at: "1 h", mach: true },
    { id: "o2", k: "machine", mesh: true, read: true, said: "Back online", t: "build-box went offline", w: "build-box", at: "6 h", mach: true },
    // The same skill name with other files on two machines, or in two agents' folders. Beep changes neither copy.
    { id: "s1", k: "skill", mesh: true, t: "Skill release-notes differs on build-box and studio", w: "Skills", at: "40 min", copies: [["build-box", "~/.claude/skills", false, "Changed 23:11 · 34 KB", true], ["studio", "~/.agents/skills", false, "Changed 22:12 · 15 KB"], ["This Mac", "~/.claude/skills", true, "Changed 22:12 · 15 KB"]] },
    // A container offline for a day is removed.
    { id: "o3", k: "machine", mesh: true, t: "ci-runner was removed after a day offline", w: "ci-runner", at: "2 h" },
  ];
  // Builds the user started, from the machine that watches the project (#338).
  const BUILDS = [
    { id: "b1", k: "build", mesh: true, t: "Build failed on cache-warmup", w: "acme/widget", at: "6 min", psel: "ci351" },
    { id: "b2", k: "buildok", mesh: true, read: true, t: "Build passed on main", w: "acme/widget", at: "45 min", psel: "actions" },
  ];
  const NF = [
    MESH[0],
    BUILDS[0],
    NOTICES[0],
    MESH[1],
    { id: "n1", k: "ci", t: "CI #351 failed on main", w: "acme/widget", at: "12 min", psel: "ci351" },
    NOTICES[1],
    MESH[2],
    { id: "n2", k: "comment", t: "kim commented on #41 Cut the cache warm-up in half", w: "acme/widget", at: "38 min", psel: "pr41", cmt: true },
    MESH[3],
    MESH[6],
    { id: "n3", k: "claude", mesh: true, t: "Claude Code finished Fix the flaky login test", w: "build-box", at: "1 h", ags: "idle" },
    MESH[4],
    { id: "n4", k: "merged", t: "#36 Drop the v1 cache reader merged", w: "acme/widget", at: "2 h", psel: "pr36" },
    { id: "n5", k: "sync", t: "File sync finished for acme/widget", w: "studio", at: "3 h", read: true, psel: "sync" },
    BUILDS[1],
    NOTICES[2],
    MESH[5],
    MESH[7],
    { id: "n6", k: "ci", t: "CI #349 failed on main", w: "acme/widget", at: "5 h", by: "agent", psel: "ci349" },
    { id: "n7", k: "codex", t: "Codex finished Bump the toolchain", w: "build-box", at: "1 d", by: "you", ags: "idle" },
  ];
  const unread = () => S.d.nfempty ? 0 : NF.filter(n => live(n) && !n.read && !n.by).length;
  const nrow = n => `<div class="nf-row${n.by ? " done" : n.said ? " said" : n.read ? "" : " unread"}" data-nf-go="${n.id}" role="button" tabindex="0">${KIND[n.k]}
    <span style="min-width:0"><div class="t">${n.t}</div><div class="sub"><span>${n.w}</span>${n.by ? `<span class="nf-by">Completed by ${n.by}</span>` : ""}${n.said ? `<span class="nf-by">${n.said}</span>` : ""}</div>${n.ask && !n.said ? `<div class="nf-acts"><button class="dbtn pri" data-nf-allow="${n.id}">Allow</button><button class="dbtn" data-nf-deny="${n.id}">Deny</button></div>` : ""}</span>
    <span class="nf-when">${n.by || n.read ? "" : '<span class="nf-dot"></span>'}${n.at}</span>
    ${n.notice || n.mesh ? `<button class="nf-ck" data-nf-dismiss="${n.id}" aria-label="Dismiss" title="Dismiss">${svg('<path d="M7 7l10 10M17 7L7 17"/>', 11)}</button>` : `<button class="nf-ck" data-nf-done="${n.id}" aria-label="${n.by ? "Reopen" : "Complete"}" title="${n.by ? "Reopen" : "Complete"}">${n.by ? CHECK : ""}</button>`}</div>`;
  // One opened notice: its text and a box to reply. Replying makes it a chat.
  const noteView = () => {
    const n = NF.find(x => x.id === S.d.nfnote);
    return `<div class="nf" role="dialog" aria-label="Notification"><div class="nf-h"><button class="dbtn quiet" data-nf-back>Notifications</button><span>${n.w} · ${n.at}</span><button class="dbtn" data-nf-dismiss="${n.id}">Dismiss</button></div><div class="nf-list nf-note"><b>${n.t}</b><p>${n.text}</p></div><div class="nf-rep"><span class="in"><span class="caret"></span>Reply to turn this into a chat</span><button class="dbtn pri" data-nf-chat="${n.id}">Send</button></div></div>`;
  };
  // A skill conflict opened: each copy on its own line, Show in Finder on this Mac's. Beep changes neither.
  const skillView = n => `<div class="nf" role="dialog" aria-label="Notification"><div class="nf-h"><button class="dbtn quiet" data-nf-back>Notifications</button><span>${n.w} · ${n.at}</span><button class="dbtn" data-nf-dismiss="${n.id}">Dismiss</button></div><div class="nf-list nf-note"><b>${n.t}</b>${n.copies.map(([m, p, here, when, newest]) => `<div style="display:flex;align-items:center;gap:8px;padding:6px 0;border-top:1px solid var(--d-border)"><span style="flex:1;min-width:0;display:grid"><span><b style="font-weight:500">${m}</b> <span style="color:var(--d-muted)">${p}</span></span><small style="color:var(--d-dim)">${when}${newest ? " · newest" : ""}</small></span>${here ? '<button class="dbtn quiet" data-toast="Opens the folder in Finder">Show in Finder</button>' : ""}<button class="dbtn${newest ? " pri" : ""}" data-toast="Kept the copy on ${m}; the others are replaced on every machine">Keep this one</button></div>`).join("")}<p style="color:var(--d-muted)">Keep one copy and it replaces the others on every machine and in every agent's folder.</p></div></div>`;
  const pane = () => {
    if (S.d.nfnote) { const n = NF.find(x => x.id === S.d.nfnote); return n.copies ? skillView(n) : noteView(); }
    const open = NF.filter(n => live(n) && !n.by), done = NF.filter(n => live(n) && n.by), un = unread();
    const body = S.d.nfempty ? '<div class="nf-empty">No notifications</div>'
      : open.map(nrow).join("") + (done.length ? `<div class="nb-lg">Completed<span class="c">${done.length}</span></div>` + done.map(nrow).join("") : "");
    return `<div class="nf" role="dialog" aria-label="Notifications"><div class="nf-h"><b>Notifications</b>${un ? `<span>${un} unread</span>` : ""}${!S.d.nfempty && open.some(n => n.notice || n.mesh) ? '<button class="dbtn" data-nf-clear>Clear</button>' : ""}${!S.d.nfempty && open.some(n => !n.notice && !n.mesh) ? '<button class="dbtn" data-nf-all>Complete all</button>' : ""}</div><div class="nf-list">${body}</div></div>`;
  };
  const bellHtml = () => { const un = unread(); return `${BELL}${un ? `<span class="nf-n">${un}</span>` : ""}`; };
  // Redraws only the button and the pane, so completing a row leaves the page and its scroll alone.
  const paint = () => {
    const win = document.querySelector("#desk .cc-win"), bell = win && win.querySelector(".nf-bell");
    if (!bell) return;
    const old = win.querySelector(".nf"), list = old && old.querySelector(".nf-list"), keep = list ? list.scrollTop : 0;
    bell.innerHTML = bellHtml();
    bell.setAttribute("aria-expanded", String(!!S.d.nfopen));
    bell.setAttribute("aria-label", `Notifications, ${unread()} unread`);
    old?.remove();
    if (S.d.nfopen) { win.insertAdjacentHTML("beforeend", pane()); win.querySelector(".nf-list").scrollTop = keep; }
  };
  const decorate = () => {
    const win = document.querySelector("#desk .cc-win"), top = win && win.querySelector(".cc-top");
    if (!top) return;
    top.insertAdjacentHTML("beforeend", '<button class="nf-bell" data-nf-bell title="Notifications (⌘B)"></button>');
    paint();
    if (S.screen === "dWinProject" && S.d.nfjump) {
      S.d.nfjump = false;
      const into = (box, el, pad) => { if (box && el) box.scrollTop += el.getBoundingClientRect().top - box.getBoundingClientRect().top - pad; };
      into(win.querySelector(".pj-list"), win.querySelector('.pj-li[aria-current="true"]'), 60);
      into(win.querySelector(".pj-det"), win.querySelector(".pj-post.nf-on"), 14);
    }
  };
  const baseRender = render;
  render = function () { baseRender(); decorate(); };
  // A row opens the entry's page and closes the pane. Builds leave the log when opened.
  const open = n => {
    n.read = true;
    if (n.notice || n.copies) { S.d.nfnote = n.id; S.d.nfopen = true; S.d.nfempty = false; render(); return; }
    if (n.k === "build" || n.k === "buildok") n.gone = true;
    S.d.nfnote = ""; S.d.nfopen = false; S.d.nfempty = false; S.d.nfc = "";
    if (n.ags) { S.d.ags = n.ags; go("dAgentSession"); return; }
    if (n.sync) { go("dSync"); return; }
    if (n.mach) { go("dMachines"); return; }
    S.d.psel = n.psel; S.d.nfc = n.cmt ? "1" : ""; S.d.nfjump = true;
    go("dWinProject");
  };
  const desk = document.getElementById("desk");
  desk.addEventListener("click", e => {
    if (e.target.closest("[data-nf-bell]")) { S.d.nfnote = ""; S.d.nfopen = !S.d.nfopen; paint(); return; }
    if (e.target.closest("[data-nf-back]")) { S.d.nfnote = ""; paint(); return; }
    const dis = e.target.closest("[data-nf-dismiss]");
    if (dis) { NF.find(x => x.id === dis.dataset.nfDismiss).gone = true; S.d.nfnote = ""; paint(); return; }
    if (e.target.closest("[data-nf-clear]")) { NF.forEach(n => { if (n.notice || n.mesh) n.gone = true; }); paint(); return; }
    const said = e.target.closest("[data-nf-allow], [data-nf-deny]");
    if (said) { const n = NF.find(x => x.id === (said.dataset.nfAllow || said.dataset.nfDeny)); n.said = said.dataset.nfAllow ? "Allowed" : "Denied"; n.read = true; paint(); return; }
    const chat = e.target.closest("[data-nf-chat]");
    if (chat) { NF.find(x => x.id === chat.dataset.nfChat).gone = true; S.d.nfnote = ""; S.d.nfopen = false; S.d.wc = "update"; go("dWinChat"); return; }
    const ck = e.target.closest("[data-nf-done]");
    if (ck) { const n = NF.find(x => x.id === ck.dataset.nfDone); if (n.by) delete n.by; else n.by = "you"; paint(); desk.querySelector(`[data-nf-done="${n.id}"]`).focus(); return; }
    if (e.target.closest("[data-nf-all]")) { NF.forEach(n => { if (!n.by && !n.notice) n.by = "you"; }); paint(); return; }
    const row = e.target.closest("[data-nf-go]");
    if (row) { open(NF.find(x => x.id === row.dataset.nfGo)); return; }
    if (S.d.nfopen && !e.target.closest(".nf")) { S.d.nfopen = false; paint(); }
  });
  desk.addEventListener("keydown", e => {
    if ((e.key === "Enter" || e.key === " ") && e.target.matches?.("[data-nf-go]")) { e.preventDefault(); e.target.click(); }
  });
  document.addEventListener("keydown", e => {
    if (e.metaKey && !e.shiftKey && !e.altKey && !e.ctrlKey && e.key.toLowerCase() === "b" && document.querySelector("#desk .nf-bell") && !e.target.closest?.("[contenteditable=true], .tm-xterm")) { e.preventDefault(); S.d.nfnote = ""; S.d.nfopen = !S.d.nfopen; paint(); return; }
    if (e.key === "Escape" && S.d.nfopen) { e.stopImmediatePropagation(); S.d.nfnote = ""; S.d.nfopen = false; paint(); }
  }, true);

  // Its own screen in the index, to try the pane and the pages it opens.
  D.dNotifications = () => D.dHome();
  const baseGo = go;
  go = screen => { if (screen === "dNotifications") { if (S.screen !== screen) { S.d.nfopen = true; S.d.nfempty = false; } } else if (screen !== S.screen) S.d.nfopen = false; baseGo(screen); };
  const jump = id => () => open(NF.find(n => n.id === id));
  VARIANTS.dNotifications = [
    ["Pane", () => { NOTICES.forEach(n => { n.gone = false; n.read = n.id === "m3"; }); S.d.nfempty = false; S.d.nfnote = ""; S.d.nfnonote = false; S.d.nfmesh = false; S.d.nfopen = true; render(); }, () => !!S.d.nfopen && !S.d.nfempty && !S.d.nfnote && !S.d.nfnonote && !S.d.nfmesh],
    ["Notice opened", () => { NOTICES.forEach(n => { n.gone = false; }); S.d.nfempty = false; S.d.nfnonote = false; S.d.nfnote = "m1"; NOTICES[0].read = true; S.d.nfopen = true; render(); }, () => !!S.d.nfnote],
    ["No notices (older server or none)", () => { S.d.nfempty = false; S.d.nfnote = ""; S.d.nfmesh = false; S.d.nfnonote = true; S.d.nfopen = true; render(); }, () => !!S.d.nfnonote],
    ["No server: from the mesh", () => { NF.forEach(n => { if (n.mesh) n.gone = false; }); S.d.nfempty = false; S.d.nfnote = ""; S.d.nfnonote = false; S.d.nfmesh = true; S.d.nfopen = true; render(); }, () => !!S.d.nfmesh && !!S.d.nfopen],
    ["Empty", () => { S.d.nfempty = true; S.d.nfopen = true; render(); }, () => !!S.d.nfopen && !!S.d.nfempty],
    ["CI failed: the run", jump("n1"), () => false],
    ["Build failed: the run", jump("b1"), () => false],
    ["PR comment: the PR", jump("n2"), () => false],
    ...VARIANTS.dHome.filter(([l]) => l.startsWith("Skin:")),
  ];
  VARIANTS.dWinProjects.push(["Sidebar: a group collapsed", () => { S.d.pjs = "list"; S.d.pjfold = "northwind"; render(); }, () => S.d.pjfold === "northwind"]);
  VARIANTS.dWinProject.push(["Sidebar: its group collapsed", () => { S.d.pjfold = "acme"; render(); }, () => S.d.pjfold === "acme"]);
  VARIANTS.dWinProject.splice(2, 0, ["From a notification: PR comment", jump("n2"), () => S.d.psel === "pr41" && !!S.d.nfc], ["From a notification: CI run", jump("n1"), () => S.d.psel === "ci351"]);
  DESK_SCREENS.find(g => g.group === "Command center").items.splice(1, 0, ["dNotifications", "Notifications", "CommandCenterWindow, NotificationsPane"]);
  if (location.hash === "#dNotifications") S.screen = "dNotifications";
  if (S.screen === "dNotifications") S.d.nfopen = true;
  render();
}
