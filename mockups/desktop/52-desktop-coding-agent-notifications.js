// ---- Desktop: coding agent notifications ----
// A coding agent session run by beep-node notifies when it asks something and when its turn ends.
// One turn makes one "finished" notification, however many tools it ran; prompts queued behind it
// run first. A question has Allow and Deny, answered without opening anything; a click opens the
// session. Nothing shows while that session is on screen in a focused window or the open bar.
// Sessions a chat or a project started report there instead. An alert goes only to the device
// that started the session: one started on the phone notifies the phone (a banner that opens
// Beep), one started on a Mac notifies that Mac. Sessions Beep did not start from a device (a
// terminal, a routine, a project agent) notify the Macs only, never the phone.
// Settings, General has a switch that turns them off on this Mac.
// Every system banner uses the bundled arrow icon, including those without actions.
(() => {
  const AGN = {
    asks: { t: "Claude Code on build-box asks", b: "Fix the flaky login test: Run cargo test -p server (cargo test -p server login)", go: "asks", ask: true },
    done: { t: "Claude Code on build-box finished", b: "Fix the flaky login test: The test raced the server start. It now waits for the port, and all 42 tests pass.", go: "idle" },
    failed: { t: "Codex on build-box failed", b: "Bump the toolchain: the agent exited: rate limit reached", go: "ended" },
    stopped: { t: "Claude Code on build-box stopped", b: "Fix the flaky login test: cancelled", go: "idle" },
    buildfail: { t: "Build failed on cache-warmup", b: "acme/widget: build.yml #351", psel: "ci351" },
    buildok: { t: "Build passed on main", b: "acme/widget: build.yml #352", psel: "actions" },
  };
  D.dNotifAgent = () => {
    const v = AGN[S.d.agn || "asks"] || AGN.asks;
    const side = v.ask ? `<div class="side"><button data-dact="dset:ags:answered" data-dgo="dAgentSession" data-toast="Allowed">Allow</button><button data-dact="dset:ags:answered" data-dgo="dAgentSession" data-toast="Denied">Deny</button></div>` : "";
    const to = v.psel ? `data-dact="dset:psel:${v.psel}" data-dgo="dWinProject"` : `data-dact="dset:ags:${v.go}" data-dgo="dAgentSession"`;
    return deskScene(`<div class="nbanner" role="alert"><div class="main" ${to}><span class="ic">${DI.tray}</span>
      <div class="tx"><div class="top"><b>Beep</b><small>now</small></div><b>${v.t}</b><span>${v.b}</span></div></div>${side}</div>`, { badge: !!v.ask });
  };
  VARIANTS.dNotifAgent = dv("agn", [["Asks", "asks"], ["Finished", "done"], ["Failed", "failed"], ["Stopped", "stopped"], ["Build failed", "buildfail"], ["Build passed", "buildok"]]);
  const g = DESK_SCREENS.find(g => g.group === "Menu bar and windows").items;
  g.splice(g.findIndex(it => it[0] === "dNotifApprove") + 1, 0, ["dNotifAgent", "Coding agent", "Notification, agent session"]);
  // Settings, General: the switch under the voice rows.
  const row = () => `<div class="dtoggle"><span><b>Coding agent notifications</b><small>${S.d.agnoff ? "Coding agents only show under Waiting" : "When a coding agent asks something or finishes a turn"}</small></span><button class="dswitch" role="switch" aria-checked="${!S.d.agnoff}" aria-label="Coding agent notifications" data-dact="dset:agnoff:${S.d.agnoff ? "" : "1"}"></button></div>`;
  for (const id of ["dSettings", "dLiveSettings"]) {
    const base = D[id];
    D[id] = (...a) => base(...a).replace(/(aria-label="When I stop talking"[\s\S]*?<\/div>)/, m => m + row());
  }
})();
