
// Settings in two panes, as macOS System Settings: the left lists the pages
// (a page's subpages nest under it once it is picked), the right shows only
// the picked page. Subpages (a machine, a coding agent) open in the
// right pane under a breadcrumb with a back link, never in another window.
// The sheet opens on the page picked last.
const CC_SET = (() => {
  const st = document.createElement("style");
  st.textContent = `.mac .cs-sheet { width: min(1040px, 94%); height: 620px; display: grid; grid-template-columns: 218px minmax(0, 1fr); border-radius: 14px; border: 1px solid var(--d-border); background: #151617; box-shadow: 0 20px 60px rgba(0,0,0,.5); overflow: hidden }
.mac .cs-nav { padding: 14px 8px; border-right: 1px solid var(--d-border); display: flex; flex-direction: column; gap: 2px; overflow-y: auto; background: rgba(255,255,255,.012) }
.mac .cs-nav > b { padding: 0 10px 10px; font-size: 15px }
.mac .cs-nav .cc-si.sub { padding-left: 24px; font-size: 12.5px; color: var(--d-muted) }
.mac .cs-nav .cc-si.sub[aria-current="true"] { color: var(--d-fg) }
.mac .cs-nav .cc-si .n { margin-left: auto; font-size: 11px; color: var(--d-dim) }
.mac .cs-body { display: flex; flex-direction: column; min-width: 0; min-height: 0 }
.mac .cs-top { display: flex; align-items: center; gap: 10px; padding: 14px 24px 12px; border-bottom: 1px solid var(--d-border) }
.mac .cs-crumbs { display: flex; align-items: center; gap: 6px; font-size: 15px; min-width: 0 }
.mac .cs-crumbs button { all: unset; cursor: pointer; color: var(--d-muted) }
.mac .cs-crumbs button:hover { color: var(--d-fg) }
.mac .cs-crumbs b { font-weight: 600 }
.mac .cs-crumbs svg { color: var(--d-dim) }
.mac .cs-back { all: unset; cursor: pointer; display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; border-radius: 6px; color: var(--d-muted); border: 1px solid var(--d-border) }
.mac .cs-top small { color: var(--d-muted); font-size: 12px }
.mac .cs-top .tools { margin-left: auto; display: flex; gap: 6px; align-items: center }
.mac .cs-page { padding: 16px 24px 24px; overflow: auto; scrollbar-width: thin; flex: 1; min-height: 0 }
.mac .cs-page .cs-wide { max-width: 760px }
.mac .cs-box { border: 1px solid var(--d-border); border-radius: 8px; overflow: hidden; margin: 6px 0 16px }
.mac .cs-h { font-size: 12px; font-weight: 600; color: var(--d-dim); margin: 14px 0 2px }
.mac .cs-kv { display: grid; grid-template-columns: 160px minmax(0, 1fr); font-size: 13px }
.mac .cs-kv > span { padding: 8px 14px; border-bottom: 1px solid var(--d-border) }
.mac .cs-kv > span:nth-child(odd) { color: var(--d-muted) }
.mac .cs-kv > span:nth-last-child(-n+2) { border-bottom: 0 }
/* Machines list: one table across machines. Line 1 name, spec, beep-node; line 2 a column per
   coding agent (mark, version, sign-in dot); line 3 the other tools, "+N" for the rest. */
.mac .mt { display: grid; grid-template-columns: repeat(var(--ag, 0), max-content) minmax(0, 1fr) auto; border: 1px solid var(--d-border); border-radius: 8px; overflow: hidden; margin: 12px 0 14px }
.mac .mt-r { grid-column: 1 / -1; display: grid; grid-template-columns: subgrid; column-gap: 6px; row-gap: 6px; align-items: center; padding: 10px 14px; border-bottom: 1px solid var(--d-border); cursor: pointer }
.mac .mt-r:last-child { border-bottom: 0 }
.mac .mt-r:hover { background: rgba(255,255,255,.025) }
.mac .mt-1 { grid-column: 1 / -2; display: flex; align-items: center; gap: 8px; min-width: 0; font-size: 13px }
.mac .mt-1 b { font-weight: 600; font-size: 13.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0 }
.mac .mt-1 .seen { font-size: 11.5px; color: var(--d-dim); white-space: nowrap }
.mac .clockoff { font-size: 11.5px; color: var(--d-warn, #e3b450); white-space: nowrap }
.mac .mt-1 .spec { margin-left: auto; font-size: 12px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; font-variant-numeric: tabular-nums }
.mac .mt-nv { flex: none; font-family: var(--mono); font-size: 11px; color: var(--d-muted); border: 1px solid var(--d-border); border-radius: 6px; padding: 0 6px; line-height: 18px; display: inline-flex; gap: 6px; align-items: center }
.mac .mt-nv .up { color: var(--d-coral); font-weight: 600 }
.mac .mt-ch { grid-column: -2; grid-row: 1 / span 3; color: var(--d-muted); display: grid; align-self: center }
.mac .mt-ag { grid-row: 2; display: inline-flex; align-items: center; gap: 6px; padding: 0 7px 0 4px; height: 22px; border: 1px solid var(--d-border); border-radius: 6px; font-family: var(--mono); font-size: 11.5px; font-variant-numeric: tabular-nums; white-space: nowrap }
.mac .mt-ag .cc-ag { width: 15px; height: 15px; border: 0; background-color: transparent; border-radius: 3px; font-size: 7px }
.mac .mt-ag .in, .mac .mt-ag .out { width: 6px; height: 6px; border-radius: 50%; margin-left: auto; flex: none }
.mac .mt-ag .in { background: var(--d-ok) }
.mac .mt-ag .out { background: var(--d-err) }
.mac .mt-3 { grid-row: 3; grid-column: 1 / -2; display: flex; gap: 4px; overflow: hidden; white-space: nowrap }
.mac .mt-t { font-family: var(--mono); font-size: 10.5px; color: var(--d-muted); background: rgba(255,255,255,.04); border-radius: 5px; padding: 0 6px; line-height: 17px }
.mac .cs-tbl { width: 100%; border-collapse: collapse; font-size: 13px }
.mac .cs-tbl th { text-align: left; font-weight: 500; color: var(--d-muted); font-size: 11.5px; padding: 8px 14px; border-bottom: 1px solid var(--d-border) }
.mac .cs-tbl td { padding: 8px 14px; border-bottom: 1px solid var(--d-border) }
.mac .cs-tbl tr:last-child td { border-bottom: 0 }
.mac .cs-tbl .mono { font-family: var(--mono); font-size: 12px }
.mac .cs-sheet .pv { margin: 0 }`;
  document.head.append(st);

  const chev = DI.chevR;
  const backIc = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M15 6l-6 6 6 6"/></svg>';
  const go = k => `data-dact="dset:set:${k === "machines" ? "machines-list" : k}" data-dgo="dWinSettings"`;
  const pages = {
    "machines-list": () => {
      // Fake machines: [name, online, spec, beep-node, behind, agents [agent, version, signed in (null: unknown)], tools, more].
      const COLS = ["claude", "codex", "opencode", "pi", "flsh"];
      const rows = [
        ["mac-box", true, "macOS 26.3 · arm64 · 14 CPU · 36 GB", "0.4.1", true, [["claude", "2.1.312", true], ["codex", "0.162.1", true], ["opencode", "1.3.13", false], ["flsh", "0.41.0", null]], ["gh", "teacli", "docker", "xcodebuild", "adb", "kubectl"], 8],
        ["laptop", true, "macOS 27.0.1 · arm64 · 10 CPU · 24 GB", "0.4.2", false, [["claude", "2.1.317", true], ["codex", "0.166.0", true], ["opencode", "2.2.5", false], ["pi", "0.99.2", null], ["flsh", "0.41.0", null]], ["gh", "teacli", "xcodebuild", "adb", "brew", "git"], 5],
        ["build-box", true, "Ubuntu 26.04.1 LTS · x86_64 · 32 CPU · 125 GB", "0.4.1", true, [["claude", "2.1.316", true], ["codex", "0.166.0", true], ["opencode", "1.19.4", true], ["pi", "0.99.2", null], ["flsh", "0.41.0", null]], ["gh", "teacli", "docker", "adb", "git", "cargo"], 4],
        ["win-box", false, "Windows 11 · x86_64 · 16 CPU · 64 GB", "0.4.1", true, [["claude", "2.1.300", false]], ["git", "node"], 0],
      ];
      const ag = (a, v, s) => `<span class="mt-ag" style="grid-column:${COLS.indexOf(a) + 1}" title="${a} ${v}${s === null ? "" : s ? ", signed in" : ", not signed in"}">${dMacAgTile(a, "cc-ag")}${v}${s === null ? "" : `<span class="${s ? "in" : "out"}"></span>`}</span>`;
      const row = ([n, on, spec, nv, behind, ags, tools, more]) => `<div class="mt-r" ${go("mach-build")}>
        <span class="mt-1"><span class="ddot"${on ? "" : ' style="background:var(--d-dim)"'}></span><b>${n}</b>${on ? "" : '<span class="seen">Seen 3 h ago</span>'}${n === "laptop" ? '<span class="clockoff" title="Sync keeps the newer file when two machines edit one at once, so turn on automatic time sync in Date and Time settings.">Clock is 2 min behind</span>' : ""}<span class="spec">${spec}</span><span class="mt-nv" title="beep-node ${nv}${behind ? ", 0.4.2 is out" : ""}">beep-node ${nv}${behind ? '<span class="up">update</span>' : ""}</span></span>
        <span class="mt-ch">${chev}</span>
        ${ags.map(([a, v, s]) => ag(a, v, s)).join("")}
        <span class="mt-3">${tools.map(t => `<span class="mt-t">${t}</span>`).join("")}${more ? `<span class="mt-t">+${more}</span>` : ""}</span></div>`;
      return `<div class="cs-wide"><div style="display:flex;align-items:center;gap:12px"><span style="font-size:12.5px;color:var(--d-muted);flex:1">${rows.filter(r => r[1]).length} online, ${rows.filter(r => !r[1]).length} offline</span><button class="dbtn" data-dgo="dAddMachine">Add a machine</button></div>
      <div class="mt" style="--ag:${COLS.length}">${rows.map(row).join("")}</div></div>`;
    },
    "mach-build": () => {
      const up = S.d.mup || "";
      const small = 'style="padding:2px 10px;font-size:12px;margin-left:4px"', mono = t => `<span style="font-family:var(--mono);font-size:12px">${t}</span>`;
      const ctl = up === "updating" ? '<span class="dtag ask">Updating…</span>' : up === "restarting" || up === "waiting" ? '<span class="dtag ask">Restarting on 0.4.2…</span>'
        : `<button class="dbtn" ${small} data-dact="dset:mup:updating">${up === "failed" ? "Try again" : "Update"}</button>`;
      const under = up === "failed" ? '<div class="derr">It did not come back on beep-node 0.4.2 yet. Check the machine.</div>'
        : up === "waiting" ? '<div class="dmuted">It restarts when its agent sessions end.</div>' : "";
      const facts = [["Status", '<span class="dtag own">Online</span>'], ["beep-node", `beep-node 0.4.1 <span class="dtag ask">0.4.2 is out</span> ${ctl}`], ["System", "Ubuntu 26.04.1 LTS, x86_64"], ["Hardware", "32 CPUs, 125 GB"], ["Host name", mono("build-box")], ["Home", mono("/path/to")],
        ["Projects in", up === "search" ? `<span style="display:flex;gap:6px;align-items:center"><span class="pv-in" style="flex:1;font-family:var(--mono);font-size:12px">~/Projects</span><button class="dbtn pri" data-dact="dset:mup:" data-toast="Searching again">Save</button><button class="dbtn quiet" data-dact="dset:mup:">Cancel</button></span>` : `${mono("~/Projects")} <button class="dbtn" ${small} data-dact="dset:mup:search">Change</button>`], ["Android emulators", "Pixel_9"]];
      const ver = (t, v) => mono(v);
      const LOG_LINES = ["2026-11-03T09:12:04.118Z   INFO beep_node::link::connection: connection.rs:188 node key a1b2c3", "2026-11-03T09:12:05.402Z   INFO beep_node::link::connection: connection.rs:452 connected", "2026-11-03T09:40:31.077Z   INFO beep_node::agents::session: session.rs:311 session started; agent=claude", "2026-11-03T10:02:48.650Z   WARN beep_node::files::sync: sync.rs:204 rescan took 41 s; folder=/path/to/work", "2026-11-03T10:15:09.233Z  ERROR beep_node::link::connection: connection.rs:530 link lost: timed out", "2026-11-03T10:15:12.871Z   INFO beep_node::link::connection: connection.rs:452 connected"];
      const logBtn = (t, attr) => `<button class="dbtn" ${small} ${attr}>${t}</button>`;
      const logFail = { "log-none": "the node's service is not installed here: there is no beep-node.service", "log-old": "beep-node on build-box is too old to send its log. Update it." };
      const logBody = up === "log" ? `<pre style="margin:6px 0 4px;max-height:280px;overflow:auto;padding:8px 10px;border:1px solid var(--d-border);border-radius:8px;font-family:var(--mono);font-size:11.5px;line-height:1.5;white-space:pre-wrap;overflow-wrap:anywhere">${LOG_LINES.join("\n")}</pre><div style="display:flex;gap:6px;align-items:center;margin-bottom:16px">${logBtn("Refresh", 'data-toast="Refreshed"')}${logBtn("Copy", 'data-toast="Copied"')}${logBtn("Save", 'data-toast="Saved build-box.log to Downloads"')}</div>`
        : up === "log-reading" ? '<div class="dmuted" style="margin-bottom:16px">Reading…</div>'
        : up === "log-empty" ? `<div class="dmuted" style="margin-bottom:6px">Nothing logged lately.</div>${logBtn("Refresh", 'data-toast="Refreshed"')}`
        : logFail[up] ? `<div class="derr" style="margin-bottom:6px">${logFail[up]}</div>${logBtn("Try again", 'data-dact="dset:mup:log"')}`
        : `<div>${logBtn("Show", 'data-dact="dset:mup:log"')}</div>`;
      const ask = up === "asking" ? `<div class="ask"><span>Remove build-box from your machines? It stops taking work. To use it again, add it with a new setup command.</span><span class="acts"><button class="dbtn warn" data-dact="dset:mup:" data-toast="build-box removed">Remove</button><button class="dbtn quiet" data-dact="dset:mup:">Cancel</button></span></div>` : "";
      // Sign-ins: Sign in on a tool the machine can sign in from here; how to, on one it cannot.
      const signedOut = up === "signin-done" ? ["opencode", "gemini", "pi"] : ["claude", "opencode", "gemini", "pi"];
      const SIGN_HOW = { gemini: "Run gemini on build-box once, or add a Gemini API key in Credentials", pi: "Run pi on build-box and use /login" };
      const signRows = [["claude", "you@example.com"], ["codex", "Signed in"], ["opencode", "Signed in"], ["gemini", ""], ["pi", ""], ["gh", "@you"]].map(([t, who]) => {
        const out = signedOut.includes(t);
        const act = !out ? "" : SIGN_HOW[t] ? `<small class="dmuted">${SIGN_HOW[t]}</small>` : `<button class="dbtn small" data-dact="dset:mup:signin-${t}">Sign in</button>`;
        return `<tr><td class="mono">${t}</td><td>${out ? '<span class="dtag ask">Not signed in</span>' : who}</td><td style="text-align:right">${act}</td></tr>`;
      }).join("");
      // The sign-in sheet: the machine runs the agent's own sign-in and passes on its link and code.
      const CLAUDE_LINK = "https://claude.com/cai/oauth/authorize?code=true&amp;client_id=…";
      const OPENAI_LINK = "https://auth.openai.com/codex/device";
      const linkRow = url => `<div class="am-label">Link</div><div style="display:flex;gap:8px;align-items:center;margin-bottom:12px"><div class="ip-path" style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-family:var(--mono);font-size:12px" title="${url}">${url}</div><button class="dbtn" data-toast="Link copied">Copy</button><button class="dbtn pri" data-toast="Opened in your browser">Open in browser</button></div>`;
      const signBody = {
        "signin-starting": ['opencode', '<div class="am-wait" style="margin-bottom:14px"><span class="dspin"></span>Starting opencode\'s sign-in on build-box…</div>'],
        "signin-opencode": ['opencode', `${linkRow(OPENAI_LINK)}<div class="am-label">Code to enter there</div><div style="display:flex;gap:8px;align-items:center;margin-bottom:12px"><span style="font-family:var(--mono);font-size:18px;letter-spacing:2px">3590-11S3E</span><button class="dbtn small" data-toast="Code copied">Copy</button></div><div class="am-wait" style="margin-bottom:14px"><span class="dspin"></span>Waiting for you to approve it in the browser</div>`],
        "signin-claude": ['claude', `${linkRow(CLAUDE_LINK)}<div class="am-label">Code from the page</div><div style="display:flex;gap:8px;margin-bottom:6px"><span class="pv-in" style="flex:1;color:var(--d-dim)">Paste the code</span><button class="dbtn pri" data-dact="dset:mup:signin-done" data-toast="claude signed in on build-box">Send</button></div><div class="am-sub">Sign in on the page, then paste the code it shows.</div>`],
        "signin-failed": ['claude', '<div class="am-msg bad" style="color:var(--d-err);font-size:12.5px;margin-bottom:14px">claude could not sign in: OAuth error: invalid code</div>'],
      }[up];
      const signFoot = up === "signin-failed" ? '<button class="dbtn quiet" data-dact="dset:mup:">Close</button><button class="dbtn pri" data-dact="dset:mup:signin-claude">Try again</button>' : '<button class="dbtn quiet" data-dact="dset:mup:">Cancel</button>';
      const signSheet = !signBody ? "" : `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:70px;justify-content:center;z-index:5">
        <div class="am-sheet" role="dialog" aria-label="Sign in ${signBody[0]} on build-box"><div class="am-head"><b>Sign in ${signBody[0]} on build-box</b><kbd>esc</kbd></div>${signBody[1]}<div style="display:flex;justify-content:flex-end;gap:8px">${signFoot}</div></div></div>`;
      return `${signSheet}<div class="cs-wide"><div class="cs-box"><div class="cs-kv">${facts.map(([k, v]) => `<span>${k}</span><span>${v}</span>`).join("")}</div></div>${under}
      <div class="cs-h">Sign-ins</div><table class="cs-box cs-tbl" style="display:table"><tr><th>Tool</th><th>Account</th><th></th></tr>${signRows}</table>
      <div style="display:flex;align-items:center;gap:8px"><div class="cs-h" style="flex:1">Tools</div><small class="dmuted">Updated teacli to 1.0.10</small><button class="dbtn small" data-toast="Updating claude">Update claude to 2.1.320</button></div><table class="cs-box cs-tbl" style="display:table"><tr><th>Tool</th><th>Version</th><th>Kind</th></tr>${[["claude", "2.1.316", 1], ["codex", "0.166.0", 1], ["opencode", "1.19.4", 1], ["pi", "0.99.2", 1], ["flsh", "0.41.0", 1], ["gh", "2.83.0"], ["teacli", "0.5.0"], ["docker", "28.5.1"], ["adb", "1.0.41"], ["git", "2.51.0"], ["cargo", "1.93.0"], ["node", "24.11.0"], ["python3", "3.13.1"], ["go", "1.25.1"], ["just", "1.43.0"]].map(([t, v, c]) => `<tr><td class="mono">${t}</td><td>${ver(t, v)}</td><td>${c ? "Coding agent" : "Tool"}</td></tr>`).join("")}</table>
      <div class="cs-h">Traffic</div><table class="cs-box cs-tbl" style="display:table"><tr><th>Protocol</th><th>Sent</th><th>Received</th></tr>${[["Link", "412 MB", "96 MB"], ["File sync", "18 MB", "22 MB"], ["Transfers", "14 GB", "3.1 GB"]].map(([n, tx, rx]) => `<tr><td>${n}</td><td class="mono">${tx}</td><td class="mono">${rx}</td></tr>`).join("")}</table>
      <div style="font-size:12px;color:var(--d-muted)">Through a relay: 1.2 GB sent, 300 MB received.</div>
      <div class="cs-h">Log</div>${logBody}
      <div class="dmach dset-rm">${ask ? "" : `<div class="dset-rm-row"><span class="t"><b>Remove this machine</b><small>It stops taking work; its sessions and projects go with it.</small></span><button class="dbtn warn" data-dact="dset:mup:asking" aria-label="Remove build-box">Remove…</button></div>`}${ask}</div></div>`;
    },
  };
  return { frame: v => SETV.frame(v, pages) };
})();
// A direct entry for a coding agent's subpage.
D.dSetHarness = () => { const keep = S.d.set; S.d.set = "acct-claude"; const html = D.dWinSettings(); S.d.set = keep; return html; };
DESK_SCREENS.find(g => g.group === "Command center").items.push(["dSetHarness", "Settings, Agent accounts > Claude Code", "SettingsSheet, CodingAgent"]);
