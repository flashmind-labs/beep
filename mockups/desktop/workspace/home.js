// ---- Desktop: skins, Home in tabs, the project in a split (2026-11-03) ----
const WORKSPACE_MOCK = (() => {
  document.head.insertAdjacentHTML("beforeend", '<link rel="stylesheet" href="workspace/base.css">');

  const skinCls = () => S.d.skin === "paper" ? " sk-paper" : S.d.skin === "flashmind" ? " sk-flashmind" : "";
  const skinned = html => html.replace('class="cc-win"', `class="cc-win${skinCls()}"`);
  const btn = (l, pri) => `<button class="dbtn${pri ? " pri" : ""}" style="padding:3px 10px;font-size:12px;min-height:24px" data-toast="${l}">${l}</button>`;
  const hc = ag => `<span class="cc-ag ${ag}" style="width:20px;height:20px;border-radius:5px;font-size:8.5px">${ag === "codex" ? "CX" : "CC"}</span>`;
  const subscribed = () => S.d.homeAccount === "subscribed";
  // On opening or waking, Home draws what the app kept; build-box and studio
  // have not answered yet. They read offline once 20 seconds pass without an answer.
  const linking = n => S.d.cc === "cached" && (n === "build-box" || n === "studio");
  const MACH = [["build-box", "busy", "2 agents · 348 GB free", "Ubuntu 26.04 · 76 / 125 GB memory"], ["studio", "", "idle · 80 GB free", "macOS 26.3 · 24 / 36 GB memory"], ["laptop", "busy", "1 agent · 120 GB free", "macOS 27.0 · 18 / 24 GB memory"], ["phone", "off", "offline", "Android · seen 12 min ago"]];
  const machStrip = () => {
    const offline = S.d.cc === "offline";
    const tm = typeof tmRow === "function" ? tmRow() : null;
    // Connecting: drawn from what the app kept, the machine not reached yet.
    const cells = MACH.filter(([n]) => subscribed() || n !== "phone").map(([n, s, t]) => offline && n === "studio" ? [n, "off", "offline"] : linking(n) ? [n, "dial", "connecting"] : [n, s, t]).map(([n, s, t]) =>
      `<button class="m${s === "off" || s === "dial" ? " off" : ""}" data-dact="dset:htab:machines" data-dgo="dHome"><span class="ddot${s ? " " + s : ""}"${s === "dial" ? ' title="Connecting"' : ""}></span><b>${n}</b><span>${t}</span></button>`).join("");
    const mac = tm ? `<div class="m off" title="This Mac: ${tm.title}"><span class="ddot off"></span><b>This Mac</b><span${tm.bad ? ' class="bad"' : ""}>${tm.state}</span>${S.d.tm === "off" ? `<button class="dbtn quiet small" data-dact="dset:tm:connecting" data-dgo="dHome">Add</button>` : ""}</div>` : "";
    return `<div class="cc-mstrip">${cells}${mac}<button class="dbtn quiet small" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button></div>`;
  };
  const sec = (title, count, body, right = "") => `<div class="nb-sec"><h4>${title}${count !== "" ? `<span class="c">${count}</span>` : ""}${right ? `<span class="c" style="margin-left:auto">${right}</span>` : ""}</h4><div class="nb-card">${body}</div></div>`;
  const row = (lead, t, sub, w) => `<div class="nb-row"><span>${lead}</span><span class="t">${t}<small>${sub}</small></span><span class="w">${w}</span></div>`;
  const empty = t => `<div class="nb-row" style="grid-template-columns:minmax(0,1fr);color:var(--d-dim)">${t}</div>`;

  // Home follows center/home.ts: machine strip, three tabs, then full-width lists.
  const PRS = [["acme/api", 41, "Cut the cache warm-up in half", "bad", "checks failing", "2 h", ["perf"]], ["you/site", 12, "Move the blog to the new theme", "warn", "your review", "5 h", ["ui"]],
    ["acme/api", 38, "Bump the sync crate", "ok", "approved", "1 d", ["deps"]], ["acme/web", 77, "Fix the login redirect", "", "draft", "3 d", ["bug"]]];
  const ISSUES = [["you/site", 5, "Dark mode flashes on load", "", "assigned", "1 d", ["bug", "ui"]], ["acme/web", 103, "Upload stalls past 2 GB", "", "", "4 d", ["bug"]], ["acme/api", 92, "Auto-sync the SDK docs", "", "", "32 d", ["docs", "ci"]]];
  const TERMS = [["cargo watch", "build-box", "acme/api · ~/code/api", "3 h", ""], ["tail -f server.log", "build-box", "~/logs", "2 d", ""], ["npm run dev", "studio", "you/site · ~/code/site", "exited 0", " off"]];
  const PROJECTS = [["acme", [["api", "build-box, studio · 2 agents", "1 min"], ["web", "studio", "20 min"], ["infra", "build-box", "3 h"]]],
    ["you", [["site", "laptop · 1 agent", "8 min"], ["dotfiles", "laptop, studio", "4 d"]]],
    ["oss", [["parser", "build-box", "2 w"], ["fmt-lint", "studio", "1 mo"]]]];
  // The Machines tab: name, state, OS, agents, free disk, memory in use, what it can run, now.
  const MT = [["build-box", "busy", "Ubuntu 26.04", "2", "348 GB free", "76 / 125 GB", "GPU · Docker", "working"], ["studio", "", "macOS 26.3", "–", "80 GB free", "24 / 36 GB", "Xcode · iOS simulator", "idle"],
    ["laptop", "busy", "macOS 27.0", "1", "120 GB free", "18 / 24 GB", "Xcode", "working"], ["phone", "", "Phone", "–", "–", "–", "Controller", "connected"]];
  const PCOL = ["#f2994a", "#8a9cf0", "#4cb782", "#f2c94c", "#b18ce0"];
  const HOME_TABS = [["overview", "Overview", ""], ["git", "PRs and issues", PRS.length + ISSUES.length], ["machines", "Machines", ""]];
  const IC_PR = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="5" r="2.2"/><circle cx="6" cy="19" r="2.2"/><circle cx="18" cy="19" r="2.2"/><path d="M6 7.2v9.6M18 16.8V10a3 3 0 00-3-3h-4M13 4.5L10.5 7 13 9.5"/></svg>';
  const IC_SYNC = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 11a8 8 0 00-14.6-4.5M4 4v3.5h3.5M4 13a8 8 0 0014.6 4.5M20 20v-3.5h-3.5"/></svg>';
  // acme/widget's files with two copies: [path, what the other copy is and where].
  const CONFLICTS = [["notes (conflict 1a2b3c4d).txt", "Another copy of notes.txt · on build-box, studio"], [".git/refs/beep-conflicts/heads/main-5e6f7a8b", "Another commit of main · on build-box"], ["docs/plan (conflict 9fbd5479).md", "Another copy of docs/plan.md · on build-box, studio"]];
  const IC_ISSUE = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="1.6" fill="currentColor"/></svg>';
  const num = n => `<span class="mono" style="color:var(--d-muted);font-size:11.5px">#${n}</span>`;
  const tag = (cls, t) => `<span class="cc-tag${cls ? " " + cls : ""}">${t}</span>`;
  const pdot = i => `<span class="cc-pdot" style="background:${PCOL[i % PCOL.length]}"></span>`;
  // section(): the title and count, a button on the right, then the body.
  // Shared by Home's lists and a project's: when it opened, newest or oldest, Select on issues, and Start N agents.
  window.__listCtl = (key, issues) => {
    const w = S.d[key + "w"] || "any", ord = S.d[key + "o"] || "newest", on = S.d[key + "s"];
    const set = (k, v) => `data-dact="dset:${key}${k}:${v}" data-dgo="${S.screen}"`;
    const seg = [["any", "Any time"], ["today", "Today"], ["yesterday", "Yesterday"], ["week", "This week"]].map(([v, l]) => `<button class="dbtn quiet small${w === v ? " on" : ""}" aria-pressed="${w === v}" ${set("w", v)}>${l}</button>`).join("");
    return `<div class="pj-ctl"><span class="seg">${seg}</span><span class="sp"></span><button class="dbtn quiet small" ${set("o", ord === "newest" ? "oldest" : "newest")}>${ord === "newest" ? "Newest first" : "Oldest first"}</button>${issues ? `<button class="dbtn quiet small${on ? " on" : ""}" aria-pressed="${!!on}" ${set("s", on ? "" : "on")}>Select</button>` : ""}</div>`;
  };
  // rows: [project, title, why it is left out or ""]; shown while Select is on.
  window.__listStart = (key, rows) => {
    if (!S.d[key + "s"]) return "";
    const go = S.screen, set = v => `data-dact="dset:${key}c:${v}" data-dgo="${go}"`;
    const n = rows.filter(r => !r[2]).length, label = `Start ${n} ${n === 1 ? "agent" : "agents"}`;
    if (!S.d[key + "c"]) return `<div class="pj-start"><span>${rows.length} picked</span><span class="sp"></span><button class="dbtn pri small" ${set("on")}>${label}</button></div>`;
    return `<div class="pj-start ask"><b>${label}?</b><span>Claude Code on build-box, one new worktree each, one after another. Each runs on your plan.</span>${rows.map(r => `<span class="${r[2] ? "skip" : ""}">${r[0]} ${r[1]}${r[2] ? ` (${r[2]})` : ""}</span>`).join("")}<span class="acts"><button class="dbtn pri small" data-toast="Starts ${n} agents, one at a time">${label}</button><button class="dbtn quiet small" ${set("")}>Cancel</button></span></div>`;
  };
  const csec = (title, count, body, action = "") => `<div><div class="cc-sec"><h4>${title}${count !== "" ? `<span class="n">${count}</span>` : ""}</h4>${action}</div>${body}</div>`;
  const panel = rows => `<div class="cc-panel">${rows}</div>`;
  // line(): a title and one muted line, opening on a click, with its right side.
  const ln = (lead, t, sub, right, go = "") => `<div class="cc-row cc-line${lead ? "" : " bare"}" ${go}>${lead}<button class="t" title="${t}"><b>${t}</b><span>${sub}</span></button><span class="r">${right}</span></div>`;
  const small = (l, pri) => `<button class="dbtn${pri ? " pri" : ""} small" data-toast="${l}">${l}</button>`;
  const beepAv = i => `<span class="cc-ag beep">${i}</span>`;
  const agentNeed = () => ln(hc0("claude"), "May it run xcodebuild on laptop?", "Claude Code · Swipeable approval strip on iOS", small("Answer", true), 'data-dact="dset:ags:asks" data-dgo="dAgentSession"')
;
  const NEEDS = () => agentNeed() + (subscribed() ? ln(beepAv(DI.tray), "Send the weekly digest to the team?", "Gmail · Inbox digest", small("Allow", true), 'data-dgo="dWinChat"') : "");
  const hc0 = ag => `<span class="cc-ag ${ag}">${ag === "codex" ? "CX" : "CC"}</span>`;
  const RUN = [["claude", "Swipeable approval strip on iOS", "you/site · laptop", tag("wait", "needs you")], ["codex", "Fix the device build script", "acme/api · build-box", "4 min"], ["claude", "Clippy fixes in the sync crate", "acme/api · build-box", "31 min"], ["", "Will solid-state batteries beat lithium by 2030?", "Research · round 2", "2 min"]];
  const running = () => RUN.filter(([ag]) => subscribed() || ag).map(([ag, t, sub, right]) => {
    const wait = ag && linking(sub.split(" · ").pop());
    return ln(ag ? hc0(ag) : "", t, sub, wait ? `<span style="color:var(--d-muted)">connecting</span>` : right, ag ? `${wait ? 'style="opacity:.55" ' : ""}data-dact="dset:ags:${wait ? "connecting" : "nochange"}" data-dgo="dAgentSession"` : 'data-dact="dset:rp:open" data-dgo="dWinResearch"');
  }).join("");
  const coming = CC_COMING.map(c => ln(`<span class="cc-when"><b>${c[0]}</b>${c[1]}</span>`, c[2], c[3], "", 'data-dgo="dWinRoutines"')).join("");
  const X = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>';
  const UPDATES = [["Dentist moved to Thursday", "G", "Gmail · 09:41"], ["Sam shared the Q4 budget", "S", "Slack · Yesterday"]];
  const updates = () => csec("Updates", UPDATES.length, panel(UPDATES.map(([t, i, sub]) => ln(beepAv(i), t, sub, `<button class="dbtn quiet" style="padding:2px 6px" aria-label="Dismiss" data-toast="Dismissed">${X}</button>`, 'title="Ask Beep about this" data-dact="dset:wc:update" data-dgo="dWinChat"')).join("")), `<button class="dbtn quiet small" data-toast="Cleared">Clear</button>`);
  const STATE_CLS = { bad: "bad", warn: "wait", ok: "run", "": "" };
  const itemIc = kind => kind === "pr" ? IC_PR : IC_ISSUE;
  // itemLine(): an open item in an Overview box.
  const itemLine = kind => ([repo, n, t, cls, st, age]) => ln(itemIc(kind), `#${n} ${t}`, st ? `<span class="cc-sub"><i class="chip ${STATE_CLS[cls]}">${st}</i>${repo}</span>` : repo, `${age}`, `title="Opens #${n}" data-toast="Opens #${n} on its host"`);
  // gitSection(): a row per project, its items under it, Review and Reply on each.
  const gitCol = (kind, title, list, key, none) => {
    const shown = S.d[key] ? list.filter(x => x[6].includes(S.d[key])) : list;
    const all = [...new Set(list.flatMap(x => x[6]))].sort();
    const chip = (tagName, l) => `<${tagName} class="pj-tag lab${S.d[key] === l ? " on" : ""}" data-dact="dset:${key}:${S.d[key] === l ? "" : l}" data-dgo="dHome">${l}</${tagName}>`;
    const w = S.d[key + "w"] || "any", picking = kind === "issue" && S.d[key + "s"];
    const ageH = x => parseFloat(x[5]) * (x[5].endsWith("d") ? 24 : 1);
    const keep = shown.filter(x => w === "any" || (w === "today" && x[5].endsWith("h")) || (w === "yesterday" && x[5] === "1 d") || (w === "week" && (x[5].endsWith("h") || parseFloat(x[5]) <= 6 && x[5].endsWith("d")))).sort((x, y) => S.d[key + "o"] === "oldest" ? ageH(y) - ageH(x) : ageH(x) - ageH(y));
    const body = !keep.length ? `<div class="cc-empty"><span>${none}</span></div>` : keep.map(([repo, n, t, cls, st, age, labels]) => `<div class="cc-row cc-line cc-gr it" data-toast="Opens #${n} on its host">${picking ? `<label><input type="checkbox"${n === 5 || n === 92 ? " checked" : ""} aria-label="Pick #${n}"></label>` : ""}<span class="ic">${itemIc(kind)}</span><button class="t" title="#${n}"><span class="num">#${n}</span><b>${t}</b><span class="pj-proj">${repo}</span>${labels.map(l => chip("i", l)).join("")}</button><span class="r">${st ? tag(STATE_CLS[cls], st) : ""}<span class="age">${age}</span>${kind === "pr" ? small("Review") : ""}${small("Reply")}</span></div>`).join("");
    const start = kind === "issue" ? __listStart(key, [["you/site", "#5 Dark mode flashes on load", ""], ["acme/api", "#92 Auto-sync the SDK docs", ""], ["oss/parser", "#14 Handle empty input", "no machine has a checkout"]]) : "";
    return csec(title, shown.length, `<div class="cc-panel cc-open">${__listCtl(key, kind === "issue")}${all.length ? `<div class="pj-labels">${all.map(l => chip("button", l)).join("")}</div>` : ""}${body}${start}</div>`);
  };
  const term = ([t, m, where, st, off]) => ln(`<span class="ddot${off}"></span>`, t, `${m} · ${where}`, st, 'data-dgo="dTerminals"');
  const box = (title, count, go, body) => csec(`<button class="lk" ${go}>${title}</button>`, count, panel(body));
  const tabGo = k => `data-dact="dset:htab:${k}" data-dgo="dHome"`;
  const two = (a, b) => `<div class="cc-cols">${a}${b}</div>`;
  const MT_TRAFFIC = {
    "build-box": ["Link 508 MB · File sync 40 MB · Transfers 17 GB", "14 GB / 3.2 GB"],
    studio: ["Link 96 MB · File sync 2.1 GB", "1.9 GB / 310 MB"],
    laptop: ["Link 41 MB · File sync 860 MB · Transfers 1.2 GB", "1.4 GB / 700 MB"],
  };
  const MT_SYNC = { "build-box": [["acme/api", "~/code/api · 12 files left", '<span class="fs-pct"><span class="fs-bar"><i style="width:63%"></i></span>63%</span>']], studio: [["you/site", "~/code/site", '<span class="pb-chip">In sync</span>']] };
  const homeBody = tab => {
    if (tab === "git") return `<div class="cc-git" id="sec-git">${two(gitCol("pr", "Pull requests", PRS, "hlp", "No open pull requests."), gitCol("issue", "Issues", ISSUES, "hli", "No open issues."))}</div>`;
    // Machines: one full-width table, a row per machine with New terminal, its
    // terminals indented under it as sub-rows (command, project and folder, state).
    if (tab === "machines") {
      const offline = S.d.cc === "offline";
      const rows = MT.filter(([n]) => subscribed() || n !== "phone").map(m => offline && m[0] === "studio" ? [m[0], "off", m[2], "–", "–", "–", m[6], "offline"] : linking(m[0]) ? [m[0], "dial", ...m.slice(2, 7), "connecting"] : m);
      const live = rows.filter(m => m[1] !== "off" && m[1] !== "dial").length;
      const head = ["Machine", "Agents", "Disk", "Memory", "Sent / received", "Can run", "Now", ""].map((t, i) => `<span class="th${["", " c-ag", " c-disk", " c-mem", " c-net", " c-can", "", ""][i]}">${t}</span>`).join("");
      const body = rows.map(([n, s, os, ag, disk, mem, can, now]) => {
        const off = s === "off" || s === "dial" ? " off" : "";
        const terms = TERMS.filter(x => x[1] === n).map(([t, , where, st, toff]) => `<span class="tr${off}" data-dgo="dTerminals"><span class="ddot${toff}"></span><span class="t"><b>${t}</b><small>${where}</small></span><span class="w">${st}</span></span>`).join("")
          + (MT_SYNC[n] || []).map(([pr, meta, right]) => `<span class="tr" data-dgo="dWinProject" title="${meta}"><span class="ddot"></span><span class="t"><b>${pr}</b><small>${meta}</small></span><span class="w">${right}</span></span>`).join("");
        return `<span class="nm${off}"><b><span class="ddot${s ? " " + s : ""}"></span>${n}</b><small>${os}</small></span><span class="num c-ag${off}">${ag}</span><span class="num c-disk${off}${n === "studio" ? " two" : ""}">${disk}${n === "studio" ? "<small>Scratch 1.2 TB free</small>" : ""}</span><span class="num c-mem${off}"${!off && n !== "phone" ? ` title="Load ${{"build-box": "4.82 on 16 cores", studio: "1.14 on 12 cores", laptop: "2.37 on 8 cores"}[n]}"` : ""}>${mem}</span><span class="num c-net${off}"${MT_TRAFFIC[n] ? ` title="${MT_TRAFFIC[n][0]}"` : ""}>${MT_TRAFFIC[n]?.[1] ?? "–"}</span><span class="num c-can${off}">${can}</span><span class="num${off}"${s === "off" ? ' title="Seen 12 min ago"' : ""}>${now}</span>`
          + `<span class="act">${n === "phone" ? "" : off ? `<button class="dbtn" disabled style="padding:3px 10px;font-size:12px;min-height:24px">New terminal</button>` : `<button class="dbtn" style="padding:3px 10px;font-size:12px;min-height:24px" data-dact="dset:tl:new" data-dgo="dTerminals">New terminal</button>`}</span>${terms}`;
      }).join("");
      return csec("Machines", `${live} of ${rows.length} online`, `<div class="cc-panel"><div class="nb-mt" role="table">${head}${body}${tmCells()}${tmRow() ? "<span></span>" : ""}</div></div>`, `<button class="dbtn small" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button>`);
    }
    if (S.d.cc === "empty") return subscribed()
      ? `<div class="cc-panel cc-empty"><span>Nothing needs you, and nothing is running.</span></div>`
      : `<div class="cc-panel cc-empty"><b>Nothing running</b><span>Start an agent on this computer, or add a machine to run it there.</span><button class="dbtn small" data-dgo="dAgentNew">Start an agent</button></div>`;
    if (S.d.cc === "old") return csec("Needs you", 1, panel(ln(beepAv(DI.tray), "Send the weekly digest to the team?", "Gmail · Inbox digest", small("Allow", true), 'data-dgo="dWinChat"'))) + csec("Coming up", CC_COMING.length, panel(coming));
    // A conflict is a row like the others: Resolve opens Sync, where the copies are compared and kept.
    const conflict = (path, sub) => ln(IC_SYNC, path, sub, `<button class="dbtn small" data-dgo="dSync">Resolve</button>`, `title="${path}" data-dgo="dSync"`);
    const conflicts = S.d.sy === "insync" ? "" : CONFLICTS.map(([path, note]) => conflict(path, `acme/widget · ${note}`)).join("");
    const review = ln(IC_PR, "#12 Move the blog to the new theme", "you/site · your review", small("Review"), 'data-toast="Opens pull request on its host"');
    return csec("Needs you", (subscribed() ? 3 : 2) + (conflicts ? 3 : 0), panel(NEEDS() + review + conflicts))
      + (subscribed() ? updates() : "")
      + box("Running", subscribed() ? RUN.length : RUN.length - 1, 'data-dgo="dAgents"', running())
      + (subscribed() ? box("Coming up", CC_COMING.length, 'data-dgo="dWinRoutines"', coming) : "");
  };
  const baseHome = D.dHome;
  // States the Home view does not draw itself (connecting, stuck) keep the earlier frame.
  const ownHome = () => !["connecting", "stuck"].includes(S.d.cc);
  const PROBLEMS = {
    unlinked: { dot: "off", foot: "Not linked", title: "This Mac is not linked yet", text: "Link it to your phone to see your chats, machines and approvals here.", action: "Link to your phone" },
    revoked: { dot: "bad", foot: "Removed", title: "This Mac was removed", text: "Your phone took this computer off the account. To use Beep here again, start over with a new key and link it from the phone.", action: "Start over" },
    refused: { dot: "bad", foot: "Refused", title: "The server turned this Mac away", text: "It did not find this Mac's account.", action: null },
  };
  const homeSide = options => {
    const html = ccSide("home", options);
    if (subscribed()) return html;
    const template = document.createElement("template");
    template.innerHTML = html;
    template.content.querySelectorAll(".chats, .research").forEach(node => node.remove());
    template.content.querySelectorAll(".cc-si").forEach(node => {
      const label = node.querySelector(".tt")?.textContent;
      if (["Coming up", "Memories", "Routines"].includes(label)) node.remove();
      if (label === "Home") node.querySelector(".n")?.replaceChildren(String(S.d.sy === "insync" ? 2 : 5));
      if (label === "Running") node.querySelector(".n")?.replaceChildren("3");
    });
    template.content.querySelectorAll('.pinned [data-dgo="dWinChat"]').forEach(node => node.remove());
    const footer = template.content.querySelector(".cc-foot > span");
    if (footer && !options.bare) footer.innerHTML = S.d.cc === "cached" ? `<span class="ddot dial"></span> <span title="build-box, gpu-box">Connecting to build-box and 1 more…</span>` : `<span class="ddot"></span> <b>${S.d.cc === "offline" ? 2 : 3}</b> of 3 machines online`;
    return template.innerHTML;
  };
  D.dHome = () => {
    if (S.d.cc === "startup") {
      const main = `<div class="cc-top"><h2>Home</h2><small>Wednesday, 4 November</small></div>
        <div class="cc-view"><div class="cc-mstrip"><button class="dbtn quiet small" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button></div>
        <div class="cc-tabs" role="tablist"><button role="tab" aria-selected="true">Overview</button><button role="tab" aria-selected="false" data-startup-machines>Machines</button></div>
        <div class="cc-panel cc-empty"><b>Start an agent</b><span>Your machines connect in the background.</span><button class="dbtn small" data-dgo="dAgentNew">Start an agent</button></div></div>`;
      return window.__mrmOver(skinned(ccWin(homeSide({ bare: true, noChats: true }), main)));
    }
    if (PROBLEMS[S.d.cc]) return mbar() + ccConnecting(false, PROBLEMS[S.d.cc]);
    if (!ownHome()) return baseHome();
    const cc = S.d.cc, old = cc === "old", empty = cc === "empty";
    const shownTabs = HOME_TABS.filter(([k]) => old ? k === "overview" : empty ? k === "overview" || k === "machines" : true);
    const tab = shownTabs.some(([k]) => k === S.d.htab) ? S.d.htab : "overview";
    const tabs = `<div class="cc-tabs" role="tablist">${shownTabs.map(([k, l, n]) => `<button role="tab" aria-selected="${k === tab}" data-dact="dset:htab:${k}" data-dgo="dHome">${l}${n !== "" ? `<span class="n">${n}</span>` : ""}</button>`).join("")}</div>`;
    const strip = old ? "" : empty ? `<div class="cc-mstrip"><button class="dbtn quiet small" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button></div>` : machStrip();
    const note = old ? `<div class="cc-note"><span class="ddot off"></span><span>Machines and running sessions aren't available for this account or server.</span></div>`
      : cc === "offline" ? `<div class="cc-note"><span class="ddot bad"></span><span><b>studio is offline.</b> Their sessions left Running now. They come back when beep-node runs there again.</span></div>` : "";
    const none = old || empty;
    const main = `<div class="cc-top"><h2>Home</h2><small>Wednesday, 4 November</small><span class="tools"></span></div>
      <div class="cc-view">${strip}${tabs}${note}${empty && tab === "machines" ? csec("Machines", "", `<div class="cc-panel cc-empty"><b>No machines linked yet</b><button class="dbtn pri" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button></div>`, `<button class="dbtn small" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button>`) : homeBody(tab)}</div>
      ${subscribed() ? `<div class="cc-comp hc"><span class="in"><span class="caret"></span><span class="ph">${none ? "Ask Beep anything" : "Ask Beep, or start an agent on a machine"}</span></span>${none ? "" : `<button class="dbtn" data-dgo="dAgentNew" title="Start a coding agent on a machine">Agent</button>`}<button class="dbtn pri" data-toast="Sent">Send<kbd>${DI.ret}</kbd></button></div>` : ""}`;
    return window.__mrmOver(skinned(ccWin(homeSide({ old, bare: empty, noChats: empty }), main)));
  };
  document.getElementById("desk").addEventListener("click", e => {
    if (!e.target.closest("[data-startup-machines]")) return;
    Object.assign(S.d, { cc: "empty", htab: "machines" });
    render();
  });
  const skinVariants = [["Skin: Flashmind", () => { S.d.skin = "flashmind"; render(); }, () => S.d.skin === "flashmind"], ["Skin: Linear", () => { S.d.skin = ""; render(); }, () => !S.d.skin], ["Skin: Light", () => { S.d.skin = "paper"; render(); }, () => S.d.skin === "paper"]];
  const keepVariants = (VARIANTS.dHome || []).filter(([l]) => !["Balanced", "Calm", "Detailed", "Default", "Wide window"].includes(l) && !l.startsWith("Git:"));
  const stuckAt = keepVariants.findIndex(([l]) => l === "Still connecting");
  keepVariants.splice(stuckAt + 1, 0, ...[["Not linked", "unlinked"], ["Removed from the account", "revoked"], ["Turned away", "refused"]].map(([l, k]) => [l, () => { Object.assign(S.d, { cc: k, mrm: "", htab: "overview" }); render(); }, () => S.d.cc === k]));
  VARIANTS.dHome = [...[["Current: mesh only", "mesh"], ["Server preview", "subscribed"]].map(([label, mode]) => [label, () => { S.d.homeAccount = mode; render(); }, () => (S.d.homeAccount || "mesh") === mode]), ...HOME_TABS.map(([k, l]) => [`Tab: ${l}`, () => { Object.assign(S.d, { htab: k, horg: false, cc: "default", mrm: "", tm: "" }); render(); }, () => (S.d.htab || "overview") === k && !S.d.horg && ownHome() && !S.d.tm && (S.d.cc || "default") === "default"]),
    ["Startup without server", () => { Object.assign(S.d, { cc: "startup", htab: "overview", mrm: "", tm: "" }); render(); }, () => S.d.cc === "startup"],
    ["Connecting, kept view", () => { Object.assign(S.d, { cc: "cached", mrm: "", tm: "" }); render(); }, () => S.d.cc === "cached"],
    ["PRs filtered by a label", () => { Object.assign(S.d, { htab: "git", hlp: "perf", cc: "default", mrm: "" }); render(); }, () => S.d.htab === "git" && !!S.d.hlp],
    ...keepVariants, ...skinVariants];
  const sideItem = DESK_SCREENS.find(g => g.group === "Command center").items.find(it => it[0] === "dHome");
  if (sideItem) sideItem[1] = "Home";

  return { skinned, hc, sec, row, empty, num, tag, csec, panel, ln, small, running, updates, term, box, skinVariants, IC_PR, IC_SYNC, CONFLICTS };
})();
