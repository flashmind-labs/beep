// ---- Desktop: move a coding agent session to another machine (#237) ----
// Session header: the machine name is a menu ("Move to"). A turn that runs waits for Stop and move.
// While moving, steps tick above the composer and the message typed waits as queued. Lists show
// "laptop to studio" with a Moving chip; a failed move stays on the source with the reason in the row's title;
// an offline machine with a copy here offers Continue on this Mac.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .dhead { position: relative }
.mac .mvb { all: unset; cursor: pointer; display: inline-flex; align-items: center; gap: 4px; padding: 0 5px; margin-left: -5px; border-radius: 5px; font-size: 12px; color: var(--d-muted) }
.mac .mvb:hover:not(:disabled), .mac .mvb[aria-expanded="true"] { background: var(--d-elev, rgba(127,127,127,.12)); color: var(--d-fg) }
.mac .mvb:disabled { cursor: default }
.mac .mvb svg { opacity: .7 }
.mac .mvm { left: 50px; right: auto; top: calc(100% - 8px); width: 300px }
.mac .mvm .mvh { padding: 4px 8px 3px; font-size: 11px; color: var(--d-dim) }
.mac .mvm button[disabled] { opacity: .45; cursor: default }
.mac .mvm button[disabled]:hover { background: none }
.mac .mvm b { font-weight: 500 }
.mac .mvs { flex: none; display: grid; gap: 5px; padding: 8px 16px; border-top: 1px solid var(--d-border); font-size: 12.5px }
.mac .mvs .h { display: flex; align-items: center; gap: 8px; min-height: 24px }
.mac .mvs .h b { font-weight: 500 }
.mac .mvs .sp { flex: 1 }
.mac .mvs .t { color: var(--d-muted) }
.mac .mvs .r { display: flex; align-items: center; gap: 8px; font-variant-numeric: tabular-nums }
.mac .mvs .r.todo { color: var(--d-dim) }
.mac .mvs .mk { width: 14px; height: 14px; display: inline-grid; place-items: center; flex: none; color: var(--d-emerald) }
.mac .mvs .r.todo .mk::before { content: ""; width: 5px; height: 5px; border-radius: 50%; background: var(--d-dim) }
.mac .mvs .mk .dspin { width: 11px; height: 11px }
.mac .mvs.bar { display: flex; align-items: center; gap: 8px }
.mac .mvline { padding: 8px 0 2px; text-align: center; font-size: 12px; color: var(--d-dim) }`;
  document.head.append(st);

  const CHEV = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';
  const OK = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5 9-9"/></svg>';
  const MACH = [["studio", "folder ready", true], ["laptop", "folder ready", true], ["mini", "no checkout: import first", false], ["build-box", "no Claude Code here", false], ["old-box", "offline: moves when it returns", true, "offline"]];
  const WT = [["studio", "worktree on feat/export", true], ["laptop", "worktree on feat/export", true], ["mini", "Syncthing is set not to sync this worktree", false], ["build-box", "no Claude Code here", false], ["old-box", "offline: moves when it returns", true, "offline"]];
  const QUEUED = '<div class="ag-queue"><span class="lbl">Queued</span><span class="t">Also update the error text in the iOS app</span><span class="w">after the move</span><button data-toast="Back in the field to edit">Edit</button><button data-toast="Removed">Remove</button></div>';
  const strip = (rows, head) => `<div class="mvs" role="status"><div class="h"><b>${head}</b><span class="sp"></span><button class="dbtn quiet small" data-dact="dset:ags:idle">Cancel move</button></div>${rows.map(([k, t]) => `<div class="r ${k}"><span class="mk">${k === "done" ? OK : k === "run" ? '<span class="dspin"></span>' : ""}</span><span>${t}</span></div>`).join("")}</div>` + QUEUED;
  const CONV = [["done", "Conversation 6.1 MB"]];
  const moving = rows => strip(rows, "Moving to studio");
  const CFG = {
    mvmenu: { base: "idle", cur: "laptop", menu: "idle" },
    mvmenuturn: { base: "working", cur: "laptop", menu: "working", turn: true },
    mvbar: { base: "working", cur: "laptop", open: "mvmenuturn", tag: "",
      extra: '<div class="mvs bar" role="status"><span>Moving to studio after this turn</span><span class="sp"></span><button class="dbtn small" data-dact="dset:ags:mvmoving">Stop and move</button><button class="dbtn quiet small" data-dact="dset:ags:working">Cancel</button></div>' },
    mvmoving: { base: "idle", cur: "laptop", busy: true, extra: moving([["run", "Conversation 4.2 MB of 6.1 MB"], ["todo", "Project files"], ["todo", "Resuming on studio"]]) },
    mvfiles: { base: "idle", cur: "laptop", busy: true, extra: moving([...CONV, ["run", "Project files 12 left"], ["todo", "Resuming on studio"]]) },
    mvwait: { base: "idle", cur: "laptop", busy: true, extra: moving([...CONV, ["run", "Waiting for laptop"], ["todo", "Project files 12 left"], ["todo", "Resuming on studio"]]) },
    mvresume: { base: "idle", cur: "laptop", busy: true, extra: moving([...CONV, ["done", "Project files"], ["run", "Resuming on studio"]]) },
    mvdone: { base: "idle", cur: "studio", open: "mvdonemenu", line: "Moved from laptop", image: true },
    mvdonemenu: { base: "idle", cur: "studio", menu: "mvdone", line: "Moved from laptop", image: true },
    mvwtmenu: { base: "idle", cur: "laptop", menu: "idle", wt: true },
    mvwtfail: { base: "idle", cur: "laptop", extra: '<div class="mvs" role="status"><div class="h"><span class="t">Move failed: the uncommitted changes do not apply on studio. Still on laptop, nothing lost.</span><span class="sp"></span><button class="dbtn quiet small" data-toast="Dismissed">Dismiss</button></div></div>' },
    mvcopy: { base: "idle", cur: "laptop", off: true, extra: '<div class="mvs"><div class="h"><span class="t">Another machine continued this session later. This version is kept</span><span class="sp"></span><button class="dbtn quiet small" data-toast="Continues here from this version">Restore</button><button class="dbtn quiet small" data-toast="Copy discarded">Discard</button></div></div>' },
    mvoffline: { base: "idle", cur: "laptop", off: true, extra: '<div class="mvs"><div class="h"><span class="t">laptop is offline. Copy from 3 min ago</span><span class="sp"></span><button class="dbtn pri small" data-toast="Continues here from the copy">Continue on this Mac</button></div></div>' },
  };
  const endOf = (html, open) => {
    const i = html.indexOf(open);
    if (i < 0) return -1;
    const re = /<div\b|<\/div>/g; re.lastIndex = i;
    let d = 0, m;
    while ((m = re.exec(html))) { d += m[0] === "</div>" ? -1 : 1; if (!d) return m.index; }
    return -1;
  };
  const menuOf = (c, turn) => `<div class="ag-wtm mvm" role="menu"><div class="mvh">Move to</div>${(c.wt ? WT : MACH).filter(([n]) => n !== c.cur).map(([n, note, ok, kind]) => {
    const go = kind === "offline" ? `data-toast="Moves when ${n} returns"` : `data-dact="dset:ags:${turn ? "mvbar" : "mvmoving"}"`;
    return `<button role="menuitem"${ok ? ` ${go}` : " disabled"}><b>${n}</b><small>${note}</small></button>`;
  }).join("")}</div>`;
  const orig = D.dAgentSession;
  D.dAgentSession = () => {
    const v = S.d.ags;
    const c = CFG[v] || (v === "idle" ? { base: "idle", cur: "laptop", open: "mvmenu" } : v === "working" ? { base: "working", cur: "laptop", open: "mvmenuturn" } : null);
    if (!c) return orig();
    const keep = S.d.ags;
    let html;
    S.d.ags = c.base;
    try { html = orig(); } finally { S.d.ags = keep; }
    if (c.image) {
      const at = endOf(html, '<div class="dcoderen user');
      if (at > 0) html = html.slice(0, at) + '<div class="pics"><i class="img" style="width:160px;height:96px"></i></div>' + html.slice(at);
    }
    const open = c.open || (c.menu ? c.menu : "");
    const btn = c.off ? `<span class="ddot off"></span> laptop offline`
      : `<button class="mvb" ${c.busy ? "disabled" : `data-dact="dset:ags:${c.menu ? c.menu : open}"`} aria-haspopup="menu" aria-expanded="${!!c.menu}" title="${c.busy ? "Moving" : "Move this session to another machine"}">${c.cur}${c.busy ? "" : CHEV}</button>`;
    html = html.replace(/<small title="([^"]*)">build-box<\/small>/, (m, t) => `<small title="${t}">${btn}</small>`);
    if (c.menu) html = html.replace('<small id="aguse"', () => menuOf(c, c.turn) + '<small id="aguse"');
    if (c.busy) html = html.replace(/(<span id="agstate">)<span class="dstate[^"]*">[^<]*<\/span>/, '$1<span class="dstate run">Moving</span>');
    if (c.off) html = html.replace(/(<span id="agstate">)<span class="dstate[^"]*">[^<]*<\/span>/, '$1<span class="dstate">Offline</span>');
    if (c.extra) html = html.replace('<div class="dinput small" id="agfield">', () => c.extra + '<div class="dinput small" id="agfield">');
    if (c.line) { const e = endOf(html, '<div class="dcoderlog">'); if (e > 0) html = html.slice(0, e) + `<div class="mvline">${c.line}</div>` + html.slice(e); }
    return html;
  };
  const add = (key, list) => VARIANTS[key].push(...list.map(([l, k, field]) => [l, () => { S.d[field] = k; render(); }, () => S.d[field] === k]));
  add("dAgentSession", [["Move: menu", "mvmenu", "ags"], ["Move: menu, turn running", "mvmenuturn", "ags"], ["Move: waits for the turn", "mvbar", "ags"], ["Move: conversation copying", "mvmoving", "ags"], ["Move: project files", "mvfiles", "ags"], ["Move: waiting for the source", "mvwait", "ags"], ["Move: resuming", "mvresume", "ags"], ["Move: done", "mvdone", "ags"], ["Machine offline, copy here", "mvoffline", "ags"], ["Move: worktree menu", "mvwtmenu", "ags"], ["Move: changes do not apply", "mvwtfail", "ags"], ["Recoverable copy", "mvcopy", "ags"]]);

  // Rows of the session lists: Home's Running and a project's Running (a row and a left-list item).
  const T = "Fix the device build script", NOTE = "Copy from 3 min before laptop went offline", WHY = "Move failed: studio went offline during the file copy. Still on laptop.";
  const BTN = '<span class="dbtn small" data-toast="Continues on this Mac from the copy">Continue on this Mac</span>';
  const EDIT = {
    mvworktree: s => s.replace(/(<span>[^<]*?)\b(build-box|studio)\b/, '$1fix-login · $2'),
    mvmoving: s => s.replace(/(<span>[^<]*?)\b(build-box|studio)\b/, '$1laptop to studio <span class="cc-tag run">Moving</span>'),
    mvfailed: s => s.replace(/(<span>[^<]*?)\b(build-box|studio)\b/, '$1laptop <span class="cc-tag bad">Move failed</span>').split(`title="${T}"`).join(`title="${T}. ${WHY}"`),
    mvdone: s => s.replace(/(<span>[^<]*?)\b(build-box)\b/, "$1studio"),
    mvcopy: s => {
      const side = s.startsWith("<button");
      const t = s.replace(/(<span>[^<]*?)\b(build-box|studio)\b/, side ? "$1laptop offline" : "$1laptop offline · Copy from 3 min before laptop went offline").split(`title="${T}"`).join(`title="${T}. ${NOTE}"`);
      return side ? t : t.replace(/<span class="r">[^<]*<\/span>/, `<span class="r">${BTN}</span>`);
    },
  };
  const rows = (html, f) => html
    .replace(new RegExp(`<div class="cc-row cc-line"[^>]*>(?:(?!</div>)[\\s\\S])*?title="${T}"(?:(?!</div>)[\\s\\S])*</div>`, "g"), s => /build\.yml/.test(s) ? s : f(s))
    .replace(new RegExp(`<button class="pj-li" title="${T}"[\\s\\S]*?</button>`, "g"), f);
  const home = D.dHome;
  D.dHome = (...a) => { const f = EDIT[S.d.cc]; const out = home(...a); return f ? rows(out, f) : out; };
  add("dHome", [["Session in a worktree", "mvworktree", "cc"], ["Move: moving", "mvmoving", "cc"], ["Move: failed", "mvfailed", "cc"], ["Move: done", "mvdone", "cc"], ["Machine offline, copy here", "mvcopy", "cc"]]);

  // The project page: the same rows in Running, plus the left list.
  const proj = D.dWinProject;
  D.dWinProject = (...a) => {
    const k = S.d.pj, f = EDIT[k];
    if (!f) return proj(...a);
    let out;
    S.d.pj = "default";
    try { out = proj(...a); } finally { S.d.pj = k; }
    return rows(out, f);
  };
  VARIANTS.dWinProject.push(...[["Move: moving", "mvmoving"], ["Move: failed", "mvfailed"], ["Machine offline, copy here", "mvcopy"]].map(([l, k]) => [l, () => { S.d.pj = k; S.d.pjv = "board"; S.d.pbo = ""; render(); }, () => S.d.pj === k]));
})();
