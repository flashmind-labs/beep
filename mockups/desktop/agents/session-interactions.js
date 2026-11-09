(() => {
  const { AGK, AG_ICON, HARNESS, av, tag, pick, variants, hint, HOSTS, SG, step, SUBS, SUBS_DISK, subRow, KIDS, kidRow } = AGENT_MOCK;
  // The streaming demo: text arrives in bursts, as the app reads it every
  // 0.3s while a thought is open. The ended one folds a moment after it shows.
  const KK_TEXT = "**Checking the retry path**\n\nThe test fails with left 2, right 1, so one invite is counted twice. The delivery queue retries a send after a timeout. If count() runs before the send result is known, a retry counts again.\n\nLooking at src/delivery/retry.rs: count() is called at the top of send(). Moving it into create() counts each invite once, whatever the retries do. The free limit check then reads the right number.";
  if (!window.__kkDemo) window.__kkDemo = setInterval(() => {
    for (const el of document.querySelectorAll(".kk[data-kkdemo]:not([data-on])")) {
      el.dataset.on = "1"; el.dataset.held = 0;
      const plain = KK_TEXT.replace(/\*\*/g, "");
      let got = 0;
      // The app reads the session every 0.3s: a burst of text per read.
      const read = () => {
        if (!el.isConnected) return;
        // Held whole for a few reads, then it starts over.
        if (got >= plain.length && ++el.dataset.held > 15) { got = 0; el.dataset.held = 0; }
        got = Math.min(plain.length, got + 8 + Math.floor(Math.random() * 12));
        el.textContent = plain.slice(0, got);
        el.classList.toggle("ov", el.scrollHeight > el.clientHeight + 1);
        el.scrollTop = el.scrollHeight;
        setTimeout(read, 300);
      };
      read();
    }
    for (const el of document.querySelectorAll(".dthink[data-folddemo]:not([data-on])")) {
      el.dataset.on = "1";
      setTimeout(() => el.classList.remove("open"), 900);
    }
  }, 250);
  VARIANTS.dAgentSession = variants("ags", [["Nothing changed", "nochange"], ["Message waits for its machine", "held"], ["From another session", "fromsession"], ["Working", "working"], ["Steer pending", "steer"], ["Steer delivered", "steered"], ["Stopping", "stopping"], ["Asks you", "asks"], ["Done", "idle"], ["Warning in a reply", "warning"], ["Done, steps open", "stepsopen"], ["Failed steps", "failed"], ["Failed step opened", "failedopen"], ["Subagents", "subagents"], ["Subagent opened", "subchild"], ["Agents started on other machines", "agentkids"], ["Started agent opened", "agentchild"], ["Started agent stopped by you", "agentstopped"], ["Answered question", "answered"], ["Model menu", "model"], ["flsh model and thinking", "flshmodel"], ["flsh session usage", "flshusage"], ["flsh balance unavailable", "flshunknown"], ["Plan mode", "plan"], ["Plan ready", "planready"], ["Accept edits mode", "modeedits"], ["Ended", "ended"], ["Started outside Beep", "outside"], ["Read from disk, subagents", "disksubs"], ["Image", "image"], ["Reply with images", "shots"], ["Image file deleted", "shotgone"], ["Image in the pane", "shotpanel"], ["Media pane, docked right", "paneright"], ["Media pane, file not read", "panegone"], ["Media pane, docked bottom", "panebottom"], ["Media pane, PDF", "panepdf"], ["Media pane, folded", "panecollapsed"], ["Files", "files"], ["Files, multiple selected", "filemulti"], ["Files, delete confirmation", "filedelete"], ["Files, right-click", "filemenu"], ["Files, right-click, Open With", "filemenuwith"], ["File opened", "filetext"], ["Dragging a path in", "dragpath"], ["Filter reads the whole tree", "filefilter"], ["Finder drop, where to", "dropchoice"], ["Terminal tab", "term"], ["Terminal docked at the bottom", "termsplit"], ["Dragging a tab to a dock", "dragtab"], ["Long paste as a pill", "pasted"], ["Slash menu", "slash"], ["Slash menu, filtered", "slashfilter"], ["File menu", "atmenu"], ["Pasted text preview", "pastepreview"], ["Artifacts", "artifacts"], ["Running commands", "running"], ["Running, one stopped", "runstop"], ["Beep tools", "beeptools"], ["Thinking", "thinking"], ["Thoughts, folded", "thoughts"], ["Thought opened", "thoughtopen"], ["Rows, working", "rich"], ["Rows opened: diff and output", "richopen"], ["Rows, a run of reads opened", "richgroup"], ["Rows, a run of commands, one opened to its whole command", "richcmd"], ["Rows, done with Markdown", "richdone"], ["Rows, error card", "richerror"], ["Rows, scrolled up", "richjump"], ["Thought streaming (karaoke)", "richthink"], ["Thought ended, folds", "richthought"]], "nochange");

  // Shift+Esc zooms the side pane over the conversation; the pane keeps its tab and state, and Shift+Esc puts it back.
  // Review with agent (mock-feedback): a button in the diff header, always there, apart from Send review. Its
  // dropdown is the start form's picker; picking a model starts a read-only review session on the machine and in the
  // checkout the diff comes from. While it works the button shows a spinner and opens that session, which is listed
  // under the session whose changes it reviews, as a subagent, and never with the running sessions. When the turn
  // ends, its findings become draft comments marked with the harness icon, which brings Send review up; a line the
  // diff does not have becomes a note on the file. An answer that is not a list shows as one note with a link to the
  // session. Without a machine that has the checkout online the button is disabled and says why.
  {
    const orig = D.dAgentSession;
    document.head.insertAdjacentHTML("beforeend", '<link rel="stylesheet" href="agents/zoom.css">');
    D.dAgentSession = () => {
      // View diff lands here: the branch's session with Changes zoomed. Esc goes to its conversation.
      if (S.d.ags === "diffzoom") {
        S.d.ags = "idle";
        try { return orig().replace('<div class="ag-shell', '<div class="ag-shell zoomed').replace(/(<kbd>esc<\/kbd>)back/, "$1conversation"); } finally { S.d.ags = "diffzoom"; }
      }
      // The worktree dropdown: the pane follows the agent until a worktree is picked by hand.
      if (S.d.ags === "wtmenu") {
        S.d.ags = "idle";
        try {
          return orig().replace("<!--wtmenu-->", `<div class="ag-wtm" role="menu"><button role="menuitemcheckbox" aria-checked="${S.d.wtfollow !== "off"}" data-wtfollow><b>Follow the agent</b></button><hr><input type="search" class="ag-wtsearch" placeholder="Search worktrees" aria-label="Search worktrees">
            <button role="menuitem" class="on" data-dact="dset:ags:idle"><code>fix-login</code><small>.worktrees/fix-login &#183; agent here</small></button>
            <button role="menuitem" data-dact="dset:ags:idle"><code>main</code><small>the session's folder</small></button>
            <button role="menuitem" data-dact="dset:ags:idle"><code>login-copy</code><small>.worktrees/login-copy</small></button></div>`);
        } finally { S.d.ags = "wtmenu"; }
      }
      // The session's worktree is gone: Changes shows the default branch of the session's folder, quietly, with the dropdown open.
      if (S.d.ags === "wtgone") {
        S.d.ags = "idle";
        try {
          return orig().replace("<!--wtmenu-->", `<div class="ag-wtm" role="menu"><button role="menuitemcheckbox" aria-checked="${S.d.wtfollow !== "off"}" data-wtfollow><b>Follow the agent</b></button><hr><input type="search" class="ag-wtsearch" placeholder="Search worktrees" aria-label="Search worktrees">
            <button role="menuitem" class="on" data-dact="dset:ags:wtgone"><code>master</code><small>the session's folder</small></button>
            <button role="menuitem" data-dact="dset:ags:wtgone"><code>login-copy</code><small>.worktrees/login-copy</small></button></div>`)
            .replace('<code>fix-login</code><span aria-hidden', '<code>master</code><span aria-hidden')
            .replace(/(<div class="ag-pbody)/, '<div class="dmuted" style="padding:8px 12px;font-size:12px">The session\'s worktree was removed. Showing master.</div>$1');
        } finally { S.d.ags = "wtgone"; }
      }
      // Files the turn made: one folded row, opened to chips grouped by kind (issue 388).
      if (S.d.ags === "made" || S.d.ags === "madeopen") {
        const st = S.d.ags, open = st === "madeopen";
        S.d.ags = "idle";
        try {
          const imgs = ["handoff-passkey-1280.png", "confirm-passkey-1280.png", "api-keys-passkey-1280.png", "api-keys-passkey-390.png", "sign-in-passkey-1280.png", "sign-in-passkey-390.png", "sign-in-email-1280.png", "sign-in-email-390.png"];
          const docs = ["playwright-ci.md", "CLAUDE.md", "LOGIN.md"];
          const chip = n => `<button class="mf-chip" title="Opens ${n} in Artifacts" data-toast="Opens ${n} in the Artifacts tab">${n}</button>`;
          const row = `<div class="mf${open ? " open" : ""}"><button class="mf-head" data-dact="dset:ags:${open ? "made" : "madeopen"}"><span class="mf-car">${open ? "▾" : "›"}</span>Made ${imgs.length + docs.length} files<small>${imgs.length} images, ${docs.length} documents</small></button>${open ? `<div class="mf-grp"><small>Images</small><div class="mf-chips">${imgs.map(chip).join("")}</div></div><div class="mf-grp"><small>Documents</small><div class="mf-chips">${docs.map(chip).join("")}</div></div>` : ""}</div>`;
          return orig().replace('<div class="dcoderlog">', `<div class="dcoderlog">${row}`);
        } finally { S.d.ags = st; }
      }
      // Comments on lines of the diff: a draft that goes to the agent as one message.
      if (S.d.ags === "diffcomment") {
        S.d.ags = "idle";
        try {
          return orig().replace('<div class="ag-shell', '<div class="ag-shell zoomed').replace(/(<kbd>esc<\/kbd>)back/, "$1conversation")
            .replace('<i class="dv-acts"></i>', `<button class="dbtn pri small" data-toast="Sent to the agent as one message, each comment with its file and line">Send review<span class="dv-n">2</span></button>`)
            .replace(/(<div class="dv-l add">.*?<\/div>)/, `$1<div class="dv-cmt"><div class="b">Use the shared limit here, not a new constant.</div><div class="a"><button data-toast="Back in the box to edit">Edit</button><button data-toast="Removed">&#215;</button></div></div><div class="dv-cmt open"><textarea rows="2" placeholder="Comment on this line" aria-label="Comment on this line"></textarea><div class="a"><button class="dbtn small pri" data-toast="Added to the draft">Add</button><button class="dbtn small quiet" data-toast="Dropped">Cancel</button></div></div>`)
            .replace(/<span class="dv-facts">/g, `<span class="dv-facts"><button class="dv-btn" title="Comment on this file" data-toast="Opens a comment on the whole file">Comment</button>`)
            .replace(/(<section class="dv-file(?:(?!<section)[\s\S])*?dv-chip deleted(?:(?!<section)[\s\S])*?)(<div class="dv-fold">)/, `$1<div class="dv-cmt"><div class="b">Why did this go? The iOS app still reads it.</div><div class="a"><button data-toast="Back in the box to edit">Edit</button><button data-toast="Removed">&#215;</button></div></div>$2`);
        } finally { S.d.ags = "diffcomment"; }
      }
      // Review with agent: the button, and what each state of it shows (issue 286).
      if (/^fb/.test(S.d.ags)) {
        const st = S.d.ags;
        S.d.ags = "idle";
        try {
          const mark = `<span class="cc-ag claude mark">${window.__claudeMark || ""}</span>`;
          const caret = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';
          const by = `<span class="by" title="Written by Claude Code">${mark}Claude Code</span>`;
          const note = (n, extra) => `<div class="dv-cmt"><div class="b">${n}</div><div class="a"><button data-toast="Back in the box to edit">Edit</button><button data-toast="Removed">&#215;</button></div></div>${extra || ""}`;
          const btn = st === "fbwork"
            ? `<span class="dv-fbw"><button class="dv-btn dv-fb working" title="Open the review session" data-toast="Opens the review session"><span class="dspin"></span>Reviewing <small>Claude Code</small></button></span>`
            : st === "fbnone"
              ? `<span class="dv-fbw"><button class="dv-btn dv-fb" disabled title="No machine with this folder is online">Review with agent</button></span>`
              : `<span class="dv-fbw"><button class="dv-btn dv-fb" aria-expanded="${st === "fbpick"}" title="Ask an agent to review these changes; it only reads" ${st === "fbpick" ? 'data-dact="dset:ags:fbwork"' : 'data-dact="dset:ags:fbpick"'}>Review with agent${caret}</button>${st === "fbpick" ? window.__agcPicker({ agent: "claude", offered: ["claude", "codex"], machine: "build-box", act: "dset:ags:fbwork" }) : ""}</span>`;
          const cmts = st === "fbdrafts" ? `<button class="dbtn pri small" data-toast="Sent to the agent as one message, each comment with its file and line">Send review<span class="dv-n">3</span></button>` : "";
          let html = orig().replace('<div class="ag-shell', '<div class="ag-shell zoomed').replace(/(<kbd>esc<\/kbd>)back/, "$1conversation")
            .replace('<i class="dv-acts"></i>', `${btn}${cmts}`);
          if (st === "fbdrafts") {
            html = html
              .replace(/(<div class="dv-l add">.*?<\/div>)/, `$1<div class="dv-cmt"><div class="b">${by}The retry never resets the counter, so a second failure waits ten times as long as it should.</div><div class="a"><button data-toast="Back in the box to edit">Edit</button><button data-toast="Removed">&#215;</button></div></div>`)
              .replace(/(<section class="dv-file(?:(?!<section)[\s\S])*?dv-chip deleted(?:(?!<section)[\s\S])*?)(<div class="dv-fold">)/, `$1<div class="dv-cmt"><div class="b">${by}Line 212: the iOS app still reads this file, and nothing here replaces it.</div><div class="a"><button data-toast="Back in the box to edit">Edit</button><button data-toast="Removed">&#215;</button></div></div>$2`)
              .replace(/(<div class="dv-files"[^>]*>)/, `$1<div class="dv-cmt"><div class="b">${by}Not on a line of the changes: the new limit is never checked when the file is empty.</div><div class="a"><button data-toast="Back in the box to edit">Edit</button><button data-toast="Removed">&#215;</button></div></div>`);
          }
          if (st === "fbfail") {
            html = html.replace(/(<div class="dv-body)/, `<div class="dv-fbnote" role="status"><b>Claude Code answered without a list of comments</b><pre>I read the diff and the retry change looks right. One thing to check is the backoff when the first call fails, which I could not run here.</pre><div class="a"><button class="dv-btn" data-toast="Opens the review session">Open the session</button><button class="dv-btn" data-toast="Dismissed">Dismiss</button></div></div>$1`);
          }
          return html;
        } finally { S.d.ags = st; }
      }
      if (S.d.ags !== "panezoom") return orig();
      S.d.ags = "paneright";
      try { return orig().replace('<div class="ag-shell', '<div class="ag-shell zoomed'); } finally { S.d.ags = "panezoom"; }
    };
    VARIANTS.dAgentSession = VARIANTS.dAgentSession.map(([l, f, on]) => [l, f, () => S.d.ags !== "panezoom" && S.d.ags !== "diffzoom" && S.d.ags !== "diffcomment" && !/^fb/.test(S.d.ags) && S.d.ags !== "wtmenu" && S.d.ags !== "wtgone" && on()]);
    const at = VARIANTS.dAgentSession.findIndex(x => x[0] === "Media pane, docked right");
    VARIANTS.dAgentSession.splice(at + 1, 0, ["Side pane zoomed (Shift+Esc)", () => { S.d.ags = "panezoom"; render(); }, () => S.d.ags === "panezoom"], ["Changes zoomed (View diff)", () => { S.d.ags = "diffzoom"; render(); }, () => S.d.ags === "diffzoom"], ["Made files, folded", () => { S.d.ags = "made"; render(); }, () => S.d.ags === "made"], ["Made files, open", () => { S.d.ags = "madeopen"; render(); }, () => S.d.ags === "madeopen"], ["Comments on the diff", () => { S.d.ags = "diffcomment"; render(); }, () => S.d.ags === "diffcomment"], ["Review with agent: picker open", () => { S.d.ags = "fbpick"; render(); }, () => S.d.ags === "fbpick"], ["Review with agent: working", () => { S.d.ags = "fbwork"; render(); }, () => S.d.ags === "fbwork"], ["Review with agent: draft comments", () => { S.d.ags = "fbdrafts"; render(); }, () => S.d.ags === "fbdrafts"], ["Review with agent: could not read its answer", () => { S.d.ags = "fbfail"; render(); }, () => S.d.ags === "fbfail"], ["Review with agent: no machine", () => { S.d.ags = "fbnone"; render(); }, () => S.d.ags === "fbnone"], ["Worktree dropdown", () => { S.d.ags = "wtmenu"; render(); }, () => S.d.ags === "wtmenu"], ["Worktree removed", () => { S.d.ags = "wtgone"; render(); }, () => S.d.ags === "wtgone"]);
  }


  // ----- New session: the agent view with an empty conversation (issue 289) -----
  // Cmd+N in a project opens it. The layout is a session's: header, conversation, side pane. The project's
  // centered start composer has the project preset, with machine, checkout and harness controls.
  // Cmd+T adds a terminal tab to the pane, on the machine and checkout the input points to; the terminal takes
  // the keys. Esc in it goes back to the input, unless a program such as vim or less owns the screen. Shift+Esc
  // makes the focused tab fill the view. Sending starts the session in place: the pane and its terminals stay.
  {
    document.head.insertAdjacentHTML("beforeend", '<link rel="stylesheet" href="agents/empty.css">');
    const base = D.dAgentSession;
    D.dAgentSession = () => {
      const st = S.d.ags;
      if (!/^draft/.test(st)) return base();
      const term = st !== "draft";
      S.d.ags = term ? "term" : "idle";
      let out;
      try { out = base(); } finally { S.d.ags = st; }
      // Use the new-agent composer with its project already selected.
      const previous = S.d.agc;
      const template = document.createElement("template");
      S.d.agc = "ready";
      try { template.innerHTML = D.dAgentNew(); } finally { S.d.agc = previous; }
      const composer = template.content.querySelector(".agc");
      if (!composer) throw new Error("new-agent composer not found");
      const project = composer.querySelector('[title="The project this agent works on"]');
      project?.nextElementSibling?.remove();
      project?.remove();
      const prompt = composer.querySelector(".pr");
      prompt.classList.add("ph");
      prompt.textContent = "Describe the task.";
      const form = composer.outerHTML;
      const keys = [["⌘T", "terminal"], ["⌘2", "files"], ["esc", "back"]];
      const slim = `<div class="dhints slim">${keys.map(([k, t], i) => `<span${i >= 3 ? ' class="lo"' : ""}><kbd>${k}</kbd>${t}</span>`).join("")}</div>`;
      const conv = `<div class="ag-conv"><div class="dbody" id="agbody"><div class="draft-start">${form}</div></div>${slim}</div>`;
      const head = `<div class="dhead"><span class="tt"><b><button class="agproj" data-dgo="dWinProject">acme/widget</button></b><small title="/srv/project">build-box</small></span><span id="agstate"></span><button class="dbtn quiet" title="Open in Zed (⌘O)" data-toast="Opens ~/Projects/widget in Zed">Open in Zed</button></div>`;
      out = out.replace(/<div class="dhead">[\s\S]*?<\/div>/, head);
      out = out.replace(/<div class="ag-conv">[\s\S]*?<\/div>(?=<div class="ag-div")/, conv);
      out = out.replace("<code>fix-login</code>", "<code>main</code>").replace(/fix-login \$/g, "widget $");
      if (term) {
        // Two terminal tabs, the new one shown; the login.rs tab of the base state goes.
        out = out.replace(/<button class="ag-tab[^"]*" data-dact="dset:ags:filetext" title="login.rs">[\s\S]*?<\/button>/, "");
        const tab = out.match(/<button class="ag-tab on"[^>]*title="fix-login 1">[\s\S]*?<\/button>/);
        if (!tab) throw new Error("terminal tab not found");
        const second = tab[0].replace('class="ag-tab on"', 'class="ag-tab"').replace(/fix-login 1/g, "widget 2");
        out = out.replace(tab[0], tab[0].replace(/fix-login 1/g, "widget 1") + second);
        out = out.replace("what you type goes there", "what you type goes there · esc, back to the input");
      }
      if (st === "draftzoom") out = out.replace('<div class="ag-shell', '<div class="ag-shell zoomed');
      return out;
    };
    VARIANTS.dAgentSession.push(
      ["New session, empty (Cmd+N)", () => { S.d.ags = "draft"; render(); }, () => S.d.ags === "draft"],
      ["New session, terminal tab (Cmd+T)", () => { S.d.ags = "drafterm"; render(); }, () => S.d.ags === "drafterm"],
      ["New session, terminal zoomed (Shift+Esc)", () => { S.d.ags = "draftzoom"; render(); }, () => S.d.ags === "draftzoom"]);
  }

})();
