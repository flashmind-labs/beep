(() => {
  const { AGK, AG_ICON, HARNESS, av, tag, pick, variants, hint, HOSTS, SG, step, SUBS, SUBS_DISK, subRow, KIDS, kidRow } = AGENT_MOCK;
  // ----- Every session -----
  // The right-click menu on a session row; Delete reads in red and goes last.
  const agMenu = (title, top, items) => `<div class="cc-menu" role="menu" style="top:${top}px;left:150px"><span class="lbl">${title}</span>${items.map(t => t[0] === "!" ? `<button class="warn" role="menuitem" data-toast="Asks before closing">${t.slice(1)}</button>` : `<button role="menuitem" data-toast="${t}">${t}</button>`).join("")}<button class="warn" role="menuitem" data-toast="Asks before deleting">Delete</button></div>`;
  D.dAgents = () => {
    const agl = pick("agl", ["list", "endedopen", "onlyended", "empty", "menurun", "menukid", "menuended", "stopall", "copies", "closing", "closefailed"], "list"), empty = agl === "empty";
    const rows = [
      ["Fix the failing login test", "Claude Code · acme/widget · build-box", '<span class="run"><span class="dspin"></span>Running cargo test</span>', "working"],
      ["Build the iOS target", "Codex · acme/widget · ios-build · mac-box · started by Fix the failing login test", '<span class="dstate ask">Needs you</span>', "asks", true],
      // A review agent sits under the session it reviews, never with the running sessions (issue 468).
      ["Review", "Codex · reviewer · started by Fix the failing login test", '<span class="w">Reviewing</span>', "working", true],
      ["Render the turntable", "Claude Code · scenes · win-box", '<span class="w">Idle</span>', "idle"],
      ["Tidy the parser module", "Claude Code · relay · build-box", '<span class="w">In terminal</span>', "outside"],
    ];
    // Ended sessions fold under one group, closed by default, and say when they last ran (issue 591).
    const ended = [
      ["Update the changelog", "Codex · acme/widget · build-box", '<span class="w">12 min ago</span>', "ended"],
      ["Normalize machine names", "Claude Code · acme/widget · build-box", '<span class="w">3 h ago</span>', "ended"],
      ["Debug energy metrics", "Codex · acme/widget · build-box", '<span class="w">2 d ago</span>', "ended"],
    ];
    const live = agl === "onlyended" ? [] : agl === "closing" ? rows.filter((_, i) => i !== 2) : rows, open = agl === "endedopen" || agl === "menuended";
    const row = ([t, s, r, k, kid], i, cls = "") => `<button class="dchat${kid ? " kid" : ""}${cls}" role="option" aria-selected="${i === 0}" data-dact="dset:ags:${k}" data-dgo="dAgentSession"><span class="t"><span>${t}</span><small>${s}</small></span>${r}</button>`;
    const fold = `<button class="dagfold" aria-expanded="${open}" data-dact="dset:agl:${open ? "list" : "endedopen"}"><span class="chev">${open ? "&#9662;" : "&#9656;"}</span>Ended<span class="n">${ended.length}</span></button>`;
    const quiet = `<div class="dagquiet">Nothing running. Start one with New.</div>`;
    const list = `<div class="dlist" role="listbox">${live.length ? live.map((r, i) => row(r, i)).join("") : quiet}${fold}${open ? ended.map((r, i) => row(r, live.length + i, " gone")).join("") : ""}</div>`;
    const visible = live;
    const copyRow = (t, s, acts) => `<div class="dchat" style="display:flex;align-items:center;gap:10px;cursor:default"><span class="tt" style="flex:1;min-width:0"><b>${t}</b><small>${s}</small></span>${acts}</div>`;
    const copyRows = `<div class="dsec bare">Copies on this Mac</div>` + copyRow("widget · Fix the failing login test", "codex · build-box is offline. Copy from 3 min ago", '<button class="dbtn small pri" data-toast="Continuing here">Continue on this Mac</button>')
      + copyRow("widget · Build the iOS target", "Another machine continued this session later. This version is kept", '<button class="dbtn small" data-toast="Restored">Restore</button><button class="dbtn small quiet" data-toast="Discarded">Discard</button>');
    const none = `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--d-dim);display:grid">${AG_ICON}</span><b style="font-size:14px">No sessions running.</b>
        <span class="dmuted" style="display:block">Start one here, ask Beep in a chat, or run Claude Code in a terminal on a linked machine.</span></div>`;
    return ccWin(ccSidePj("agents"), `<div class="cc-top"><h2>Coding agents</h2><small>On your machines</small></div>
      <div class="ag-sess"><div class="dhead"><span class="tt"><b>Coding agents</b><small>${empty || !visible.length ? "Nothing running" : `${visible.length} live on 3 machines, 1 started by an agent`}</small></span><button class="dbtn" data-dgo="dAgentNew">${AG_ICON}New<kbd>⌘⇧A</kbd></button></div>
      ${empty ? none : `<div style="position:relative">${list}${agl === "copies" ? copyRows : ""}${agl === "menurun" ? agMenu("Fix the failing login test", 40, ["Open", "Stop turn", "Stop all", "Open in editor", "Open folder", "Copy id", "!Close session"]) : agl === "menukid" ? agMenu("Build the iOS target", 74, ["Open", "Open parent", "Stop turn", "Stop all", "Open folder", "Copy id", "!Close session"]) : agl === "menuended" ? agMenu("Update the changelog", 290, ["Open", "Resume", "Move to project…", "Open in editor", "Open folder", "Copy id"]) : ""}</div>`}</div>
      ${agl === "stopall" ? `<div class="am-scrim" style="position:fixed"><div class="am-sheet" role="dialog" aria-modal="true" aria-label="Stop all of this session's work?"><div class="am-head"><b>Stop all of this session's work?</b><kbd>esc</kbd></div>
        <div class="am-msg">Stops the turn, its subagents, background commands and the agents it started.</div><div class="am-sub">While subagents run, the agent itself is stopped. Your next message resumes it.</div>
        <div class="gp-foot"><button class="dbtn quiet" data-dact="dset:agl:list">Cancel</button><button class="dbtn warn" data-dact="dset:agl:list" data-toast="Stopped all">Stop all</button></div></div></div>` : ""}
      ${agl === "closefailed" ? `<div role="alert" style="position:fixed;inset:auto 16px 16px;z-index:1000;background:var(--d-bg);padding:10px 12px;border:1px solid var(--d-border);border-radius:8px">Could not close session: Machine is offline<button style="float:right" data-dact="dset:agl:list" aria-label="Dismiss">&times;</button></div>` : ""}
      ${hint([[DI.ret, "open"], ["⌘⇧A", "new"], ["esc", "back"]])}`);
  };
  VARIANTS.dAgents = variants("agl", [["List, ended folded", "list"], ["Ended open", "endedopen"], ["Only ended", "onlyended"], ["Empty", "empty"], ["Right-click, running", "menurun"], ["Right-click, a started agent", "menukid"], ["Right-click, ended", "menuended"], ["Stop all, confirm", "stopall"], ["Copies on this Mac", "copies"], ["Session removed immediately while closing", "closing"], ["Close failed", "closefailed"]], "list");
})();
