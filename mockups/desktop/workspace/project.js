(() => {
  const { skinned, sec, row, empty, num, tag, csec, panel, ln, small, running, updates, term, box, skinVariants } = WORKSPACE_MOCK;
  const hc = agent => `<span class="cc-ag ${agent}"><img src="../ios/assets/${agent}.svg" width="18" height="18" alt="${agent === "claude" ? "Claude Code" : "Codex"}"></span>`;
  // ---- Project: everything happening on the left, the pick on the right ----
  {
    document.head.insertAdjacentHTML("beforeend", '<link rel="stylesheet" href="workspace/project-layout.css"><link rel="stylesheet" href="workspace/project-details.css"><link rel="stylesheet" href="workspace/project-review.css">');
  }
  const IC_CHAT = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>';
  const pb = (l, cls = "") => desktopButton(l, cls, { "data-toast": l });
  // Pull and Push under "Where it lives": a button with nothing to do is disabled, a running one says so.
  const gb = (l, cls, off) => desktopButton(l, cls, { disabled: off, "data-toast": off ? null : l });
  // Pull and Push as icons, as VS Code's status bar: a down arrow with the commits behind, an up
  // arrow with those ahead. The tooltip says what a click moves; nothing to move dims it; a running
  // one turns its arrow into a spinner.
  const ARROW = { pull: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M6 13l6 6 6-6"/></svg>', push: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M6 11l6-6 6 6"/></svg>' };
  const gtip = (dir, n) => (dir === "pull" ? "Pull" : "Push") + (n ? ` ${n} commit${n === 1 ? "" : "s"}` : "");
  const gi = (dir, n, off, busy) => desktopButton(gtip(dir, n), "quiet small gi", {
    title: busy ? (dir === "pull" ? "Pulling…" : "Pushing…") : gtip(dir, n),
    "aria-label": gtip(dir, n), disabled: off || busy,
    "data-toast": off || busy ? null : gtip(dir, n),
  }, (busy ? '<span class="dspin"></span>' : ARROW[dir]) + n);
  const gpair = (behind, ahead, pull, push, busy = "") => gi("pull", behind, !pull, busy === "pull") + gi("push", ahead, !push, busy === "push");
  // The branch only: the counts are on the icons. "not pushed" stays, as no icon says it.
  const gnote = (branch, text, err) => `<span class="gs" style="display:block;font-size:12.5px;margin-top:2px;color:var(${err ? "--d-err" : "--d-muted"})"><code>${branch}</code>${text ? " " + text : ""}</span>`;
  // Remove from this machine: a quiet x at the row's end, and in the row's menu. Off, with why, on
  // the last machine and while an agent works there.
  const RM_X = '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>';
  const rmb = (name, off) => off
    ? `<button class="dbtn quiet small rmx" disabled title="${off}" aria-label="Remove from ${name}">${RM_X}</button>`
    : `<button class="dbtn quiet small rmx" title="Remove from ${name}" aria-label="Remove from ${name}" data-dact="dset:pwr:confirm" data-dgo="${S.screen === "dWinProjectWindow" ? "dWinProjectWindow" : "dWinProject"}">${RM_X}</button>`;
  const chip = (cls, t, spin) => `<span class="pj-st${cls ? " " + cls : ""}">${spin ? '<span class="dspin"></span>' : ""}${t}</span>`;
  const pnum = n => `<span class="num">#${n}</span>`;
  const pick = id => `data-dact="dset:psel:${id}" data-dgo="${S.screen === "dWinProjectWindow" ? "dWinProjectWindow" : "dWinProject"}"`;
  const PJ_ISSUES = [
      { id: "is92", lead: pnum(92), t: "Auto-sync the SDK docs", sub: "opened by contributor-a · 32 d", tags: ["docs", "ci"] },
      { id: "is53", lead: pnum(53), t: "Publish the CLI like the SDK", sub: "opened by contributor-b · 173 d · 1 comment", tags: ["release"] }];
  const ISSUE_LABELS = ["ci", "docs", "release"];
  const serverPreview = () => S.d.projectAccount === "server";
  // Keep the rows and order aligned with project/split.ts.
  const LIST = [
    { title: "", items: [{ id: "overview", t: "Overview", sub: "" },
      { id: "card", t: "Needs you", sub: "1", server: true },
      { id: "running", t: "Running", sub: "2" }] },
    { title: "", items: [
      { id: "actions", t: "Actions", sub: "1 running · 1 failed" },
      { id: "machines", t: "Machines", sub: "2 of 3 · 1 terminal" },
      { id: "issues", t: "Issues", sub: `${PJ_ISSUES.length} open` }] },
    { title: "", items: [
      { id: "tasks", t: "Tasks", sub: "2 running", server: true },
      { id: "agents", t: "Sessions", sub: "2 past" },
      { id: "branches", t: "Git", sub: "History and worktrees" }] },
    { title: "Pull requests", labels: ["cache", "deps", "perf"], items: [
      { id: "pr41", lead: pnum(41), t: "Cut the cache warm-up in half", sub: "sam · 5 d · CI failing", tags: ["perf", "cache"], r: chip("blocked", "needs reply") },
      { id: "pr38", lead: pnum(38), t: "Bump the sync crate", sub: "you · 9 d · ready · CI passing", tags: ["deps"], r: chip("done", "ready") }] },
  ];
  const RUNNING = [
    { id: "run0", open: "dAgentSession", lead: hc("claude"), t: "Swipeable approval strip", sub: "build-box · fix-approval", r: '<span class="cc-running"><span class="dspin" role="img" aria-label="Working"></span>now</span>' },
    { id: "run1", open: "dAgentSession", lead: hc("codex"), t: "Fix the device build script", sub: "studio · main", r: "3m ago" },
  ];
  const ITEMS = LIST.flatMap(g => g.items);
  const md = s => `<div class="pj-md">${s}</div>`;
  const post = (who, when, body, cls = "", extra = "") => `<div class="pj-post${cls}"><div class="by"><b>${who}</b> · ${when}${extra}</div>${md(body)}</div>`;
  const hostName = forgejo => forgejo ? "Forgejo" : "GitHub";
  const prHead = (title, n, by, when, tags, extra, acts) => `<div class="pj-dh"><h3>${title}</h3><div class="meta">${chip("running", "open")}<span>#${n} opened by <b>${by}</b> ${when}</span>${tags.map(l => `<i class="pj-tag">${l}</i>`).join("")}${extra}</div><div class="acts">${acts}</div></div>`;
  // Plan and Discuss need a server, so the issue details hide them without one.
  const foot = forgejo => `<div class="pj-cbox"><textarea class="field" rows="3" aria-label="Comment" placeholder="Comment"></textarea><div class="row"><button class="dbtn pri" data-toast="Comment posted to ${hostName(forgejo)}">Comment</button></div></div>`;
  // One machine owns its folder, sync details and terminals.
  const projectMachines = () => {
    const back = S.screen === "dWinProjectWindow" ? "dWinProjectWindow" : "dWinProject";
    const paused = S.d.pmsPaused === "yes";
    const healthy = S.d.fss === "healthy";
    const single = S.d.pwr === "one";
    const act = (label, key, value, cls = "small", attributes = {}) => desktopButton(label, cls, {
      "data-dact": `dset:${key}:${value}`, "data-dgo": back, ...attributes,
    });
    const machine = (name, folder, status, offline = false) => {
      const blocked = single || name === "build-box";
      const actions = `<div class="pm-actions">${act("Folder…", "fss", "change", "small", {
        "aria-label": `Change folder on ${name}`, onclick: `S.d.pmmachine='${name}'`,
      })}${act("Remove", "pwr", "confirm", "small warn", {
        "aria-label": `Remove from ${name}`, disabled: blocked,
        title: blocked ? single ? "A project stays on at least one machine." : "Stop the agent working here first." : null,
        onclick: `S.d.pmremove='${name}'`,
      })}</div>`;
      const state = paused ? "Paused" : status;
      const warning = name === "studio" && !healthy && !paused;
      const sync = `<div class="pm-sync"><span class="pm-child-label">Git sync</span><span class="${warning ? "pm-warning" : "pm-status"}">${state}</span><code>main</code><span class="pm-sync-actions">${!offline && !paused ? gpair(warning ? 1 : 0, name === "build-box" && !healthy ? 2 : 0, true, true) : ""}${warning ? act("Fix with an agent", "pmnotice", "Repair draft opened for studio", "small") : ""}</span></div>${warning ? '<p class="pm-sync-note">Commit or stash local changes before syncing.</p>' : ""}`;
      const files = S.d.igs ? `<div class="pm-sync"><span class="pm-child-label">Ignored folders</span><span>assets/models · data/captures</span></div>` : "";
      const term = `<div class="pm-terminals"><div class="pm-terminal-heading"><span>Terminals</span><small>${name === "build-box" ? "1 running" : offline ? "Machine offline" : "None running"}</small></div>${name === "build-box" ? `<button class="pm-terminal" data-dact="dset:ags:drafterm" data-dgo="dAgentSession"><span class="ddot"></span><b>cargo watch</b><code>/srv/project/widget</code><span>3 h</span></button>` : ""}</div>`;
      return `<article class="pm-machine" aria-label="${name}"><div class="pm-machine-head"><span class="ddot${offline ? " off" : ""}"></span><b>${name}</b><code>${folder}</code>${actions}</div><div class="pm-body">${sync}${files}<hr>${term}</div></article>`;
    };
    return `<div class="cc-sec"><h4>Machines</h4><span class="fs-tools"><small>${single ? "1 copy" : "3 copies"} · ${paused ? "sync paused" : healthy ? "2 in sync · 1 offline" : "1 needs attention · 1 offline"}</small>${act(paused ? "Resume sync" : "Pause sync", "pmsPaused", paused ? "" : "yes")}</span></div>
      <div class="pm-list">${machine("build-box", "/srv/project/widget", healthy ? "In sync" : "2 commits ahead")}${single ? "" : machine("studio", "/srv/widget", healthy ? "In sync" : "Local edits block sync") + machine("mini-box", "/srv/project/widget", "Waiting for machine", true)}
      <div class="pm-add"><span class="ddot"></span><span class="pm-identity"><b>laptop</b><small>Project is not on this machine</small></span>${desktopButton("Add project", "small", { "data-dact": "dset:pa:form", "data-dgo": "dAddToMachine" })}</div></div>
${S.d.pmnotice ? `<p class="pm-footnote" role="status">${S.d.pmnotice}</p>` : ""}`;
  };
  const details = {
    pr41: () => prHead("Cut the cache warm-up in half", 41, "sam", "5 days ago", ["perf", "cache"], chip("blocked", "changes requested") + chip("blocked", "checks failing"),
        `<span class="acts">${pb("Let the agent reply", "pri")}${pb("Continue")}${openReview("Review")}</span>${pb("Open on GitHub")}`)
      + `<div class="pj-db">${md("<p>The warm-up reads every entry twice. This reads the index once and fills the rest on demand.</p><ul><li>Cold start: 2.4 s to 1.1 s</li><li>No change to the on-disk format</li></ul><p>Run <code>cargo bench -p cache</code> to compare.</p>")}
        ${post("kim", "4 days ago", "<p>The lazy path skips the checksum. Can we keep it on first read?</p>")}${post("sam", "3 days ago", "<p>Good catch, fixed in the last push.</p>")}
        ${post("kim", "2 days ago", "<p>Cold start looks right here. The bench job still fails on main, unrelated?</p>")}${post("sam", "1 day ago", "<p>Unrelated, it fails without this branch too.</p>")}
        ${post("kim", "38 min ago", "<p>This changes when the index file is opened. Does the watcher still see a rename during warm-up?</p>", S.d.nfc ? " nf-on" : "", S.d.nfc ? pb("Open on GitHub") : "")}${foot(false)}</div>`,
    pr38: () => prHead("Bump the sync crate", 38, "you", "9 days ago", ["deps"], chip("running", "approved") + chip("done", "checks passing"),
        `<span class="acts">${pb("Continue", "pri")}${openReview("Review")}</span>${pb("Open on GitHub")}`)
      + `<div class="pj-db">${md("<p>Picks up the fix for the watcher on rename.</p>")}${foot(false)}</div>`,
    is92: () => prHead("Auto-sync the SDK docs", 92, "contributor-a", "32 days ago", ["docs", "ci"], "",
        `<span class="acts">${pb("Work on it", "pri")}</span>${pb(`Open on ${hostName(S.d.pjf)}`)}`)
      + `<div class="pj-db">${md("<p>The SDK docs are built by hand before each release. A workflow could build them on every tag.</p>")}${post("contributor-b", "30 days ago", "<p>+1, the docs for 0.13 are still missing.</p>")}${foot(S.d.pjf)}</div>`,
    is53: () => prHead("Publish the CLI like the SDK", 53, "contributor-b", "173 days ago", ["release"], "",
        `<span class="acts">${pb("Work on it", "pri")}</span>${pb("Open on GitHub")}`)
      + `<div class="pj-db">${md("<p>The SDK publishes from a tag. The CLI still needs a release by hand.</p>")}${post("sam", "170 days ago", "<p>Same workflow should do.</p>")}${foot(false)}</div>`,
    // Reached from a notification: approved, and not in the list.
    pr36: () => `<div class="pj-dh"><h3>Drop the v1 cache reader</h3><div class="meta">${chip("", "merged")}<span>#36 opened by <b>sam</b> 2 days ago</span>${chip("done", "checks passing")}</div><div class="acts">${pb("Open on GitHub")}</div></div>
      <div class="pj-db">${md("<p>Nothing reads the v1 format since 0.12. This removes the reader and its tests.</p>")}${post("kim", "3 h", "<p>Approved.</p>")}${foot(false)}</div>`,
    card: () => csec("Needs you", "the project chat waits on these", `<div class="pj-needs"><div class="pj-plan"><b>Plan 3 tasks</b><small>Tasks</small><ol><li>Split the warm-up from the index read</li><li>Keep the checksum on first read</li><li>Add a bench for cold start</li></ol><div class="row">${pb("Approve", "pri small")}${pb("Allow for this session", "small")}${pb("Deny", "warn small")}</div></div></div>`),
    tasks: () => `<div class="pj-cols">${csec("Tasks", "2 running · 1 in review · 1 done", `<div class="cc-panel">
        <div class="pj-task"><span class="t">Swipeable approval strip</span>${chip("running", "running", true)}<span class="m"><code>fix-approval</code><span>Claude Code on build-box</span></span><span class="acts">${openDiff("small")}${pb("Pause", "quiet small")}</span></div>
        <div class="pj-task"><span class="t">Fix the device build script</span>${chip("running", "running", true)}<span class="m"><code>main</code><span>Codex on studio</span></span><span class="acts">${pb("Pause", "quiet small")}</span></div>
        <div class="pj-task"><span class="t">Bump the sync crate</span>${chip("review", "review")}<span class="m"><a href="#">PR #38</a><code>bump-sync</code></span><span class="acts">${openReview("Review", "pri small")}${pb("Merge…", "small")}${pb("Open PR", "small")}${openDiff("small")}${pb("Back to open", "quiet small")}</span></div>
        <div class="pj-task"><span class="t">Add a bench for cold start</span>${chip("done", "done")}<span class="m"><code>bench-cold</code></span><span class="acts">${openDiff("small")}${pb("Reopen", "quiet small")}</span></div></div>`)}
      ${csec("Workers", "2 running", `<div class="cc-panel pj-tree">
        <div class="pj-node"><span class="cc-ag beep">${DI.tray}</span><span class="t"><b>Project agent</b><span>Plans tasks, files issues, places workers</span></span></div>
        <div class="pj-node child link">${hc("claude")}<span class="t"><b>Swipeable approval strip</b><span>Claude Code on build-box · running 1 min</span></span><span class="acts">${pb("Stop", "quiet small")}</span><span class="out">Wired the strip to the approval card.</span></div>
        <div class="pj-node child link">${hc("codex")}<span class="t"><b>Fix the device build script</b><span>Codex on studio · running 4 min</span></span><span class="acts">${pb("Stop", "quiet small")}</span></div></div>`)}</div>`,
    branches: () => window.gitTreeMock(),
    machines: () => projectMachines(),
    sync: () => projectMachines(),
  };
  const CHECK_MARK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>';
  const FAIL_MARK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M7 7l10 10M17 7L7 17"/></svg>';
  // The run opened in Actions: its jobs, steps, the end of the log and Fix this.
  const runOpen = (who) => `<span class="pj-run"><span class="pj-jobs"><b class="bad">test</b><small>failed, 3 min</small><b>lint</b><small>passed, 48 s</small></span>
    <span class="pj-steps"><span class="pj-step ok"><span class="ddot off"></span><span>1. Check out</span><small>4 s</small></span><span class="pj-step bad"><span class="ddot bad"></span><span>2. cargo test</span><small>3 min</small></span><span class="pj-step skip"><span class="ddot off"></span><span>3. Upload results</span><small>skipped</small></span></span>
    <span class="pj-log"><small>End of the log of test, cargo test</small><pre>error[E0061]: this function takes 2 arguments but 1 argument was supplied
   at src/index/warm.rs:212:18
error: could not compile \`cache\` (lib) due to 1 previous error
Error: Process completed with exit code 101.</pre></span>
    <span class="acts">${pb("Fix this", "pri")}${pb("Full log")}</span></span>`;
  const runRow = (n, title, branch, who, when, took, ok, open) => `<div class="pj-runrow ${ok ? "ok" : "bad"}${open ? " open" : ""}"><div class="rr-head" role="button" tabindex="0" aria-expanded="${!!open}" ${pick(open ? "actions" : "ci" + n)}>
      <span class="rr-st ${ok ? "ok" : "bad"}">${ok ? CHECK_MARK : FAIL_MARK}</span>
      <div class="rr-body"><div class="rr-t"><b>CI #${n}</b><span>${title}</span></div><div class="rr-m"><code class="br">${branch}</code><span class="ev">push</span><span class="ev">${who}</span></div></div>
      <div class="rr-side"><span class="rr-when">${when}</span><span class="rr-took">${took}</span></div><span class="rr-acts">${pb("Open", "quiet small")}</span><span class="rr-chev${open ? " on" : ""}"></span></div>${open ? runOpen(who) : ""}</div>`;
  const actions = open => `<div class="pj-actions"><div class="cc-sec"><h4>Actions</h4><small>2 failed, 1 passed</small><small class="sw-age">synced 1 min ago</small>${pb("Refresh", "quiet small")}</div>
    <div class="cc-sec"><h4>Running</h4><small>1</small></div>
    <div class="cc-panel pj-runbox">${runRow(353, "Fix the device build script", "main", "you", "1 min ago", "running", true, false).replace(/<span class="rr-st ok">.*?<\/span>/, '<span class="rr-st running"><span class="ddot"></span></span>')}</div>
    <div class="cc-sec"><h4>Finished</h4><small>3</small></div>
    <div class="pj-labels"><button class="pj-tag lab" data-toast="Filter: failed">failed 2</button><button class="pj-tag lab" data-toast="Filter: passed">passed 1</button></div>
    <div class="cc-panel pj-runbox">${runRow(351, "Cut the cache warm-up in half", "main", "sam", "12 min ago", "3m 12s", false, open === 351)}${runRow(349, "Retry uploads when the token expires", "main", "kim", "5 h ago", "2m 41s", false, open === 349)}${runRow(347, "Bump the sync crate", "bump-sync", "you", "9 d ago", "2m 30s", true, false)}</div></div>`;
  details.actions = () => actions(0);
  details.ci351 = () => actions(351);
  details.ci349 = () => actions(349);
  // The project's Overview: a box per list, each title opening that row.
  details.overview = () => {
    const box = (title, count, id, rows) => csec(`<button class="lk" ${pick(id)}>${title}</button>`, count, panel(rows));
    const rowsOf = g => (g === "Running" ? RUNNING : g === "Issues" ? PJ_ISSUES : LIST.find(x => x.title === g).items).map(i => ln(i.lead, i.t, i.sub, i.r || "", i.open ? `data-dgo="${i.open}"` : pick(i.id))).join("");
    const dot = off => `<span class="ddot${off ? " off" : ""}"></span>`;
    const tagged = i => [i.r || "", ...(i.tags || []).slice(0, 3).map(l => `<i class="pj-tag">${l}</i>`)].join("");
    const scroll = (rows, go) => rows.map(i => ln(i.lead, i.t, i.sub, tagged(i), pick(i.id))).join("");
    return `<div class="pj-ov"><div>${box("Running", 2, "running", rowsOf("Running"))}
      ${csec("Past sessions", 2, panel(ln("", "Retry uploads when the token expires", "Claude Code · build-box · ended 2 h ago", "") + ln("", "Bump the sync crate", "Codex · studio · ended 9 d ago", "")))}
      ${box("Machines", "2/3", "machines", ln(dot(), "build-box", "widget", "", pick("machines")) + ln(dot(), "studio", "widget", "", pick("machines")) + ln(dot(true), "mini-box", "offline", "", pick("machines")))}
      ${box("Terminals", 1, "machines", ln(dot(), "cargo watch", "build-box · acme/widget · ~/code/widget", "3 h", 'data-dact="dset:ags:drafterm" data-dgo="dAgentSession"'))}</div>
      <div>${box("Pull requests", 2, "pr41", rowsOf("Pull requests"))}
      ${csec(`<button class="lk" ${pick("issues")}>Issues</button>`, PJ_ISSUES.length, `<div class="cc-panel pj-scroll">${scroll(PJ_ISSUES)}</div>`)}</div></div>`;
  };
  details.running = () => csec("Agents", RUNNING.length, panel(RUNNING.map(i => ln(i.lead, i.t, i.sub, i.r, `data-dgo="${i.open}"`)).join("")));
  details.agents = () => projectSessionTabs(hc, pb);
  const pjRow = (i, lit, nav = false) => nav
    ? `<button class="pj-li nav" title="${i.t}"${i.id === lit ? ' aria-current="true"' : ""} ${pick(i.id)}><span class="t"><b>${i.t}</b></span><span class="c">${i.sub}</span></button>`
    : `<button class="pj-li" title="${i.t}"${i.id === lit ? ' aria-current="true"' : ""} ${i.open ? `data-dgo="${i.open}"` : pick(i.id)}><span class="lead">${i.lead || ""}</span><span class="t"><b>${i.t}</b><span>${i.sub}${(i.tags || []).map(l => `<i class="pj-tag lab">${l}</i>`).join("")}</span></span>${i.r ? `<span class="r">${i.r}</span>` : ""}</button>`;
  // Issues list at the right; one issue opens there with a way back.
  details.issues = () => __listCtl("pji", true) + `<div class="pj-labels">${ISSUE_LABELS.map(l => `<button class="pj-tag lab" data-toast="Filter: ${l}">${l}</button>`).join("")}</div><div class="pj-rows">${PJ_ISSUES.map(i => S.d.pjis ? `<div class="pj-prow"><label><input type="checkbox"${i.id === "is92" ? " checked" : ""} aria-label="Pick #${i.t}"></label>${pjRow(i)}</div>` : pjRow(i)).join("")}</div>` + __listStart("pji", [["acme/widget", "#92 Auto-sync the SDK docs", ""]]);
  for (const id of ["is92", "is53"]) {
    const page = details[id];
    details[id] = () => `<button class="dbtn quiet small pj-back" ${pick("issues")}>All issues</button>` + page();
  }
  // Which row a detail keeps lit.
  const LIT = { ci351: "actions", ci349: "actions", review: "branches", is92: "issues", is53: "issues", terminals: "machines" };
  // Machines and their terminals are one pane, as on Home.
  details.terminals = details.machines;
  details.review = () => window.__rvPane();
  window.__pjReviewPage = () => { const keep = S.d.psel; S.d.psel = "review"; try { return D.dWinProject(); } finally { S.d.psel = keep; } };
  const openDiff = (cls = "") => `<button class="dbtn${cls ? " " + cls : ""}" data-dact="dset:ags:diffzoom" data-dgo="dAgentSession">View diff</button>`;
  const openReview = (l, cls = "") => `<button class="dbtn${cls ? " " + cls : ""}" data-dact="dset:psel:review" data-dgo="dWinProject">${l}</button>`;
  const CHEV = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';
  // Top actions, then the project-wide sync toggles under a label, then Remove after a line.
  // Choose ignored folders shows while ignored-folder sync is on; the Syncthing switch reads by who syncs now.
  const pjMenu = () => {
    const check = on => on ? `<span class="ok">${DI.check}</span>` : "";
    const toggle = (label, on, act) => `<button class="mi" role="menuitemcheckbox" aria-checked="${on}" data-dact="${act}" data-dgo="dWinProject"><b>${label}</b>${check(on)}</button>`;
    const sep = '<div role="separator" style="height:1px;margin:4px 3px;background:var(--d-border)"></div>';
    const beep = S.d.stFiles === "off";
    return `<div class="dmenu" role="menu" aria-label="acme/widget" style="left:auto;top:44px;right:12px;z-index:9;width:290px"><button class="mi" role="menuitem" data-toast="Pin"><b>Pin</b></button>${S.screen === "dWinProjectWindow" ? "" : '<button class="mi" role="menuitem" data-dgo="dWinProjectWindow"><b>Open in New Window</b></button>'}${sep}<div role="group" aria-label="Sync on all machines"><div style="padding:4px 9px 3px;font-size:11.5px;color:var(--d-muted)">Sync on all machines</div>${toggle("Preserve edits before Git sync", !!S.d.gitrecovery, `dset:gitrecovery:${S.d.gitrecovery ? "" : "1"}`)}${toggle("Sync ignored folders", !!S.d.igs, S.d.igs ? "dset:igs:" : "dset:igsform:1")}${S.d.igs ? '<button class="mi" role="menuitem" data-dact="dset:igsform:1" data-dgo="dWinProject"><b>Choose ignored folders…</b></button>' : ""}<button class="mi" role="menuitem" data-dact="dset:stFiles:${beep ? "on" : "off"}" data-dgo="dWinProject"><b>${beep ? "Use Syncthing instead of Beep sync" : "Use Beep sync instead of Syncthing"}</b></button></div>${sep}<button class="mi" role="menuitem" data-toast="Remove project"><b style="color:var(--d-err)">Remove project…</b></button></div>`;
  };
  // The composer always names a real machine: the last picked while it has a checkout and an
  // agent, else the first such machine. With none, it reads "Add to a machine" and picking one
  // adds the project there (the machine row's Add), the progress on the line under it.
  const pjComp = () => {
    const add = S.d.pjadd || "", none = add === "menu" || add === "none", adding = add === "adding";
    const machines = [["build-box", "Claude Code, Codex"], ["mac-box", "Claude Code"]];
    const menu = add === "menu" ? `<div class="dmenu" style="top:auto;bottom:calc(100% + 6px);width:240px">${machines.map(([m, a]) => `<button class="mi" data-dact="dset:pjadd:adding" data-dgo="dWinProject"><b>${m}</b><small>${a}</small></button>`).join("")}</div>` : "";
    const pill = none ? `<span style="position:relative"><button class="pill" type="button" data-dact="dset:pjadd:${add === "menu" ? "none" : "menu"}" data-dgo="dWinProject">Add to a machine ${CHEV}</button>${menu}</span>`
      : `<button class="pill pj-machine" type="button" title="Shift+Tab switches machines"${adding ? " disabled" : ""}>${S.d.pjmachine || 'build-box'}${adding ? " <small>adding</small>" : ""}</button>`;
    const ph = none || adding ? "Add acme/widget to a machine to start an agent" : "Describe a task for a coding agent in acme/widget";
    const err = adding ? `<span class="cc-err" role="status" data-dact="dset:pjadd:" data-dgo="dWinProject">Copying to build-box: 1,204 of 1,880 files</span>` : "";
    // Images and files picked, pasted or dropped go with the first prompt.
    const att = S.d.pjfile ? `<div class="ag-att"><span title="Copied to the selected machine when you start">screen-recording.mov<em data-dact="dset:pjfile:" data-dgo="dWinProject">&#215;</em></span></div>` : S.d.pjimg ? `<div class="ag-att"><span><i></i>Screenshot 2026-10-07.png<em data-dact="dset:pjimg:" data-dgo="dWinProject">&#215;</em></span></div>` : "";
    return `<form class="cc-pjc" onsubmit="return false"><div class="box">${att}<textarea rows="1" placeholder="${ph}">${S.d.pjimg ? "The login button overlaps the field, as in this screenshot" : ""}</textarea><div class="row">${pill}<button class="pill" type="button"${none || adding ? " disabled" : ""}>Current checkout <small>as it is</small></button><span class="sp"></span><button class="pill" type="button">${hc("claude")}Claude Code <small>Default · Medium</small></button><button class="dbtn quiet" type="button" aria-label="Attach files" title="Attach files" data-dact="dset:pjfile:true" data-dgo="dWinProject"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg></button><button class="dbtn pri" type="button" data-toast="Started"${none || adding ? " disabled" : ""}>Start<kbd>⏎</kbd></button></div></div>${err}</form>`;
  };
  document.addEventListener('keydown', e => {
    if (e.key !== 'Tab' || !e.shiftKey || e.altKey || e.ctrlKey || e.metaKey || e.isComposing || !e.target.matches('.cc-pjc textarea')) return;
    const pill = e.target.closest('.cc-pjc').querySelector('.pj-machine:not(:disabled)');
    if (!pill) return;
    e.preventDefault();
    S.d.pjmachine = pill.textContent === 'build-box' ? 'mac-box' : 'build-box';
    pill.textContent = S.d.pjmachine;
  });
  D.dWinProject = () => {
    const sel = S.d.psel === "sync" || S.d.psel === "terminals" ? "machines" : S.d.psel || "overview", lit = LIT[sel] || sel;
    const list = LIST.map((g, at) => (g.title ? `<h4>${g.title}<span class="n">${g.items.length}</span></h4>` : at ? "<hr>" : "")
      + (g.title === "Pull requests" ? __listCtl("pjp") : "")
      + (g.labels ? `<div class="pj-labels">${g.labels.map(l => `<button class="pj-tag lab" data-toast="Filter: ${l}">${l}</button>`).join("")}</div>` : "")
      + g.items.filter(i => !i.server || serverPreview()).map(i => pjRow(i, lit, !g.title)).join("")).join("");
    const strip = `<div class="cc-mstrip pj-strip" aria-label="Project status">${[
      ["main", "branch", "branches"], ["Actions", "1 running · 1 failed", "actions"], ["2 agents", "running", "running"],
      ["Machines", "2 of 3", "machines"], ["2 PRs", "1 needs a reply", "pr41"], ["2 issues", "open", "issues"], ["Sessions", "2 past", "agents"],
    ].map(([label, text, id]) => `<button class="m" ${pick(id)}><b>${label}</b><span${id === "actions" ? ' class="bad"' : ""}>${text}</span></button>`).join("")}</div>`;
    const forgejo = S.d.pjf && sel === "is92";
    const forges = `<button class="dbtn small" title="github.com/acme/widget" data-toast="Opens github.com/acme/widget">GitHub</button><button class="dbtn small" title="forge.example.com/acme/widget" data-toast="Opens forge.example.com/acme/widget">Forgejo</button>`;
    const main = `<div class="nb-main" style="grid-template-rows:auto minmax(0,1fr) auto"><div class="cc-top"><h2>acme/widget</h2><small>${forgejo ? "forgejo.example.com" : "github.com"} · imported 3 d</small><span class="tools">${forges}<span class="ed-tools">${pb("Open in Zed", "quiet small")}<button class="dbtn quiet small" aria-label="Choose an editor" data-toast="Editors">${CHEV}</button></span><button class="dbtn quiet small" aria-label="Project menu" data-dact="dset:pmenu:${S.d.pmenu ? "" : "1"}" data-dgo="dWinProject">⋯</button></span></div>
      <div class="cc-view split project-current">${strip}<div class="pj-split${sel === "branches" ? " gh-layout" : ""}">${sel === "branches" ? window.gitHistoryNavigation() : `<div class="pj-list">${list}</div>`}${sel === "branches" ? window.gitHistoryDetail() : `<div class="pj-det">${details[sel]()}</div>`}</div></div>
      ${pjComp()}${S.d.pmenu ? pjMenu() : ""}</div>`;
    const side = document.createElement("template");
    side.innerHTML = ccSidePj("acme/widget");
    if (!serverPreview()) {
      side.content.querySelectorAll(".chats, .research").forEach(node => node.remove());
      side.content.querySelectorAll(".cc-si").forEach(node => {
        if (["Coming up", "Memories", "Routines"].includes(node.querySelector(".tt")?.textContent)) node.remove();
      });
      side.content.querySelectorAll('.pinned [data-dgo="dWinChat"]').forEach(node => node.remove());
      const machines = document.createElement("div");
      machines.className = "cc-grp";
      machines.innerHTML = `<h3>Machines</h3>${["build-box", "studio", "mini-box"].map(name => `<button class="cc-si" data-dact="dset:htab:machines" data-dgo="dHome"><span class="ddot${name === "mini-box" ? " off" : ""}"></span><span class="tt">${name}</span></button>`).join("")}`;
      const settings = [...side.content.querySelectorAll(".cc-grp")].find(node => node.querySelector('h3')?.textContent === "Beep");
      if (settings) settings.before(machines);
      const footer = side.content.querySelector(".cc-foot > span");
      if (footer) footer.innerHTML = '<span class="ddot"></span> <b>2</b> of 3 machines online';
    }
    const page = skinned(ccWin(side.innerHTML, main));
    if (S.d.igsform === "1") return ccSheetOver(page, `<div class="am-head"><b>Sync gitignored</b><kbd>esc</kbd></div>
      <label class="am-line"><input type="checkbox" checked>Sync selected ignored folders on all machines</label>
      <label class="am-label" for="ignored-folders">Folders relative to the project, one per line</label><textarea id="ignored-folders" class="ip-path ip-list" rows="4">assets/models
data/captures</textarea>
      <div class="am-sub">Each folder must be wholly ignored by Git and contain no tracked or staged files. These folders use file sync, including conflict copies. Other uncommitted edits stay local. Build output, linked repositories and paths excluded in .beepignore stay local.</div>
      <div class="am-line ip-acts"><button class="dbtn quiet" data-dact="dset:igsform:" data-dgo="dWinProject">Cancel</button><button class="dbtn pri" data-dact="dset:igs:1" data-dgo="dWinProject" data-toast="Ignored-folder sync saved for all machines" onclick="S.d.igsform=''">Save</button></div>`);
    if (sel === "machines" && S.d.fss === "change") {
      const machine = S.d.pmmachine || "studio";
      return ccSheetOver(page, `<div class="am-head"><b>Change folder on ${machine}</b><kbd>esc</kbd></div>
        <div class="am-sub">Choose another folder for this project's copy on ${machine}.</div>
        <label class="am-label" for="pm-folder">Folder on ${machine}</label>
        <input id="pm-folder" class="ip-path" value="/srv/project/widget">
        <div class="am-sub">Choose an empty folder or a checkout of the same repository. The current folder stays on disk.</div>
        <div class="am-line ip-acts"><button class="dbtn quiet" data-dact="dset:fss:stuck" data-dgo="dWinProject">Cancel</button><button class="dbtn pri" data-dact="dset:fss:stuck" data-dgo="dWinProject" data-toast="Project folder changed">Change folder</button></div>`);
    }
    if (sel !== "machines" || (S.d.pwr !== "confirm" && S.d.pwr !== "refused" && S.d.pwr !== "delete")) return page;
    // Remove asks first, then closes when removal completes without waiting for a list refresh.
    // Escape and Cancel can dismiss pending removal; the operation continues.
    // The node stops syncing; its folder stays unless deletion was selected.
    const back = S.screen === "dWinProjectWindow" ? "dWinProjectWindow" : "dWinProject";
    const err = S.d.pwr === "refused" ? `<div class="am-msg bad">An agent is working in acme/widget on mini-box. Stop it first.</div>` : "";
    const del = S.d.pwr === "delete";
    const target = S.d.pmremove || "mini-box";
    return ccSheetOver(page, `<div class="am-head"><b>Remove acme/widget from ${target}?</b><kbd>esc</kbd></div>
      <div class="am-msg">It stops syncing to ${target} and leaves the project's machines.</div>
      <label class="am-sub" style="display:flex;gap:8px;align-items:center"><input type="checkbox"${del ? " checked" : ""} data-dact="dset:pwr:${del ? "confirm" : "delete"}" data-dgo="${back}">Also delete the folder on ${target}</label>
      <div class="am-sub"><code>${target === "studio" ? "/srv/widget" : "/srv/project/widget"}</code> ${del ? "is deleted on ${target} once it stops syncing there. The other machines keep their copies." : "stays on disk as it is."}</div>${err}
      <div class="gp-foot"><button class="dbtn quiet" data-dact="dset:pwr:" data-dgo="${back}">Cancel</button><button class="dbtn warn" data-dact="dset:pwr:" data-dgo="${back}" data-toast="${del ? `Project removed from ${target}. Folder deleted there.` : `Project removed from ${target}. Its folder stays.`}">${del ? "Remove and delete" : "Remove"}</button></div>`);
  };
  VARIANTS.dWinProject = [
    ["Current: mesh only", () => { S.d.projectAccount = "mesh"; S.d.psel = "overview"; render(); }, () => !serverPreview()],
    ["Server preview", () => { S.d.projectAccount = "server"; render(); }, serverPreview],
    ["Running", () => { S.d.psel = "running"; render(); }, () => S.d.psel === "running"],
    ["Custom agents", () => { S.d.psel = "agents"; S.d.projectSessionTab = "agents"; render(); }, () => S.d.psel === "agents" && S.d.projectSessionTab === "agents"],
    ["Overview", () => { S.d.psel = "overview"; S.d.pmenu = ""; S.d.pjadd = ""; S.d.pjimg = false; S.d.pjfile = false; render(); }, () => (S.d.psel || "overview") === "overview" && !S.d.pmenu && !S.d.pjadd && !S.d.pjimg && !S.d.pjfile],
    ["Composer: no checkout on an agent machine", () => { S.d.psel = "overview"; S.d.pjadd = "menu"; render(); }, () => S.d.pjadd === "menu" || S.d.pjadd === "none"],
    ["Composer: adding to a machine", () => { S.d.psel = "overview"; S.d.pjadd = "adding"; render(); }, () => S.d.pjadd === "adding"],
    ["Composer: a video attached", () => { S.d.psel = "overview"; S.d.pjadd = ""; S.d.pjimg = false; S.d.pjfile = true; render(); }, () => !!S.d.pjfile],
    ["Composer: an image attached", () => { S.d.psel = "overview"; S.d.pjadd = ""; S.d.pjimg = true; S.d.pjfile = false; render(); }, () => !!S.d.pjimg],
    ["Pull request", () => { S.d.psel = "pr41"; S.d.nfc = ""; render(); }, () => S.d.psel === "pr41" && !S.d.nfc],
    ["Issues", () => { S.d.psel = "issues"; render(); }, () => S.d.psel === "issues"],
    ["Issue", () => { S.d.psel = "is92"; S.d.pjf = false; render(); }, () => S.d.psel === "is92" && !S.d.pjf],
    ["Issue on Forgejo", () => { S.d.psel = "is92"; S.d.pjf = true; render(); }, () => S.d.psel === "is92" && !!S.d.pjf],
    ["Needs you", () => { S.d.projectAccount = "server"; S.d.psel = "card"; render(); }, () => S.d.psel === "card"],
    ["Tasks", () => { S.d.projectAccount = "server"; S.d.psel = "tasks"; render(); }, () => S.d.psel === "tasks"],
    ["Sessions", () => { S.d.psel = "agents"; S.d.projectSessionTab = "sessions"; render(); }, () => S.d.psel === "agents"],
    ["Git", () => { S.d.psel = "branches"; render(); }, () => S.d.psel === "branches"],
    ["Review", () => { S.d.psel = "review"; render(); }, () => S.d.psel === "review"],
    ["Actions", () => { S.d.psel = "actions"; render(); }, () => S.d.psel === "actions"],
    ["Machines", () => { S.d.psel = "machines"; S.d.pwr = ""; render(); }, () => S.d.psel === "machines" && !S.d.pwr],
    ["Machines: remove from one", () => { S.d.psel = "machines"; S.d.pwr = "confirm"; render(); }, () => S.d.psel === "machines" && S.d.pwr === "confirm"],
    ["Machines: removal refused", () => { S.d.psel = "machines"; S.d.pwr = "refused"; render(); }, () => S.d.psel === "machines" && S.d.pwr === "refused"],
    ["Machines: remove and delete the folder", () => { S.d.psel = "machines"; S.d.pwr = "delete"; render(); }, () => S.d.psel === "machines" && S.d.pwr === "delete"],
    ["Machines: on one machine", () => { S.d.psel = "machines"; S.d.pwr = "one"; render(); }, () => S.d.psel === "machines" && S.d.pwr === "one"],
    ["Terminals", () => { S.d.psel = "terminals"; S.d.ptm = ""; render(); }, () => S.d.psel === "terminals"],
    ["Machines: sync details", () => { S.d.psel = "sync"; S.d.fss = ""; render(); }, () => S.d.psel === "sync" && !S.d.fss],
    ["Machines: sync needs attention", () => { S.d.psel = "sync"; S.d.fss = "stuck"; render(); }, () => S.d.psel === "sync" && S.d.fss === "stuck"],
    ["Machines: change folder", () => { S.d.psel = "sync"; S.d.fss = "change"; render(); }, () => S.d.psel === "sync" && S.d.fss === "change"],

    ["Project menu", () => { S.d.psel = "overview"; S.d.pmenu = "1"; render(); }, () => !!S.d.pmenu],
    ...skinVariants].map(([l, f, on]) => l === "Project menu" ? [l, f, on] : [l, () => { S.d.pmenu = ""; f(); }, on]);
  D.dProjectMachines = () => {
    S.d.psel = "machines";
    return D.dWinProject();
  };
  DESK_SCREENS.find(g => g.group === "Command center").items.push(["dProjectMachines", "Project: machines and sync", "ProjectMachines"]);
  if (location.hash === "#dProjectMachines") { S.screen = "dProjectMachines"; render(); }
  // A project with no project agent: its sessions, where it lives, what runs there.
  window.__pjOne = () => `<div class="pj-cols pj-half">${csec("Past sessions", 2, `<div class="cc-panel"><div class="pv-row" style="cursor:pointer" data-dgo="dAgentSession"><span class="t">Retry uploads when the token expires</span><span class="m" style="align-items:center">${pvMark("claude")}<span>ended</span><span>build-box</span><span>widget</span><span>2 h</span></span></div><div class="pv-row" style="cursor:pointer" data-dgo="dAgentSession"><span class="t">Bump the toolchain</span><span class="m" style="align-items:center">${pvMark("codex")}<span>ended</span><span>studio</span><span>widget</span><span>1 d</span></span></div></div>`)}<div class="ca-col">${details.machines()}${csec("Agents", "", '<div class="cc-panel cc-empty"><span>No agent is working in this project right now.</span></div>')}${csec("Custom agents", "", '<div class="cc-panel cc-empty"><span>No custom agents in .claude/agents, .opencode/agent, .codex/agents, .pi/agents or .agents yet.</span></div>', pb("Reload", "quiet small"))}</div></div>`;

})();
