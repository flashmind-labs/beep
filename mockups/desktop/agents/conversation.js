(() => {
  const { AGK, AG_ICON, HARNESS, av, tag, pick, variants, hint, HOSTS, SG, step, SUBS, SUBS_DISK, subRow, KIDS, kidRow } = AGENT_MOCK;
  D.dAgentSession = () => {
    const variant = pick("ags", ["nochange", "editorerror", "held", "connecting", "fromsession", "warning", "working", "asks", "idle", "stepsopen", "failed", "failedopen", "subagents", "subchild", "agentkids", "agentchild", "agentstopped", "answered", "customdefault", "model", "flshmodel", "flshusage", "flshunknown", "ended", "outside", "outsideconfirm", "outsidewait", "disksubs", "image", "shots", "shotgone", "shotpanel", "paneright", "panegone", "panebottom", "panepdf", "panecollapsed", "files", "filemulti", "filedelete", "filemenu", "filemenuwith", "filetext", "filefilter", "dragpath", "dropchoice", "term", "termsplit", "dragtab", "pasted", "pastepreview", "artifacts", "running", "runstop", "thinking", "thoughts", "thoughtopen", "steer", "steered", "stopping", "rich", "richopen", "richcmd", "richgroup", "richdone", "richerror", "richjump", "richthink", "richthought"], "nochange");
    const v = variant === "flshusage" || variant === "flshunknown" ? "idle" : variant;
    // Enter while a turn runs shows the message in the chat immediately.
    // It stays pending until the agent receives it after its current step.
    // Cmd-. stops at once: the turn reads
    // Stopping until the agent confirms.
    const stopping = v === "stopping";
    // A paste of more than 10 lines is one pill in the field, removed as a
    // unit; a click previews it, and it goes out as the full text.
    const pasted = v === "pasted" || v === "pastepreview";
    // Typing / at the start of the message lists the agent's commands and skills, filtered by name then
    // description; typing @ after a space or at the start lists files, the folder in muted text. Arrows move,
    // Enter or Tab picks (Enter does not send while it is open), Esc closes and leaves the text. A pick fills
    // `/name ` or the path, and the user sends it.
    const cmdMenu = (() => {
      const row = (a, b, c, on) => `<div class="cm-row${on ? " on" : ""}" role="option" aria-selected="${on}" data-toast="Picked ${a}"><span class="n">${a}</span><span class="d">${b}</span>${c ? `<small>${c}</small>` : ""}</div>`;
      const hint = "<span><kbd>&#8593;&#8595;</kbd>choose</span><span><kbd>Tab</kbd>pick</span><span><kbd>esc</kbd>close</span>";
      const frame = rows => `<div class="cm-menu" role="listbox">${rows}<div class="cm-foot">${hint}</div></div>`;
      const dir = d => `<span class="dir">${d}</span>`;
      if (v === "slash") return frame(row("/compact", "Free up context by summarizing the conversation", "[focus]", true) + row("/review", "Review the pending changes", "") + row("/init", "Write a CLAUDE.md for this project", "") + row("/pr-comments", "Read the comments on a pull request", "[number]") + row("/security-review", "Check the changes for security problems", "") + row("/release-notes", "Draft notes from merged pull requests", ""));
      if (v === "slashfilter") return frame(row("/review", "Review the pending changes", "", true) + row("/security-review", "Check the changes for security problems", "") + row("/pr-comments", "Read the comments on a pull request", "[number]"));
      if (v === "atmenu") return frame(row("login.rs", dir("src/auth/"), "", true) + row("login_form.ts", dir("web/src/"), "") + row("logging.rs", dir("src/"), "") + row("log.md", dir("docs/"), ""));
      return "";
    })();
    const slashIn = v === "slash" ? "/" : v === "slashfilter" ? "/rev" : v === "atmenu" ? "Why does @log" : "";
    const pasteIn = `Why does this fail? <span class="cx-chip${v === "pastepreview" ? " on" : ""}" data-dact="dset:ags:${v === "pastepreview" ? "pasted" : "pastepreview"}">Pasted text <small>· 42 lines</small></span>`;
    const pastePop = v === "pastepreview" ? `<div class="cx-pop"><div class="h"><b>Pasted text</b><span>42 lines</span><span class="sp"></span><button class="dbtn quiet" data-toast="The full text is in the field now">Expand</button><button class="dbtn quiet warn" data-toast="Removed">Remove</button></div><pre>error[E0308]: mismatched types
  --&gt; src/login.rs:48:20
   |
48 |     let user = find(&amp;email)?;
   |                ^^^^^^^^^^^^ expected \`User\`, found \`Option&lt;User&gt;\`
   |
note: return type of find is Option&lt;User&gt;</pre></div>` : "";
    // The media pane's states: docked right, at the bottom, folded to a strip.
    const media = ["paneright", "panegone", "panebottom", "panepdf", "panecollapsed"].includes(v);
    // The Files tab (the session's worktree), a file opened from it, the
    // drag of a path into the field, a Finder drop asking where to upload,
    // and a terminal tab in the session's folder.
    const fpane = ["files", "filemulti", "filedelete", "filemenu", "filemenuwith", "filetext", "filefilter", "dragpath", "dropchoice", "term", "termsplit", "dragtab"].includes(v);
    // A reply that names image files links them and shows each as a
    // thumbnail; a click opens it in the side panel. Images the agent only
    // looked at stay small and dim inside the folded work.
    const shots = v === "shots" || v === "shotpanel" || v === "shotgone";
    // Pages and documents the agent published (an Artifact call), or wrote
    // outside its checkout (.html, .md, .svg, images): the Artifacts tab
    // lists them and shows one. Repo files it edits stay in Files.
    const arts = v === "artifacts";
    const disk = v === "disksubs";
    const image = v === "image", asks = v === "asks", ended = v === "ended", outside = ["outside", "outsideconfirm", "outsidewait"].includes(v) || disk;
    const kids = v === "agentkids", subs = v === "subagents" || disk || kids, child = v === "subchild" || v === "agentchild" || v === "agentstopped", modelOpen = v === "model" || v === "flshmodel";
    const done = ["nochange", "editorerror", "held", "connecting", "fromsession", "idle", "stepsopen", "failed", "failedopen", "answered", "ended", "outside", "outsideconfirm", "disksubs", "image", "shots", "shotgone", "shotpanel", "beeptools", "richdone", "richerror", "running", "runstop"].includes(v) || media || fpane || arts;
    const state = asks ? tag("ask", "Waiting for you") : ended ? tag("", "Ended") : done ? tag("", "Idle") : stopping ? tag("run", "Stopping") : tag("run", "Working");
    const you = `<div class="dcoderen user"><b>You</b><span>Fix the failing login test<br>Keep the other tests passing<details style="margin:4px 0;border:1px solid var(--d-border);border-radius:6px"><summary style="cursor:pointer;padding:3px 8px;font-size:12px;color:var(--d-muted)">Pasted text · 8 lines</summary><pre style="margin:0;padding:6px 8px;font:11.5px/16px var(--mono,monospace);white-space:pre-wrap;border-top:1px solid var(--d-border)">FAIL auth.spec.ts:42
expected 200, got 401</pre></details></span></div>`;
    const head = (label, open, clickable, act) => { const m = label.match(/^([^<]*)(?:<span class="stats">· (.*)<\/span>)?$/); return dHead(m[1], { stats: m[2] || "", when: "17:14", act: clickable ? `dset:ags:${act || (open ? "idle" : "stepsopen")}` : "", open }); };
    const folded = (line, act = "stepsopen") => `<button class="fold" data-dact="dset:ags:${act}"><span class="mk">${SG.fold}</span><span>${line}</span><span class="gt">›</span></button>`;
    // The first turn, finished. Its failed steps fold with the rest; opened,
    // each is one line until clicked.
    const failing = v === "failed" || v === "failedopen", stepsOpen = v === "stepsopen" || v === "failedopen" || v === "beeptools";
    const failedStep = step("bad", "Ran cargo clippy -p widget-core", v === "failedopen"
      ? { exit: 101, open: true, act: "failed", out: "error: unused variable: `token`\n  --> src/login.rs:42:9\n   |\n42 |     let token = read_token();\n   |         ^^^^^ help: prefix it with an underscore", all: 37 }
      : { exit: 101, act: "failedopen" });
    const steps = [
      step("ok", "Ran cargo test -p widget-core login", { open: v === "stepsopen", out: v === "stepsopen" ? "test login::accepts_good_password ... ok\ntest login::rejects_bad_password ... FAILED\nassertion failed: left == right\n  left: \"Wrong password\"\n test result: FAILED. 3 passed; 1 failed" : "", all: v === "stepsopen" ? 48 : 0 }),
      step("ok", "Read src/login.rs"),
      ...(failing ? [step("bad", "Ran cd /path/to/work; git ls-files | grep -n \"Wrong password\" tests/ …", { exit: 1, tip: "cd /path/to/work; git ls-files | grep -n &quot;Wrong password&quot; tests/\n  | head -20" })] : []),
      step("ok", "Searched for \"Wrong password\""),
      step("ok", "Edited src/login.rs"),
      step("ok", "Edited tests/login.rs"),
      ...(failing ? [failedStep] : []),
      step("ok", "Ran cargo test -p widget-core"),
    ].join("");
    // Calls to Beep's own tools: the machine, the command or prompt, the file count and the status, not mcp__beep__run_on.
    const BEEP_IC = DI.tray;
    const bstep = (mk, verb, o = {}) => `<div class="st beep${mk === "bad" ? " bad" : ""}${o.open ? " open" : ""}"${o.act ? ` data-dact="dset:ags:${o.act}"` : ""}><span class="mk">${mk === "run" ? '<span class="dspin"></span>' : mk === "bad" ? SG.bad : SG.ok}</span><span class="tt"${o.tip ? ` title="${o.tip}"` : ""}><span class="bi">${BEEP_IC}</span><span class="v">${verb}</span>${o.m ? `<span class="mc">${o.m}</span>` : ""}${o.d ? `<code>${o.d}</code>` : ""}</span><span class="bs">${[o.files, mk === "run" ? "Running" : mk === "bad" ? "Failed" : "Done"].filter(Boolean).join(" · ")}</span>${o.out ? `<pre class="out">${o.out}</pre>` : ""}</div>`;
    const beepSteps = [
      bstep("ok", "Listed machines", { open: true, out: "build-box · online · Linux · Docker, GPU · claude, codex\nmac-mini · online · macOS · Xcode · claude\nwin-box · offline" }),
      bstep("ok", "Synced", { d: "widget-core", m: "build-box" }),
      bstep("ok", "Sent files to", { m: "build-box", files: "3 files", tip: "fixtures/login.json, fixtures/users.csv, .env.test" }),
      bstep("ok", "Ran on", { m: "build-box", d: "cargo test -p widget-core --release" }),
      bstep("ok", "Started codex on", { m: "mac-mini", d: "Build the iOS app and run the login UI tests" }),
      bstep("ok", "Checked job", { d: "job-7f2c" }),
      bstep("bad", "Ran on", { m: "win-box", d: "cargo test", tip: "win-box is offline" }),
      bstep("ok", "Asked you", { d: "Push the fix to main?" }),
    ].join("");
    const answered = `<div class="qa"><span class="q">Which error text should the login screen show?</span><span class="a">Use "Wrong email or password", it matches the web app.</span></div>`;
    const note = `<div class="note">Switched to model-small from the next turn</div>`;
    const picture = image ? `<div class="pics"><i class="img" style="width:160px;height:96px"></i></div>` : "";
    // Video, 3D model and PDF files a reply names link the same way; each opens in its own tab.
    const mlink = (f, tab) => `<a class="imgl" data-dact="dset:ags:${tab}">${f}</a>`;
    const answer = media
      ? `<div class="dcoderen agent md">The turntable renders cleanly now. The model is ${mlink("out/turntable.glb", "paneright")}, the render ${mlink("out/walkcycle.mp4", "panebottom")} and the contact sheet ${mlink("out/contact-sheet.pdf", "panepdf")}.<div class="shots"><i class="img big b2" data-dact="dset:ags:shotpanel"></i></div></div>`
      : v === "shotgone"
      ? `<div class="dcoderen agent md"><p>Latest screenshot:</p><span class="agwarn" title="Sign-in screen">The file is no longer on disk. Unable to load image.</span></div>`
      : shots
      ? `<div class="dcoderen agent md">The button no longer overlaps the label. Before and after are in <a class="imgl" data-dact="dset:ags:shotpanel">shots/login-before.png</a> and <a class="imgl" data-dact="dset:ags:shotpanel">shots/login-after.png</a>.<p>Latest screenshot:</p><p><img class="img big" src="assets/inline-image.svg" alt="Sign-in screen" data-dact="dset:ags:shotpanel" width="240" height="160"></p></div>`
      : v === "beeptools"
      ? `<div class="dcoderen agent md">The release build passes on build-box, and the iOS UI tests are running on mac-mini (job-7f2c). win-box is offline, so the Windows run waits.</div>`
      : arts
      ? `<div class="dcoderen agent md">The usage report is in <a class="imgl" data-dact="dset:ags:artifacts">reports/usage.html</a>, published as an artifact. The notes are in <a class="imgl" data-dact="dset:ags:artifacts">reports/NOTES.md</a>.</div>`
      : `<div class="dcoderen agent md">${v === "warning" ? '<p class="agwarn">Warning: Falling back to the HTTPS transport. The stream disconnected before completion.</p>' : ""}Done. The login test passes, and 2 files changed. I did not touch the other tests.</div>`;
    // A plain start opens on the prompt: Beep made no branch, so there is no "Started on a new branch" line.
    // The heading is the fold: how long it worked and what it did, on one
    // line that opens the steps. Opened, it counts the calls instead.
    const workLine = v === "failed" ? "Ran 4 commands (2 failed), edited 2 files, performed 2 other actions" : shots ? "Ran 2 commands, edited 2 files, viewed 2 images, performed 2 other actions" : "Ran 2 commands, edited 2 files, performed 2 other actions";
    const turn1 = [you, dHead("Worked for 1m 12s", { when: "17:14" }),
      dThink({ secs: 4, open: !!S.d.agprethought, act: `dset:agprethought:${S.d.agprethought ? "" : "1"}`, text: "I will check the failing assertion before changing the fixture." }),
      dHead("Ran 1 command", { act: `dset:agpretools:${S.d.agpretools ? "" : "1"}`, open: !!S.d.agpretools }),
      S.d.agpretools ? dRows(step("ok", "Ran cargo test -p widget-core login")) : "",
      `<div class="dcoderen agent md">The test expects the old error message. I will update the fixture and run the suite again.</div>`,
      head(`${stepsOpen ? "6 tool calls · 2 files changed" : workLine}`, stepsOpen, true),
      stepsOpen ? `<div class="${v === "beeptools" ? "steps" : "tools"}">${v === "beeptools" ? beepSteps : steps}</div>` : "",
      ...(shots ? [`<div class="shots small"><i class="img"></i><i class="img b2"></i></div>`] : []),
      ...(v === "answered" ? [answered] : []), picture,
      ...(disk ? [`<div class="subgrp"><div class="h">${SG.sub}<b>2 subagents</b><span>1 working · 1 done</span></div>${SUBS_DISK.map(subRow).join("")}</div>`] : []), answer].join("");
    // The running turn: earlier steps folded, the newest one, the thought's line.
    const group = kids ? `<div class="subgrp"><div class="h">${SG.sub}<b>2 agents started</b><span>1 working · 1 asks you</span></div>${KIDS.map(kidRow).join("")}</div>` : `<div class="subgrp"><div class="h">${SG.sub}<b>3 subagents</b><span>2 working · 1 done</span></div>${SUBS.map(subRow).join("")}</div>`;
    // Thoughts: each its own line in order with the steps. "Thinking" sweeps
    // while it streams, with its newest line faded beside it; done, it reads
    // "Thought for 12s". A click opens the text (Markdown) in a bordered block.
    const tk = (live, open, label, act, text) => `<div class="dthink${live ? " live" : ""}${open ? " open" : ""}"><button data-dact="dset:ags:${act}" aria-expanded="${open}"><span class="lb">${label}</span>${live ? `<span class="ln">Checking why the error text changed</span>` : ""}<span class="cv">${SG.chev}</span></button><div class="bd"><div><div class="tx">${text}</div></div></div></div>`;
    const T1 = "<p>The test expects <code>Wrong password</code> but the screen now says <code>Wrong email or password</code>.</p><p>Either the copy changed on purpose or the fixture is stale. I'll check <b>src/errors.rs</b> first, then the fixtures.</p>";
    const T2 = "<p>Checking why the error text changed. The last commit touched <code>LoginError::Display</code>, so the suite needs a rerun.</p>";
    const thinkV = ["thinking", "thoughts", "thoughtopen"].includes(v);
    const live = done ? "" : [
      `<div class="dcoderen user"><b>You</b><span>${subs ? "Check every place that shows this error" : "Now run the whole suite"}</span></div>`,
      head(`${asks ? "Waiting for you · 48s" : "Working for 48s"}${thinkV ? "" : '<span class="stats">· Ran 2 commands, read 3 files</span>'}`, false, !thinkV),
      thinkV ? tk(false, v === "thoughtopen", "Thought for 12s", v === "thoughtopen" ? "thoughts" : "thoughtopen", T1) : "",
      thinkV ? dHead("Read 1 file", { act: `dset:agpretools:${S.d.agpretools ? "" : "1"}`, open: !!S.d.agpretools }) + (S.d.agpretools ? dRows(step("ok", "Read src/errors.rs")) : "") : "",
      thinkV && v !== "thinking" ? tk(false, false, "Thought for 4s", "thoughtopen", T2) + dHead("Ran 1 command", { act: "dset:agpretools:1" }) : "",
      subs ? group : thinkV && v !== "thinking" ? dRows(step("run", "Running cargo test -p widget-core")) : thinkV ? "" : dRows(step("run", asks ? "Run rm -rf target && cargo build --release" : "Running cargo test -p widget-core")),
      v === "thinking" ? dThink({ live: true, text: T2 }) : "",
      dLive(asks ? "Waiting for you" : v === "thinking" ? "Thinking" : "Working", "48s", { doing: asks || subs || thinkV ? "" : "Checking why the error text changed" }),
      v === "steered" ? `<div class="dcoderen user md"><b>You</b><div class="said"><p>Use the fixture in tests/data, not a new one</p></div></div>` : "",
    ].join("");
    const kidView = `<div class="crumb"><button data-dact="dset:ags:agentkids">Fix the failing login test</button><span>›</span><b>Build the iOS target</b><span class="sp"></span><button class="dbtn" data-dact="dset:ags:agentkids">Open parent<kbd>esc</kbd></button></div>
      <div class="note">Started by Fix the failing login test on build-box. Codex on mac-box, in widget.</div>
      <div class="dcoderen user"><b>Task</b><span>Build the iOS app and run the login UI tests</span></div>
      ${head("Working for 2m", false, false)}<div class="tools">${[step("ok", "Ran xcodebuild -scheme Widget build"), step("run", "Ran xcodebuild test -scheme WidgetUITests")].join("")}</div>
      <div class="dcoderen agent md">Allow <code>xcodebuild test</code> on mac-box? <button class="dbtn small pri" data-toast="Allowed">Allow</button> <button class="dbtn small quiet" data-toast="Denied">Deny</button></div>
      <div class="note">Only you can answer this: the parent cannot.</div>`;
    // Stopped from its row or its own log: the log says who stopped it, and
    // the parent gets a note that the user stopped it.
    const kidStopped = `<div class="crumb"><button data-dact="dset:ags:agentkids">Fix the failing login test</button><span>›</span><b>Build the iOS target</b><span class="sp"></span><button class="dbtn" data-dact="dset:ags:agentkids">Open parent<kbd>esc</kbd></button></div>
      <div class="note">Started by Fix the failing login test on build-box. Codex on mac-box, in widget.</div>
      <div class="dcoderen user"><b>Task</b><span>Build the iOS app and run the login UI tests</span></div>
      ${head("Worked for 2m", false, false)}<div class="tools">${[step("ok", "Ran xcodebuild -scheme Widget build")].join("")}</div>
      <div class="note">Stopped by you</div>`;
    // ----- The transcript as rows -----
    // Each tool call is one line: an icon for its kind, the verb, its target
    // (a path or the command), a detail on the right (+2 -1, a line range,
    // exit 1) and its status: a spinner, then a check or a cross. A click
    // opens it: a command's output in a mono box that scrolls, an edit as
    // its changed lines, a read as the path and range. Calls of one kind in
    // a row fold into one line ("Read 6 files") that opens to them. The turn
    // heading counts the calls and the files changed. Thoughts stay in order.
    const rich = ["rich", "richopen", "richcmd", "richgroup", "richdone", "richerror", "richjump", "richthink", "richthought"].includes(v);
    const RI = { ...AG_IC, chev: AG_IC.next };
    // A row: kind, verb, target, detail on the right, status; `body` when opened.
    const rw = (k, verb, tg, o = {}) => `<div class="dcoderen tool row${o.open ? " open" : ""}${o.st === "bad" ? " bad" : ""}" data-dact="dset:ags:${o.act || (v === "richopen" ? "rich" : "richopen")}"><span class="ki">${RI[k]}</span><span class="vb">${verb}</span><code class="tt" title="${tg}">${o.open && k === "run" ? "" : tg}</code>${o.mt ? `<span class="rm">${o.mt}</span>` : ""}<span class="mk">${o.st === "run" ? '<span class="dspin"></span>' : o.st === "bad" ? RI.bad : RI.ok}</span>${o.open && (o.body || k === "run") ? `<div class="bdy">${k === "run" ? `<pre class="cmd">${tg}</pre>` : ""}${o.body || ""}</div>` : ""}</div>`;
    const plusMinus = (a, r) => `<span class="pl">+${a}</span> <span class="mi">-${r}</span>`;
    const diffBox = `<div class="df"><div class="dh">src/invites/limits.rs</div><div class="dc"> fn daily_limit(user: &amp;User) -&gt; u32 {</div><div class="dc">     match user.plan {</div><div class="dr">-        Plan::Free =&gt; 3,</div><div class="da">+        Plan::Free =&gt; 5,</div><div class="da">+        Plan::Trial =&gt; 5,</div><div class="dc">         Plan::Pro =&gt; 50,</div></div>`;
    const outBox = (txt) => `<pre class="box">${txt}</pre>`;
    const testOut = "running 14 tests\ntest invites::create ... ok\ntest invites::limit_free ... ok\ntest invites::limit_trial ... ok\ntest delivery::retry_counts_once ... FAILED\n\nfailures:\n---- delivery::retry_counts_once ----\nassertion `left == right` failed\n  left: 2\n right: 1\n\ntest result: FAILED. 13 passed; 1 failed";
    const reads = ["src/invites/limits.rs", "src/invites/create.rs", "src/delivery/queue.rs", "src/delivery/retry.rs", "tests/invites.rs", "tests/delivery.rs"];
    const groupOpen = v === "richgroup";
    const readRun = `<div class="dtrun${groupOpen ? " open" : ""}"><button data-dact="dset:ags:${groupOpen ? "rich" : "richgroup"}"><span class="ki">${RI.read}</span><span class="vb">Read 6 files</span><span class="sm">limits.rs, create.rs, queue.rs and 3 more</span><span class="cv">${RI.chev}</span></button>${groupOpen ? `<div class="in">${reads.map((r, i) => rw("read", "Read", r, { mt: ["L1-88", "L1-140", "L1-210", "L30-96", "L1-64", "L1-120"][i], act: "richgroup" })).join("")}</div>` : ""}</div>`;
    const opened = v === "richopen";
    const LONG_CMD = "/bin/sh -lc 'ls /srv/project/generated_images/2026-11/batch-07/previews/first-pass | tail -n 40 &amp;&amp; du -sh /srv/project/generated_images/2026-11/batch-07'";
    const longOut = "preview-038.png\npreview-039.png\npreview-040.png\n212M\t/srv/project/generated_images/2026-11/batch-07";
    const cmdOpen = v === "richcmd";
    const cmdRun = `<div class="dtrun${cmdOpen ? " open" : ""}"><button data-dact="dset:ags:${cmdOpen ? "rich" : "richcmd"}"><span class="ki">${RI.run}</span><span class="vb">Ran 3 commands</span><span class="sm">git status, just lint and 1 more</span><span class="cv">${RI.chev}</span></button>${cmdOpen ? `<div class="in">${rw("run", "Ran", "git status --short", { act: "richcmd" })}${rw("run", "Ran", "just lint", { act: "richcmd" })}${rw("run", "Ran", LONG_CMD, { open: true, act: "rich", body: outBox(longOut) })}</div>` : ""}</div>`;
    const errorCard = `<div class="errc"><b>The turn did not finish</b><span>claude exited with code 1 after 41s. The last command was still running.</span></div>`;
    const doneRich = v === "richdone" || v === "richerror";
    const richTurn = [
      `<div class="dcoderen user"><b>You</b><span>Why do invites stop after 3 a day?</span></div>`,
      dHead(doneRich ? "Worked for 2m 14s" : "Working for 48s", { stats: doneRich ? "12 tool calls · 3 files changed" : "9 tool calls · 1 file changed", when: "17:14", act: "dset:ags:rich", open: true }),
      `<div class="tools"><div class="dthink"><button data-dact="dset:ags:thoughtopen" aria-expanded="false"><span class="lb">Thought for 6s</span><span class="cv">${SG.chev}</span></button></div>`,
      readRun,
      cmdRun,
      rw("search", "Searched", "\"daily_limit\" in src/", { mt: "4 results" }),
      rw("edit", "Edited", "src/invites/limits.rs", { mt: plusMinus(2, 1), open: opened, body: diffBox }),
      rw("run", "Ran", "cargo test -p invites", { mt: '<span class="mi">exit 101</span>', st: "bad", open: opened, body: `<pre class="cmd"><span class="k">python3</span> - &lt;&lt;<span class="s">'PY'</span>\n<span class="k">import</span> json\n<span class="c"># read the report</span>\nprint(<span class="s">"failed"</span>, <span class="n">1</span>)\nPY</pre>` + outBox(testOut) }),
      rw("web", "Fetched", "docs.rs/tokio/latest/tokio/time", { mt: "200 · 14 KB" }),
      rw("mcp", "linear", "get_issue INV-142", { mt: "MCP" }),
      doneRich ? rw("run", "Ran", "cargo test -p invites", { mt: "14 passed" }) : rw("run", "Running", "cargo test -p invites delivery", { st: "run", mt: "12s" }),
      doneRich ? "" : v === "richthink"
        ? `<div class="dthink open"><button aria-expanded="true"><span class="lb">Thought</span><span class="cv">${SG.chev}</span></button><div class="bd"><div><div class="tx"><div class="kk" data-kkdemo></div></div></div></div></div>`
        : v === "richthought"
        ? `<div class="dthink open" data-folddemo><button data-dact="dset:ags:richthink" aria-expanded="false"><span class="lb">Thought for 9s</span><span class="cv">${SG.chev}</span></button><div class="bd"><div><div class="tx"><p><b>Checking the retry path</b></p><p>The retry path calls count() before it knows the send failed, so a retried invite counts twice.</p></div></div></div></div>`
        : dThink({ live: true }),
      v === "richthink" ? dLive("Thinking", "48s") : "",
      "</div>",
      v === "richerror" ? errorCard : "",
      doneRich && v !== "richerror" ? `<div class="dcoderen agent md"><p>The delivery queue counted <b>retries as new invites</b>, so three sends with one retry each hit the limit early.</p><ul><li>Count an invite once, in <code>create</code>.</li><li>The free limit is 5, as the pricing page says.</li></ul><pre class="code"><button class="cp" data-toast="Copied">Copy</button><code>fn count(&amp;self, invite: &amp;Invite) {\n    if invite.attempt == 0 { self.sent += 1 }\n}</code></pre><table><tr><th>Plan</th><th>Before</th><th>After</th></tr><tr><td>Free</td><td>3</td><td>5</td></tr><tr><td>Pro</td><td>50</td><td>50</td></tr></table></div>` : "",
    ].join("");
    const noChange = `<div class="dcoderen user"><b>You</b><span>which custom agents do we have in this folder?</span></div>${head('Worked for 8s<span class="stats">· Searched once</span>', false, true, "nochange")}<div class="dcoderen agent md"><p>None. This folder has no <code>.claude/agents/</code> directory, and no <code>AGENTS.md</code> either.</p><p>The types available here are all built-in or global: <code>claude</code>, <code>Explore</code>, <code>general-purpose</code>, <code>Plan</code> and <code>statusline-setup</code>.</p></div>`;
    // What Beep told the agent (its brief, the user's standing rules) is a
    // muted row folded to "Beep context"; what another session sent is its
    // own row under that session's name. Neither is in the user's box.
    const ctx = `<details class="dcoderen beepctx"><summary>Beep context</summary><pre>Beep: the beep MCP server's tools reach the user's other machines. list_machines says which machines there are...\n\nBeep: the user's standing rules:\n- Use the render box for Blender.</pre></details>`;
    const fromSession = [
      ctx,
      `<div class="dcoderen fromsession"><b>Other session wrote<span class="who">Fix the failing login test</span></b><span>Build the iOS app and run the login UI tests</span></div>`,
      head("Worked for 2m", false, true, "fromsession"),
      `<div class="dcoderen agent md"><p>Built and ran <code>WidgetUITests</code>: 12 passed.</p></div>`,
      `<div class="dcoderen fromsession"><b>Other session wrote<span class="who">Fix the failing login test</span></b><span>Now run them on the iPad simulator too.</span></div>`,
      head("Worked for 1m 10s", false, true, "fromsession"),
      `<div class="dcoderen agent md"><p>Same 12 pass on the iPad.</p></div>`,
      ctx,
      `<div class="dcoderen user"><b>You</b><span>ok, let's keep going</span></div>`,
      head("Working for 8s", false, false),
    ].join("");
    // Messages sent while the machine is unreachable (a node restarting for an
    // update) wait on this device, in the order typed, and go once it answers.
    // The field stays empty; Edit puts one back in it. A refused one keeps its reason.
    const held = noChange;
    // Opened from what this device kept, before build-box answers: one muted
    // line under the log, gone once the first live read or event arrives.
    const connecting = `${noChange}<div class="dmuted">Connecting to build-box...</div>`;
    const log = v === "fromsession" ? fromSession : v === "nochange" ? noChange : v === "held" ? held : v === "connecting" ? connecting : rich ? richTurn : v === "agentchild" ? kidView : v === "agentstopped" ? kidStopped : turn1 + live;
    const pm = x => `<span style="width:16px;display:grid;place-items:center">${x}</span>`;
    const plan = done || child ? "" : `<div class="sec">Plan</div><div class="plan">
        <div class="row">${pm(`<span class="dok">${DI.check || "&#10003;"}</span>`)}<span>Read the failing output</span></div>
        <div class="row">${pm(asks ? '<span class="ddot" style="background:var(--d-coral)"></span>' : '<span class="dspin"></span>')}<span>Fix the error text</span></div>
        <div class="row">${pm('<span class="ddot off"></span>')}<span style="color:var(--d-muted)">Run the whole suite</span></div></div>`;
    const q = asks ? `<div class="q"><b style="font-weight:600">Run this command?</b><code style="font-family:var(--mono);font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere">rm -rf target &amp;&amp; cargo build --release</code>
        <div class="dups"><button class="dbtn pri" data-toast="Allowed">Allow<kbd>⌘${DI.ret}</kbd></button><button class="dbtn" data-toast="Allowed for this session">Allow for this session</button><button class="dbtn warn" data-toast="Denied">Deny<kbd>⌘D</kbd></button></div></div>` : "";
    // The card sits under the log, next to the field: the question, the plan.
    const card = q || plan ? `<div class="dcard dcoder">${q}${plan}<div style="height:12px"></div></div>` : "";
    const qatt = (p) => `<span class="ag-qatt" title="Goes with it as @${p}">${p}</span>`;
    const outRow = (text, why) => `<div class="ag-queue out"><span class="lbl">Queued</span><span class="t">${text}</span><span class="w">${why}</span><button data-toast="Back in the field to edit">Edit</button><button aria-label="Remove" data-toast="Removed">&#215;</button></div>`;
    const outbox = v === "held" ? outRow("add one that reviews the diff", "waiting for connection") + outRow("and run it on the last commit", "waiting for connection") : "";
    const steerRow = outbox || (v === "steer" ? `<div class="ag-queue steer"><span class="lbl">Steer</span><span class="t">Use the fixture in tests/data, not a new one</span>${qatt(".beep/uploads/build.log")}<span class="w">after the current step</span><button data-toast="Edit message">Edit</button><button data-toast="Not sent">&#215;</button></div>` : "");
    // The Files tab's icons, the tree and the terminal.
    const FI = {
      files: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>',
      file: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/></svg>',
      term: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>',
      code: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 7l-5 5 5 5M16 7l5 5-5 5"/></svg>',
      chev: '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>',
    };
    // A path dragged from the tree is a chip; the message carries it as @path.
    // An upload shows its progress until the file is on the machine.
    const mention = (p, up) => `<span class="ag-ment" title="Goes in the message as @${p}">${FI.file}${p}${up ? `<small>${up}</small>` : ""}<em data-toast="Removed">&#215;</em></span>`;
    const att = image ? `<div class="ag-att"><span><i></i>Screenshot 2026-10-07.png<em data-toast="Removed">&#215;</em></span></div>`
      : v === "dragpath" ? `<div class="ag-att">${mention("src/errors.rs")}<span class="ag-dropin">${FI.file}Drop to mention src/login.rs</span></div>`
      : v === "dropchoice" ? `<div class="ag-att"><div class="ag-upask"><span>Upload 2 files (1.4 MB) to build-box:</span><button class="dbtn pri" data-toast="Uploading to src/auth/">Into src/auth/</button><button class="dbtn" title="Outside the project. Deleted when this session is closed." data-toast="Uploading as temporary attachments">Temporary attachments</button><button class="dbtn quiet" data-toast="Not uploaded">Cancel</button></div></div>`
      : (v === "filemenu" || v === "filemenuwith") ? `<div class="ag-att">${mention("build.log", "62%")}${mention("src/login.rs")}</div>` : "";
    // The model pill: harness, model and thinking; a switch applies from the next turn.
    const canSwitch = ended || !outside;
    const pillAgent = v === "customdefault" ? "codex" : ["flshmodel", "flshusage", "flshunknown"].includes(variant) ? "flsh" : "claude", pillName = pillAgent === "codex" ? "Codex" : pillAgent === "flsh" ? "flsh" : "Claude Code";
    // An ended session keeps the model choice for its next turn.
    // A session without a reported model resolves the configured default.
    const pill = `<span id="agmodel"><button class="agc-pill agm" id="agmodelb"${canSwitch ? ` data-dact="dset:ags:${modelOpen ? "working" : "model"}"` : " disabled"} aria-expanded="${modelOpen}" title="${canSwitch ? "Switch the model or thinking; applies from the next turn" : `${pillName} cannot switch models in a running session`}"><span class="cc-ag ${pillAgent} mark">${(window.__MARKS || HARNESS)[pillAgent]}</span>${pillAgent === "codex" ? "6.1 Sol" : pillAgent === "flsh" ? "openai:gpt-test" : "Opus 4.6"}<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg></button></span>`;
    const menu = modelOpen ? window.__agcPicker({ agent: pillAgent, offered: [pillAgent], machine: "build-box", act: "dset:ags:working" }) : "";
    const stopBtn = done ? "" : stopping ? `<button class="dbtn" id="agstop" disabled><span class="dspin"></span>Stopping</button>` : `<button class="dbtn" id="agstop" data-dact="dset:ags:stopping" aria-label="Stop main turn" title="Stop main turn (subagents keep running)"><span style="width:9px;height:9px;border-radius:2px;background:currentColor"></span>Stop</button>`;
    // Shift+Tab cycles the modes the agent offers; the pill shows the current one, unless it is full access (#430).
    const planning = v === "plan" || v === "planready";
    const modeName = planning ? "Plan" : v === "modeedits" ? "Accept edits" : "Full access";
    const modePill = canSwitch && modeName !== "Full access" ? `<button class="agc-pill agmode${planning ? " on" : ""}" data-dact="dset:ags:${planning ? "modeedits" : v === "modeedits" ? "working" : "plan"}" title="Switch mode (Shift+Tab)">${modeName}</button>` : "";
    const planCard = v === "planready" ? `<div class="ag-plan"><div class="h"><b>Plan ready</b><small>Claude Code waits for you</small></div>
      <div class="pj-md"><ol><li>Add a retry flag to the uploader</li><li>Reuse the backoff from the sync client</li><li>Cover a dropped connection in tests</li></ol></div>
      <div class="acts"><button class="dbtn pri" data-dact="dset:ags:working">Approve and start</button><button class="dbtn" data-dact="dset:ags:plan">Keep planning</button></div></div>` : "";
    const field = ended ? `<div class="dinput small" id="agfield"><span class="txt ph">Message ${pillName} to continue</span>${pill}</div>` : outside
      // Continue here closes the CLI in its terminal after a confirm; while a
      // turn runs it waits for the turn to end, and can be cancelled.
      ? `<div class="dmuted" style="display:flex;align-items:center;gap:8px;padding:10px 16px;border-top:1px solid var(--d-border)"><span style="flex:1">Running outside Beep. Beep follows it here.</span>${v === "outsidewait" ? `<button class="dbtn" disabled><span class="dspin"></span>Waiting for the turn to end</button><button class="dbtn quiet" data-dact="dset:ags:outside">Cancel</button>` : `<button class="dbtn" data-dact="dset:ags:outsideconfirm">Continue here</button>`}</div>${v === "outsideconfirm" ? `<div class="am-scrim" style="position:fixed"><div class="am-sheet" role="dialog" aria-modal="true" aria-label="Continue here?"><div class="am-head"><b>Continue here?</b><kbd>esc</kbd></div>
        <div class="am-msg">Claude Code closes in the terminal it runs in, and the session goes on here.</div><div class="am-sub">If a turn is running, Beep waits for it to end.</div>
        <div class="gp-foot"><button class="dbtn quiet" data-dact="dset:ags:outside">Cancel</button><button class="dbtn warn" data-dact="dset:ags:ended" data-toast="Closed in the terminal">Continue here</button></div></div></div>` : ""}`
      : `${planCard}${v === "working" ? `<div class="agtasks"><button class="agtasksline" data-dact="dset:ags:running" title="Show them in the Running tab"><span class="dspin"></span>2 commands running</button></div>` : ""}<div class="dinput small" id="agfield">${menu}${pastePop}${cmdMenu}<button class="dbtn quiet" aria-label="Attach files" title="Attach files" data-toast="Choose files"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg></button><span class="txt${pasted || slashIn ? " typed" : " ph"}">${pasted ? pasteIn : slashIn ? `${slashIn}<i class="cm-caret"></i>` : image ? "Add a message (optional)" : `Message ${pillName}`}</span>${modePill}${pill}${stopBtn}</div>`;
    // While a turn runs, Enter steers and Cmd-. stops; Esc goes back.
    const keys = cmdMenu ? [["\u2191\u2193", "choose"], ["Tab", "pick"], ["esc", "close"]] : asks ? [[`⌘${DI.ret}`, "allow"], ["⌘D", "deny"], ["esc", "back"]] : child ? [["esc", "parent session"]] : outside ? [["esc", "back"]] : ended ? [[DI.ret, "continue"], ["esc", "back"]]
      : done ? [["⌘U", "thoughts"], ["⌘Y", "commands"], ["esc", "back"]]
      : stopping ? [["esc", "back"]]
      : [["⇧Tab", "mode"], ["⌘.", "stop"], ["esc", "back"]];
    const slim = `<div class="dhints slim">${keys.map(([k, t]) => `<span><kbd>${k}</kbd>${t}</span>`).join("")}<details class="ag-shortcuts"><summary>Show more</summary><div><span><kbd>⌘1</kbd>chat</span><span><kbd>⌘2</kbd>files</span><span><kbd>⌘3</kbd>changes</span><span><kbd>⌘4</kbd>artifacts</span><span><kbd>⇧↩</kbd>new line</span><span><kbd>⌘U</kbd>thoughts</span><span><kbd>⌘Y</kbd>commands</span></div></details></div>`;
    const why = "";
    // The media pane beside or under the conversation. Each file the agent
    // made or looked at opens as a tab, next to Subagents while there are
    // any. The header switches the dock (right or bottom) and folds the
    // pane; the divider drags to resize and double-clicks back to the
    // default. Dock and size are kept per window.
    const PI = {
      right: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/></svg>',
      bottom: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 14h18"/></svg>',
      fold: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>',
      foldb: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>',
      image: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="M21 17l-5-5-8 8"/></svg>',
      model: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 2l9 5v10l-9 5-9-5V7z"/><path d="M12 22V12M3 7l9 5 9-5"/></svg>',
      video: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="3" y="5" width="13" height="14" rx="2"/><path d="M16 10l5-3v10l-5-3"/></svg>',
      pdf: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/></svg>',
    };
    const DIFFI = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 4v8M8 8h8M8 18h8"/></svg>';
    const tabOn = { files: "files", filemulti: "files", filedelete: "files", filemenu: "files", filemenuwith: "files", filefilter: "files", dragpath: "files", dropchoice: "files", filetext: "text", term: "term", termsplit: "files", dragtab: "term", shotpanel: "image", artifacts: "arts", running: "run", runstop: "run", subagents: "subs", disksubs: "subs", paneright: "model", panegone: "model", panebottom: "video", panepdf: "pdf", panecollapsed: "model" }[v] || "changes";
    const AI = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18M9 21V9"/></svg>';
    const fixed = [["files", FI.files, "Files"], ["changes", DIFFI, "Changes"]];
    const RUNI = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 17l6-6-6-6M12 19h8"/></svg>';
    const runTab = v === "running" || v === "runstop";
    const tabs = [...fixed, ...(runTab ? [["run", RUNI, "Running", v === "runstop" ? "3" : "4"]] : []), ...(arts ? [["arts", AI, "Artifacts", "3"]] : fpane ? [["text", FI.code, "login.rs"], ...(v === "termsplit" ? [] : [["term", FI.term, "fix-login 1"]])] : subs ? [["subs", SG.sub, "Subagents", disk ? "2" : "3"]] : v === "shotpanel" ? [["image", PI.image, "login-after.png"]]
      : media ? [["image", PI.image, "login-after.png"], ["model", PI.model, "turntable.glb"], ["video", PI.video, "walkcycle.mp4"], ["pdf", PI.pdf, "contact-sheet.pdf"]] : [])];
    const tabAct = { changes: v === "nochange" ? "nochange" : "idle", arts: "artifacts", run: "running", files: "files", text: "filetext", term: "term", image: "shotpanel", model: "paneright", video: "panebottom", pdf: "panepdf", subs: "subagents" };
    const bottom = v === "panebottom";
    // A wireframe stand-in for the model: the real pane draws it with three.js.
    const modelSvg = `<svg viewBox="0 0 240 180" class="ag-3d" aria-label="turntable.glb"><defs><radialGradient id="agfloor" cx="50%" cy="50%" r="50%"><stop offset="0" stop-color="#2C2F45" stop-opacity=".7"/><stop offset="1" stop-color="#2C2F45" stop-opacity="0"/></radialGradient></defs>
      <ellipse cx="120" cy="146" rx="86" ry="16" fill="url(#agfloor)"/><g fill="none" stroke="#6FA8DC" stroke-width="1.2" stroke-linejoin="round"><path d="M120 30l48 26v56l-48 26-48-26V56z" fill="rgba(111,168,220,.12)"/><path d="M120 86l48-30M120 86l-48-30M120 86v52"/><path d="M96 43l48 26v56M144 43l-48 26v56" opacity=".35"/></g></svg>`;
    // The tree: folders load as they open; git marks (M changed, A added,
    // U untracked) roll up to their folder; ignored files hide unless shown.
    const row = (d, name, o = {}) => `<div class="ag-tr${o.on ? " on" : ""}${o.drop ? " drop" : ""}${o.ign ? " ign" : ""}" style="--d:${d}" draggable="true" data-dact="dset:ags:${o.dir ? "files" : "filetext"}" title="Drag into the message to mention it">${o.dir ? `<span class="tw${o.open ? " open" : ""}">${FI.chev}</span>${FI.files}` : `<span class="tw"></span>${FI.file}`}<span class="nm">${name}</span>${o.git ? `<span class="gm ${o.git}">${o.git}</span>` : ""}</div>`;
    const tree = v === "filefilter" ? [row(0, "assets", { dir: true, open: true }), row(1, "models", { dir: true, open: true }), row(2, "turntable.glb"), row(2, "walkcycle.glb"), `<div class="ag-tr note" style="--d:0"><span class="dspin"></span>Reading folders…</div>`].join("") : [row(0, "src", { dir: true, open: true, git: "M" }), row(1, "auth", { dir: true, drop: v === "dropchoice" }),
      row(1, "errors.rs"), row(1, "login.rs", { git: "M", on: v === "filemenu" || v === "filemenuwith" || v === "filemulti" || v === "filedelete" || v === "dragpath" }), row(1, "main.rs", { on: v === "filemulti" || v === "filedelete" }), row(0, "tests", { dir: true, git: "U" }),
      row(0, "Cargo.toml"), row(0, "README.md")].join("");
    const withApps = ["Zed (default)", "Preview", "TextEdit", "Visual Studio Code"];
    // File refreshes update tree rows while the open menu keeps its target and focus.
    const menuF = (v === "filemenu" || v === "filemenuwith") ? `<div class="ag-ctx" style="left:120px;top:16px;max-height:100%;overflow-y:auto">${["Open in viewer", "Mention in message", "-", "Copy path", "Copy relative path", "Copy", "Cut", "Paste into folder", "Rename...", "-", "New file...", "New folder...", "Refresh", "-", "Save to this Mac", "Delete permanently...", "-", "Open", "Open With", "Reveal in Finder"].map(t => t === "-" ? "<hr>" : `<button data-toast="${t}"${t === "Open With" ? ' style="display:flex;justify-content:space-between;gap:16px"' : ""}>${t}${t === "Open With" ? "<span>&gt;</span>" : ""}</button>`).join("")}</div>${v === "filemenuwith" ? `<div class="ag-ctx" style="left:300px;top:216px">${withApps.map(t => `<button data-toast="Opened in ${t}">${t}</button>`).join("")}</div>` : ""}` : "";
    const code = [["use", " crate::errors::LoginError;"], ["", ""], ["pub fn", " login(user: &amp;str, pass: &amp;str) -&gt; Result&lt;Session, LoginError&gt; {"], ["    if", " !check(user, pass) {"], ["        return", ' Err(LoginError::new(<span class="s">"Wrong email or password"</span>));'], ["", "    }"], ["", "    Ok(Session::new(user))"], ["", "}"]];
    const body = {
      files: () => `<div class="ag-fbar"><input class="ag-ffind" placeholder="Filter files" aria-label="Filter files"${v === "filefilter" ? ' value="glb"' : ""}><label class="ag-fign"><input type="checkbox"> Show ignored</label><button class="ag-pbtn" data-toast="Read the folders again" title="Read the folders again">&#8635;</button></div><div class="ag-pbody list ag-tree${v === "dropchoice" ? " dropping" : ""}">${tree}${menuF}${v === "filedelete" ? `<div class="ag-ctx" role="dialog" aria-label="Delete 2 items?" style="left:16px;top:64px"><b>Delete 2 items permanently?</b><p>Folders include everything inside. This cannot be undone.</p><button data-dact="dset:ags:filemulti">Cancel</button><button data-toast="Deleted 2 items">Delete 2 items</button></div>` : ""}</div><div class="ag-pacts"><span class="meta mono">${v === "filemulti" || v === "filedelete" ? "2 selected" : "/srv/project/.worktrees/fix-login"}</span><span class="sp"></span><span class="hint">Cmd-click to select, Shift-click for a range. Cmd+Delete asks before deleting.</span></div>`,
      text: () => `<div class="ag-pbody ag-code"><pre>${code.map(([k, r], i) => `<span class="ln">${i + 1}</span>${k ? `<span class="k">${k}</span>` : ""}${r}`).join("\n")}</pre></div><div class="ag-pacts"><span class="meta mono" title="src/login.rs">src/login.rs · Rust · 8 lines · 290 B</span><span class="sp"></span><button class="dbtn" data-toast="Goes in the message as @src/login.rs">Mention</button><button class="dbtn" data-toast="Saved to Downloads">Save</button></div>`,
      term: () => `<div class="ag-pbody ag-term"><pre><span class="p">fix-login $</span> cargo test -p widget-core login\n    Finished test profile in 3.2s\ntest login::accepts_good_password ... ok\ntest login::rejects_bad_password ... ok\n\n<span class="p">fix-login $</span> <i class="cur"></i></pre></div><div class="ag-pacts"><span class="meta mono">build-box · what you type goes there</span><span class="sp"></span><button class="dbtn warn" data-toast="Closed the terminal">Close</button></div>`,
      arts: () => {
        // #365: a file a command made (a script, a build) is found at the end of its turn; a file
        // that is gone stays listed, greyed, as Removed, its claude.ai link still there.
        const kinds = { html: "HTML", md: "Markdown", svg: "SVG", image: "Image", pdf: "PDF" };
        const list = [["usage.html", "reports/usage.html", "html", "17:21", 2, true, true, "2 versions"], ["coverage.pdf", "target/coverage.pdf", "pdf", "17:20", 2, false, false, "Made by a command"], ["NOTES.md", "reports/NOTES.md", "md", "17:19", 2, false, true, ""], ["flow.svg", "/tmp/agent-out/flow.svg", "svg", "17:12", 1, false, false, "Removed", true]];
        const rows = list.map(([n, p, k, t, turn, on, url, more, gone]) => `<button class="ag-art${on ? " on" : ""}"${gone ? ' style="opacity:.5"' : ""} data-toast="${gone ? `${n} is not on build-box any more` : `Shows ${n}`}" title="${p}"><span class="k ${k}">${kinds[k]}</span><span class="t"><b>${n}</b><small>${p}</small></span><span class="r">${url ? '<span class="pub">claude.ai</span>' : ""}<span>Turn ${turn} · ${t}</span>${more ? `<small>${more}</small>` : ""}</span></button>`).join("");
        const page = `<div class="ag-artpage"><div class="hd"><b>Usage, last 30 days</b><span>Updated 17:21</span></div><div class="kp"><span><small>Requests</small><b>48,210</b></span><span><small>Errors</small><b>0.4%</b></span><span><small>p95</small><b>212 ms</b></span></div><div class="ch" title="chart.png, loaded from beside the page">${[38, 52, 47, 61, 58, 72, 66, 80, 74, 88, 83, 91].map(x => `<i style="height:${x}%"></i>`).join("")}</div><small style="color:#6b6f76;font-size:11px">Figure: chart.png</small></div>`;
        const fold = !!S.d.artfold;
        return `<div class="ag-artlist">${rows}</div><div class="ag-pbody ag-artview"${fold ? " hidden" : ""} title="A sandboxed frame: scripts run, no Beep access, no navigation. Images, CSS and scripts beside the page load from its folder.">${page}</div><div class="ag-pacts"><button class="dbtn" data-artfold aria-expanded="${!fold}" title="Fold or show the preview">${fold ? "Show" : "Fold"}</button><span class="meta mono" title="/srv/project/reports/usage.html">reports/usage.html · HTML · 18 KB</span><span class="sp"></span><button class="dbtn" data-toast="Opens claude.ai in the browser">Open on claude.ai</button><button class="dbtn" data-dact="dset:ags:files">Open file</button><button class="dbtn" data-toast="Copied /srv/project/reports/usage.html">Copy path</button></div>`;
      },
      run: () => {
        // #402: what the agent left running, read from its process tree (its group and what left it).
        const procs = [["npm run dev", "next dev", "3000", "21:41", "group"], ["/usr/bin/node /srv/project/node_modules/typescript/bin/tsc --watch --project tsconfig.build.json --preserveWatchOutput", "", "", "21:44", "left the group"]].filter((_, i) => v !== "runstop" || i === 0);
        const rows = procs.map(([cmd, sub, port, at, how]) => `<div class="ag-art" style="cursor:default"><span class="t"><b class="mono" title="Click to show the whole command" style="font-family:var(--mono);font-size:12px;cursor:pointer;${cmd.length > 40 ? "white-space:pre-wrap;overflow-wrap:anywhere" : ""}">${cmd}</b><small>${[sub, `since ${at}`, how === "group" ? "" : how].filter(Boolean).join(" · ")}</small></span><span class="r" style="display:flex;gap:6px;align-items:center">${port ? `<a href="#" data-toast="Opens localhost:${port}">:${port}</a>` : ""}<button class="dbtn small" data-toast="Stopped ${cmd}">Stop</button></span></div>`).join("");
        // #567: background commands the agent started list here with their Stop; the log keeps one line.
        const tasks = ["npm run dev -- --port 4173", "cargo watch -x check"].map(cmd => `<div class="ag-art" style="cursor:default"><span class="t"><b class="mono" style="font-family:var(--mono);font-size:12px">${cmd}</b><small>Background command</small></span><span class="r" style="display:flex;gap:6px;align-items:center"><button class="dbtn small" data-toast="Asked the agent to stop ${cmd}">Stop</button></span></div>`).join("");
        return `<div class="ag-pbody list">${tasks}${rows}</div><div class="ag-pacts"><span class="meta">build-box · closing the session stops these</span><span class="sp"></span><button class="dbtn small warn" data-toast="Stopped all">Stop all</button></div>`;
      },
      changes: () => window.__dvBody(v === "nochange" ? "empty" : "mixed", false),
      subs:() => `<div class="ag-pbody list">${(disk ? SUBS_DISK : SUBS).map(r => r[2] === "run" ? subRow(r) : subRow(r).replace("</span></button>", `</span><em class="x" title="Dismiss from pane" data-toast="Dismissed from pane">${SG.close}</em></button>`)).join("")}</div>`,
      image: () => `<div class="ag-pbody"><i class="img fit b2"></i></div><div class="ag-pacts"><button class="dbtn on">Fit</button><button class="dbtn" data-toast="Actual size, scrolls">Actual size</button><span class="zoom">100%</span><span class="meta">1280×800 · 240 KB</span><span class="sp"></span><span class="hint">Pinch or Ctrl + scroll to zoom, swipe with two fingers or drag to pan</span><button class="dbtn" data-toast="Opens it in Preview">Open in Preview</button><button class="dbtn" data-toast="Saved to Downloads">Save</button></div>`,
      model: () => v === "panegone" ? `<div class="ag-pbody"><div class="derr" style="padding:16px;text-align:center">File gone, or build-box is offline.</div></div>` : `<div class="ag-pbody stage">${modelSvg}</div><div class="ag-pacts ag-anim"><button class="dbtn ibtn" data-toast="Paused" aria-label="Pause" title="Pause"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 5v14M16 5v14"/></svg></button><select aria-label="Clip"><option>Turntable</option><option>Walk</option><option>Idle</option></select><input type="range" min="0" max="4" step="0.01" value="1.2" aria-label="Time"><span class="time">0:01.2 / 0:04.0</span><button class="dbtn ibtn on" aria-pressed="true" data-toast="Plays once and holds the last pose" aria-label="Loop" title="Loop"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 2l4 4-4 4"/><path d="M3 11V9a3 3 0 013-3h15"/><path d="M7 22l-4-4 4-4"/><path d="M21 13v2a3 3 0 01-3 3H3"/></svg></button></div><div class="ag-pacts"><button class="dbtn" data-toast="Frames the whole model again">Reset view</button><button class="dbtn" data-toast="Shows the edges">Wireframe</button><span class="meta">GLB · 12,480 triangles · 3 clips · 3.1 MB</span><span class="sp"></span><span class="hint">Drag to orbit, right-drag to pan, wheel to zoom</span><button class="dbtn" data-toast="Opens it on this Mac">Open</button><button class="dbtn" data-toast="Saved to Downloads">Save</button></div>`,
      video: () => `<div class="ag-pbody stage"><div class="ag-video"><span class="play">&#9654;</span><div class="bar"><span class="t">0:04 / 0:12</span><span class="track"><i style="width:33%"></i></span></div></div></div><div class="ag-pacts"><span class="meta">0:12 · 1920×1080 · 4.2 MB</span><span class="sp"></span><button class="dbtn" data-toast="Opens in QuickTime">Open in QuickTime</button><button class="dbtn" data-toast="Saved to Downloads">Save</button></div>`,
      pdf: () => `<div class="ag-pbody stage"><div class="ag-pdf"><b>Contact sheet</b>${'<i></i>'.repeat(6)}</div></div><div class="ag-pacts"><span class="meta">PDF · 820 KB</span><span class="sp"></span><button class="dbtn" data-toast="Opens in Preview">Open in Preview</button><button class="dbtn" data-toast="Saved to Downloads">Save</button></div>`,
    };
    const tabBtns = tabs.map(([k, ic, nm, n]) => `<button class="ag-tab${k === tabOn ? " on" : ""}${v === "dragtab" && k === "term" ? " dragging" : ""}" data-dact="dset:ags:${tabAct[k]}" title="${nm}">${ic}<span>${nm}</span>${n ? `<small>${n}</small>` : ""}${k === "subs" || k === "arts" || k === "files" || k === "changes" ? "" : `<em data-toast="Closed the tab">&#215;</em>`}</button>`).join("");
    const dockBtn = bottom ? `<button class="ag-pbtn" data-dact="dset:ags:paneright" title="Dock these tabs on the right">${PI.right}</button>` : `<button class="ag-pbtn" data-dact="dset:ags:panebottom" title="Dock these tabs at the bottom">${PI.bottom}</button>`;
    const pane = v === "panecollapsed"
      ? `<button class="ag-pstrip" data-dact="dset:ags:paneright" title="Show the pane">${PI.model}<span>turntable.glb</span><small>6</small></button>`
      : `<div class="ag-div" title="Drag to resize, double-click to reset" data-toast="Drag to resize, double-click to reset"></div><aside class="ag-pane"><div class="ag-pane-content"><div class="ag-phead"><div class="ag-tabs">${tabBtns}</div><span class="ag-wtw"><button class="ag-pbtn ag-wt" aria-haspopup="menu" title="The worktree Files and Changes show" data-dact="dset:ags:wtmenu"><code>${v === "subchild" ? "sub-bench" : "fix-login"}</code><span aria-hidden="true">&#9662;</span></button><!--wtmenu--></span><button class="ag-pbtn ag-addterm" data-dact="dset:ags:term" title="New terminal in this folder (&#8963;\`)">+ Terminal</button>${dockBtn}<button class="ag-pbtn" data-dact="dset:ags:panecollapsed" title="Hide the pane">${bottom ? PI.foldb : PI.fold}</button></div>${body[tabOn]()}</div></aside>`;
    // Each tab drags to the bottom or right edge of the session and docks
    // there on its own; both docks show, each resizable. Dragging the last
    // tab out of a dock removes it. Where each tab sits is kept per window.
    const split = v === "termsplit", wrap = split || v === "dragtab";
    const under = split ? `<div class="ag-div" title="Drag to resize, double-click to reset" data-toast="Drag to resize, double-click to reset"></div><aside class="ag-pane"><div class="ag-pane-content"><div class="ag-phead"><div class="ag-tabs"><button class="ag-tab on" title="fix-login 1">${FI.term}<span>fix-login 1</span><em data-toast="Closed the tab">&#215;</em></button></div><button class="ag-pbtn ag-addterm" data-toast="New terminal under the conversation" title="New terminal in this folder (&#8963;\`)">+ Terminal</button><button class="ag-pbtn" data-dact="dset:ags:term" title="Dock on the right">${PI.right}</button><button class="ag-pbtn" data-toast="Folded to a strip" title="Hide the pane">${PI.foldb}</button></div>${body.term()}</div></aside>`
      : v === "dragtab" ? `<div class="ag-dropzone">${PI.bottom}Dock at the bottom</div><span class="ag-tab on ag-tabghost">${FI.term}<span>fix-login 1</span></span>` : "";
    const usage = outside ? "" : variant === "flshunknown" ? "42%/1.05M · 182k tokens · $1.27 / ?" : pillAgent === "flsh" ? "42%/1.05M · 182k tokens · $1.27 / $7.25" : v === "nochange" ? "12%/200k · 9.4k tokens · $0.04" : "42%/200k · 182k tokens · $1.27";
    const folder = v === "nochange" ? ["/srv/project/sdk", "sdk"] : ["/srv/project/.worktrees/fix-login", "widget · fix-login"];
    return ccWin(ccSidePj("agents"), `<div class="cc-top"><h2>Coding agents</h2><small>Follow it and answer it here</small><span class="tools"></span></div>
      <div class="dhead">${pillAgent === "flsh" ? `<span class="cc-ag flsh mark">${window.__MARKS.flsh}</span>` : av("claude")}<span class="tt"><b><button class="agproj" data-dgo="dProjects">acme/api</button></b><small title="${folder[0]}">build-box</small></span><small id="aguse" class="aguse" title="Context used / capacity · session tokens · session spend / remaining credits">${usage}</small><span id="agstate">${state}</span><span class="agedit"><button class="dbtn quiet small" title="Open in Zed (⌘O)" data-toast="Opens /srv/project in Zed">Open in Zed</button><button class="dbtn quiet small" style="padding:0 6px" aria-label="Other editors" title="Open in VS Code" data-toast="Opens /srv/project in VS Code">${DI.chevD}</button></span></div>
      ${v === "editorerror" ? '<div class="dsaved bad" role="status"><span>Could not open Zed: The application could not be launched. Try opening Zed directly.</span></div>' : ""}
      <div class="ag-shell${bottom ? " dock-bottom" : ""}">${wrap ? '<div class="ag-main">' : ""}<div class="ag-conv"><div class="dbody" id="agbody"><div class="dagmain">${why}<div class="dcoderlog">${log}</div>${card}</div>${v === "richjump" ? `<button class="ag-jump" data-dact="dset:ags:rich">Jump to latest<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M6 13l6 6 6-6"/></svg></button>` : ""}</div>
      ${steerRow}${att}${field}${slim}</div>${wrap ? `${under}</div>` : ""}${pane}</div>`);
  };
})();
