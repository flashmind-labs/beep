// ---- Command center on the phone (#467, 2026-11-07) ----
// With server features off the phone opens here: the desktop's Home, projects, coding
// agents, terminals and machines, one column each, under five bottom tabs. Everything on
// these screens goes over the mesh. The iOS and Android copies differ only in `ios`.
(() => {
  const ios = true, views = ios ? P : R, catalog = ios ? IOS_SCREENS : SCREENS;
  const C = {
    homeTab: "overview", needs: "some", prTab: "prs", prFilter: "All", agentsTab: "running",
    session: "ask", pane: "log", term: "shell", mods: [], termMode: "control",
    addKind: "node", skin: "system", sheet: null, stay: false, machine: "build-box", answered: "",
  };
  const P_ = {
    home: "M3 11l9-7 9 7v9h-6v-6H9v6H3z", projects: "M3 7h7l2 2h9v11H3z", agents: "M4 5h16v11H9l-5 4z M8 9h8M8 12.5h5",
    terminals: "M4 5h16v14H4z M7 9l3 3-3 3M12 15h5", machines: "M3 4h18v6H3z M3 14h18v6H3z M7 7h.1M7 17h.1",
    bell: "M6 16V11a6 6 0 0112 0v5l2 2H4z M10 21h4", gear: "M12 9a3 3 0 100 6 3 3 0 000-6z M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M5 19l2-2M17 7l2-2",
    back: ios ? "M15 5l-7 7 7 7" : "M19 12H5M11 6l-6 6 6 6", chev: "M9 6l6 6-6 6", plus: "M12 5v14M5 12h14", dots: "M5 12h.1M12 12h.1M19 12h.1",
    send: "M5 12h14M13 6l6 6-6 6", image: "M4 5h16v14H4z M4 16l5-5 4 4 3-3 4 4 M15 9h.1", at: "M16 12a4 4 0 10-1.5 3.1c.9.9 3.5 1 3.5-3.1a6 6 0 10-2.5 4.9",
    search: "M11 4a7 7 0 100 14 7 7 0 000-14z M20 20l-4-4", stop: "M7 7h10v10H7z", kbd: "M3 7h18v11H3z M7 11h.1M11 11h.1M15 11h.1M7 14.5h10",
    copy: "M8 8h12v12H8z M16 8V4H4v12h4", refresh: "M20 11a8 8 0 10-2 6M20 5v6h-6",
  };
  const ic = (n, s = 18) => `<svg width="${s}" height="${s}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${P_[n]}"/></svg>`;
  const go = to => `data-cc-go="${to}"`;
  const act = (a, v = "") => `data-cc-act="${a}"${v ? ` data-v="${v}"` : ""}`;
  const toast = t => `data-cc-toast="${t}"`;
  const tag = (t, k = "") => `<span class="cc-tag${k ? " " + k : ""}">${t}</span>`;
  const btn = (t, k = "", attr = "") => `<button class="cc-btn${k ? " " + k : ""}" ${attr || toast(t)}>${t}</button>`;
  const kind = (t, k = "") => `<span class="cc-kind${k ? " " + k : ""}">${t}</span>`;
  const ag = a => `<span class="cc-ag${a === "CX" ? " cx" : a === "GM" ? " gm" : ""}">${a}</span>`;
  const row = (lead, t, sub, right = "", to = "", cls = "") => `<${to ? "button" : "div"} class="cc-row${cls ? " " + cls : ""}" ${to ? go(to) : ""}>${lead}<span class="t"><b>${t}</b>${sub ? `<small>${sub}</small>` : ""}</span><span class="r">${right}${to ? `<span class="chev">${ic("chev", 14)}</span>` : ""}</span></${to ? "button" : "div"}>`;
  const sec = (title, n, body, right = "") => `<div><div class="cc-sec"><h4>${title}${n !== "" ? `<span class="n">${n}</span>` : ""}</h4>${right}</div>${body}</div>`;
  const panel = h => `<div class="cc-panel">${h}</div>`;
  const seg = (key, items) => `<div class="cc-seg">${items.map(([v, l, n]) => `<button ${act("seg", key + ":" + v)} aria-pressed="${C[key] === v}">${l}${n ? `<span class="n">${n}</span>` : ""}</button>`).join("")}</div>`;
  const chips = (key, items) => `<div class="cc-chips">${items.map(v => `<button ${act("seg", key + ":" + v)} aria-pressed="${C[key] === v}">${v}</button>`).join("")}</div>`;
  const kv = pairs => `<div class="cc-kv">${pairs.map(([k, v]) => `<span>${k}</span><span>${v}</span>`).join("")}</div>`;

  const skin = () => C.skin === "system" ? (document.documentElement.dataset.theme === "light" ? "paper" : "linear") : C.skin;
  const TABS = [["home", "Home", "ccHome"], ["projects", "Projects", "ccProjects"], ["agents", "Agents", "ccAgents"], ["terminals", "Terminals", "ccTerminals"], ["machines", "Machines", "ccMachines"]];
  const tabs = on => `<nav class="cc-tabs">${TABS.map(([id, l, to]) => `<button ${go(to)} aria-current="${on === id}"><span class="ic">${ic(id, 21)}</span>${l}${id === "home" && C.needs === "some" && on !== "home" ? '<span class="dot"></span>' : ""}</button>`).join("")}</nav>`;
  const top = (title, right = "", back = "", sub = "") => `<header class="cc-top${back ? " sub" : ""}">${back ? `<button class="cc-ib" ${go(back)} aria-label="Back">${ic("back", 20)}</button>` : ""}<h1>${title}${sub ? `<small>${sub}</small>` : ""}</h1>${right}</header>`;
  const bell = () => `<button class="cc-ib" ${go("ccBell")} aria-label="Notifications">${ic("bell", 19)}${C.needs === "some" ? '<span class="n">3</span>' : ""}</button>`;
  const sheet = () => {
    if (!C.sheet) return "";
    const body = SHEETS[C.sheet]();
    return `<div class="cc-sheet-bg" ${act("close")}><div class="cc-sheet" data-cc-stop><div class="grab"></div>${body}</div></div>`;
  };
  const screen = (tab, html) => `<div class="cc ${ios ? "ios" : "android"}" data-skin="${skin()}">${html}${tab ? tabs(tab) : ""}${sheet()}</div>`;
  const page = (tab, head, body) => screen(tab, head + `<div class="cc-body">${body}</div>`);

  // ---- Data ----
  const MACHINES = [
    { id: "build-box", os: "Linux", spec: "Ubuntu 26.04 · 32 cores · 124 GB", agents: 2, free: "410 GB", mem: [41, 124], disk: [590, 1000], on: true, now: "2 agents", node: "0.5.15" },
    { id: "gpu-box", os: "Linux", spec: "Ubuntu 24.04 · 16 cores · 64 GB · RTX 5090", agents: 1, free: "88 GB", mem: [22, 64], disk: [912, 1000], on: true, now: "1 agent", node: "0.5.14", low: true },
    { id: "laptop", os: "macOS", spec: "macOS 26 · 10 cores · 24 GB", agents: 0, free: "120 GB", mem: [14, 24], disk: [380, 500], on: true, now: "idle", node: "0.5.15" },
    { id: "win-box", os: "Windows", spec: "Windows 11 · 12 cores · 32 GB", agents: 0, free: "", mem: [0, 32], disk: [0, 1], on: false, now: "offline 2 h", node: "0.5.13" },
  ];
  const strip = () => `<div class="cc-strip">${MACHINES.map(m => `<button ${act("machine", m.id)}><b><span class="cc-dot${m.on ? "" : " off"}"></span>${m.id}</b><small>${m.on ? `${m.now} · ${m.free} free` : m.now}</small></button>`).join("")}<button class="add" ${go("ccAddMachine")} aria-label="Add a machine">${ic("plus", 18)}</button></div>`;
  const askOpts = () => C.answered ? `<div class="mu">${C.answered}</div>` : `<div class="opts">${btn("Yes", "pri", act("answer", "Allowed once"))}${btn("Yes, and don't ask again for <span class='mono'>cargo test</span>", "", act("answer", "Allowed for this session"))}${btn("No, tell Claude what to do instead", "", act("answer", "Denied"))}</div>`;
  const NEEDS = () => [
    row(ag("CC"), "Run <span class='mono'>cargo test -p sample-core</span>?", "Add keyboard shortcuts · build-box · 1 min", btn("Answer", "pri", go("ccSession")), ""),
    row(kind("PR", "wait"), "#42 Add keyboard shortcuts", "sample-project · review requested by a teammate", btn("Review", "", go("ccPr"))),
    row(kind("⇄", "bad"), "notes/plan.md changed on two machines", "sample-docs · build-box and laptop", btn("Resolve", "", go("ccSync"))),
  ].join("");
  const RUNNING = () => [
    row(ag("CC"), "Add keyboard shortcuts", "sample-project · feature/shortcuts · build-box", tag("Asks", "wait"), "ccSession"),
    row(ag("CX"), "Speed up the search index", "sample-project · worktree search-speed · gpu-box", `<span class="cc-shimmer">Editing index.rs</span>`, "ccSession"),
    row(ag("CC"), "Write the migration test", "sample-library · main · gpu-box", `<span class="mu">Running tests</span>`, "ccSession", "kid"),
  ].join("");

  // ---- Home ----
  const homeTabs = () => seg("homeTab", [["overview", "Overview"], ["prs", "PRs and issues", "9"], ["machines", "Machines"]]);
  views.ccHome = () => {
    const head = top("Home", `<button class="cc-pill" ${act("sheet", "usage")}><b>$4.20</b> today</button>${bell()}<button class="cc-ib" ${go("ccSettings")} aria-label="Settings">${ic("gear", 19)}</button>`);
    let body = strip() + homeTabs();
    if (C.homeTab === "overview") {
      body += C.needs === "some" ? sec("Needs you", 3, panel(NEEDS())) : panel(`<div class="cc-empty"><b>Nothing needs you</b>Agents ask here when they need an answer.</div>`);
      body += sec("Running", 3, panel(RUNNING()), `<button class="link" ${go("ccStart")}>Start an agent</button>`);
    } else if (C.homeTab === "prs") body += prsBody();
    else body += machinesTable();
    return page("home", head, body);
  };
  const PRS = [
    [kind("PR", "wait"), "#42 Add keyboard shortcuts", "sample-project · feature/shortcuts", tag("Review", "wait")],
    [kind("PR", "bad"), "#40 Cache search results", "sample-project · yours", tag("Checks failing", "bad")],
    [kind("PR"), "#37 Update dependencies", "sample-library · yours · draft", tag("Draft")],
    [kind("PR", "ok"), "#12 Fix the Windows path", "sample-cli · yours", tag("Approved", "ok")],
  ];
  const ISSUES = [
    ["#93 Settings page forgets the zoom level", "sample-project · bug · 2 comments"],
    ["#91 Export to CSV", "sample-project · feature"],
    ["#88 Windows install fails behind a proxy", "sample-cli · bug · 5 comments"],
    ["#80 Docs for the plugin API", "sample-library · docs"],
  ];
  const prsBody = () => seg("prTab", [["prs", "Pull requests", "5"], ["issues", "Issues", "4"]]) + chips("prFilter", C.prTab === "prs" ? ["All", "Waiting on you", "Yours", "Failing"] : ["All", "bug", "feature", "docs"]) +
    (C.prTab === "prs"
      ? panel(PRS.map(([k, t, s, r]) => row(k, t, s, r, "ccPr")).join("")) + `<p class="cc-note">Across GitHub and Forgejo. Repos without a project show Import.</p>`
      : panel(ISSUES.map(([t, s]) => row(`<span class="cc-toggle" style="width:18px;height:18px;border-radius:5px" aria-hidden="true"></span>`, t, s, "", "ccIssue")).join("")) + btn("Start 2 agents", "pri big", go("ccStart")));
  const machinesTable = () => panel(`<div class="cc-tbl" style="grid-template-columns:minmax(0,1.4fr) .8fr .9fr .8fr">
      <span class="th">Machine</span><span class="th">Agents</span><span class="th">Disk free</span><span class="th end">Memory</span>
      ${MACHINES.map(m => `<span><span class="cc-dot${m.on ? "" : " off"}"></span><b style="font-weight:500">${m.id}</b></span><span>${m.on ? m.agents : "-"}</span><span class="${m.low ? "wait" : ""}">${m.on ? m.free : "-"}</span><span class="end">${m.on ? `${m.mem[0]}/${m.mem[1]} GB` : "-"}</span>`).join("")}
    </div>`) + sec("On build-box", "", panel(row(kind(">_"), "dev server", "sample-project · npm run dev", tag("Running", "ok"), "ccTerminal") + row(kind("⇄"), "sample-docs", "File sync · 3 files left", `<span class="mu">92%</span>`, "ccSync"))) +
    sec("Not in the mesh yet", 1, panel(row(kind("?"), "old-server", "Signed in to beep-server, no mesh ticket", btn("Copy join line")))) ;
  VAR("ccHome", [["Overview", () => C.homeTab = "overview", () => C.homeTab === "overview"], ["Needs you", () => { C.homeTab = "overview"; C.needs = "some"; }, () => C.homeTab === "overview" && C.needs === "some"], ["All clear", () => { C.homeTab = "overview"; C.needs = "none"; }, () => C.homeTab === "overview" && C.needs === "none"], ["PRs and issues", () => C.homeTab = "prs", () => C.homeTab === "prs"], ["Machines tab", () => C.homeTab = "machines", () => C.homeTab === "machines"]]);

  // ---- Notifications and usage ----
  views.ccBell = () => page("home", top("Notifications", btn("Clear", "", toast("Cleared")), "ccHome"),
    sec("Today", "", panel(
      `<div class="cc-row">${ag("CC")}<span class="t"><b>Run <span class="mono">cargo test -p sample-core</span>?</b><small>Add keyboard shortcuts · build-box · 1 min</small></span></div><div class="cc-row" style="justify-content:flex-end;gap:6px">${btn("Deny")}${btn("Allow", "pri")}</div>` +
      row(kind("✓", "ok"), "Codex finished: Speed up the search index", "gpu-box · 14 min · 6 files changed", "<span>12:40</span>", "ccSession") +
      row(kind("✕", "bad"), "CI failed on #40 Cache search results", "sample-project · test (windows)", "<span>12:12</span>", "ccRun") +
      row(kind("⇄", "bad"), "Sync conflict in sample-docs", "notes/plan.md", "<span>11:05</span>", "ccSync"))) +
    sec("Yesterday", "", panel(row(kind("!"), "win-box went offline", "Last seen 18:22", "") + row(kind("↗"), "Move finished", "Write the migration test, build-box to gpu-box", ""))));
  const SHEETS = {
    usage: () => `<h3>Usage</h3>` + sec("Plan limits", "", panel(`<div class="cc-row"><span class="t"><b>Claude · 5 hours</b><div class="cc-bar"><i style="width:62%"></i></div></span><span class="r">62% · resets 14:00</span></div><div class="cc-row"><span class="t"><b>Claude · week</b><div class="cc-bar"><i style="width:31%"></i></div></span><span class="r">31%</span></div><div class="cc-row"><span class="t"><b>Codex · 5 hours</b><div class="cc-bar warn"><i style="width:88%"></i></div></span><span class="r wait">88%</span></div>`)) +
      sec("Spend", "", panel(kv([["Today", "$4.20"], ["This week", "$31.80"], ["This month", "$112.40"]]))) + sec("Today by project", "", panel(kv([["sample-project", "$3.10"], ["sample-library", "$0.90"], ["sample-cli", "$0.20"]]))),
    move: () => `<h3>Move to another machine</h3><p class="cc-note">The session, its worktree and uncommitted changes go together. It continues where it stopped.</p>` +
      panel(row(`<span class="cc-dot"></span>`, "gpu-box", "Claude Code 2.3 · signed in · clean checkout at the same commit", btn("Move", "pri")) + row(`<span class="cc-dot"></span>`, "laptop", "Claude Code 2.3 · signed in · needs a worktree", btn("Move")) + row(`<span class="cc-dot off"></span>`, "win-box", "Offline", "")),
    sessionMenu: () => `<h3>Add keyboard shortcuts</h3>` + panel(row("", "Move to another machine", "", "", "") .replace('class="cc-row"', `class="cc-row" ${act("sheet", "move")}`) + row("", "Open parent session", "") + row("", "Copy session id", "") + row("", "Stop turn", "") + row("", "Close session", "") + `<div class="cc-row"><span class="t"><b class="bad">Delete…</b></span></div>`),
    model: () => `<h3>Agent</h3>` + sec("Harness", "", `<div class="cc-picks">${["Claude Code", "Codex", "Gemini", "pi", "OpenCode", "flsh"].map((n, i) => `<button class="cc-btn${i === 0 ? " pri" : ""}">${n}</button>`).join("")}</div>`) +
      sec("Model", "", panel(row("", "Opus 4.7", "Default", tag("✓")) + row("", "Sonnet 4.6", "") + row("", "Haiku 4.5", ""))) + sec("Thinking", "", `<div class="cc-picks">${["Off", "Low", "Medium", "High", "Max"].map(n => `<button class="cc-btn${n === "High" ? " pri" : ""}">${n}</button>`).join("")}</div>`) +
      sec("Mode", "", `<div class="cc-picks">${["Ask first", "Accept edits", "Plan", "Bypass"].map(n => `<button class="cc-btn${n === "Ask first" ? " pri" : ""}">${n}</button>`).join("")}</div>`),
    projectMenu: () => `<h3>sample-project</h3>` + panel(row("", "Unpin", "") + row("", "Sync with Syncthing", "Off", `<span class="cc-toggle"></span>`) + row("", "Open on GitHub", "") + `<div class="cc-row"><span class="t"><b class="bad">Remove project…</b><small>Files stay on every machine</small></span></div>`),
    answer: () => `<h3>Run cargo test -p sample-core?</h3><pre class="cc-code">cargo test -p sample-core</pre><p class="cc-note">Claude Code on build-box, in feature/shortcuts.</p>` + askOpts(),
  };

  // ---- Projects ----
  const PROJECTS = [
    ["sample-project", "2 agents · 1 asks", "build-box, gpu-box, laptop", "2 min", true, "#5e6ad2"],
    ["sample-library", "1 agent", "gpu-box", "20 min", true, "#4cb782"],
    ["sample-cli", "idle", "build-box, win-box", "Yesterday", false, "#f2994a"],
    ["sample-docs", "idle · sync conflict", "build-box, laptop", "3 days", false, "#c792ea"],
  ];
  const pj = ([n, a, m, w, , c], to = "ccProject") => row(`<span class="cc-kind" style="background:${c}22;color:${c}">${n[7].toUpperCase()}</span>`, n, `${a} · ${m}`, `<span>${w}</span>`, to);
  views.ccProjects = () => page("projects", top("Projects", `<button class="cc-ib" ${toast("New group")} aria-label="New group">${ic("dots", 20)}</button><button class="cc-ib" ${go("ccAddProject")} aria-label="Add">${ic("plus", 20)}</button>`),
    `<label class="cc-search">${ic("search", 16)}<input placeholder="Filter projects"></label>` +
    sec("Pinned", 2, panel(PROJECTS.filter(p => p[4]).map(p => pj(p)).join(""))) +
    sec("example", 2, panel(PROJECTS.filter(p => !p[4]).map(p => pj(p)).join(""))) +
    sec("Found on your machines", 2, panel(row(kind("+"), "~/src/scratch-api", "build-box · git · main", btn("Import")) + row(kind("+"), "~/work/site", "laptop · git · gh-pages", btn("Import")))));
  views.ccAddProject = () => page("projects", top("Add a project", "", "ccProjects"),
    seg("addKind", [["node", "Clone"], ["container", "Folder"], ["desktop", "New"]]) +
    `<label class="cc-field">Repository<input value="example/sample-api"></label>
     <label class="cc-field">Machine<select><option>build-box</option><option>gpu-box</option></select></label>
     <label class="cc-field">Folder<input class="mono" value="~/src/sample-api"></label>` + btn("Clone and add", "pri big") +
    sec("Your repos without a project", "", panel(row(kind("R"), "example/sample-api", "Updated today", btn("Clone")) + row(kind("R"), "example/infra", "Updated last week", btn("Clone")))));
  const STATUS = () => `<div class="cc-strip">${[["main", ""], ["CI 1 failed", "bad"], ["2 running", ""], ["3/4 machines", ""], ["3 PRs", ""], ["7 issues", ""]].map(([t, k]) => `<span class="cc-pill${k ? " " + k : ""}">${t}</span>`).join("")}</div>`;
  views.ccProject = () => screen("projects", top("sample-project", `<button class="cc-ib" ${act("sheet", "projectMenu")} aria-label="More">${ic("dots", 20)}</button>`, "ccProjects", "example/sample-project") +
    `<div class="cc-body">${STATUS()}` +
    sec("Needs you", 1, panel(row(ag("CC"), "Run <span class='mono'>cargo test -p sample-core</span>?", "Add keyboard shortcuts · build-box", btn("Answer", "pri", go("ccSession"))))) +
    panel([["Running", "2", "ccAgents"], ["Actions", "<span class='bad'>1 failed</span>", "ccRun"], ["Machines", "3 of 4", "ccProjectMachines"], ["File sync", "in sync", "ccSync"], ["Pull requests", "3", "ccPr"], ["Issues", "7", "ccIssue"], ["Sessions", "14 past", "ccAgents"], ["Branches", "5", "ccBranches"]].map(([t, n, to]) => row("", t, "", `<span>${n}</span>`, to)).join("")) +
    sec("Running", 2, panel(RUNNING().split("</button>").slice(0, 2).join("</button>") + "</button>")) +
    `</div><div class="cc-compose"><div class="box"><textarea placeholder="Ask an agent to work on sample-project"></textarea><button class="cc-send" ${go("ccSession")} aria-label="Start">${ic("send", 18)}</button></div>
     <div class="row"><button class="cc-pill" ${act("sheet", "model")}><b>Claude Code</b> Opus · High</button><span class="cc-pill">build-box</span><span class="cc-pill">New worktree</span><button class="cc-ib" aria-label="Add image">${ic("image", 18)}</button></div></div>`);
  views.ccProjectMachines = () => page("projects", top("Machines", "", "ccProject", "sample-project"),
    [["build-box", "~/src/sample-project", "main · up to date", "Synced", ""], ["gpu-box", "~/work/sample-project", "feature/search · 2 ahead, 1 behind", "Synced", "push"], ["laptop", "~/Projects/sample-project", "main · 3 uncommitted files", "Synced by Syncthing", "handoff"]].map(([m, f, b, s, x]) =>
      panel(`<div class="cc-mc"><div class="l1"><span class="cc-dot"></span><b>${m}</b><span class="sp"></span>${tag(s, "ok")}<button class="cc-ib" aria-label="More">${ic("dots", 18)}</button></div><div class="spec mono">${f}</div><div class="spec">${b}</div><div class="cc-picks">${btn("Pull")}${btn("Push", x === "push" ? "pri" : "")}${x === "handoff" ? btn("Bring work here") : ""}${btn("Start an agent here")}</div></div>`)).join("") +
    panel(`<div class="cc-mc"><div class="l1"><span class="cc-dot off"></span><b>win-box</b><span class="sp"></span>${tag("No checkout")}</div><div class="cc-picks">${btn("Add")}${btn("Clone here")}</div></div>`) +
    sec("Terminals", 1, panel(row(kind(">_"), "dev server", "build-box · npm run dev", tag("Running", "ok"), "ccTerminal"))));
  views.ccRun = () => page("projects", top("test (windows)", btn("Open"), "ccProject", "CI · #40 · 4 min ago"),
    panel(kv([["Workflow", "CI"], ["Branch", "feature/cache"], ["Commit", "<span class='mono'>a41f9c2</span> Cache search results"], ["Took", "6 min 12 s"]])) +
    sec("Jobs", 4, panel(row(kind("✓", "ok"), "lint", "38 s") + row(kind("✓", "ok"), "test (linux)", "3 min 2 s") + row(kind("✓", "ok"), "test (macos)", "4 min 40 s") + row(kind("✕", "bad"), "test (windows)", "6 min 12 s", btn("Full log")))) +
    sec("Failure", "", `<pre class="cc-code">---- cache::evicts_oldest stdout ----
thread 'cache::evicts_oldest' panicked at
src/cache.rs:88:9:
assertion failed: left == right
  left: 3
 right: 2</pre>`) + btn("Fix this with an agent", "pri big", go("ccStart")));
  views.ccPr = () => page("projects", top("#42 Add keyboard shortcuts", btn("Open"), "ccProject", "feature/shortcuts into main"),
    `<div class="cc-picks">${tag("Review requested", "wait")}${tag("Checks passing", "ok")}${tag("+184 −36 · 7 files")}</div>` +
    `<div class="cc-say">Adds a shortcut map for the editor and a settings page to change bindings. Closes #31.</div>` +
    `<div class="cc-picks">${btn("Review diff", "pri", go("ccChanges"))}${btn("Continue with an agent", "", go("ccSession"))}${btn("Agent review")}</div>` +
    sec("Comments", 2, panel(row(kind("A"), "Looks good. Could the chord parser reject empty input?", "teammate · 1 h", "") + row(kind("B"), "Done in 3f2a1b0.", "you · 40 min", ""))) +
    `<label class="cc-field">Comment<textarea placeholder="Write a comment"></textarea></label>` + btn("Comment", "big"));
  views.ccIssue = () => page("projects", top("#93 Settings page forgets the zoom level", btn("Open"), "ccProject", "sample-project · bug"),
    `<div class="cc-say">After a restart the zoom goes back to 100%. Expected: the last zoom stays.</div>` + btn("Work on it", "pri big", go("ccStart")) + sec("Comments", 1, panel(row(kind("A"), "Seen on macOS and Windows.", "teammate · 2 days", ""))));
  views.ccBranches = () => page("projects", top("Branches", "", "ccProject", "sample-project"), panel([["main", "default"], ["feature/shortcuts", "#42 · 3 ahead"], ["feature/search", "gpu-box worktree · 2 ahead"], ["fix/windows-path", "merged"]].map(([b, s]) => row(kind("⑂"), `<span class="mono">${b}</span>`, s, btn("Diff", "", go("ccChanges")))).join("")));
  views.ccSync = () => page("projects", top("File sync", "", "ccHome"),
    sec("sample-docs", "1 conflict", panel(`<div class="cc-row"><span class="t"><b>notes/plan.md</b><small>Changed on build-box 11:02 and laptop 11:04</small></span></div><div class="cc-row" style="flex-wrap:wrap;gap:6px">${btn("Compare")}${btn("Keep laptop's", "pri")}${btn("Keep build-box's")}${btn("Fix with an agent")}</div>`) +
      `<div style="height:8px"></div>` + panel(row(`<span class="cc-dot"></span>`, "build-box", "Up to date", "") + row(`<span class="cc-dot warn"></span>`, "laptop", "3 files left · 1.2 MB/s · 4 s", btn("Pause")))) +
    sec("sample-project", "", panel(row(`<span class="cc-dot"></span>`, "3 machines", "In sync", ""))));

  // ---- Agents ----
  views.ccAgents = () => page("agents", top("Coding agents", `<button class="cc-pill" ${act("sheet", "usage")}><b>62%</b> Claude 5 h</button><button class="cc-ib" ${go("ccStart")} aria-label="Start">${ic("plus", 20)}</button>`),
    seg("agentsTab", [["running", "Running", "3"], ["past", "Past"], ["copies", "Copies", "1"]]) +
    (C.agentsTab === "running" ? sec("Needs you", 1, panel(row(ag("CC"), "Add keyboard shortcuts", "Claude Code · feature/shortcuts · build-box", tag("Asks", "wait"), "ccSession"))) +
      sec("Working", 2, panel(row(ag("CX"), "Speed up the search index", "Codex · worktree search-speed · gpu-box", `<span class="cc-shimmer">Editing index.rs</span>`, "ccSession") + row(ag("CC"), "Write the migration test", "Claude Code · sample-library · gpu-box", `<span class="mu">Running tests</span>`, "ccSession", "kid") + row(ag("GM"), "Triage new issues", "Gemini · sample-cli · build-box · in a terminal", tag("In terminal"), "ccTerminal")))
      : C.agentsTab === "past" ? sec("Today", "", panel(row(ag("CX"), "Fix the Windows path", "sample-cli · build-box · 40 min", tag("Done", "ok"), "ccSession") + row(ag("CC"), "Update dependencies", "sample-library · laptop · 12 min", tag("Closed"), "ccSession"))) + sec("Started outside Beep", "", panel(row(ag("CC"), "refactor auth module", "build-box · ended", btn("Take over"), "ccSession")))
      : sec("Copies left by moves", 1, panel(row(ag("CC"), "Write the migration test", "Moved to gpu-box · copy on build-box", `${btn("Discard")}${btn("Continue here")}`)))));
  VAR("ccAgents", [["Running", () => C.agentsTab = "running", () => C.agentsTab === "running"], ["Past", () => C.agentsTab = "past", () => C.agentsTab === "past"], ["Copies", () => C.agentsTab = "copies", () => C.agentsTab === "copies"]]);
  views.ccStart = () => page("agents", top("Start an agent", "", "ccAgents"),
    `<label class="cc-field">Task<textarea placeholder="What should the agent do?">Fix the failing evicts_oldest test on Windows</textarea></label>` +
    panel(row("", "Agent", "", `<span>Claude Code · Opus 4.7 · High</span>`, "").replace('class="cc-row"', `class="cc-row" ${act("sheet", "model")}`) + row("", "Project", "", "<span>sample-project</span>", "ccProjects") + row("", "Machine", "", "<span>build-box</span>", "ccMachines") + row("", "Checkout", "", "<span>New worktree from main</span>", "") + row("", "Branch", "", "<span class='mono'>fix/evicts-oldest</span>", "")) +
    `<div class="cc-picks">${btn("Add image")}${btn("Take photo")}</div>` + btn("Start", "pri big", go("ccSession")));

  const LOG = () => `<div class="cc-turn"><div class="cc-you">Add keyboard shortcuts for the editor, with a settings page to change them.</div></div>
    <div class="cc-turn"><div class="cc-think">Thought for 8 s</div>
      <div class="cc-tool">${kind("R")}<span>Read <b>src/editor/keys.ts</b></span></div>
      <div class="cc-tool">${kind("S")}<span>Searched <b>bindKey</b> · 14 results</span></div>
      <div class="cc-tool">${kind("E")}<span>Edited <b>src/editor/shortcuts.ts</b> <span class="ok">+96</span> <span class="bad">−4</span></span></div>
      <div class="cc-panel cc-diff"><div class="h">src/editor/shortcuts.ts</div><div class="a">+export function parseChord(text: string) {</div><div class="a">+  if (!text.trim()) throw new Error("empty");</div><div>   const keys = text.split("+");</div></div>
      <div class="cc-say">The shortcut map and parser are in. Next I'll run the tests.</div></div>
    <div class="cc-plan"><b>Plan</b><span class="d">1. Shortcut map and chord parser</span><span class="d">2. Settings page</span><span class="now">3. Run the tests</span><span class="mu">4. Update the docs</span></div>`;
  views.ccSession = () => {
    const tagS = C.session === "ask" ? tag("Asks", "wait") : C.session === "working" ? tag("Working", "run") : tag("Ended");
    const paneBody = {
      log: () => LOG() + (C.session === "ask" ? ""
        : C.session === "working" ? `<div class="cc-tool">${kind("$")}<span>Running <b>cargo test -p sample-core</b></span></div><div class="cc-shimmer" style="font-size:12px">Thinking</div>` : `<div class="cc-say">All 48 tests pass. The docs are updated in docs/shortcuts.md.</div>`),
      files: () => panel([["src/", ""], ["  editor/", ""], ["    keys.ts", "M"], ["    shortcuts.ts", "A"], ["  settings/", ""], ["    ShortcutsPage.tsx", "A"], ["docs/", ""], ["package.json", ""]].map(([n, g]) => `<button class="cc-row" style="min-height:36px;padding-block:6px" ${n.endsWith("/") ? "" : go("ccFile")}><span class="t mono" style="white-space:pre">${n}</span><span class="r ${g === "A" ? "ok" : g ? "wait" : ""}">${g}</span></button>`).join("")) + `<div class="cc-picks">${btn("Show ignored files")}${btn("New file")}${btn("Refresh")}</div>`,
      changes: () => panel(row(kind("M", "wait"), "src/editor/keys.ts", "+12 −4", "", "ccChanges") + row(kind("A", "ok"), "src/editor/shortcuts.ts", "+96", "", "ccChanges") + row(kind("A", "ok"), "src/settings/ShortcutsPage.tsx", "+76", "", "ccChanges")) + `<div class="cc-picks">${btn("Review diff", "pri", go("ccChanges"))}${btn("Review with agent")}</div>`,
      running: () => panel(row(kind("$"), "npm run dev", "Started by the agent · 12 min", btn("Stop"))),
      artifacts: () => panel(row(kind("▣"), "shortcuts-preview.html", "HTML · 4 KB", "", "") + row(kind("▣"), "keymap.svg", "SVG · 2 KB", "", "")),
    }[C.pane]();
    const ended = C.session === "ended";
    return screen("", top("Add keyboard shortcuts", `<button class="cc-ib" ${act("sheet", "sessionMenu")} aria-label="More">${ic("dots", 20)}</button>`, "ccAgents") +
      `<div class="cc-meta">${tagS}<span class="cc-pill">sample-project</span><span class="cc-pill mono">feature/shortcuts</span><button class="cc-pill" ${act("sheet", "move")}>@build-box</button><span class="cc-pill">41k ctx · $0.42</span></div>` +
      `<nav class="cc-pane">${[["log", "Log"], ["files", "Files"], ["changes", "Changes", "3"], ["running", "Running", "1"], ["artifacts", "Artifacts", "2"]].map(([v, l, n]) => `<button ${act("seg", "pane:" + v)} aria-selected="${C.pane === v}">${l}${n ? `<span class="n">${n}</span>` : ""}</button>`).join("")}</nav>` +
      `<div class="cc-body">${paneBody}</div>` +
      (C.session === "ask" && !C.answered ? `<div class="cc-dock"><div class="cc-ask"><div class="q">Run <span class="mono">cargo test -p sample-core</span>?</div>${askOpts()}</div></div>` : "") +
      (ended ? `<div class="cc-compose">${btn("Resume session", "pri big")}</div>` :
      `<div class="cc-compose">${C.session === "working" ? `<div class="queued">${tag("Queued")}<span>Also add a cheat sheet</span><span style="flex:1"></span>${btn("Edit")}</div>` : ""}
        <div class="box"><textarea placeholder="${C.session === "working" ? "Steer this turn or queue a message" : "Message Claude Code"}"></textarea>${C.session === "working" ? `<button class="cc-send" style="background:var(--c-elev);color:var(--c-fg)" aria-label="Stop">${ic("stop", 16)}</button>` : ""}<button class="cc-send" aria-label="Send">${ic("send", 18)}</button></div>
        <div class="row"><button class="cc-ib" aria-label="Add image">${ic("image", 18)}</button><button class="cc-ib" aria-label="Mention a file">${ic("at", 18)}</button><button class="cc-ib mono" aria-label="Commands">/</button><button class="cc-pill" ${act("sheet", "model")}><b>Ask first</b></button><button class="cc-pill" ${act("sheet", "model")}><b>Opus 4.7</b> High</button></div></div>`) + "", "");
  };
  VAR("ccSession", [["Asks", () => { C.session = "ask"; C.pane = "log"; C.answered = ""; }, () => C.session === "ask"], ["Working", () => { C.session = "working"; C.pane = "log"; }, () => C.session === "working"], ["Ended", () => { C.session = "ended"; C.pane = "log"; }, () => C.session === "ended"], ["Files", () => C.pane = "files", () => C.pane === "files"], ["Changes", () => C.pane = "changes", () => C.pane === "changes"], ["Move", () => C.sheet = "move", () => C.sheet === "move"]]);
  views.ccChanges = () => screen("", top("Changes", `<button class="cc-ib" ${toast("Refreshed")} aria-label="Refresh">${ic("refresh", 18)}</button>`, "ccSession", "3 files · +184 −8 · uncommitted"),
    `<div class="cc-body" style="padding-inline:0;gap:10px"><div style="padding:0 12px">${chips("prFilter", ["All", "Uncommitted"])}</div>
    <div class="cc-panel cc-diff" style="border-radius:0;border-inline:0"><div class="h">src/editor/keys.ts  +12 −4</div><div class="h">@@ -40,7 +40,15 @@</div>
      <div>   export function bindKey(chord: string) {</div><div class="d">-  const keys = chord.split("-");</div><div class="a">+  const keys = parseChord(chord);</div><div class="a">+  if (keys.length > 4) return null;</div>
      <div class="note"><b>You:</b> Return an error instead of null so the settings page can show it.</div>
      <div>   return register(keys);</div><div>   }</div>
      <div class="h">src/editor/shortcuts.ts  +96</div><div class="a">+export function parseChord(text: string) {</div><div class="a">+  if (!text.trim()) throw new Error("empty");</div><div class="a">+  return text.split("+").map(normalize);</div></div>
    <p class="cc-note" style="padding:0 12px">Tap a line to comment. Comments go to the agent together.</p></div>
    <div class="cc-compose"><div class="box">${btn("Review with agent")}<span style="flex:1"></span>${btn("Send review · 1", "pri")}</div></div>`);
  views.ccFile = () => screen("", top("shortcuts.ts", `${btn("Mention")}${btn("Save", "pri")}`, "ccSession", "src/editor · build-box"),
    `<pre class="cc-term" style="background:var(--c-surface);color:var(--c-fg);padding:10px 12px;white-space:pre;overflow:auto"><span class="k"> 1</span> import { normalize } from "./keys";
<span class="k"> 2</span>
<span class="k"> 3</span> export function parseChord(text: string) {
<span class="k"> 4</span>   if (!text.trim()) throw new Error("empty");
<span class="k"> 5</span>   return text.split("+").map(normalize);
<span class="k"> 6</span> }
<span class="k"> 7</span>
<span class="k"> 8</span> export const DEFAULTS = {
<span class="k"> 9</span>   save: "Mod+S",
<span class="k">10</span>   find: "Mod+F",
<span class="k">11</span>   palette: "Mod+K",
<span class="k">12</span> };</pre>`);

  // ---- Terminals ----
  views.ccTerminals = () => page("terminals", top("Terminals", `<button class="cc-ib" ${go("ccNewTerminal")} aria-label="New terminal">${ic("plus", 20)}</button>`),
    sec(`<span class="cc-dot" style="display:inline-block;margin-right:6px"></span>build-box`, 3, panel(row(kind(">_"), "shell", "~/src/sample-project · started 2 h ago", tag("Running", "ok"), "ccTerminal") + row(kind(">_"), "dev server", "sample-project · npm run dev · 12 min", tag("Running", "ok"), "ccTerminal") + row(kind("GM"), "Triage new issues", "sample-cli · gemini", tag("Agent"), "ccTerminal")), btn("New", "", go("ccNewTerminal"))) +
    sec(`<span class="cc-dot" style="display:inline-block;margin-right:6px"></span>gpu-box`, 1, panel(row(kind(">_"), "htop", "~ · started 5 min ago", tag("Running", "ok"), "ccTerminal")), btn("New", "", go("ccNewTerminal"))) +
    sec(`<span class="cc-dot off" style="display:inline-block;margin-right:6px"></span>win-box`, 1, panel(row(kind(">_"), "build", "C:\\src\\sample-cli · offline", tag("Offline"), ""))));
  views.ccNewTerminal = () => page("terminals", top("New terminal", "", "ccTerminals"),
    `<div class="cc-field">Machine<div class="cc-picks">${MACHINES.filter(m => m.on).map((m, i) => `<button class="cc-btn${i === 0 ? " pri" : ""}">${m.id}</button>`).join("")}</div></div>
     <label class="cc-field">Folder<input class="mono" value="~/src/sample-project"></label>
     <label class="cc-field">Command<input class="mono" placeholder="Empty opens a shell"></label>
     <label class="cc-field">Name<input placeholder="shell"></label>` + btn("Open", "pri big", go("ccTerminal")));
  const TERM = {
    shell: `<span class="g">user@build-box</span>:<span class="b">~/src/sample-project</span> <span class="m">(main)</span>
$ cargo test -p sample-core
<span class="g">   Compiling</span> sample-core v0.4.2
<span class="g">    Finished</span> \`test\` profile in 6.21s
<span class="g">     Running</span> unittests src/lib.rs

running 48 tests
test keys::parses_chord ... <span class="g">ok</span>
test keys::rejects_empty ... <span class="g">ok</span>
test store::round_trip ... <span class="g">ok</span>
test cache::evicts_oldest ... <span class="g">ok</span>
<span class="k">...</span>
test result: <span class="g">ok</span>. 48 passed; 0 failed; 0 ignored

<span class="g">user@build-box</span>:<span class="b">~/src/sample-project</span> <span class="m">(main)</span>
$ git status --short
<span class="r"> M</span> src/editor/keys.ts
<span class="g">?? </span>src/editor/shortcuts.ts
$ <span class="cur"> </span>`,
    htop: `  <span class="c">1</span>  [<span class="g">||||||||</span><span class="r">||</span>            38.2%]
  <span class="c">2</span>  [<span class="g">||||</span>                  14.0%]
  <span class="c">Mem</span>[<span class="g">|||||||</span><span class="b">||</span><span class="y">||</span>      41.2G/124G]
  <span class="c">Swp</span>[                       0K/8.0G]
  <span class="c">Tasks:</span> 214, 1203 thr; <span class="g">3 running</span>
  <span class="c">Load average:</span> 2.41 1.98 1.77

<span class="inv">  PID USER  CPU% MEM%   TIME+  Command        </span>
 4121 user  92.0  3.1 12:04.11 cargo test
 3980 user  41.3  1.2  8:51.02 rust-analyzer
 2210 user  12.7  0.8  2:10.40 node dev.js
 1877 user   3.2  0.4 31:22.90 beep-node
  912 root   0.7  0.1  4:01.13 systemd-journal
 1201 user   0.3  0.2  0:44.02 zsh

<span class="inv">F1</span>Help <span class="inv">F2</span>Setup <span class="inv">F3</span>Search <span class="inv">F9</span>Kill <span class="inv">F10</span>Quit`,
  };
  // Sticky modifiers: a tap holds one for the next key, as on the desktop keyboard.
  const MODS = ["Ctrl", "Alt", "Shift"];
  views.ccTerminal = () => {
    const view = C.termMode === "view", ended = C.termMode === "ended";
    return screen("", top(C.term === "htop" ? "htop" : "shell", `<button class="cc-ib" ${toast("Detached; it keeps running")} aria-label="More">${ic("dots", 20)}</button>`, "ccTerminals", `${C.term === "htop" ? "gpu-box · ~" : "build-box · ~/src/sample-project"}`) +
      `<div class="cc-termbar"><span class="cc-dot${ended ? " off" : ""}"></span><span>${ended ? "Ended · exit 0" : view ? "View only" : "Attached · 54×31"}</span><span class="sp"></span>${ended ? btn("Close") : btn(view ? "Take control" : "View only", "", act("seg", "termMode:" + (view ? "control" : "view")))}${ended ? "" : btn("Close…", "danger")}</div>` +
      `<div class="cc-term">${TERM[C.term]}</div>` +
      (view || ended ? "" : `<div class="cc-keys">${["Esc", "Tab", "Ctrl", "Alt", "Shift", "←", "↑", "↓", "→", "|", "/", "~", "-"].map(k => `<button ${MODS.includes(k) ? act("mod", k) + ` aria-pressed="${C.mods.includes(k)}"` : ""}>${k}</button>`).join("")}<button aria-label="Hide keyboard">${ic("kbd", 16)}</button></div><div class="cc-kbd">Keyboard</div>`));
  };
  VAR("ccTerminal", [["Shell", () => { C.term = "shell"; C.termMode = "control"; }, () => C.term === "shell" && C.termMode === "control"], ["htop", () => { C.term = "htop"; C.termMode = "control"; }, () => C.term === "htop" && C.termMode === "control"], ["View only", () => C.termMode = "view", () => C.termMode === "view"], ["Ended", () => C.termMode = "ended", () => C.termMode === "ended"]]);

  // ---- Machines ----
  const card = m => panel(`<button class="cc-mc" style="width:100%" ${act("machine", m.id)}><div class="l1"><span class="cc-dot${m.on ? "" : " off"}"></span><b>${m.id}</b>${m.node !== "0.5.15" && m.on ? tag("update", "wait") : ""}<span class="sp"></span><span class="mu" style="font-size:11.5px">${m.on ? "seen now" : m.now}</span></div>
    <div class="spec">${m.spec}</div>
    ${m.on ? `<div class="agents"><span>${ag("CC")}Claude Code<small>2.3 <span class="cc-dot" style="display:inline-block"></span></small></span><span>${ag("CX")}Codex<small>0.9 <span class="cc-dot${m.id === "laptop" ? " warn" : ""}" style="display:inline-block"></span></small></span></div>
    <div class="meters"><span>Disk · ${m.free} free<div class="cc-bar${m.low ? " warn" : ""}"><i style="width:${Math.round(m.disk[0] / m.disk[1] * 100)}%"></i></div></span><span>Memory · ${m.mem[0]}/${m.mem[1]} GB<div class="cc-bar"><i style="width:${Math.round(m.mem[0] / m.mem[1] * 100)}%"></i></div></span></div>
    <div class="tools">${(m.id === "gpu-box" ? ["Docker", "CUDA", "Blender"] : m.id === "laptop" ? ["Xcode", "Android", "Docker"] : ["Docker", "Android"]).map(t => tag(t)).join("")}</div>` : ""}</button>`);
  views.ccMachines = () => page("machines", top("Machines", `<button class="cc-ib" ${go("ccAddMachine")} aria-label="Add a machine">${ic("plus", 20)}</button>`),
    MACHINES.map(card).join("") + `<p class="cc-note" style="text-align:center">3 of 4 online</p>`);
  views.ccMachine = () => {
    const m = MACHINES.find(x => x.id === C.machine) || MACHINES[0];
    return page("machines", top(m.id, `<button class="cc-ib" ${toast("Log copied")} aria-label="More">${ic("dots", 20)}</button>`, "ccMachines", m.on ? "Online · seen now" : m.now),
      panel(kv([["beep-node", `${m.node}${m.node !== "0.5.15" ? " · 0.5.15 out" : ""}`], ["System", m.spec.split(" · ")[0]], ["Hardware", m.spec.split(" · ").slice(1).join(" · ")], ["Host name", m.id], ["Home", "/home/user"]])) +
      (m.node !== "0.5.15" ? btn("Update beep-node to 0.5.15", "pri big") : "") +
      sec("Signed in", "", panel(row(ag("CC"), "Claude Code 2.3", "Max plan · user@example.com", tag("Signed in", "ok")) + row(ag("CX"), "Codex 0.9", "Not signed in", btn("Sign in", "pri")) + row(ag("GM"), "Gemini 1.4", "API key", tag("Signed in", "ok")))) +
      sec("Tools", "", panel(row(kind("D"), "Docker 28.1", "", "") + row(kind("G"), "gh 2.70", "2.74 out", btn("Update")) + row(kind("N"), "Node 24.3", "", ""))) +
      sec("Projects in", "", panel(row("", "<span class='mono'>~/src, ~/work</span>", "Beep looks for checkouts here", btn("Edit")))) +
      sec("Traffic today", "", panel(kv([["Direct", "1.4 GB sent · 220 MB received"], ["Relay", "12 MB"]]))) +
      sec("Log", "", `<pre class="cc-code">12:40:11 agent session ended (codex)
12:41:02 sync sample-docs: 3 files
12:41:05 terminal attached: dev server</pre>`) + `<div class="cc-picks">${btn("Refresh")}${btn("Copy")}${btn("Share")}</div>` + btn("Remove machine…", "danger big"));
  };
  VAR("ccMachine", [["build-box", () => C.machine = "build-box", () => C.machine === "build-box"], ["gpu-box (update)", () => C.machine = "gpu-box", () => C.machine === "gpu-box"], ["win-box (offline)", () => C.machine = "win-box", () => C.machine === "win-box"]]);
  views.ccAddMachine = () => page("machines", top("Add a machine", "", "ccMachines"),
    seg("addKind", [["node", "Node"], ["container", "Container"], ["desktop", "Desktop"]]) +
    (C.addKind === "container" ? `<p class="cc-note">Removed after a day offline.</p>` : C.addKind === "desktop" ? `<p class="cc-note">Install Beep on the computer, then choose Join with a ticket.</p>` : "") +
    (C.addKind !== "desktop" ? sec("1. Install beep-node", "", `<div class="cc-seg" style="margin-bottom:6px"><button aria-pressed="true">Linux and macOS</button><button aria-pressed="false">Windows</button></div><pre class="cc-code">curl -fsSL https://example.com/install.sh | sh</pre>`) +
      `<label class="cc-field">Search for projects in<input class="mono" value="~/src"></label>` : "") +
    sec(`${C.addKind === "desktop" ? "1" : "2"}. Join the mesh`, "", `<pre class="cc-code">${C.addKind === "desktop" ? "beepjoin2…7Kq4" : "beep-node mesh join beepjoin2…7Kq4"}</pre>`, `<span class="mu" style="font-size:12px">4:52 left</span>`) +
    `<div class="cc-picks">${btn("Copy", "pri")}${btn("Share")}</div>` +
    panel(`<div class="cc-row">${kind("…")}<span class="t"><b>Waiting for the machine to join</b><small>Keep Beep open until it does</small></span></div>`));
  VAR("ccAddMachine", [["Node", () => C.addKind = "node", () => C.addKind === "node"], ["Container", () => C.addKind = "container", () => C.addKind === "container"], ["Desktop", () => C.addKind = "desktop", () => C.addKind === "desktop"]]);

  // ---- Settings ----
  views.ccSettings = () => page("home", top("Settings", "", "ccHome"),
    panel(row("", "Appearance", "", `<span>${C.skin === "system" ? "Match system" : C.skin}</span>`, "ccAppearance") + row("", "Notifications", "", "", "ccNotifySettings")) +
    panel(row("", "Machines", "", "<span>3 of 4 online</span>", "ccMachines") + row("", "Coding agents", "", "<span>6 harnesses</span>", "ccHarness") + row("", "Credentials", "", "<span>2 accounts</span>", "ccCredentials") + row("", "Instructions", "", "", "ccInstructions") + row("", "Sync coding agent skills", "", `<span class="cc-toggle" aria-pressed="true"></span>`)) +
    panel(row("", "Linked workspace", "This phone · 4 machines", "", "") + row("", "Report a problem", "", "", "")) + `<p class="cc-note" style="text-align:center">Beep 0.6.0</p>`);
  const SKINS = [["system", "Match system", ["#0f1011", "#5e6ad2", "#fafaf9"]], ["linear", "Linear", ["#151617", "#5e6ad2", "#4cb782"]], ["paper", "Light", ["#fafaf9", "#1f5fbf", "#2f8a4c"]], ["tokyo", "Tokyo Night", ["#1f2335", "#7aa2f7", "#9ece6a"]], ["nord", "Nord", ["#3b4252", "#88c0d0", "#a3be8c"]], ["geist", "Geist", ["#000", "#ededed", "#ff6166"]], ["solarized", "Solarized", ["#fdf6e3", "#268bd2", "#859900"]]];
  views.ccAppearance = () => page("home", top("Appearance", "", "ccSettings"),
    `<div class="cc-skins">${SKINS.map(([id, n, sw]) => `<button ${act("skin", id)} aria-pressed="${C.skin === id}"><span class="sw" style="background:${sw[0]}">${sw.slice(1).map(c => `<i style="background:${c}"></i>`).join("")}</span>${n}</button>`).join("")}</div>` +
    `<p class="cc-note">The desktop's skins. Match system uses Linear in dark mode and Light in light mode. The mock shows six of the seventeen.</p>`);
  views.ccNotifySettings = () => page("home", top("Notifications", "", "ccSettings"),
    panel(row("", "Agent asks", "Allow and Deny on the notification", `<span class="cc-toggle" aria-pressed="true"></span>`) + row("", "Agent finished", "", `<span class="cc-toggle" aria-pressed="true"></span>`) + row("", "CI failed on your PRs", "", `<span class="cc-toggle" aria-pressed="true"></span>`) + row("", "Sync conflicts", "", `<span class="cc-toggle" aria-pressed="true"></span>`)) +
    (ios ? `<p class="cc-note">Asks arrive live while Beep is open. When it is closed, iOS lets Beep check now and then.</p>`
      : panel(`<button class="cc-row" ${act("stay")}><span class="t"><b>Stay connected</b><small>Asks arrive while Beep is closed. Android shows a lasting notification.</small></span><span class="cc-toggle" aria-pressed="${C.stay}"></span></button>`)));
  views.ccHarness = () => page("home", top("Claude Code", "", "ccSettings"),
    sec("Machines", "", panel(MACHINES.filter(m => m.on).map(m => `<div class="cc-row"><span class="cc-dot"></span><span class="t"><b>${m.id}</b><small>2.3.1${m.id === "gpu-box" ? " · 2.3.4 out" : ""}</small></span><span class="r">${m.id === "gpu-box" ? btn("Update") : ""}<span class="cc-toggle" aria-pressed="true"></span></span></div>`).join(""))) +
    sec("On build-box", "", `<div style="display:grid;gap:10px"><label class="cc-field">Binary<input class="mono" value="~/.local/bin/claude"></label><label class="cc-field">Launch arguments<input class="mono" placeholder="--verbose"></label><label class="cc-field">Environment<textarea class="mono" style="min-height:60px">CLAUDE_CODE_MAX_OUTPUT_TOKENS=32000</textarea></label></div>`) +
    `<div class="cc-picks">${btn("Save", "pri")}${btn("Save on every machine")}</div>` +
    sec("Compaction", "", panel(row("", "Compact at", "Share of the context window", "<span>80%</span>") + row("", "Per model", "Opus 4.7: 85%", "", ""))));
  views.ccCredentials = () => page("home", top("Credentials", "", "ccSettings"),
    sec("GitHub", 2, panel(row(kind("G"), "example-user", "Default · machines get it as GH_TOKEN · ••••a91f", tag("Default", "ok")) + row(kind("G"), "example-work", "••••02c4", btn("Make default")))) +
    `<div class="cc-picks">${btn("Sign in with GitHub", "pri")}${btn("Paste a token")}</div>` +
    sec("Forgejo", 1, panel(row(kind("F"), "git.example.com", "••••77e0 · checked today", btn("Check")))) + btn("Add a server", "big"));
  views.ccInstructions = () => page("home", top("Instructions", btn("Save", "pri"), "ccSettings", "One text for every agent"),
    `<div class="cc-seg"><button aria-pressed="true">Editor</button><button aria-pressed="false">Import</button><button aria-pressed="false">Sync</button><button aria-pressed="false">History</button></div>` +
    `<textarea class="cc-input mono" style="min-height:300px;font-size:12px;line-height:1.5"># Agent notes

- Run the tests before you finish.
- Keep commits small.
- Never push to main.</textarea><p class="cc-note">Goes to AGENTS.md and CLAUDE.md on every machine.</p>`);

  // ---- Registration, events ----
  function VAR(id, list) { VARIANTS[id] = list.map(([n, f, on]) => [n, () => { f(); render(); }, on]); }
  catalog.unshift({ group: "Command center", items: [
    ["ccHome", "Home", "Overview, PRs and issues, Machines"], ["ccBell", "Notifications", ""], ["ccProjects", "Projects", ""], ["ccAddProject", "Add a project", ""], ["ccProject", "Project", ""], ["ccProjectMachines", "Project machines", ""],
    ["ccRun", "CI run", ""], ["ccPr", "Pull request", ""], ["ccIssue", "Issue", ""], ["ccBranches", "Branches", ""], ["ccSync", "File sync", ""],
    ["ccAgents", "Coding agents", ""], ["ccStart", "Start an agent", ""], ["ccSession", "Session", ""], ["ccChanges", "Diff review", ""], ["ccFile", "File", ""],
    ["ccTerminals", "Terminals", ""], ["ccNewTerminal", "New terminal", ""], ["ccTerminal", "Terminal", ""],
    ["ccMachines", "Machines", ""], ["ccMachine", "Machine", ""], ["ccAddMachine", "Add a machine", ""],
    ["ccSettings", "Settings", ""], ["ccAppearance", "Appearance", ""], ["ccNotifySettings", "Notification settings", ""], ["ccHarness", "Coding agent settings", ""], ["ccCredentials", "Credentials", ""], ["ccInstructions", "Instructions", ""],
  ] });
  document.addEventListener("click", e => {
    if (e.target.closest("[data-theme-choice]")) { setTimeout(render); return; }
    const root = e.target.closest(".cc");
    if (!root) return;
    const sheetBg = e.target.closest(".cc-sheet-bg"), inSheet = e.target.closest("[data-cc-stop]");
    const el = e.target.closest("[data-cc-go],[data-cc-act],[data-cc-toast]");
    if (sheetBg && !inSheet) { C.sheet = null; render(); e.stopImmediatePropagation(); return; }
    if (!el) return;
    e.preventDefault(); e.stopImmediatePropagation();
    if (el.dataset.ccGo) { C.sheet = null; window.go(el.dataset.ccGo); return; }
    if (el.dataset.ccToast) { window.toast(el.dataset.ccToast); return; }
    const a = el.dataset.ccAct, v = el.dataset.v || "";
    if (a === "seg") { const [k, x] = v.split(":"); C[k] = x; }
    if (a === "sheet") C.sheet = v;
    if (a === "close") C.sheet = null;
    if (a === "machine") { C.machine = v; C.sheet = null; window.go("ccMachine"); return; }
    if (a === "answer") { C.answered = v; C.sheet = null; if (S.screen === "ccSession") C.session = "working"; window.toast(v); }
    if (a === "skin") C.skin = v;
    if (a === "mod") C.mods = C.mods.includes(v) ? C.mods.filter(m => m !== v) : [...C.mods, v];
    if (a === "stay") C.stay = !C.stay;
    render();
  }, true);
  const hash = location.hash.slice(1);
  S.screen = views[hash] ? hash : "ccHome";
  render();
})();
