// ---- Desktop v2 ----
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .dscope { display: inline-flex; align-items: center; gap: 4px; color: var(--d-fg); font-weight: 500 }
.mac .dshot { position: relative; width: 300px; max-width: 100%; aspect-ratio: 16 / 10; border-radius: 9px; overflow: hidden; border: 1px solid var(--d-border); background: linear-gradient(165deg, #23252b, #141517) }
.mac .dshot .w { position: absolute; left: 10%; top: 12%; right: 10%; bottom: 10%; border-radius: 6px; background: #E9EEEA; overflow: hidden }
.mac .dshot .w::before { content: ""; position: absolute; left: 0; right: 0; top: 0; height: 12%; background: #CDD6D0 }
.mac .dshot .ln { position: absolute; left: 8%; height: 5%; border-radius: 3px; background: #B9C5BE }
.mac .dshot .ring { position: absolute; width: 22px; height: 22px; margin: -11px 0 0 -11px; border-radius: 50%; border: 2.5px solid var(--d-coral); box-shadow: 0 0 0 4px rgba(242,153,74,.25) }
.mac .dshot small { position: absolute; left: 6px; bottom: 4px; font-size: 10px; color: rgba(227,228,230,.75) }
.mac .dagent, .mac .dshot { flex: none }
.mac .dagent .row { display: flex; align-items: center; gap: 8px; font-size: 13px; line-height: 20px }
.mac .dagent .sec { font-size: 12px; font-weight: 600; color: var(--d-dim); padding: 8px 12px 2px }
.mac .dagent .plan { display: grid; gap: 2px; padding: 0 12px }
.mac .dagent .files { display: grid; gap: 1px; padding: 0 12px; font-family: var(--mono); font-size: 12px }
.mac .dagent .files span { display: flex; gap: 10px } .mac .dagent .files b { flex: 1; font-weight: 400 }
.mac .dagent .add { color: var(--d-emerald) } .mac .dagent .del { color: var(--d-err) }
.mac .dagent .q { margin: 10px 12px 0; padding: 10px; border-radius: 8px; background: var(--d-surface); display: grid; gap: 6px; font-size: 13px; line-height: 19px }
.mac .dstate { font-size: 11px; font-weight: 600; padding: 0 8px; line-height: 19px; border-radius: 999px; border: 1px solid var(--d-border); color: var(--d-muted); flex: none }
.mac .dstate.run { border-color: rgba(94,106,210,.5); color: var(--d-emerald) } .mac .dstate.ask { border-color: rgba(242,153,74,.5); color: var(--d-coral) }`;
  document.head.append(st);
})();

Object.assign(D, {
  // exec on a machine: the step names the machine as a chip and the command's first line.
  dShell: () => {
    const s = S.d.shell;
    if (s === "offline") return deskScene(dBar(D_ASK + `<div class="dbody">${dLog(`${dYou("Check the GPU memory on gpu-box")}
      ${dDone([["Checking your machines"], ["Running on gpu-box: nvidia-smi", "bad"]], 3)}
      <div class="dcoderen agent md"><div><b>gpu-box is offline.</b> It was last seen 3 hours ago, so beep-node is not running there. Start it with ${dCode("beep-node run")} and ask again.</div></div>`)}
    </div>` + dHints()));
    const running = s === "running";
    const out = `Filesystem      Size  Used Avail Use% Mounted on
/dev/nvme0n1p2  480G  439G   41G  92% /
/dev/nvme1n1    1.8T  612G  1.2T  34% /srv`;
    return deskScene(dBar((running ? dAskF(`<button class="dbtn" data-dact="dset:shell:done"><span style="width:9px;height:9px;border-radius:2px;background:currentColor"></span>Stop<kbd>⌘.</kbd></button>`) : D_ASK) + `<div class="dbody">${dLog(`${dYou("How much disk is left on build-box?")}
      ${running ? dWork([["Checking your machines"], ["Running on build-box: df -h /; du -xhd 1 ~ 2>/dev/null | sort -h", "run"]], 3)
      : `${dDone([["Checking your machines"], ["Running on build-box: df -h /; du -xhd 1 ~ 2>/dev/null | sort -h"]], 5)}
      <pre class="dpre"><code>${out}</code><button class="dcopy" data-toast="Copied">${D_COPY}</button></pre>
      <div class="dcoderen agent md"><div><b>build-box has 41 GB free on /</b>, 92% used. Most of it is Docker: 210 GB of images, 160 GB of them unused. ${dCode("/srv")} has 1.2 TB free.</div></div>
      <div class="dups"><button class="dbtn" data-dgo="dStream" data-toast="Sent: Prune the unused images">Prune the unused images</button><button class="dbtn" data-toast="Sent: Leave it">Leave it</button></div>`}`)}
    </div>` + dHints(running ? [[DI.ret, "queue"], ["⌘.", "stop"], ["esc", "to hide"]] : DHINTS)));
  },
  // node tool, `<node> claude start|status|answer`: Beep starts Claude Code on a machine and keeps checking it.
  // It answers the agent's permission questions itself when they are within what was asked, and asks here when not.
  dAgent: () => {
    const s = S.d.agent, done = s === "done", ask = s === "ask";
    const mark = st => st === "ok" ? `<span class="dok">${DI.check}</span>` : st === "run" ? '<span class="dspin"></span>' : st === "wait" ? '<span class="ddot" style="background:var(--d-coral)"></span>' : '<span class="ddot off"></span>';
    const plan = [["Find why the login test fails", "ok"], ["Fix the session expiry in the cookie", done ? "ok" : "ok"], ["Run the auth tests", done ? "ok" : ask ? "ok" : "run"], ["Push the branch fix-login", done ? "ok" : ask ? "wait" : ""]];
    const state = done ? '<span class="dstate">Idle</span>' : ask ? '<span class="dstate ask">Waiting for you</span>' : '<span class="dstate run">Working</span>';
    const card = `<div class="dcard dcoder dcodercard dagent">
      <div class="head" style="background:rgba(94,106,210,.1)"><span class="dapp dcoderav claude mark">${window.__claudeMark}</span><div style="flex:1;min-width:0;display:grid"><b style="font-size:13.5px">Claude Code on build-box</b><small style="font-size:12px;color:var(--d-muted);font-family:var(--mono);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">/srv/project/atlas</small></div>${state}</div>
      ${done ? "" : `<div class="row" style="padding:8px 12px 0;color:var(--d-muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${ask ? "git push origin fix-login" : "Editing src/auth/session.ts"}</div>`}
      ${ask ? `<div class="q"><b style="font-weight:600">Run git push?</b><code style="font-family:var(--mono);font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere">git push origin fix-login</code>
        <div class="dups"><button class="dbtn pri" data-dact="dset:agent:done" data-toast="Answered: Yes">Yes</button><button class="dbtn" data-dact="dset:agent:done" data-toast="Answered: Yes, and don't ask again for git push">Yes, don't ask again for git push</button><button class="dbtn warn" data-dact="dset:agent:done" data-toast="Told it not to push">No</button></div></div>` : ""}
      <div class="sec">Plan</div><div class="plan">${plan.map(([l, st]) => `<div class="row"><span style="width:16px;display:grid;place-items:center">${mark(st)}</span><span style="${st ? "" : "color:var(--d-muted)"}">${l}</span></div>`).join("")}</div>
      <div class="sec">Files changed</div><div class="files"><span><b>src/auth/session.ts</b></span><span><b>tests/login.test.ts</b></span></div>
      <div class="dups" style="padding:10px 12px 0"><button class="dbtn" data-dgo="dAgents">Open</button></div>
      <div style="height:12px"></div></div>`;
    const steps = [["Checking build-box"], ["Starting claude on build-box"], ...(done ? [["Answering claude on build-box"], ["Checking claude on build-box"]] : [["Checking claude on build-box", ask ? "" : "run"]])];
    const reply = done ? `<div class="dcoderen agent md">Fixed. The session cookie expired a minute before the token did, so the login test failed on slow runs. <b>All 48 auth tests pass</b>, and the fix is pushed on ${dCode("fix-login")}.</div>
      <div class="dups"><button class="dbtn" data-toast="Sent: Open a pull request">Open a pull request</button><button class="dbtn" data-toast="Sent: Close the session">Close the session</button></div>`
      : ask ? `<div class="dcoderen agent md">It found the bug and the tests pass. Should it push the branch?</div>` : "";
    return deskScene(dBar((done ? D_ASK : dAskF(`<span style="display:inline-flex;align-items:center;gap:6px;font-size:12px;color:var(--d-emerald);flex:none"><span class="dspin"></span>${ask ? "Waiting" : "Working"}</span>`)) + `<div class="dbody">${dLog(`${dYou("Fix the failing login test in atlas on build-box")}
      ${done ? dDone(steps, 252) : dWork(steps, ask ? 188 : 96)}${card}${reply}`)}</div>` + dHints(DHINTS)));
  },
  // node tool, `<node> desktop ...` on this Mac: screenshot, tree, click, type. The steps say what it did.
  dControl: () => {
    const s = S.d.ctl;
    if (s === "perm") return deskScene(dBar(D_ASK + `<div class="dbody">${dLog(`${dYou("Open the parcel tracking page in Safari and tell me when it arrives")}
      ${dDone([["Looking at the screen on this Mac", "bad"]], 2)}
      <div class="dcoderen agent md">I cannot see the screen on this Mac yet. Allow <b>Screen Recording</b> for Beep, then ask again.</div>
      <div class="dups"><button class="dbtn" data-dgo="dSettings" data-dact="dset:mac:perm">Open Settings…</button></div>`)}</div>` + dHints()));
    const running = s === "running";
    const shot = `<div class="dshot" role="img" aria-label="Screenshot of Safari with the tracking page"><div class="w"><span class="ln" style="top:24%;width:50%"></span><span class="ln" style="top:38%;width:70%"></span><span class="ln" style="top:52%;width:34%;background:#5E6AD2"></span><span class="ln" style="top:66%;width:60%"></span></div><span class="ring" style="left:28%;top:60%"></span><small>Screenshot, 1280 × 800</small></div>`;
    const steps = running ? [["Looking at the screen on this Mac"], ["Opening an app on this Mac"], ["Typing on this Mac"], ["Clicking on this Mac", "run"]]
      : [["Looking at the screen on this Mac"], ["Opening an app on this Mac"], ["Typing on this Mac"], ["Clicking on this Mac"], ["Looking at the screen on this Mac"]];
    return deskScene(dBar((running ? dAskF(`<span style="display:inline-flex;align-items:center;gap:6px;font-size:12px;color:var(--d-emerald);flex:none"><span class="dspin"></span>Using this Mac</span>`) : D_ASK) + `<div class="dbody">${dLog(`${dYou("Open the parcel tracking page in Safari and tell me when it arrives")}
      ${running ? `${dWork(steps, 9)}${shot}` : `${dDone(steps, 14)}${shot}
      <div class="dcoderen agent md">Your parcel arrives <b>tomorrow between 10:00 and 14:00</b>. I clicked Track on the order page; it is out for delivery from the local depot.</div>`}`)}
    </div>` + dHints(running ? [["⌘.", "stop"], ["esc", "to hide"]] : DHINTS)));
  },
  // Settings, Other machines. This Mac is the same switch and permissions as Settings.
  dMachines: () => {
    const ic = `<span class="ic">${small(I.server, 15)}</span>`;
    const row = (name, tags, sub, on) => `<div class="ddev">${ic}<span class="nm">${name}${tags.map(t => `<span class="dtag${t === "Online" ? " own" : ""}">${t}</span>`).join("")}<small>${sub}</small></span>${on ? '<span class="ddot"></span>' : '<span class="ddot off"></span>'}</div>`;
    return deskScene(dBar(`<div class="dhead"><span class="tt"><b>Machines</b><small>Where Beep runs commands and coding agents</small></span></div>
      <div class="dform" style="padding-top:12px">${dThisMac()}
        <div class="dsec" style="padding:6px 0 0">Other machines</div>
        <div class="ddevs">${row("build-box", ["Online"], "Ubuntu 24.04, x86_64 · claude, codex, git, docker +3 · gh as build-user", true)}${row("gpu-box", [], "Last seen 3 hours ago · Debian 13, x86_64 · python, nvidia-smi, git", false)}</div>
        <div class="dmuted" style="display:block">To add one, use Add a machine: it gives you a one-time command to run there.</div>
      </div>` + dHints([["esc", "close"]])));
  },
});
DESK_SCREENS.splice(DESK_SCREENS.findIndex(g => g.group === "Research"), 0,
  { group: "Machines", items: [["dShell", "Command on a machine", "PromptBar, node shell"], ["dAgent", "Coding agent", "PromptBar, node claude"], ["dControl", "Using this Mac", "PromptBar, node desktop"], ["dMachines", "Machines", "SettingsView, This Mac"]] },
  { group: "Live voice", items: [["dLive", "Listening", "LiveView, listening"], ["dThink", "Thinking", "LiveView, thinking"], ["dSpeak", "Speaking", "LiveView, speaking"],
    ["dInterrupt", "Interrupted", "LiveView, tap or “stop”"], ["dCommands", "Spoken commands", "listen::Command"], ["dLeft", "Left live mode", "PromptBar, draft"], ["dSpokenChat", "The chat afterwards", "PromptBar, spoken turns"], ["dLiveSettings", "When I stop talking", "SettingsView, voice"]] });
const dv = (key, list) => list.map(([label, val]) => [label, () => { S.d[key] = val; render(); }, () => S.d[key] === val]);
Object.assign(VARIANTS, {
  dShell: dv("shell", [["Done", "done"], ["Running", "running"], ["Machine offline", "offline"]]),
  dAgent: dv("agent", [["Working", "work"], ["Asks you", "ask"], ["Done", "done"]]),
  dControl: dv("ctl", [["Done", "done"], ["Working", "running"], ["No Screen Recording", "perm"]]),
  dMachines: [["Mac off", () => { S.d.mac = "off"; render(); }, () => (S.d.mac || "off") === "off"], ["Mac needs permissions", () => { S.d.mac = "perm"; render(); }, () => S.d.mac === "perm"], ["Mac ready", () => { S.d.mac = "ready"; render(); }, () => S.d.mac === "ready"]],
  dLive: dv("lv", [["Words coming in", "words"], ["About to send", "soon"], ["A pause, waiting for “send”", "pause"], ["Nothing said yet", "empty"], ["Holding Option", "hold"], ["Heard “send”", "send"], ["Voice unavailable", "off"], ["What to say", "sheet"]]),
  dThink: dv("lt", [["Sent", "think"], ["Using a tool", "tool"]]),
  dSpeak: dv("ls", [["Reply", "reply"], ["A voice note arrives", "note"]]),
  dInterrupt: dv("li", [["Tapped Option", "tap"], ["Said “stop”", "said"]]),
  dCommands: dv("lc", [["“stop” while talking", "stop"], ["Inside a sentence", "normal"]]),
  dLiveSettings: dv("vsend", [["Send when I stop talking", "pause"], ["Wait until I say “send”", "word"]]),
  dLeft: dv("le", [["Tapped Option or esc", "draft"], ["2 minutes quiet", "quiet"], ["Nothing said", "empty"]]),
});
