// ---- Local first: no subscription, and joining machines by ticket ----
// Desktops and beep-nodes work with each other directly, with no account. A subscription
// adds chats, memories, routines, skills, apps, research, the project agent and phones.
{
  const IC = {
    proj: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6a2 2 0 012-2h4l2 2h8a2 2 0 012 2v10a2 2 0 01-2 2H5a2 2 0 01-2-2z"></path></svg>',
    mach: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="12" rx="2"></rect><path d="M8 20h8M12 16v4"></path></svg>',
    card: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="5" width="18" height="14" rx="2"></rect><path d="M3 10h18"></path></svg>',
  };
  const TICKET = "beepjoin1example7qf3m9xa2kdpwt4h";
  const MONO = "font-family:var(--mono,monospace);font-size:12.5px;padding:9px 11px;border-radius:8px;background:#0f1011;border:1px solid var(--d-border)";
  const go = (k, v) => `data-dact="dset:${k}:${v}" data-dgo="dNoServer"`;

  // The sidebar: what works without a subscription.
  const nsSide = (cur, online = 1, total = 1, chats = "") => {
    const si = (ic, label, k) => `<button class="cc-si"${cur === k ? ' aria-current="true"' : ""} ${go("ns", k)}>${ic}<span class="tt">${label}</span></button>`;
    return `<aside class="cc-side">
      <div class="cc-lights"><i></i><i></i><i></i></div>
      <div class="cc-brand"><b>Beep</b></div>
      <div class="cc-grp">
        ${si(CC_IC.home, "Home", "home")}${si(IC.proj, "Projects", "projects")}${si(CC_IC.run, "Agents", "agents")}${si(IC.mach, "Machines", "machines")}${si(CC_IC.term, "Terminals", "terminals")}
      </div>
      ${chats}
      <div class="cc-grp"><button class="cc-si"${cur === "settings" ? ' aria-current="true"' : ""} ${go("ns", "settings")}>${CC_IC.gear}<span class="tt">Settings</span><kbd style="margin-left:auto;font-size:10.5px;opacity:.7">⌘,</kbd></button></div>
      <div class="cc-foot"><span><span class="ddot"></span> <b>${online}</b> of ${total} ${total === 1 ? "machine" : "machines"} online</span><button class="dbtn quiet" style="margin-left:auto;padding:2px 6px" aria-label="Add a machine" title="Add a machine" data-dact="dset:ts:ready" data-dgo="dAddTicket">${CC_IC.plus}</button></div>
    </aside>`;
  };
  const row = (dot, t, sub, r) => `<div class="cc-row cc-line"><span class="ddot${dot}"></span><button class="t" title="${t}"><b>${t}</b><span>${sub}</span></button><span class="r">${r}</span></div>`;
  const addBtn = (cls = "dbtn small") => `<button class="${cls}" data-dact="dset:ts:ready" data-dgo="dAddTicket">${CC_IC.plus} Add a machine</button>`;
  const empty = (t, sub, btn = "") => `<div class="cc-panel cc-empty"><b>${t}</b><span>${sub}</span>${btn}</div>`;
  const top = (t, sub, tools = "") => `<div class="cc-top"><h2>${t}</h2><small>${sub}</small><span class="tools">${tools}</span></div>`;

  const homeMachines = two => two
    ? [row("", "This computer", "macOS · Claude Code, Codex", "idle"), row("", "build-box", "Linux · Docker, Claude Code", "1 agent"), row("", "gpu-box", "Linux · Codex", "idle")].join("")
    : row("", "This computer", "macOS · Claude Code, Codex", "idle");

  const nsHome = two => ccWin(nsSide("home", two ? 3 : 1, two ? 3 : 1),
    top("Home", "Your machines", addBtn()) + `<div class="cc-view">
      <div><div class="cc-sec"><h4>Machines<span class="n">${two ? 3 : 1}</span></h4></div><div class="cc-panel">${homeMachines(two)}</div></div>
      <div><div class="cc-sec"><h4>Running<span class="n">${two ? 1 : ""}</span></h4></div>${two
        ? `<div class="cc-panel"><div class="cc-row cc-line" data-dgo="dAgentSession"><span class="cc-ag claude" style="width:20px;height:20px;border-radius:5px;font-size:8.5px">CC</span><button class="t" title="Swipeable approval strip"><b>Swipeable approval strip</b><span>build-box · fix-approval</span></button><span class="r">1 min</span></div></div>`
        : empty("Nothing running", "Start an agent on this computer, or add a machine to run it there.", `<button class="dbtn small" data-dgo="dAgentNew">Start an agent</button>`)}</div></div>`);

  const nsProjects = () => ccWin(nsSide("projects", 3, 3), top("Projects", "Repositories on your machines", `<button class="dbtn small" data-dgo="dAddProject">${CC_IC.plus} Add a project</button>`) + `<div class="cc-view"><div class="cc-sec"><h4>Projects<span class="n">2</span></h4></div><div class="cc-mt cc-pt" role="table"><span class="th">Project</span><span class="th">Machine and folder</span><span class="th">Agents</span><span class="th">Last activity</span>
    <span class="nm"><b><button type="button" title="acme/widget" ${go("ns", "project")}>acme/widget</button></b><span class="os">github.com</span></span>
    <span class="nm"><span class="os"><b>build-box</b> /srv/projects/acme/widget</span><span class="os"><b>This computer</b> ~/Projects/acme/widget</span></span><span class="num">1</span><span class="num">6 min ago</span>
    <span class="nm"><b><button type="button" title="acme/relay" ${go("ns", "project")}>acme/relay</button></b><span class="os">github.com</span></span>
    <span class="nm"><span class="os"><b>gpu-box</b> /srv/projects/acme/relay</span></span><span class="num">–</span><span class="num">2 d ago</span></div></div>`);

  // The project page without the project agent: no card waiting, no tasks or plan.
  const li = (t, sub, cur) => `<button class="pj-li" title="${t}"${cur ? ' aria-current="true"' : ""} data-toast="${t}"><span class="lead"></span><span class="t"><b>${t}</b><span>${sub}</span></span></button>`;
  const nsProject = () => ccWin(nsSide("projects", 3, 3), `<div class="nb-main" style="grid-template-rows:auto minmax(0,1fr) auto">${top("acme/widget", "github.com · imported 3 d", `<button class="dbtn small" data-toast="GitHub">GitHub</button>`)}
    <div class="cc-view split"><div class="pj-split"><div class="pj-list">${li("Overview", "1 running · 2 PRs · 2 issues", true)}
      <h4>Running<span class="n">1</span></h4><button class="pj-li" title="Swipeable approval strip" data-dgo="dAgentSession"><span class="lead"><span class="cc-ag claude" style="width:20px;height:20px;border-radius:5px;font-size:8.5px">CC</span></span><span class="t"><b>Swipeable approval strip</b><span>build-box · fix-approval</span></span><span class="r">1 min</span></button>
      <h4>Project<span class="n">3</span></h4>${li("Actions", "1 failed")}${li("Machines", "2/2 · 1 terminal")}${li("File sync", "in sync")}
      <h4>Work<span class="n">2</span></h4>${li("Agents", "1 running")}${li("Branches", "3 with work")}
      <h4>Pull requests<span class="n">2</span></h4>${li("Cut the cache warm-up in half", "sam · 5 d · CI failing")}${li("Bump the sync crate", "you · 9 d · ready")}</div>
    <div class="pj-det"><div class="pj-ov"><div>
      <div><div class="cc-sec"><h4>Running<span class="n">1</span></h4></div><div class="cc-panel"><div class="cc-row cc-line" data-dgo="dAgentSession"><span class="cc-ag claude" style="width:20px;height:20px;border-radius:5px;font-size:8.5px">CC</span><button class="t" title="Swipeable approval strip"><b>Swipeable approval strip</b><span>build-box · fix-approval</span></button><span class="r">1 min</span></div></div></div>
      <div><div class="cc-sec"><h4>Machines<span class="n">2/2</span></h4></div><div class="cc-panel">${row("", "build-box", "fix-approval", "")}${row("", "This computer", "main", "")}</div></div>
      <div><div class="cc-sec"><h4>Actions<span class="n">1 failed</span></h4></div><div class="cc-panel"><div class="cc-row cc-line bare"><button class="t" title="Cut the cache warm-up in half"><b>Cut the cache warm-up in half</b><span>build.yml #351</span></button><span class="r"><span class="pj-st blocked">failed</span></span></div></div></div>
    </div></div></div></div></div></div>`);

  const nsMachines = () => ccWin(nsSide("machines", 3, 3), top("Machines", "3 of 3 online", `<button class="dbtn small" data-dact="dset:tk:other" data-dgo="dAddTicket">Link another desktop</button>${addBtn()}`) + `<div class="cc-view"><div class="cc-panel">${homeMachines(true)}</div></div>`);
  const nsAgents = () => ccWin(nsSide("agents"), top("Agents", "On your machines", `<button class="dbtn small" data-dgo="dAgentNew">${CC_IC.plus} Start an agent</button>`) + `<div class="cc-view">${empty("No agents running", "Start one on this computer or on any machine you added.")}</div>`);
  const nsTerminals = () => ccWin(nsSide("terminals"), top("Terminals", "Kept running on your machines") + `<div class="cc-view">${empty("No terminals", "Open one on a machine and it keeps running when you close the window.")}</div>`);
  const nsSettings = () => ccWin(nsSide("settings"), top("Settings", "Beep for macOS 0.2.0") + `<div class="cc-pane"><div class="cc-panel"><div class="cs-row cs-go"><span>${IC.gear}</span><span><b>General</b><small>Start at login, shortcuts.</small></span><span class="end"></span></div><div class="cs-row cs-go"><span>${IC.gear}</span><span><b>Appearance</b><small>Skin and text size.</small></span><span class="end"></span></div><div class="cs-row cs-go"><span>${IC.key}</span><span><b>Credentials</b><small>Git hosts and API keys. Two to enter again.</small></span><span class="end"><span class="dtag warn">2</span></span></div><div class="cs-row cs-go"><span>${IC.gear}</span><span><b>Machines</b><small>The machines in this mesh.</small></span><span class="end"></span></div><div class="cs-row cs-go"><span>${IC.gear}</span><span><b>Instructions</b><small>What every agent reads first.</small></span><span class="end"></span></div><div class="cs-row cs-go" data-dgo="dReport"><span>${IC.gear}</span><span><b>Report a problem</b><small>Collect logs from your machines and send them to us.</small></span><span class="end"></span></div></div></div>`);
  const nsBar = () => deskScene(dBar(`<div class="dinput" style="cursor:text"><span class="caret" style="height:22px"></span><span class="txt ph" style="margin-left:-10px">Run a command or start an agent</span></div>` + dHints([["⌘K", "search"], ["⌘N", "new agent"], ["esc", "to hide"]])));



  // The tray with no server: no status line, no Subscription item, nothing of the assistant.
  // Quick agent opens the bar, which starts an agent. Waiting counts what coding agents ask.
  const nsTray = () => {
    const it = (label, key = "", attrs = "") => `<button class="it" role="menuitem" ${attrs}><span style="display:grid"></span><span>${label}</span><span class="k">${key}</span></button>`;
    const menu = `<div class="nmenu" role="menu" aria-label="Beep">
      ${it("Open Beep", "", go("ns", "home"))}
      ${it("Quick agent", "", go("ns", "bar"))}
      <div class="hdr">Or press Option twice, anywhere</div>
      <hr>
      <div class="hdr">Waiting</div>
      ${it(`<span style="display:flex;gap:7px;align-items:center"><span class="ddot" style="background:var(--d-coral)"></span>Run cargo test? (fix-approval)</span>`, "", go("ns", "agents"))}
      <hr>
      ${it("Report a Problem", "", 'data-dgo="dReport"')}
      ${it("Settings", "⌘,", go("ns", "settings"))}
      ${it("Check for Updates", "", 'data-toast="Beep is up to date"')}
      ${it("Show Logs", "", 'data-toast="Opens the log folder in Finder"')}
      <hr>
      ${it("Quit Beep", "⌘Q", 'data-toast="Quit installs a downloaded update; disabled in the mock"')}
    </div>`;
    return deskScene("", { tray: true, menu, badge: true });
  };

  // First run, over Home: the ticket card, as it is.
  const frCard = body => `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5">
    <div role="dialog" aria-label="First run" style="width:min(440px,86%);padding:20px 22px;border-radius:14px;border:1px solid var(--d-border);background:#151617;box-shadow:0 20px 60px rgba(0,0,0,.5);display:grid;gap:12px">${body}</div></div>`;
  const frOver = body => nsHome(false).replace(/<\/div><\/div>\s*$/, frCard(body) + "</div></div>");
  const FR_LOGO = '<svg width="34" height="34" viewBox="0 0 28 28" fill="currentColor" aria-hidden="true"><path d="M14 1 27 27h-3.1L15.4 10v17h-2.8V10L4.1 27H1Z"/></svg>';
  const frBig = (title, text, attrs, pri) => `<button class="dbtn${pri ? " pri" : ""}" ${attrs} style="display:grid;gap:3px;justify-items:start;width:100%;height:auto;padding:13px 15px;text-align:left;white-space:normal"><b style="font-size:14px;font-weight:600">${title}</b><span style="font-size:12.5px;font-weight:400;opacity:.8;line-height:1.4">${text}</span></button>`;
  const frTicket = () => frOver(S.d.frlink ? `<div>${FR_LOGO}</div><div><b style="font-size:16px">Link another desktop</b><div style="font-size:12.5px;color:var(--d-muted);margin-top:4px;line-height:1.45">On your other desktop, open Machines, then Add a machine, choose Desktop and copy the ticket. This computer then shares its machines, projects and keys.</div></div>
      <input id="jt" aria-label="Ticket" placeholder="Paste a ticket" style="${MONO};width:100%;box-sizing:border-box">
      <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri" disabled>Link</button><button class="dbtn quiet" data-dact="dset:frlink:" data-dgo="dNoServer">Back</button></div>` : `<div style="display:grid;gap:8px">${FR_LOGO}<div><b style="font-size:16px">Welcome to Beep</b><div style="font-size:12.5px;color:var(--d-muted);margin-top:2px">Run coding agents on your machines.</div></div></div>
      ${frBig("Start", "Use Beep on this computer. You can add machines later.", go("ns", "setup"), true)}
      ${frBig("Link another desktop", "You already use Beep on another computer. Share its machines, projects and keys with this one.", 'data-dact="dset:frlink:1" data-dgo="dNoServer"', false)}`);
  // First run, after Start: set up this computer. One button installs what is missing, then
  // opens each sign-in in the browser in turn. Every row can be skipped.
  const SU_TOOLS = [
    ["git", "Git", "2.47.1", "Projects and commits"],
    ["node", "Node.js", "", "Needed to install the agents"],
    ["claude", "Claude Code", "", "Coding agent"],
    ["codex", "Codex", "", "Coding agent"],
  ];
  const suRow = (name, sub, status, tone, action) => `<div style="display:grid;grid-template-columns:1fr auto;gap:10px;align-items:center;padding:10px 12px;border-top:1px solid var(--d-border)">
      <div style="min-width:0"><b style="font-size:13px;font-weight:600">${name}</b><div style="font-size:12px;color:var(--d-muted)">${sub}</div></div>
      <div style="display:flex;gap:8px;align-items:center;white-space:nowrap;font-size:12.5px;color:${tone}">${status}${action || ""}</div></div>`;
  const OK = "var(--d-emerald)", MUTED = "var(--d-muted)";
  const skip = k => S.d["suskip_" + k] ? "" : ` <button class="dbtn quiet small" data-dact="dset:suskip_${k}:1" data-dgo="dNoServer">Skip</button>`;
  const frSetup = () => {
    const st = S.d.su || "ready";
    const rows = SU_TOOLS.map(([k, name, version, sub]) => {
      if (S.d["suskip_" + k]) return suRow(name, sub, "Skipped", MUTED);
      if (k === "git") return suRow(name, sub, `Installed · v${version}`, OK);
      if (st === "ready") return suRow(name, sub, "Not installed", MUTED, skip(k));
      if (st === "installing") return k === "node" ? suRow(name, sub, "Installed · v24.11.0", OK)
        : k === "claude" ? suRow(name, sub, '<span class="dspin"></span>Installing', MUTED) : suRow(name, sub, "Waiting", MUTED);
      if (st === "signin") return k === "node" ? suRow(name, sub, "Installed · v24.11.0", OK)
        : k === "claude" ? suRow(name, sub, "Signed in as you@example.com", OK)
        : suRow(name, sub, '<span class="dspin"></span>Waiting for your browser', MUTED, ' <button class="dbtn quiet small" data-toast="Opens the Codex sign-in page again">Open again</button>');
      return suRow(name, sub, k === "node" ? "Installed · v24.11.0" : "Signed in as you@example.com", OK);
    }).join("");
    const missing = SU_TOOLS.filter(([k]) => k !== "git" && !S.d["suskip_" + k]).length;
    const foot = st === "ready" ? `<button class="dbtn pri" data-dact="dset:su:installing" data-dgo="dNoServer"${missing ? "" : " disabled"}>Set up</button><span style="font-size:12px;color:var(--d-muted)">Installs ${missing}, then opens each sign-in.</span><span style="margin-left:auto"><button class="dbtn quiet" ${go("ns", "first")}>Later</button></span>`
      : st === "installing" ? `<button class="dbtn pri" disabled>Setting up…</button><span style="font-size:12px;color:var(--d-muted)">Installing Claude Code (2 of 3)</span><span style="margin-left:auto"><button class="dbtn quiet" data-dact="dset:su:ready" data-dgo="dNoServer">Cancel</button></span>`
      : st === "signin" ? `<button class="dbtn pri" disabled>Signing in…</button><span style="font-size:12px;color:var(--d-muted)">Signing in to Codex</span><span style="margin-left:auto"><button class="dbtn quiet" ${go("ns", "first")}>Later</button></span>`
      : `<button class="dbtn pri" ${go("ns", "first")}>Open Beep</button><span style="font-size:12px;color:var(--d-muted)">This computer is ready to run agents.</span>`;
    return frOver(`<div style="display:grid;gap:8px">${FR_LOGO}<div><b style="font-size:16px">Set up this computer</b><div style="font-size:12.5px;color:var(--d-muted);margin-top:2px">What Beep needs to run coding agents here.</div></div></div>
      <div style="border:1px solid var(--d-border);border-radius:10px;overflow:hidden;margin-top:4px">${rows.replace('border-top:1px solid var(--d-border)', '')}</div>
      <div style="display:flex;gap:10px;align-items:center">${foot}</div>`);
  };
  D.dNoServer = () => {
    const v = S.d.ns || "first";
    if (v === "bar") return nsBar();
    if (v === "tray") return nsTray();
    if (v === "ticket") return frTicket();
    if (v === "setup") return frSetup();
    const pages = { first: () => nsHome(false), home: () => nsHome(true), projects: nsProjects, project: nsProject, machines: nsMachines, agents: nsAgents, terminals: nsTerminals, settings: nsSettings };
    return (pages[v] || pages.first)();
  };
  VARIANTS.dNoServer = [["First run: Home", "first"], ["Home with machines", "home"], ["Projects", "projects"], ["One project, no agent panel", "project"], ["Machines", "machines"], ["Settings", "settings"], ["Bar", "bar"], ["Tray", "tray"], ["First run: ticket", "ticket"]]
    .map(([l, k]) => [l, () => { S.d.ns = k; render(); }, () => (S.d.ns || "first") === k]);
  VARIANTS.dNoServer.push(...[["Setup: ready", "ready"], ["Setup: installing", "installing"], ["Setup: signing in", "signin"], ["Setup: done", "done"]]
    .map(([l, k]) => [l, () => { S.d.ns = "setup"; S.d.su = k; render(); }, () => S.d.ns === "setup" && S.d.su === k]));

  // Add a machine by ticket. The ticket works once and lasts an hour.
  const line = (txt, toast) => `<div style="display:flex;gap:8px;align-items:center;margin-bottom:14px"><div style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;${MONO}">${txt}</div><button class="dbtn" data-toast="${toast}" style="flex:none">Copy</button></div>`;
  const step = t => `<div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">${t}</div>`;
  const kindCard = (k, t, sub, on) => `<button class="dbtn" role="radio" aria-checked="${on}" aria-pressed="${on}" data-dact="dset:tk:${k}" data-dgo="dAddTicket" style="display:grid;gap:2px;text-align:left;padding:9px 12px;height:auto;${on ? "border-color:var(--d-emerald);background:rgba(76,183,130,.08)" : ""}"><b style="font-weight:600">${t}</b><span style="font-size:12px;color:var(--d-muted);font-weight:400">${sub}</span></button>`;
  const waiting = who => `<div style="display:flex;gap:8px;align-items:center;font-size:12.5px;color:var(--d-muted)"><span class="dspin"></span>Waiting for ${who} · 58 min left<span style="margin-left:auto"><button class="dbtn quiet" data-dact="dset:ts:expired" data-dgo="dAddTicket">Cancel</button></span></div>`;
  const note = `<div style="font-size:12.5px;color:var(--d-muted);margin-bottom:12px">Works once and expires in an hour.</div>`;

  const otherMac = () => {
    const s = S.d.ts || "empty";
    const val = ["pasted", "joining", "bad"].includes(s) ? TICKET : "";
    const msg = s === "joining" ? `<div style="display:flex;gap:8px;align-items:center;font-size:12.5px;color:var(--d-muted)"><span class="dspin"></span>Linking</div>`
      : s === "bad" ? `<div style="font-size:12.5px;color:var(--d-err)">This ticket expired or was already used. Ask for a new one.</div>` : "";
    const sheet = `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5"><div style="width:min(560px,86%);padding:18px 20px;border-radius:14px;border:1px solid var(--d-border);background:#151617;display:grid;gap:12px">
      <div style="display:flex;justify-content:space-between"><b style="font-size:15px">Link another desktop</b><kbd style="font-size:11px;opacity:.7">esc</kbd></div>
      <div style="font-size:12.5px;color:var(--d-muted);line-height:1.45">On the other desktop, open Machines, then Add a machine, choose Desktop and copy the ticket. Paste it here to share its machines, projects and keys.</div>
      <input id="jt" aria-label="Desktop ticket" placeholder="Paste a ticket" value="${val}" style="${MONO};width:100%;box-sizing:border-box">
      ${msg}
      <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri"${val && s !== "joining" ? ` data-dact="dset:ts:joining" data-dgo="dAddTicket"` : " disabled"}>Link</button><button class="dbtn quiet" ${go("ns", "machines")}>Cancel</button></div></div></div>`;
    return nsMachines().replace(/<\/div><\/div>\s*$/, sheet + "</div></div>");
  };

  D.dAddTicket = () => {
    const k = S.d.tk || "node", s = S.d.ts || "ready";
    if (k === "other") return otherMac();
    const node = k !== "desktop" && k !== "phone", box = k === "container", who = k === "phone" ? "Phone" : box ? "ci-runner" : node ? "build-box" : "MacBook Air";
    let body;
    if (s === "joined") body = `<div style="display:flex;gap:10px;align-items:center;font-size:14px"><span class="ddot"></span><b>${k === "phone" ? "Phone paired" : who + " joined"}</b></div>
       <div style="font-size:12.5px;color:var(--d-muted);margin:8px 0 14px">${k === "phone" ? "Observe projects and agents from your phone. Phones do not host projects or run code, and stay out of Machines." : box ? "Linux, Claude Code. It shows under Machines and can run agents now." : node ? "Linux, Docker, Claude Code, Codex. It shows under Machines and can run agents now." : "macOS, Claude Code, Codex. It shows under Machines and can run agents now."}</div>
       <button class="dbtn pri" ${go("ns", "machines")}>Done</button>`;
    else if (s === "expired") body = `<div style="font-size:13px;margin-bottom:12px">${k === "phone" ? "This code expired. Phone codes work once and last 5 minutes." : "This ticket expired. A ticket works once and lasts an hour."}</div><button class="dbtn pri" data-dact="dset:ts:ready" data-dgo="dAddTicket">New ticket</button>`;
    else if (node) body = `${step("1. Install beep-node on the machine")}
       ${line("curl -fsSL https://releases.flashmind.com/beep/node/install.sh | sh", "Copied")}
       ${step("2. Run this there")}
       ${line(`beep-node mesh join ${TICKET}`, "Copied")}
       ${s === "waiting" ? waiting("the machine") : note}`;
    else if (k === "phone") body = `${step("Scan with your phone")}<div style="width:200px;height:200px;background:#fff;color:#111;display:grid;place-items:center;margin:12px auto" aria-label="Phone pairing QR code">QR code</div><p>In Beep on your phone, open Linked devices and scan this code.</p><details><summary>Copy ticket instead</summary>${line(TICKET, "Copied")}</details><p>Expires in 5 minutes. Keep this ticket private.</p>${waiting("the phone")}`;
    else body = `${step("Ticket")}
       ${line(TICKET, "Copied")}
       <div style="font-size:12.5px;color:var(--d-muted);margin-bottom:12px">On the other desktop, open Machines, then Link another desktop, and paste this ticket.</div>
       ${s === "waiting" ? waiting("the other desktop") : note}`;
    const sheet = `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5">
      <div style="width:min(560px,86%);padding:18px 20px;border-radius:14px;border:1px solid var(--d-border);background:#151617;box-shadow:0 20px 60px rgba(0,0,0,.5)">
      <div style="display:flex;justify-content:space-between;margin-bottom:12px"><b style="font-size:15px">Add a machine</b><kbd style="font-size:11px;opacity:.7">esc</kbd></div>
      ${s === "joined" || s === "expired" ? "" : `<div role="radiogroup" aria-label="Kind of machine" style="display:grid;grid-template-columns:1fr 1fr;gap:8px;margin-bottom:14px">${kindCard("node", "Node", "A server or any always-on box", k === "node")}${kindCard("container", "Container", "Removed after a day offline", box)}${kindCard("desktop", "Desktop", "macOS or Windows", k === "desktop")}${kindCard("phone", "Phone", "Observe projects and agents", k === "phone")}</div>`}${body}</div></div>`;
    return nsHome(S.d.ns === "home").replace(/<\/div><\/div>\s*$/, sheet + "</div></div>");
  };
  const T = (l, kind, st) => [l, () => { S.d.tk = kind; S.d.ts = st; render(); }, () => (S.d.tk || "node") === kind && (S.d.ts || "ready") === st];
  VARIANTS.dAddTicket = [
    T("Node: ticket ready", "node", "ready"), T("Node: waiting", "node", "waiting"), T("Node: joined", "node", "joined"), T("Node: expired", "node", "expired"),
    T("Container: ticket ready", "container", "ready"), T("Container: joined", "container", "joined"),
    T("Desktop: ticket ready", "desktop", "ready"), T("Desktop: waiting", "desktop", "waiting"), T("Desktop: joined", "desktop", "joined"), T("Desktop: expired", "desktop", "expired"),
    T("Other desktop: Link another desktop", "other", "empty"), T("Other desktop: ticket pasted", "other", "pasted"), T("Other desktop: linking", "other", "joining"), T("Other desktop: ticket expired", "other", "bad"),
  ];

  // Machines list for an account that already has machines: the ones not in the mesh yet.
  const nj = (name, os, ticket) => `<div class="cc-row cc-line"><span class="ddot"></span><button class="t" title="${name}"><b>${name}</b><span>${os} · still works through your account</span></button><span class="r"><span class="pj-st blocked">Not joined</span><button class="dbtn small" data-toast="Copied beep-node mesh join ${ticket}">Copy join line</button></span></div>`;
  D.dMachinesMesh = () => {
    const all = S.d.mm === "joined";
    return ccWin(ccSide("machines"), top("Machines", all ? "3 of 3 online" : "3 of 3 online · 2 not joined", addBtn())
      + `<div class="cc-view"><div><div class="cc-sec"><h4>In the mesh<span class="n">${all ? 3 : 1}</span></h4></div><div class="cc-panel">${row("", "This computer", "macOS · Claude Code, Codex", "idle")}${all ? row("", "build-box", "Linux · Docker, Claude Code", "1 agent") + row("", "gpu-box", "Linux · Codex", "idle") : ""}</div></div>
      ${all ? "" : `<div><div class="cc-sec"><h4>Not in the mesh yet<span class="n">2</span></h4></div><div class="cc-panel">${nj("build-box", "Linux", "beepjoin1example3kd7")}${nj("gpu-box", "Linux", "beepjoin1example9wt2")}</div></div>`}</div>`);
  };
  VARIANTS.dMachinesMesh = [["Two not joined", "pending"], ["All joined", "joined"]].map(([l, k]) => [l, () => { S.d.mm = k; render(); }, () => (S.d.mm || "pending") === k]);

  DESK_SCREENS.splice(DESK_SCREENS.findIndex(g => g.group === "Command center") + 1, 0, { group: "Local first", items: [
    ["dNoServer", "No server", "CommandCenterWindow, PromptBar, TrayMenu, FirstRun, no server"],
    ["dAddTicket", "Add a machine by ticket", "CommandCenterWindow, AddMachineSheet, JoinWindow"],
    ["dMachinesMesh", "Machines, not joined", "CommandCenterWindow, MachinesList"]] });
  render();
}
