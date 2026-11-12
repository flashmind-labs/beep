// ---- Settings, as settings/ in the app draws them ----
// One set of sections: the window's sheet shows one page at a time (settingsSections with
// sheet=true), the bar shows them in a column. Apps, an app, an MCP server, Agent accounts,
// Credentials, Instructions and a coding agent are the same markup in the sheet, the bar and
// the window's own views.
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
    const cur = S.d.vn || "ask", word = S.d.vsend === "word", agn = !S.d.agnoff;
    return dt("Launch at login", "Keep Beep in the menu bar after a restart", swi(S.d.login !== false, "Launch at login", 'data-dact="login"'))
      + dt("Voice notes", VN_SAYS[cur])
      + `<div class="dchips" role="radiogroup" aria-label="Voice notes" style="margin:6px 0 10px">${VN_CHOICES.map(([k, label]) => `<button class="dchip" role="radio" aria-checked="${cur === k}" aria-pressed="${cur === k}" data-dact="dset:vn:${k}">${label}</button>`).join("")}</div>`
      + dt("When I stop talking", word ? "Pauses never send. Say “send” or “stop” when you are done." : "Sends after a short pause. Saying “send” sends at once.")
      + `<div class="dchips" role="radiogroup" aria-label="When I stop talking" style="margin:6px 0 10px"><button class="dchip" role="radio" aria-checked="${!word}" aria-pressed="${!word}" data-dact="dset:vsend:pause">Send when I stop talking</button><button class="dchip" role="radio" aria-checked="${word}" aria-pressed="${word}" data-dact="dset:vsend:word">Wait until I say “send”</button></div>`
      + dt("Coding agent notifications", agn ? "When a coding agent asks something or finishes a turn" : "Coding agents only show under Waiting", swi(agn, "Coding agent notifications", `data-dact="dset:agnoff:${S.d.agnoff ? "" : "1"}"`))
      + dt("Project sync", S.d.projectSyncOff ? "Paused for every project on all machines. Turn on to reconcile your projects." : "Sync projects across all your machines. Turning off keeps existing files in place.", swi(!S.d.projectSyncOff, "Project sync", `data-dact="dset:projectSyncOff:${S.d.projectSyncOff ? "" : "1"}"`))
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

  // Models
  const CHOICES = [["Claude Haiku 5.5", "$1 in, $5 out per M tokens"], ["GPT-6 Luna", "$1.25 in, $10 out per M tokens"], ["GLM 5.3 Flash", "$0.2 in, $0.8 out per M tokens, text only"]];
  const modelsBody = () => {
    const auto = S.d.setAuto !== "off";
    const pick = CHOICES[1];
    return dt("Model", pick[1], `<select aria-label="Model">${CHOICES.map(([n], i) => `<option${i === 1 ? " selected" : ""}>${n}</option>`).join("")}</select>`)
      + dt("Auto thinking", auto ? "Beep picks the thinking for each prompt" : "Every prompt thinks as set below", swi(auto, "Auto thinking", `data-dact="dset:setAuto:${auto ? "off" : "on"}"`))
      + `<div class="dfield"><span>Thinking${auto ? " (Auto picks it per prompt)" : ""}</span><div class="dchips">${["Off", "Low", "Medium", "High"].map(l => `<button class="dchip" aria-pressed="${!auto && l === "Medium"}"${auto ? " disabled" : ""} data-toast="Thinking: ${l}">${l}</button>`).join("")}</div></div>`;
  };

  // Apps
  const letter = n => `<span class="dalogo">${n.slice(0, 1).toUpperCase()}</span>`;
  const waiting = act => `<span class="dmuted"><span class="dspin"></span> Waiting for browser…</span><button class="dbtn quiet" data-dact="${act}">Cancel</button>`;
  const appsList = sheet => {
    const s = S.d.apps || "list", wait = S.d.appWait || "Notion";
    const open = n => sheet ? `data-dact="dset:set:app-gmail" data-dgo="dWinSettings"` : `data-dgo="dAppPerm" aria-label="${n} permissions"`;
    const row = ([n, l, c, desc], end, attrs = "") => `<div class="dappr${attrs ? " on" : ""}" ${attrs}${attrs ? ' role="button" tabindex="0"' : ""}>${dLogo(l, c)}<span class="tt"><b>${n}</b><small>${desc}</small></span>${end}</div>`;
    const none = s === "none";
    const connected = none ? "" : D_APPS.slice(0, 2).map(a => row(a, `<span class="dtag">Connected</span><span class="dmuted">${chev}</span>`, open(a[0]))).join("");
    const more = (none ? D_APPS : D_APPS.slice(2)).map(a => row(a, s === "nokey" ? `<span class="dmuted">Unavailable</span>`
      : s === "waiting" && a[0] === wait ? waiting("appCancel")
      : `<button class="dbtn pri" data-dact="appConnect:${a[0]}" data-toast="Opening ${a[0]} sign-in in the browser"${s === "waiting" ? " disabled" : ""}>Connect</button>`)).join("");
    const server = ([n, l, host, tools]) => {
      const end = s === "mcpWait" && !tools ? waiting("appCancel")
        : tools ? `<span class="dtag ok">${tools} tools</span><span class="dmuted">${chev}</span>`
        : `<span class="dtag ask">Sign-in expired</span><button class="dbtn pri" data-toast="Opening ${n} sign-in in the browser">Sign in</button><button class="dbtn quiet warn" data-toast="${n} removed">Remove</button>`;
      const go = tools && s !== "mcpWait" ? (sheet ? `data-dact="dset:set:mcp-wiki" data-dgo="dWinSettings"` : `data-dgo="dMcp" aria-label="${n} tools"`) : "";
      return `<div class="dappr${go ? " on" : ""}" ${go}${go ? ' role="button" tabindex="0"' : ""}>${letter(n)}<span class="tt"><b>${n}</b><small>${host}</small></span>${end}</div>`;
    };
    const down = s === "mcpDown" ? `<div class="dappr">${letter("Docs")}<span class="tt"><b>Docs</b><small>docs.example.org</small></span><span class="dtag ask">Not connected</span><button class="dbtn pri" data-toast="Connecting to Docs">Retry</button><button class="dbtn quiet warn" data-toast="Docs removed">Remove</button></div>` : "";
    const add = s === "mcpAdd" || s === "mcpBad"
      ? `<div class="dform dmcp"><label class="dfield"><span>Name</span><input maxlength="60" placeholder="Team notes" autocomplete="off" spellcheck="false" value="Team notes"></label>
        <label class="dfield${s === "mcpBad" ? " err" : ""}"><span>${s === "mcpBad" ? "Use an https address on the public internet" : "Server URL"}</span><input class="url" maxlength="2000" placeholder="https://example.com/mcp" autocomplete="off" spellcheck="false"${s === "mcpBad" ? ' value="http://example.com/mcp"' : ""}></label>
        <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri" data-toast="Connecting to Team notes">Add server<kbd>${DI.ret}</kbd></button><button class="dbtn quiet" data-dact="mcpCancel">Cancel</button></div></div>`
      : `<div style="padding:4px 10px"><button class="dbtn" data-dact="mcpAdd">Add MCP server</button></div>`;
    return `<div class="dlist dapps">${connected ? `<div class="dsec">Connected</div>${connected}` : ""}<div class="dsec">${connected ? "More apps" : "Available"}</div>${more}
      <div class="dsec">Your MCP servers</div>${MCP_SERVERS.map(server).join("")}${down}${add}
      <div class="dmuted" style="padding:6px 10px;display:block">Beep asks before every tool on a server you add.</div></div>`;
  };
  const appPerm = (fm, back) => {
    const lvl = S.d.appLevel || "ask";
    const allowed = S.d.appAllowed || (S.d.appAllowed = { "Send email": false, "Reply to email": true, "Move to trash": false, "Save a draft": true });
    const levels = [["ask", "Ask first", "Beep shows what it is about to do and waits for you."], ["allow", "Always allow", "Beep sends, changes and deletes without asking."], ["read", "Read only", "Beep can search and read, never change anything."]];
    const [an, al, ac] = fm ? ["Fastmail", FM_LOGO, "#fff"] : ["Gmail", "G", "#F28B7D"];
    const accts = fm === "one" || fm === "removeone" ? ["alex@example.com"] : ["alex@example.com", "alex@work.example"];
    const accounts = !fm ? "" : fm === "failed"
      ? `<div class="dsec">Accounts</div><div class="daction"><span class="derr">Fastmail accounts did not load: the server took too long</span></div><div style="padding:4px 10px"><button class="dbtn" data-dact="fm:two">Retry</button></div>`
      : `<div class="dsec">Accounts</div>${accts.map((a, i) => {
          const asking = (fm === "remove" && i === 1) || fm === "removeone";
          return `<div class="daction"><span>${a}${i === 0 && accts.length > 1 ? ' <small class="dmuted" style="display:inline">Default</small>' : ""}</span>${asking
            ? `<span class="dmuted">${accts.length === 1 ? "Disconnects Fastmail" : "Beep stops using this account"}</span><button class="dbtn quiet" data-dact="fm:${accts.length === 1 ? "one" : "two"}">Cancel</button><button class="dbtn warn" ${accts.length === 1 ? `data-dgo="dApps" data-toast="Fastmail disconnected"` : `data-dact="fm:one" data-toast="alex@work.example removed"`}>Remove</button>`
            : `<button class="dbtn quiet warn" data-dact="fm:${accts.length === 1 ? "removeone" : "remove"}">Remove</button>`}</div>`;
        }).join("")}${fm === "adding" ? `<div style="padding:4px 10px;display:flex;gap:8px;align-items:center">${waiting("fm:two")}</div>` : `<div style="padding:4px 10px"><button class="dbtn" data-dact="fm:adding" data-toast="Opening Fastmail sign-in in the browser">Add another account</button></div>`}
        <div class="dmuted" style="padding:6px 10px;display:block">Beep uses the first account unless you name another, like "check my work inbox". To add a different account, sign out of Fastmail in the browser first.</div>`;
    const tag = a => lvl === "read" ? `<span class="dtag">Off</span>` : lvl === "allow" || allowed[a] ? `<span class="dtag ok">Allowed</span>` : `<span class="dtag ask">Asks</span>`;
    const acts = Object.keys(allowed).map(a => `<div class="daction"><span>${a}</span>${tag(a)}${lvl === "ask" ? swi(allowed[a], `Always allow ${a}`, `data-dact="appAllow:${a}"`) : ""}</div>`).join("");
    return `<div class="dhead">${dLogo(al, ac)}<span class="tt"><b>${an}</b><small>What Beep may do here</small></span></div>
      <div class="dlist dapps">${accounts}<div class="dsec">When Beep acts in ${an}</div>${levels.map(([k, t, d]) => `<button class="dlevel" role="radio" aria-checked="${lvl === k}" data-dact="appLevel:${k}"><span class="dlradio"></span><span class="tt"><b>${t}</b><small>${d}</small></span></button>`).join("")}
      <div class="dsec">Actions</div>${acts}
      <div class="dmuted" style="padding:8px 10px;display:block">Searching and reading is always allowed.</div>
      <div style="padding:6px 10px 2px"><button class="dbtn warn" ${back} data-toast="${an} disconnected">Disconnect ${an}${fm && accts.length > 1 ? " (all accounts)" : ""}</button></div></div>`;
  };
  const mcpPage = back => {
    const tools = S.d.mcpMany ? MCP_MANY : MCP_FEW;
    if (!S.d.mcpOn) { S.d.mcpOn = {}; tools.filter(t => !MCP_TOO_LARGE.includes(t)).slice(0, MCP_LIMIT).forEach(t => S.d.mcpOn[t] = true); }
    const on = S.d.mcpOn, count = tools.filter(t => on[t]).length;
    const rows = tools.map(t => {
      if (S.d.mcpAsk === t) return `<div class="daction"><span>${t}<small>More than ${MCP_LIMIT} tools makes replies slower and tool picks worse</small></span><button class="dbtn quiet" data-dact="mcpNo">Cancel</button><button class="dbtn pri" data-dact="mcpYes">Turn on</button></div>`;
      if (MCP_TOO_LARGE.includes(t)) return `<div class="daction off"><span>${t}<small>Too large for Beep to use</small></span>${swi(false, `Use ${t}`, "disabled")}</div>`;
      return `<div class="daction"><span>${t}</span>${swi(!!on[t], `Use ${t}`, `data-dact="mcpTool:${t}"`)}</div>`;
    }).join("");
    return `<div class="dhead">${letter("Team wiki")}<span class="tt"><b>Team wiki</b><small class="dmono">https://wiki.example.com/mcp</small></span></div>
      <div class="dlist dapps"><div class="dsec dsplit"><span>Tools</span><span${count > MCP_LIMIT ? ' class="over"' : ""}>${count} of ${MCP_LIMIT} on</span></div>${rows}
      <div class="dmuted" style="padding:8px 10px;display:block">Beep uses up to ${MCP_LIMIT} tools from a server, so turn on the ones you need. It asks before every tool on a server you add.</div>
      <div style="padding:6px 10px 2px;display:flex;gap:8px;align-items:center">${S.d.mcpRm ? `<span class="dmuted">Beep stops using this server</span><button class="dbtn quiet" data-dact="dset:mcpRm:">Cancel</button><button class="dbtn warn" ${back} data-toast="Team wiki removed">Remove</button>` : `<button class="dbtn warn" data-dact="dset:mcpRm:1">Remove server</button>`}</div></div>`;
  };

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
    : v === "acct-old" ? `<div class="dmuted">The server needs an update before it keeps agent accounts.</div>`
    : v === "acct-load" ? skel(2)
    : `<div style="font-size:12.5px;color:var(--d-muted);margin-top:4px">How your coding agents sign in on your machines.</div>${claudeBox(v)}${codexBox(v)}`;
  const CODING = [["claude", "Claude Code"], ["codex", "Codex"], ["gemini", "Gemini CLI"], ["pi", "pi"], ["opencode", "OpenCode"], ["flsh", "flsh"], ["hermes", "Hermes"]];
  const codingAgents = () => `<div class="dset-h">Coding agents</div><div class="dset-box">${CODING.map(([a, n]) => `<div class="dset-row" role="button" tabindex="0" ${dsetGo(`acct-${a}`)}>${tile(a)}<span class="tt"><b>${n}</b><small>Version and updates, on or off, binary path, launch arguments and env vars on each machine</small></span>${chev}</div>`).join("")}</div>`;

  // Credentials: credentials.ts.
  return { chev, tile, sm, dt, swi, sec, skel, dsetGo, generalBody, SK, SKL, skinsHtml, CHOICES, modelsBody, letter, waiting, appsList, appPerm, mcpPage, macBody, box, mrow, agentForm, claudeBox, codexBox, acctBody, CODING, codingAgents };
})();
