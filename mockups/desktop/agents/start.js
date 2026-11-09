// ---- Desktop: coding agents in the window (start form, one session, every session) ----
// Mirrors agentnew, agent and agents in apps/desktop/dist/app.js. Opened from the composer's Agent button,
// Cmd+Shift+A, a Running row on Home, or Running in the sidebar.
const AGENT_MOCK = (() => {
  document.head.insertAdjacentHTML("beforeend", '<link rel="stylesheet" href="agents/conversation.css"><link rel="stylesheet" href="agents/docks.css"><link rel="stylesheet" href="agents/composer.css"><link rel="stylesheet" href="agents/files.css">');

  const AGK = { claude: ["CC", "Claude Code"], codex: ["CX", "Codex"], gemini: ["GE", "Gemini CLI"], pi: ["PI", "pi"], flsh: ["FL", "flsh"] };
  const AG_ICON = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>';
  const HARNESS = {
    claude: '<svg aria-hidden="true" viewBox="0 0 24 24"><path fill="#D97757" d="m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z"/></svg>',
  };
  window.__claudeMark = HARNESS.claude;
  const av = a => HARNESS[a] ? `<span class="dapp dcoderav ${a} mark">${HARNESS[a]}</span>` : `<span class="cc-ag ${a}">${AGK[a][0]}</span>`;
  const tag = (k, label) => `<span class="dstate${k ? " " + k : ""}">${label}</span>`;
  const pick = (key, ok, def) => ok.includes(S.d[key]) ? S.d[key] : def;
  const variants = (key, list, def) => list.map(([l, k]) => [l, () => { S.d[key] = k; render(); }, () => pick(key, list.map(x => x[1]), def) === k]);
  const hint = items => `<div class="cc-hints">${items.map(([k, t]) => `<span><kbd>${k}</kbd>${t}</span>`).join("")}</div>`;

  // ----- Start form -----
  const HOSTS = { "build-box": ["GPU, CUDA, Docker", ["claude", "codex", "pi"]], "mac-box": ["Xcode, iOS simulator", ["claude", "codex", "gemini"]], "win-box": ["GPU, Blender, Unreal", ["claude", "flsh"]] };
  D.dAgentNew = () => {
    const v = pick("ag", ["auto", "chosen", "none"], "auto");
    const none = v === "none", chosen = v === "chosen";
    const chip = (label, on, act, title) => `<button class="dchip" aria-pressed="${on}"${act ? ` data-dact="dset:ag:${act}"` : ""}${title ? ` title="${title}"` : ""}>${label}</button>`;
    const machines = none ? chip("Auto", true)
      : chip("Auto", !chosen, "auto", "Beep picks from what each machine has") + Object.entries(HOSTS).map(([n, [caps]]) => chip(n, chosen && n === "build-box", "chosen", caps)).join("");
    const offered = none ? [] : chosen ? HOSTS["build-box"][1] : ["claude", "codex", "gemini", "pi", "flsh"];
    const agents = chip("Any", true) + offered.map(a => chip(AGK[a][1], false)).join("");
    const note = none ? "None of your machines online offers a coding agent. Run beep-node on one with Claude Code, Codex, Gemini CLI, pi or flsh."
      : chosen ? "Has GPU, CUDA, Docker"
      : "Beep picks the machine with what the task needs, such as Xcode, a GPU or Blender, else your environment or the least busy one.";
    const why = v === "auto" ? `<div class="note why"><span class="ddot"></span><span>Auto would pick <b>mac-box</b>: has Xcode, and the prompt mentions the iOS app.</span></div>` : "";
    const prompt = none ? "" : "Fix the failing login test";
    const form = `<div class="cc-panel ag-form">
        <div class="ag-f"><span>What should it do?</span><div class="box area${prompt ? "" : " ph"}"${none ? ' style="min-height:34px"' : ""}>${prompt || "Fix the failing login test"}</div></div>
        <div class="ag-f"><span>Machine</span><div class="dchips">${machines}</div><span class="note">${note}</span>${why}</div>
        <div class="ag-f"><span>Folder</span><div class="box mono${chosen ? "" : " ph"}">${chosen ? "/srv/project" : none ? "Workspace or home folder" : "Workspace or home folder"}</div></div>
        <div class="ag-f"><span>Agent</span><div class="dchips">${agents}</div></div>
        <div class="ag-acts"><button class="dbtn pri"${none ? " disabled" : ` data-dgo="dAgentSession" data-dact="dset:ags:working"`}>Start<kbd>⌘${DI.ret}</kbd></button><button class="dbtn quiet" data-dgo="dWinChat">Cancel</button><span class="hint">Or ask Beep in a chat to start one</span></div>
      </div>`;
    const help = none ? `<div class="cc-panel ag-none"><b>No machine can run an agent yet</b>
        <ol><li>Install beep-node on a computer that has Claude Code, Codex, Gemini CLI, pi or flsh.</li><li>Run <code>beep-node link</code> there.</li><li>Paste the link it prints into a chat here.</li></ol>
        <div class="ag-acts"><button class="dbtn" data-dgo="dMachines">Machines settings</button></div></div>` : "";
    return ccWin(ccSidePj("agents"), `<div class="cc-top"><h2>Start a coding agent</h2><small>It runs on your machine; follow it and answer it here</small><span class="tools"><button class="dbtn" data-dgo="dAgents">All sessions</button></span></div>
      <div class="cc-view"><div class="ag-wrap">${form}${help}</div></div>
      ${hint([["tab", "next field"], ["⌘" + DI.ret, "start"], ["esc", "cancel"]])}`);
  };
  VARIANTS.dAgentNew = variants("ag", [["Auto", "auto"], ["Machine chosen", "chosen"], ["No agents", "none"]], "auto");

  // ----- One session -----
  // Each turn reads top to bottom: the message, the agent's work, its reply.
  // A finished turn folds its work into "Worked for" and a summary line; what
  // still matters stays out of the fold: answered questions, notes such as a
  // model switch, subagents and pictures. Failed steps fold with the rest and
  // the summary counts them ("Ran 4 commands (2 failed)"). A failed step is
  // one line like any other: its command cut to one line (all of it on
  // hover), "exit N" at the right; a click opens the output's last lines.
  // The command never prints twice. The running turn shows only its newest
  // step (open when it failed) and the thought's line under "Working for".
  const SG = {
    close: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"><path d="M18 6 6 18M6 6l12 12"/></svg>',
    ok: AG_IC.ok,
    bad: AG_IC.bad,
    chev: AG_IC.chev,
    fold: '<span class="dv-pm">−</span>',
    sub: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V13a5 5 0 0 0 5 5h4.5"/></svg>',
  };
  const step = (mk, label, o = {}) => {
    const sp = label.indexOf(" "), verb = sp < 0 ? label : label.slice(0, sp), tg = sp < 0 ? "" : label.slice(sp + 1);
    const body = (o.out ? `<pre class="box">${o.out}</pre>` : "") + (o.all ? `<button class="all" data-toast="Opens all ${o.all} lines in a box that scrolls">Show all ${o.all} lines</button>` : "");
    return `<div class="dcoderen tool row${o.bad || mk === "bad" ? " bad" : ""}${o.open ? " open" : ""}"${o.act ? ` data-dact="dset:ags:${o.act}"` : ""}><span class="ki">${AG_IC[dKind(label)]}</span><span class="vb">${verb}</span><code class="tt" title="${o.tip || tg}">${tg}</code><span class="rm">${o.exit ? `exit ${o.exit}` : ""}</span><span class="mk">${mk === "run" ? '<span class="dspin"></span>' : mk === "bad" ? SG.bad : SG.ok}</span>${body ? `<div class="bdy">${body}</div>` : ""}</div>`;
  };
  const SUBS = [
    ["Explore auth module", "Find where the login error text is set", "done", "41s", 9],
    ["Check the iOS copy", "Look for the same string in the iOS app", "run", "18s", 4],
    ["Search old fixtures", "List fixtures that assert on error text", "run", "12s", 3],
  ];
  // A session read from disk (imported, or started outside Beep) shows the
  // subagents its agent wrote there, each with its type or custom agent's
  // name. A background subagent can still run after the parent's turn ends,
  // including while a long command writes no new transcript lines.
  const SUBS_DISK = [
    ["Explore auth module", "Find where the login error text is set", "done", "", 9, "Explore"],
    ["Check the release notes", "Compare the changelog with the diff", "run", "", 14, "release-checker"],
  ];
  const subRow = ([n, d, st, el, k, a]) => `<button class="sr" data-open-sub="${n}" title="Open ${n}"><span class="mk">${st === "run" ? '<span class="dspin"></span>' : SG.ok}</span><span class="t"><b>${n}${a ? `<span class="kind">${a}</span>` : ""}</b><small>${[st === "run" ? "working" : "done", el, `${k} steps`].filter(Boolean).join(" · ")}</small></span></button>`;
  // A coding agent another agent started (start_agent): a normal session on
  // its own machine, listed in the parent's log like a subagent with its
  // harness, machine and live state. A click opens its own log, which has a
  // line back to the parent. Its permission asks go to you, not the parent.
  const KIDS = [
    ["Build the iOS target", "Build the iOS app and run the login UI tests", "ask", "2m", "codex", "Codex", "mac-box"],
    ["Run the Windows suite", "Run cargo test on the Windows build", "run", "18s", "claude", "Claude Code", "win-box"],
  ];
  const kidRow = ([n, d, st, el, ag, hn, m]) => `<button class="sr kid" data-dact="dset:ags:agentchild" title="Open ${n}"><span class="cc-ag ${ag}" title="${hn}">${hn === "Codex" ? "CX" : "CC"}</span><span class="t"><b>${n}<span class="kind">${m}</span></b><small>${[st === "run" ? "working" : "Needs you", el, hn].join(" · ")}</small></span><em class="kstop" data-dact="dset:ags:agentstopped" title="Stop ${n}">Stop</em></button>`;
  return { AGK, AG_ICON, HARNESS, av, tag, pick, variants, hint, HOSTS, SG, step, SUBS, SUBS_DISK, subRow, KIDS, kidRow };
})();
