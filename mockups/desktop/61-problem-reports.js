
(() => {
  const REF = "R-7F3K9Q2M", PERIODS = ["Last hour", "Today", "Last day"];
  const TEXT = "The sync stalls after a move";
  const ISSUE = `${TEXT}

Reference: ${REF}
Device: 3f9a2c
Beep 0.2.0

node, linux 6.8, 0.4.1: answered
desktop, macos 15.1, 0.2.0: answered
node, linux 6.8, 0.3.0: older node
node, windows 11, 0.4.1: offline`;
  const FILES = [
    ["build-box/log.txt", "412 KB", `2026-11-06T09:12:03Z INFO node started
2026-11-06T09:12:04Z INFO mesh joined, 3 devices
2026-11-06T09:14:41Z WARN sync paused: <r>[path]</r> is held by another folder
2026-11-06T09:14:42Z INFO retry in 30s
2026-11-06T09:15:12Z ERROR session move failed: <r>[host]</r> did not answer
2026-11-06T09:15:12Z INFO token <r>[secret]</r> not logged`],
    ["build-box/status.txt", "2 KB", `beep-node 0.4.1
os linux 6.8
devices 3, online 2
sessions 2 open
sync: 1 project paused`],
    ["This Mac/log.txt", "96 KB", `2026-11-06 09:12:01 INFO beep started
2026-11-06 09:12:02 INFO mesh opened for <r>[user]</r>
2026-11-06 09:15:12 WARN move of <r>[session]</r> failed, source did not answer`],
  ];
  const style = "position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:70px;justify-content:center;z-index:5";
  const sheet = (label, body) => {
    const s = `<div style="${style}"><div class="am-sheet ip-sheet" role="dialog" aria-label="${label}"><div class="am-head"><b>${label}</b><kbd>esc</kbd></div>${body}</div></div>`;
    return D.dHome().replace(/<\/div><\/div>\s*$/, s + "</div></div>").replace('class="cc-win"', 'class="cc-win" style="position:relative"');
  };
  const lab = t => `<div class="am-label">${t}</div>`;

  D.dReport = () => {
    const per = S.d.rpp || "Today", typed = S.d.rpt === "typed";
    const chips = PERIODS.map(n => `<button class="dchip" role="radio" aria-checked="${per === n}" aria-pressed="${per === n}" data-dact="dset:rpp:${n}" data-dgo="dReport">${n}</button>`).join("");
    return sheet("Report a problem", `${lab("What happened?")}
      <div class="rp-ta focus" role="textbox" aria-label="What happened?" aria-multiline="true" style="margin-bottom:14px">${typed ? TEXT : '<span class="ph">What happened?</span>'}</div>
      <div class="dchips" role="radiogroup" aria-label="Period" style="margin-bottom:16px">${chips}</div>
      <div class="rp-foot"><button class="dbtn quiet" data-dgo="dHome">Cancel</button><button class="dbtn" data-dgo="dAgentDebug" title="Starts a coding agent that looks into Beep and sends a report">Debug with an agent</button><button class="dbtn pri" data-dgo="dReportFiles">Collect</button></div>`);
  };
  VARIANTS.dReport = [["Empty", "empty"], ["Typed", "typed"]].map(([l, k]) => [l, () => { S.d.rpt = k; render(); }, () => (S.d.rpt || "empty") === k]);

  D.dReportFiles = () => {
    const many = S.d.rpm === "mixed", open = S.d.rpf;
    const mach = [["build-box", "answered"], ["This Mac", "answered"],
      ...(many ? [["gpu-box", "older node"], ["studio", "did not answer"], ["win-box", "offline"]] : [["gpu-box", "offline"]])];
    const rows = mach.map(([n, st]) => `<div class="rp-row"><span>${n}</span><span class="m${st === "did not answer" ? " bad" : ""}">${st}</span></div>`).join("");
    const files = FILES.map(([n, sz], i) => `<button class="rp-row" aria-current="${open === String(i)}" data-dact="dset:rpf:${i}" data-dgo="dReportFiles"><code>${n}</code><span class="m">${sz}</span></button>`).join("");
    const f = FILES[open];
    const pane = f ? `<div class="rp-pane-head"><b><code>${f[0]}</code></b><small>Read only, secrets removed</small></div><pre class="rp-pane" aria-label="${f[0]}">${f[2].replace(/<r>(.*?)<\/r>/g, '<span class="r">$1</span>')}</pre>` : "";
    return sheet("Report a problem", `${lab("Machines")}<div class="rp-box">${rows}</div>
      ${lab("Files")}<div class="rp-box">${files}</div>${pane}
      <div class="rp-total"><span>Total</span><span>510 KB</span></div>
      <div class="rp-foot" style="margin-top:12px"><button class="dbtn quiet" data-dgo="dHome">Cancel</button><button class="dbtn pri" data-dgo="dReportSent">Send</button></div>`);
  };
  VARIANTS.dReportFiles = [["Collected", "", ""], ["Some missing", "mixed", ""], ["Viewing a file", "", "0"]]
    .map(([l, m, f]) => [l, () => { S.d.rpm = m; S.d.rpf = f; render(); }, () => (S.d.rpm || "") === m && (S.d.rpf || "") === f]);

  const refRow = `<div class="rp-ref"><span>${REF}</span><button class="dbtn small" data-toast="Copied ${REF}">Copy</button></div>`;
  D.dReportSent = () => {
    const gh = S.d.rpg !== "nogh";
    return sheet("Sent", `${refRow}
      ${gh ? `${lab("Report it on GitHub too?")}<pre class="rp-issue" aria-label="Issue text">${ISSUE}</pre>
      <div class="rp-foot"><button class="dbtn quiet" data-dgo="dHome">No</button><button class="dbtn pri" data-dgo="dReportIssue">Yes</button></div>`
      : `<div class="rp-foot"><button class="dbtn pri" data-dgo="dHome">Done</button></div>`}`);
  };
  VARIANTS.dReportSent = [["gh signed in", "gh"], ["gh not signed in", "nogh"]].map(([l, k]) => [l, () => { S.d.rpg = k; render(); }, () => (S.d.rpg || "gh") === k]);

  D.dReportIssue = () => sheet("Sent", `${refRow}
    <div class="rp-row" style="padding:0 0 14px"><span class="m">Issue</span><code>https://github.com/flashmind-labs/beep/issues/412</code></div>
    <div class="rp-foot"><button class="dbtn" data-toast="Opens the issue in the browser">Open</button><button class="dbtn pri" data-dgo="dHome">Done</button></div>`);

  D.dReportFailed = () => sheet("Report a problem", `<div class="rp-err" role="alert">This computer's clock is off by 12 minutes; set it right and send again.</div>
    <div class="rp-foot"><button class="dbtn quiet" data-dgo="dReportFiles">Back</button><button class="dbtn pri" data-dgo="dReportSent">Try again</button></div>`);

  D.dReportIssueTimeout = () => sheet("Sent", `<div class="rp-err" role="alert">the report was sent, but the GitHub issue did not open: gh did not finish in time</div>
    <div class="rp-hint">Check GitHub before trying again: the issue may have opened.</div>
    <div class="rp-foot"><button class="dbtn quiet" data-dgo="dReportSent">Back</button><button class="dbtn pri" data-dgo="dReportIssue">Try again</button></div>`);

  // Debug with an agent (#445): the start form on this Mac with the report sheet's text in a prompt
  // that has the agent use beep_status, beep_logs and file_report.
  D.dAgentDebug = () => {
    const keep = S.d.ag, keepM = S.d.agm;
    S.d.ag = "chosen"; S.d.agm = "";
    const html = D.dAgentNew();
    S.d.ag = keep; S.d.agm = keepM;
    const said = S.d.rpt === "typed" ? ` What I saw: ${TEXT}` : "";
    const prompt = `Something in Beep went wrong.${said}<br><br>Find out why with the Beep tools: beep_status for every machine, beep_logs for the logs around when it happened. Explain the cause in plain words, then send a problem report with file_report, your findings as its description, and give me its reference.`;
    return html
      .replace(/(<div class="box area)[^"]*("[^>]*>)[^<]*(<\/div>)/, `$1$2${prompt}$3`)
      .replace(/>build-box</, ">This Mac<")
      .replace("Has GPU, CUDA, Docker", "Has Xcode")
      .replace("/srv/project", "Home folder")
      .replace(/(<span>Agent<\/span><div class="dchips">)<button class="dchip" aria-pressed="true"([^>]*)>Any<\/button>/, '$1<button class="dchip" aria-pressed="false"$2>Any</button>')
      .replace(/<button class="dchip" aria-pressed="false">Claude Code<\/button>/, '<button class="dchip" aria-pressed="true">Claude Code</button>');
  };
  VARIANTS.dAgentDebug = [["Nothing typed", "empty"], ["Typed", "typed"]].map(([l, k]) => [l, () => { S.d.rpt = k; render(); }, () => (S.d.rpt || "empty") === k]);

  DESK_SCREENS.find(g => g.group === "Command center").items.push(
    ["dReport", "Report a problem", "ReportSheet"], ["dReportFiles", "Report: files", "ReportSheet, files"],
    ["dReportSent", "Report: sent", "ReportSheet, sent"], ["dReportIssue", "Report: issue", "ReportSheet, issue"],
    ["dReportFailed", "Report: failed", "ReportSheet, failed"],
    ["dReportIssueTimeout", "Report: issue timed out", "ReportSheet, failed"],
    ["dAgentDebug", "Debug with an agent", "ReportSheet, AgentNew"]);
})();
// Keyboard shortcuts (#363): Settings > General > Keyboard shortcuts, and ⌘/ anywhere in the window.
// One list in code feeds it; sections follow where a key works. Keys of hidden features
// (the assistant, live mode) are left out while those are off. The filter matches keys and words.
(() => {
  const SECTIONS = [
    ["Anywhere", [["⌥ ⌥", "Open or hide the quick bar"], ["⌥Tab", "Switch active conversations; hold Option to choose"], ["⌥⇧Tab", "Previous active conversation"]]],
    ["Window", [["⌘K", "Search"], ["⌘,", "Settings"], ["⌘/", "Keyboard shortcuts"], ["⌘N", "New agent session"], ["⌘[  ⌘]", "Back, forward"], ["⌘W", "Close the tab, else hide the window"], ["esc", "Up one page"]]],
    ["Projects", [["⇧⌘N", "Open the project in its own window"], ["⌘N", "New session in this project"], ["⌘T", "New terminal in this project"]]],
    ["Agent session", [["↩", "Send; while it works, steer"], ["⌥↩", "Queue for after the turn"], ["⇧↩", "New line"], ["⇧tab", "Cycle the mode"], ["⌘↩", "Allow"], ["⌘D", "Deny"], ["⌘.", "Stop the turn"], ["⌘1  ⌘2  ⌘3  ⌘4", "Chat, Files, Changes, Artifacts"], ["⌘O", "Open the folder in the editor"], ["⌘U", "Open or fold every thought"], ["⌘Y", "Open or fold every command"], ["⌘↑  ⌘↓", "Start, end of the log"], ["⌃`", "New terminal in the session folder"]]],
    ["Side pane", [["⇧esc", "Zoom the pane, again to restore"], ["⌃tab  ⌃⇧tab", "Next, previous tab"], ["⌘T", "New terminal"]]],
    ["Terminals", [["⌘N", "New terminal like this one"], ["⌘W", "Close the terminal"], ["⌃←  ⌃→", "Word back, forward"], ["⌥←  ⌥→", "Word back, forward"], ["⌘←  ⌘→", "Start, end of the line"], ["⌥⌫", "Delete the word before"], ["⌘⌫", "Delete to the line start"]]],
    ["Review", [["j  k", "Next, previous file"], ["x", "Fold the file"], ["/", "Filter files"], ["⌘↩", "Add the comment"]]],
    ["Search", [["↑  ↓", "Move through results"], ["↩", "Open"], ["esc", "Close"]]],
  ];
  const style = "position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:50px;justify-content:center;z-index:5";
  const css = `<style>.sk-sheet{width:560px;max-height:calc(100% - 80px);display:flex;flex-direction:column}.sk-find{margin:0 0 10px;height:30px;border-radius:7px;border:1px solid var(--d-border);background:var(--d-bg);color:var(--d-muted);display:flex;align-items:center;padding:0 10px;font-size:12px}.sk-list{overflow:auto;min-height:0}.sk-sec{font-size:11px;font-weight:600;color:var(--d-muted);text-transform:uppercase;letter-spacing:.04em;margin:14px 0 4px}.sk-sec:first-child{margin-top:0}.sk-row{display:flex;align-items:center;gap:12px;padding:6px 0;border-top:1px solid var(--d-border);font-size:12.5px}.sk-row span{flex:1}.sk-row kbd{font:12px ui-monospace,SFMono-Regular,monospace;padding:2px 7px;border-radius:5px;border:1px solid var(--d-border);background:var(--d-elev);white-space:pre}</style>`;
  const list = SECTIONS.map(([t, rows]) => `<div class="sk-sec">${t}</div>` + rows.map(([k, d]) => `<div class="sk-row"><span>${d}</span><kbd>${k}</kbd></div>`).join("")).join("");
  D.dShortcuts = () => {
    const s = `${css}<div style="${style}"><div class="am-sheet sk-sheet" role="dialog" aria-label="Keyboard shortcuts"><div class="am-head"><b>Keyboard shortcuts</b><kbd>esc</kbd></div><div class="sk-find">Filter</div><div class="sk-list">${list}</div></div></div>`;
    return D.dHome().replace(/<\/div><\/div>\s*$/, s + "</div></div>").replace('class="cc-win"', 'class="cc-win" style="position:relative"');
  };
  DESK_SCREENS.find(g => g.group === "Command center").items.push(["dShortcuts", "Keyboard shortcuts", "ShortcutsSheet"]);
})();
