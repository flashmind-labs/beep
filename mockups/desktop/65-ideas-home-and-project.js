// ---- Ideas (2026-11-07): Home and the project page, two directions each ----
{
  const st = document.createElement("style");
  st.textContent = `
.mac .ix-note { margin: 0; padding: 8px 12px; border: 1px dashed var(--d-border); border-radius: 8px; font-size: 12px; color: var(--d-muted); line-height: 1.5 }
.mac .ix-note b { color: var(--d-fg); font-weight: 600 }
.mac .ix-kind { width: 22px; height: 22px; border-radius: 6px; display: grid; place-items: center; background: var(--d-elev); color: var(--d-muted); font-size: 10px; font-weight: 600 }
.mac .ix-kind.bad { color: var(--d-err) } .mac .ix-kind.wait { color: #f2c94c } .mac .ix-kind.ok { color: #4cb782 }
.mac .ix-tbl { display: grid; font-size: 12.5px }
.mac .ix-tbl > span { display: flex; align-items: center; gap: 8px; min-width: 0; padding: 7px 12px; border-top: 1px solid var(--d-border); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .ix-tbl > .th { border-top: 0; font-size: 12px; font-weight: 600; color: var(--d-muted); padding-block: 6px }
.mac .ix-tbl .mono { font-family: var(--mono); font-size: 11.5px; color: var(--d-muted) }
.mac .ix-tbl .mu { color: var(--d-muted) }
.mac .ix-tbl .end { justify-content: flex-end }
.mac .ix-tbl .cc-ag { width: 18px; height: 18px; border-radius: 5px; font-size: 8px; flex: none }
.mac .ix-cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(340px, 100%), 1fr)); gap: 14px; align-items: start }
.mac .ix-card { border: 1px solid var(--d-border); border-radius: 8px; background: var(--d-surface); overflow: hidden }
.mac .ix-card > .hd { display: flex; align-items: center; gap: 8px; padding: 9px 12px; border-bottom: 1px solid var(--d-border); font-size: 13px }
.mac .ix-card > .hd b { font-weight: 600 } .mac .ix-card > .hd .mono { font-family: var(--mono); font-size: 11.5px; color: var(--d-muted) }
.mac .ix-card > .hd .sp { flex: 1 }
.mac .ix-card .cc-row { border-top: 0 } .mac .ix-card .cc-row + .cc-row { border-top: 1px solid var(--d-border) }
.mac .ix-quiet { font-size: 12.5px; color: var(--d-muted); padding: 2px 2px } .mac .ix-quiet button { all: unset; cursor: pointer; color: var(--d-fg) } .mac .ix-quiet button:hover { text-decoration: underline }
.mac .ix-pj { display: grid; grid-template-rows: auto auto minmax(0, 1fr) auto; height: 100%; min-height: 0 }
.mac .ix-pj .cc-mstrip { margin: 10px 16px 0; flex: none }
.mac .ix-pj .cc-mstrip > .m .bad { color: var(--d-err) }
.mac .ix-pj .pj-split { margin-top: 10px; border-top: 1px solid var(--d-border) }
.mac .ix-det { display: grid; grid-template-rows: minmax(0, 1fr); min-height: 0; height: 100% }
.mac .ix-det .sc { overflow: auto; padding: 14px 16px; display: grid; gap: 14px; align-content: start; scrollbar-width: thin }
.mac .ix-flat .pj-li { grid-template-columns: minmax(0, 1fr) auto; padding: 7px 14px }
.mac .ix-flat .pj-li .c { font-size: 11.5px; color: var(--d-muted) } .mac .ix-flat .pj-li .c.bad { color: var(--d-err) } .mac .ix-flat .pj-li .c.wait { color: #f2c94c }
.mac .ix-flat hr { border: 0; border-top: 1px solid var(--d-border); margin: 6px 14px }
.mac .ix-feed { display: grid; gap: 4px }
.mac .ix-day { font-size: 12px; font-weight: 600; color: var(--d-dim); padding: 8px 2px 4px }
.mac .ix-ev { display: grid; grid-template-columns: 44px 22px minmax(0, 1fr) auto; gap: 10px; align-items: center; padding: 7px 12px; border: 1px solid var(--d-border); border-radius: 8px; background: var(--d-surface); font-size: 13px }
.mac .ix-ev .tm { font-size: 11.5px; color: var(--d-muted); font-variant-numeric: tabular-nums }
.mac .ix-ev .t { min-width: 0 } .mac .ix-ev .t b { font-weight: 500; display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .ix-ev .t small { display: block; color: var(--d-muted); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .ix-ev .r { display: flex; gap: 6px; align-items: center }
.mac .ix-two { display: grid; grid-template-columns: minmax(0, 1fr) 260px; gap: 14px; align-items: start }
.mac .ix-kv { display: grid; grid-template-columns: 90px minmax(0, 1fr); font-size: 12.5px }
.mac .ix-kv > span { padding: 7px 12px; border-top: 1px solid var(--d-border); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .ix-kv > span:nth-child(-n+2) { border-top: 0 } .mac .ix-kv > span:nth-child(odd) { color: var(--d-muted) }
.mac .ix-ptabs { position: static; margin: -14px -16px 0; padding: 0 12px; overflow: visible }
.mac .ix-diff { font-family: var(--mono); font-size: 11px } .mac .ix-diff .a { color: #4cb782 } .mac .ix-diff .d { color: var(--d-err) }
`;
  document.head.append(st);

  const CHEV = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';
  const skinned = html => html.replace('class="cc-win"', `class="cc-win${S.d.skin === "paper" ? " sk-paper" : S.d.skin === "flashmind" ? " sk-flashmind" : ""}"`);
  const ag = a => `<span class="cc-ag ${a}">${a === "codex" ? "CX" : "CC"}</span>`;
  const tg = (cls, t) => `<span class="cc-tag${cls ? " " + cls : ""}">${t}</span>`;
  const btn = (l, cls = "") => `<button class="dbtn small${cls ? " " + cls : ""}" data-toast="${l}">${l}</button>`;
  const kind = (cls, t) => `<span class="ix-kind${cls ? " " + cls : ""}">${t}</span>`;
  const ln = (lead, t, sub, right) => `<div class="cc-row cc-line${lead ? "" : " bare"}">${lead}<button class="t" title="${t}"><b>${t}</b><span>${sub}</span></button><span class="r">${right}</span></div>`;
  const sec = (title, n, body, right = "") => `<div><div class="cc-sec"><h4>${title}${n !== "" ? `<span class="n">${n}</span>` : ""}</h4>${right}</div>${body}</div>`;
  const panel = rows => `<div class="cc-panel">${rows}</div>`;
  const note = html => `<p class="ix-note">${html}</p>`;
  const IC = {
    ask: "?", sync: "⇄", ci: "✕", pr: "PR", done: "✓",
  };
  const strip = cells => `<div class="cc-mstrip">${cells.map(([dot, b, t, cls]) => `<button class="m" data-toast="${b}">${dot ? `<span class="ddot ${dot}"></span>` : ""}<b>${b}</b><span${cls ? ` class="${cls}"` : ""}>${t}</span></button>`).join("")}</div>`;
  const MACHINES = [["busy", "build-box", "2 agents · 348 GB free"], ["", "studio", "idle · 80 GB free"], ["busy", "laptop", "1 agent · 120 GB free"], ["off", "phone", "offline"]];
  const comp = (ph, extra = "") => `<div class="cc-comp hc"><span class="in"><span class="caret"></span><span class="ph">${ph}</span></span>${extra}<button class="dbtn pri" data-toast="Sent">Send<kbd>${DI.ret}</kbd></button></div>`;
  const tabs = (list, on) => `<div class="cc-tabs" role="tablist">${list.map(([k, l, n]) => `<button role="tab" aria-selected="${k === on}">${l}${n ? `<span class="n">${n}</span>` : ""}</button>`).join("")}</div>`;

  // ---------- Home ----------

  // One "Needs you" for everything waiting on the user: asks, conflicts, red CI on their PRs,
  // review requests and finished agents. One button per row.
  const NEEDS = [
    [kind("wait", IC.ask), "May it run xcodebuild on laptop?", "Claude Code · you/site · Swipeable approval strip", btn("Answer", "pri")],
    [kind("bad", IC.pr), "#41 Cut the cache warm-up in half", "acme/api · checks failing", btn("Open")],
    [kind("", IC.pr), "#12 Move the blog to the new theme", "you/site · your review", btn("Review")],
    [kind("wait", IC.sync), "notes (conflict 1a2b3c4d).txt", "acme/widget · Another copy of notes.txt · on build-box, studio", btn("Resolve")],
  ];
  const needs = () => sec("Needs you", NEEDS.length, panel(NEEDS.map(([k, t, s, b]) => ln(k, t, s, b)).join("")));

  const RUN = [["claude", "Swipeable approval strip on iOS", "you/site", "laptop", "fix-approval", tg("wait", "needs you")],
    ["codex", "Fix the device build script", "acme/api", "build-box", "main", "4 min"],
    ["claude", "Retry uploads when the token expires", "acme/widget", "build-box", "retry-upload", "31 min"]];
  const runRows = () => sec("Running", RUN.length, panel(RUN.map(([a, t, p, m, , w]) => ln(ag(a), t, `${p} · ${m}`, w)).join("")));

  const coming = () => sec("Coming up", CC_COMING.length, panel(CC_COMING.map(c => ln(`<span class="cc-when"><b>${c[0]}</b>${c[1]}</span>`, c[2], c[3], "")).join("")));

  const PROJ = [
    { name: "acme/api", branch: "main", ci: tg("bad", "CI failing"), on: "build-box, studio", rows: [ln(ag("codex"), "Fix the device build script", "build-box · 4 min", ""), ln(kind("bad", IC.ci), "#41 Cut the cache warm-up in half", "checks failing", btn("Fix with an agent")), ln(kind("ok", IC.done), "Clippy fixes in the sync crate", "finished · +84 −31", btn("Review", "pri"))] },
    { name: "you/site", branch: "fix-approval", ci: tg("run", "CI passing"), on: "laptop", rows: [ln(ag("claude"), "Swipeable approval strip on iOS", "laptop · asks to run xcodebuild", btn("Answer", "pri")), ln(kind("", IC.pr), "#12 Move the blog to the new theme", "wants your review", btn("Review"))] },
    { name: "acme/widget", branch: "main", ci: tg("", "no CI"), on: "build-box, studio, laptop", rows: [ln(ag("claude"), "Retry uploads when the token expires", "build-box · 31 min", ""), ln(kind("wait", IC.sync), "notes.txt has two copies", "build-box and studio differ", btn("Resolve"))] },
  ];
  const byProject = () => sec("Active projects", PROJ.length, `<div class="ix-cards">${PROJ.map((p, i) => `<div class="ix-card"><div class="hd"><span class="cc-pdot" style="background:${["#f2994a", "#8a9cf0", "#4cb782"][i]}"></span><b>${p.name}</b><span class="mono">${p.branch}</span><span class="sp"></span>${p.ci}</div>${p.rows.join("")}</div>`).join("")}</div>`)
    + `<div class="ix-quiet">Quiet: <button>acme/web</button> 20 min · <button>acme/infra</button> 3 h · <button>oss/parser</button> 2 w · <button>you/dotfiles</button> 4 d</div>`;

  const homeTabs = tabs([["overview", "Overview"], ["git", "PRs and issues", 7], ["machines", "Machines"]], "overview");
  D.dIdeaHome = () => {
    const v = S.d.ixh || "attention";
    const body = v === "attention"
      ? note("<b>Attention first (built, #466).</b> One <b>Needs you</b> list holds agents' questions, chat approvals, PRs waiting on you and sync conflicts, each with one button. Running lists every session. Projects, Terminals, PR and Issue boxes leave Overview: the sidebar, the PRs and issues tab and the Machines tab have them. Updates show only with a server.")
        + needs() + runRows() + coming()
      : note("<b>By project.</b> Each project with something going on gets a card: its agents, its PRs and CI, its conflicts. Quiet projects fold into one line. Needs you is the same list, so the cards repeat it in context.")
        + byProject() + coming();
    const main = `<div class="cc-top"><h2>Home</h2><small>Saturday, 7 November</small><span class="tools"></span></div>
      <div class="cc-view">${strip(MACHINES)}${homeTabs}${body}</div>${comp("Ask Beep, or start an agent on a machine", `<button class="dbtn" data-toast="Start a coding agent">Agent</button>`)}`;
    return skinned(ccWin(ccSide("home"), main));
  };
  VARIANTS.dIdeaHome = [["A: Attention first", "attention"], ["B: By project", "project"]].map(([l, k]) => [l, () => { S.d.ixh = k; render(); }, () => (S.d.ixh || "attention") === k]);

  // ---------- Project page ----------

  const pjStrip = () => strip([["", "main", "branch"], ["", "Actions", "1 running · 1 failed", "bad"], ["", "2 agents", "running"], ["", "Machines", "3/4 · 1 terminal"], ["", "2 PRs", "1 needs a reply"], ["", "2 issues", "open"]]);
  // A flat list: what needs you, then each part with a count. No summary rows, no Agents next to Running.
  const NAV = [["Overview", "", ""], ["Needs you", "1", "wait"], ["Running", "2", ""], null, ["Actions", "1 failed", "bad"], ["Machines", "3/4", ""], ["File sync", "in sync", ""], ["Issues", "2 open", ""], null, ["Tasks", "3", ""], ["Sessions", "43 past", ""], ["Git", "13 commits", ""]];
  const projectNav = title => {
    const view = { Overview: "strip", Sessions: "sessions", Git: "branches" }[title];
    return view ? `data-dact="dset:ixp:${view}" data-dgo="dIdeaProject"` : "";
  };
  const nav = (on = "Overview") => `<div class="pj-list ix-flat">${NAV.map(n => !n ? "<hr>" : `<button class="pj-li" ${projectNav(n[0])}${n[0] === on ? ' aria-current="true"' : ""}><span>${n[0]}</span><span class="c${n[2] ? " " + n[2] : ""}">${n[1]}</span></button>`).join("")}</div>`;
  const pjComp = () => `<form class="cc-pjc" onsubmit="return false"><div class="box"><textarea rows="1" placeholder="Describe a task for a coding agent in acme/widget"></textarea><div class="row"><button class="pill" type="button">build-box</button><button class="pill" type="button">Current checkout <small>as it is</small></button><span class="sp"></span><button class="pill" type="button">${ag("claude")}Claude Code <small>Default · Medium</small></button><button class="dbtn pri" data-toast="Started">Start<kbd>${DI.ret}</kbd></button></div></div></form>`;

  const MACH = [["busy", "build-box", "/srv/project/widget", "main", tg("run", "in sync"), "↓1 ↑2"], ["", "studio", "/srv/widget", "main", tg("run", "Syncthing"), ""],
    ["bad", "mini-box", "/srv/project/widget", "main", tg("wait", "Local edits block Git sync"), ""], ["bad", "dev-vm", "/path/to/widget", "main", tg("bad", "Git sync needs review"), "↓2 ↑1"]];
  const machTable = () => sec("Machines", "3 of 4 in sync", panel(`<div class="ix-tbl" style="grid-template-columns:minmax(0,.8fr) minmax(0,1.3fr) minmax(0,.8fr) auto auto auto">`
    + ["Machine", "Folder", "Branch", "Sync", "", ""].map(t => `<span class="th">${t}</span>`).join("")
    + MACH.map(([d, m, f, b, s, x]) => `<span><span class="ddot ${d}"></span>${m}</span><span class="mono">${f}</span><span class="mono">${b}</span><span>${s}</span><span class="mu">${x}</span><span class="end">${d === "bad" ? `<button class="dbtn small" data-toast="Agent draft: inspect both Git histories and read recent transcripts on both machines before merging. You choose when to start.">Fix with an agent</button>` : `<button class="dbtn quiet small" aria-label="More" data-toast="Start an agent here, Open in Finder, Copy path, Remove">⋯</button>`}</span>`).join("")
    + `</div>`), `<button class="dbtn quiet small" data-toast="Add to laptop">Add to laptop</button>`);

  const overviewA = () => note("<b>Status strip (built, #466).</b> The strip says branch, Actions, agents, machines, PRs and issues; each opens its row of the list. The list loses its Project and Work headings, and Agents becomes Sessions (past sessions and custom agents), since running agents have their own rows. Actions leaves the overview: the strip and the list have it. Under Machines, Start an agent here, Open in Finder and Copy path go in one ⋯ menu; a fix keeps its button.")
    + sec("Needs you", 1, panel(ln(kind("wait", "?"), "The project agent waits on you", "Plan for #92 Auto-sync the SDK docs: 3 steps on build-box", btn("Open plan", "pri"))))
    + sec("Running", 2, panel(ln(ag("claude"), "Swipeable approval strip", "build-box · fix-approval", "now") + ln(ag("codex"), "Fix the device build script", "studio · main", "3 min")))
    + `<div class="cc-cols">${sec("Pull requests", 2, panel(ln(kind("bad", "PR"), "#41 Cut the cache warm-up in half", "sam · 5 d · checks failing", tg("wait", "needs reply")) + ln(kind("ok", "PR"), "#38 Bump the sync crate", "you · 9 d · checks passing", tg("run", "ready"))))}${sec("Issues", 2, panel(ln(kind("", "#"), "#92 Auto-sync the SDK docs", "contributor-a · 32 d · docs, ci", "") + ln(kind("", "#"), "#53 Publish the CLI like the SDK", "contributor-b · 173 d · release", "")))}</div>`
    + machTable();

  const EVENTS = [["Today", [["11:42", ag("claude"), "Swipeable approval strip started", "build-box · fix-approval", ""],
    ["11:20", kind("bad", "✕"), "build.yml #351 failed on #41", "test cache::warm_up timed out", btn("Fix with an agent")],
    ["10:58", kind("ok", "✓"), "Retry uploads when the token expires finished", `build-box · <span class="ix-diff"><span class="a">+120</span> <span class="d">−14</span></span> in 4 files`, btn("Review", "pri")],
    ["10:15", kind("", "PR"), "sam replied on #41", "“The warm-up still races on cold start”", btn("Reply")],
    ["09:30", kind("wait", "⇄"), "dev-vm can't fast-forward main", "Local commit on dev-vm not on origin", btn("Fix with an agent")]]],
  ["Yesterday", [["18:04", kind("ok", "✓"), "build.yml #347 passed on #38", "Bump the sync crate", ""], ["16:40", ag("codex"), "Bump the toolchain ended", "studio · merged as #39", ""], ["14:12", kind("", "#"), "contributor-a commented on #92", "Asked for the SDK docs to sync on release", ""]]]];
  const overviewB = () => note("<b>Activity feed.</b> The overview answers “what happened since I looked”: sessions, CI runs, PR and issue replies and sync problems in time order, each with its one action. A narrow column on the right keeps the facts. The list and composer stay as in A.")
    + `<div class="ix-two"><div class="ix-feed">${sec("Needs you", 1, panel(ln(kind("wait", "?"), "The project agent waits on you", "Plan for #92: 3 steps on build-box", btn("Open plan", "pri"))))}${EVENTS.map(([d, evs]) => `<div class="ix-day">${d}</div>${evs.map(([tm, k, t, s, r]) => `<div class="ix-ev"><span class="tm">${tm}</span>${k}<span class="t"><b>${t}</b><small>${s}</small></span><span class="r">${r}</span></div>`).join("")}`).join("")}</div>`
    + `<div>${sec("At a glance", "", panel(`<div class="ix-kv"><span>Branch</span><span class="mono">main</span><span>CI</span><span style="color:var(--d-err)">failing · #351</span><span>Agents</span><span>2 running</span><span>Machines</span><span>3 of 4 in sync</span><span>PRs</span><span>2 open · 1 needs reply</span><span>Issues</span><span>2 open</span><span>Hosts</span><span>GitHub, Forgejo</span></div>`))}</div></div>`;

  // Sessions: past sessions and the project's custom agents in two tabs, so the
  // sessions list gets the whole height instead of five rows.
  const PAST = [["codex", "Session close command termination", "devbox", "13 min"], ["codex", "Windows build availability", "laptop", "14 min"], ["codex", "Brainstorm mock UI intro", "devbox", "16 min"],
    ["claude", "Fix Cmd+[ session navigation", "devbox", "23 min"], ["codex", "Add two-finger image panning", "devbox", "25 min"], ["claude", "Retry uploads when the token expires", "build-box", "2 h"],
    ["codex", "Bump the toolchain", "studio", "1 d"], ["claude", "Clippy fixes in the sync crate", "build-box", "1 d"], ["codex", "Pin the SDK to 4.2", "build-box", "2 d"], ["claude", "Draft the release notes", "laptop", "3 d"]];
  const sessionsTabs = () => {
    const tab = S.d.ixst || "sessions";
    const bar = `<div class="cc-tabs ix-ptabs" role="tablist">${[["sessions", "Sessions", 43], ["agents", "Agents", 0]].map(([k, l, n]) => `<button role="tab" aria-selected="${k === tab}" data-dact="dset:ixst:${k}" data-dgo="dIdeaProject">${l}${n ? `<span class="n">${n}</span>` : ""}</button>`).join("")}</div>`;
    const body = tab === "sessions"
      ? panel(PAST.map(([a, t, m, w]) => ln(ag(a), t, `ended · ${m} · widget`, w)).join(""))
      : `<div class="cc-panel cc-empty"><span>No custom agents in .claude/agents, .opencode/agent, .codex/agents, .pi/agents or .agents yet.</span></div>`;
    const right = tab === "agents" ? `<div class="cc-sec"><h4></h4><button class="dbtn quiet small" data-toast="Reloaded">Reload</button></div>` : "";
    return bar + right + body;
  };

  D.dIdeaProject = () => {
    const v = S.d.ixp || "strip";
    if (v === "sessions") {
      const main = `<div class="ix-pj"><div class="cc-top"><h2>acme/widget</h2><small>github.com · imported 3 d</small><span class="tools"><button class="dbtn small">Open ${CHEV}</button><button class="dbtn quiet small" aria-label="Project menu">⋯</button></span></div>${pjStrip()}
        <div class="pj-split">${nav("Sessions")}<div class="ix-det"><div class="sc">${sessionsTabs()}</div></div></div>${pjComp()}</div>`;
      return skinned(ccWin(ccSidePj("acme/widget"), main));
    }
    const selected = v === "branches" || v === "review" ? "Git" : "Overview";
    const content = v === "branches" ? window.gitHistoryDetail?.() || ""
      : v === "review" ? `<button class="dbtn quiet small pj-back" data-dact="dset:ixp:branches" data-dgo="dIdeaProject">All branches</button>${window.__rvPane()}`
      : v === "strip" ? overviewA() : overviewB();
    const tools = `<span class="tools"><button class="dbtn small" data-toast="GitHub, Forgejo, Open in Zed">Open ${CHEV}</button><button class="dbtn quiet small" aria-label="Project menu" data-toast="Pin, New window, Remove">⋯</button></span>`;
    const main = `<div class="ix-pj"><div class="cc-top"><h2>acme/widget</h2><small>github.com · imported 3 d</small>${tools}</div>${pjStrip()}
      <div class="pj-split${v === "branches" ? " gh-layout" : ""}">${v === "branches" ? window.gitHistoryNavigation?.() || "" : nav(selected)}<div class="ix-det${v === "branches" ? " gh-inspector" : ""}"><div class="sc">${content}</div></div></div>${pjComp()}</div>`;
    return skinned(ccWin(ccSidePj("acme/widget"), main));
  };
  VARIANTS.dIdeaProject = [["A: Status strip, centered composer", "strip"], ["A: Sessions in tabs", "sessions"], ["Git history", "branches"], ["Review", "review"], ["B: Activity feed", "feed"]].map(([l, k]) => [l, () => { S.d.ixp = k; render(); }, () => (S.d.ixp || "strip") === k]);

  DESK_SCREENS.unshift({ group: "Ideas (7 Nov)", items: [["dIdeaHome", "Home ideas", "Home: attention first or by project"], ["dIdeaProject", "Project page ideas", "Project: status strip or activity feed"]] });
  const want = location.hash.slice(1);
  if (want === "dIdeaHome" || want === "dIdeaProject") S.screen = want;
  render();
}
