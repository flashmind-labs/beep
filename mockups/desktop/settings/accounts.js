// ---- Settings, as settings/ in the app draws them ----
// One set of sections: the window's sheet shows one page at a time (settingsSections with
// sheet=true), the bar shows them in a column. Credentials, Instructions and a coding agent
// are the same markup in the sheet, the bar and the window's own views.
const SETTINGS_ACCOUNTS = (() => {
  document.head.insertAdjacentHTML("beforeend", '<link rel="stylesheet" href="settings/base.css">');

  const chev = DI.chevR;
  const tile = a => dMacAgTile(a, "cc-ag");
  const sm = 'style="font-size:11.5px;padding:2px 8px;min-height:24px"';
  const dt = (name, sub, end = "") => `<div class="dtoggle"><span><b>${name}</b><small>${sub}</small></span>${end}</div>`;
  const swi = (on, label, attrs = "") => `<button class="dswitch" role="switch" aria-checked="${on}" aria-label="${label}" ${attrs}></button>`;
  const sec = (title, body) => `<section><div class="dsec flat" style="padding-top:6px">${title}</div><div>${body}</div></section>`;
  const skel = n => `<div class="dskel-rows">${[78, 56, 68, 48].slice(0, n).map(w => `<div class="dskel" style="width:${w}%"></div>`).join("")}</div>`;
  const dsetGo = k => `data-dact="dset:set:${k}" data-dgo="dWinSettings"`;

  // General
  const generalBody = () => {
    const agn = !S.d.agnoff;
    return dt("Launch at login", "Keep Beep in the menu bar after a restart", swi(S.d.login !== false, "Launch at login", 'data-dact="login"'))
      + dt("Coding agent notifications", agn ? "When a coding agent asks something or finishes a turn" : "Coding agents only show under Waiting", swi(agn, "Coding agent notifications", `data-dact="dset:agnoff:${S.d.agnoff ? "" : "1"}"`))
      + dt("Project sync", S.d.projectSyncOff ? "Paused for every project on all machines. Turn on to reconcile your projects." : "Sync projects across all your machines. Turning off keeps existing files in place.", swi(!S.d.projectSyncOff, "Project sync", `data-dact="dset:projectSyncOff:${S.d.projectSyncOff ? "" : "1"}"`))
      + dt("Save files to", S.d.saveDir || "~/Downloads", `${S.d.saveDir ? '<button class="dbtn quiet" data-dact="dset:saveDir:">Use Downloads</button>' : ""}<button class="dbtn" data-dact="dset:saveDir:~/Work/Beep saves">Choose…</button>`)
      + dt("Keyboard shortcuts", "Every shortcut, by where it works", '<button class="dbtn" data-dgo="dShortcuts">Show…</button>')
      + dt("Report a problem", "Collect logs from your machines and send them to us", '<button class="dbtn" data-dgo="dReport">Report…</button>');
  };

  // Appearance
  const SK = [["flashmind", "Flashmind", "#0d161e"], ["linear", "Linear", "#151617"], ["classic", "Classic", "#1a2a23"], ["quiet", "Quiet", "#161b18"], ["github", "GitHub dark", "#161b22"], ["zed", "Zed", "#2f343e"], ["tokyo", "Tokyo Night", "#1f2335"], ["mocha", "Catppuccin Mocha", "#313244"], ["nord", "Nord", "#3b4252"], ["gruvbox", "Gruvbox", "#3c3836"], ["rosepine", "Rose Pine", "#26233a"], ["geist", "Geist", "#000"], ["mono", "High contrast", "#000"]];
  const SKL = [["paper", "Light", "#f3f3f1"], ["solarized", "Solarized", "#eee8d5"], ["macos", "macOS", "#e9e7ea"], ["warm", "Warm", "#f3efe8"]];
  const skinsHtml = () => {
    const on = S.d.skin || "linear";
    const group = (title, list) => `<div class="dskins"><small>${title}</small><div>${list.map(([id, n, c]) => `<button class="dskin" aria-pressed="${id === on}" ${id === "linear" || id === "paper" || id === "flashmind" ? `data-dact="dset:skin:${id === "linear" ? "" : id}" data-dgo="dAppearance"` : `data-toast="Skin: ${n}"`}><i style="background:${c}"></i>${n}</button>`).join("")}</div></div>`;
    // Home and projects each open on the board or on the list from before it.
    const layoutPick = (key, name, note) => {
      const on = S.d[key] === "list" ? "list" : "board";
      return `<div class="dtoggle"><span><b>${name}</b><small>${note}</small></span><div class="hv-seg" role="radiogroup" aria-label="${name} layout">${[["board", "Board"], ["list", "List"]].map(([id, label]) => `<button role="radio" aria-checked="${id === on}" aria-pressed="${id === on}" data-dact="dset:${key}:${id === "list" ? "list" : ""}" data-dgo="${S.screen}">${label}</button>`).join("")}</div></div>`;
    };
    return group("Dark", SK) + group("Light", SKL) + layoutPick("frontHome", "Home", "Lanes of cards, or sections of rows") + layoutPick("frontProject", "Projects", "A project's front page");
  };

  // A letter tile, for a host without a logo.
  const letter = n => `<span class="dalogo">${n.slice(0, 1).toUpperCase()}</span>`;

  // This computer: thisMac(mac) in settings.ts.
  const macBody = () => dThisMac();

  // Agent accounts: agentaccounts.ts.
  const box = inner => `<div style="border:1px solid var(--d-border);border-radius:10px;padding:12px 14px;margin-top:12px">${inner}</div>`;
  const mrow = (name, off, act) => `<div style="display:flex;align-items:center;gap:8px;padding:6px 0;font-size:12.5px"><span class="ddot${off ? " off" : ""}"></span><span>${name}</span><span style="margin-left:auto;color:var(--d-muted);display:flex;gap:6px;align-items:center">${act}</span></div>`;
  const agentForm = agent => {
    const claude = agent === "claude";
    const note = claude ? `Run <code style="font-size:11px">claude setup-token</code> in Terminal and paste the token it prints.` : "An OpenAI API key. Codex uses it on every machine instead of signing in.";
    return `<div style="display:grid;gap:8px;margin-top:10px"><div style="display:flex;gap:8px"><input type="password" autocomplete="off" spellcheck="false" placeholder="${claude ? "sk-ant-oat01-…" : "sk-…"}" aria-label="${claude ? "Claude Code token" : "Codex API key"}" style="flex:1;font-family:var(--mono,monospace);font-size:12px">
      <button class="dbtn pri" data-toast="Saved">Save</button><button class="dbtn quiet" ${dsetGo("acct")}>Cancel</button></div><span style="font-size:11.5px;color:var(--d-muted)">${note}</span></div>`;
  };
  const claudeBox = v => {
    const head = sub => `<div style="display:flex;align-items:center;gap:10px">${tile("claude")}<span style="display:grid;gap:2px;min-width:0"><b style="font-size:13.5px">Claude Code</b>${sub ? `<span style="font-size:11.5px;color:var(--d-muted)">${sub}</span>` : ""}</span>`;
    if (v === "acct-flow") return box(`${head("")}<span style="margin-left:auto"><button class="dbtn quiet" ${sm} ${dsetGo("acct-none")}>Cancel</button></span></div>
      <div style="font-size:12px;color:var(--d-muted);margin-top:8px;display:flex;gap:8px;align-items:center"><span class="dspin"></span>Approve in your browser. Beep saves the token once you do.</div>`);
    if (v === "acct-none" || v === "acct-unread" || v === "acct-paste-none") {
      const why = v === "acct-unread" ? "Beep can no longer read the saved token. Sign in again." : "Sign in for all to give every machine one token. Beep keeps it encrypted and never writes it to their disks.";
      const edit = v === "acct-paste-none";
      return box(`${head("signs in on each machine")}<span style="margin-left:auto;display:flex;gap:6px;flex:none"><button class="dbtn" ${dsetGo("acct-flow")}>Sign in for all</button>${edit ? "" : `<button class="dbtn quiet" ${dsetGo("acct-paste-none")}>Paste a token</button>`}</span></div>
        <div style="margin-top:6px">${mrow("This computer", false, "signed in as you@example.com")}${mrow("Workspace", false, "not signed in")}${mrow("gpu-box", true, "not checked")}</div><div style="font-size:12px;color:var(--d-muted);margin-top:6px">${why}</div>${edit ? agentForm("claude") : ""}`);
    }
    const edit = v === "acct-paste";
    return box(`${head(`signed in today · <code style="font-size:11px">…a91f</code>`)}<span style="margin-left:auto;display:flex;gap:6px;flex:none"><button class="dbtn" ${dsetGo("acct-flow")}>Replace</button><button class="dbtn warn" ${dsetGo("acct-none")} data-toast="Revoked">Revoke</button></span></div>
      ${edit ? agentForm("claude") : ""}<div style="font-size:12px;color:var(--d-muted);margin:8px 0 4px">Used on</div>
      ${mrow("build-box", false, "2 sessions now")}${mrow("Workspace", false, "idle")}${mrow("This computer", false, "1 session now")}${mrow("gpu-box", true, "offline · gets it when back")}
      ${edit ? "" : `<button class="dbtn quiet" ${sm} ${dsetGo("acct-paste")}>Paste a token instead</button>`}`);
  };
  const codexBox = v => {
    const key = v === "acct-apikey", form = v === "acct-keyform";
    const sub = key ? `API key <code style="font-size:11px">…7c2e</code> on every machine · added 3 days ago` : "signs in on each machine";
    const acts = key ? `<button class="dbtn" ${sm} ${dsetGo("acct-keyform")}>Replace key</button><button class="dbtn quiet warn" ${sm} ${dsetGo("acct")}>Remove key</button>` : `<button class="dbtn" ${dsetGo("acct-keyform")}>Use an API key</button>`;
    const flow = `<div style="padding:8px 10px;margin:4px 0;border-radius:8px;background:var(--d-elev);border:1px solid var(--d-border);font-size:12.5px;display:grid;gap:6px"><span>Workspace: open <b>auth.openai.com/codex/device</b> and enter</span><span style="font-family:var(--mono,monospace);font-size:18px;letter-spacing:.12em;user-select:all">K7QD-M2XR</span><span style="display:flex;gap:6px;align-items:center;color:var(--d-muted)"><span class="dspin"></span>Waiting for you to approve · 14:32 left<span style="margin-left:auto;display:flex;gap:6px"><button class="dbtn" ${sm} data-toast="Copied">Copy code</button><button class="dbtn quiet" ${sm} data-toast="Opens the page">Open page</button><button class="dbtn quiet" ${sm} ${dsetGo("acct")}>Cancel</button></span></span></div>`;
    const work = v === "acct-code" ? flow
      : v === "acct-start" ? mrow("Workspace", false, '<span class="dspin"></span>starting sign-in')
      : mrow("Workspace", false, `<span>not checked</span><button class="dbtn" ${sm} ${dsetGo("acct-code")}>Sign in</button>`) + (v === "acct-signfail" ? '<div class="derr" style="padding-left:15px">Workspace could not start codex login</div>' : "");
    return box(`<div style="display:flex;align-items:center;gap:10px">${tile("codex")}<span style="display:grid;gap:2px;min-width:0"><b style="font-size:13.5px">Codex</b><span style="font-size:11.5px;color:var(--d-muted)">${sub}</span></span><span style="margin-left:auto;display:flex;gap:6px;flex:none">${form ? "" : acts}</span></div>
      ${form ? agentForm("codex") : ""}<div style="margin-top:6px">${mrow("build-box", false, "signed in")}${mrow("This computer", false, "signed in")}${work}${mrow("gpu-box", true, "offline")}</div>`);
  };
  const acctBody = v => v === "acct-fail" ? `<div class="dmuted">Could not load your agent accounts.</div>`
    : v === "acct-load" ? skel(2)
    : `<div style="font-size:12.5px;color:var(--d-muted);margin-top:4px">How your coding agents sign in on your machines.</div>${claudeBox(v)}${codexBox(v)}`;
  const CODING = [["claude", "Claude Code"], ["codex", "Codex"], ["gemini", "Gemini CLI"], ["pi", "pi"], ["opencode", "OpenCode"], ["flsh", "flsh"], ["hermes", "Hermes"]];
  const codingAgents = () => `<div class="dset-h">Coding agents</div><div class="dset-box">${CODING.map(([a, n]) => `<div class="dset-row" role="button" tabindex="0" ${dsetGo(`acct-${a}`)}>${tile(a)}<span class="tt"><b>${n}</b><small>Version and updates, on or off, binary path, launch arguments and env vars on each machine</small></span>${chev}</div>`).join("")}</div>`;

  // Credentials: credentials.ts.
  return { chev, tile, sm, dt, swi, sec, skel, dsetGo, generalBody, skinsHtml, letter, macBody, box, mrow, agentForm, claudeBox, codexBox, acctBody, CODING, codingAgents };
})();
