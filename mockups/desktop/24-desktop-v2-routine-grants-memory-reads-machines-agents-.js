// ---- Desktop v2: routine grants, memory reads, machines, agents, routine edit ----
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .dreplyacts .dagent { align-self: center; font-size: 11px; color: var(--d-dim); padding-left: 8px }
.mac .dgkv { display: grid; grid-template-columns: 70px 1fr; gap: 2px 10px; padding: 4px 0 2px; font-size: 12px; color: var(--d-fg) }
.mac .dgkv span:nth-child(odd) { color: var(--d-muted) }
.mac .dchips .dsep { width: 1px; align-self: stretch; background: var(--d-border); margin: 3px 2px }
.mac .dmach { display: grid; gap: 1px; padding: 8px 10px; border-radius: 9px }
.mac .dmach + .dmach { border-top: 1px solid var(--d-border); border-radius: 0 }
.mac .dmach .nm { display: flex; align-items: center; gap: 6px; font-size: 14px; font-weight: 600 }
.mac .dmach small { font-size: 12px; color: var(--d-muted); line-height: 17px }
.mac .dmach .cd { font-size: 12.5px; color: var(--d-fg); line-height: 18px }`;
  document.head.append(st);
})();

// Routine grants (ListGrants), the same as the phone's GrantsScreen. Revoke asks first, then RevokeGrant;
// the routine asks again. Each grant ends on its own: 8 hours unless the agent asked for longer.
const DGRANTS = [
  { app: "G", action: "Gmail: reply to thread", short: "reply to thread", routine: "Inbox sweep", per: "1 each run", used: "Last used 40 min ago", ends: "Ends in 5 h",
    fields: [["To", "ana@example.com only"], ["Only in", "this thread"], ["Subject", "Re: Saturday plans"]] },
  { app: "F", action: "Fastmail: send email", short: "send email", routine: "Weekly review", per: "up to 2 each run", used: "Not used yet", ends: "Ends in 6 d",
    fields: [["To", "alex@example.com only"]] },
];
D.dGrants = () => {
  const st = S.d.gr || "list";
  const shown = st === "revoked" ? DGRANTS.slice(1) : DGRANTS;
  const head = `<div class="dhead"><span class="tt"><b>Routine grants</b><small>${st === "empty" ? "Nothing granted" : "What routines may do without asking"}</small></span></div>`;
  if (st === "empty") return deskScene(dBar(head + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--d-dim);display:grid">${I.shield}</span>
    <b style="font-size:14px">No routine grants.</b><span class="dmuted" style="display:block">Beep asks in a chat when a routine needs one.</span></div>` + dHints([["esc", "back"]])));
  const rows = shown.map((g, i) => {
    const on = i === 0;
    if (on && st === "confirm") return `<div class="drt" aria-selected="true"><span class="tt"><span style="font-size:14px">Revoke? <b>${g.routine}</b> will ask before each ${g.short} again.</span></span>
      <span class="acts"><button class="dbtn warn" data-dact="dset:gr:revoked" data-toast="Grant revoked">Revoke</button><button class="dbtn quiet" data-dact="dset:gr:list">Cancel</button></span></div>`;
    const acts = on ? `<span class="acts"><button class="dbtn warn" data-dact="dset:gr:confirm">Revoke<kbd>⌘⌫</kbd></button></span>` : "";
    return `<div class="drt" role="option" aria-selected="${on}" style="align-items:flex-start"><span class="dapp" style="margin-top:2px">${g.app}</span>
      <span class="tt"><b>${g.routine}</b><span>${g.action}, ${g.per}</span><small>${g.ends} · ${g.used}</small>
      ${on ? `<span class="dgkv">${g.fields.map(([k, v]) => `<span>${k}</span><span>${v}</span>`).join("")}</span>` : ""}</span>${acts}</div>`;
  }).join("");
  const hints = st === "confirm" ? [[DI.ret, "revoke"], ["esc", "cancel"]] : [["↑↓", "move"], ["⌘⌫", "revoke"], ["esc", "back"]];
  return deskScene(dBar(head + `<div class="dlist" style="padding-top:8px">${rows}</div>` + dHints(hints)));
};

// Routines: Grants in the header opens the grants; Tab shows Edit next to Run now and Delete.
const dRoutinesBase = D.dRoutines;
D.dRoutines = () => {
  let html = dRoutinesBase().replace(/(<button class="dbtn" data-dgo="dNewRoutine">)/, `<button class="dbtn" data-dgo="dGrants" data-dact="dset:gr:list">Grants <span style="color:var(--d-muted)">2</span><kbd>⌘G</kbd></button>$1`);
  if (S.d.rt === "empty") return html;
  html = html.replace(/(<button class="dbtn warn" data-dact="rt:delete">)/, `<button class="dbtn" data-dgo="dEditRoutine" data-dact="dset:er:time">Edit<kbd>⌘E</kbd></button>$1`);
  return html;
};

// UpdateCron changes the name and the schedule; what Beep does stays, so it shows as text.
D.dEditRoutine = () => {
  const st = S.d.er || "time", place = st === "place";
  const chip = (k, l) => `<button class="dchip" aria-pressed="${k === "weekdays"}">${l}</button>`;
  const when = place
    ? `<div class="dfield"><span>Starts</span><div class="dmuted" style="display:block;font-size:13.5px;color:var(--d-fg)">When you arrive at a saved place</div><span>Change the place in a chat.</span></div>`
    : `<div class="dfield"><span>Repeat</span><div class="dchips">${chip("day", "Every day")}${chip("weekdays", "Weekdays")}${chip("week", "Weekly")}${chip("hour", "Every hour")}${chip("once", "Once")}${chip("cron", "Cron")}</div></div>
       <label class="dfield" style="max-width:140px"><span>Time</span><span class="f">17:30</span></label>`;
  const name = place ? "Groceries at home" : "Inbox sweep";
  const task = place ? "Remind me to put the groceries away." : "Archive newsletters and list what still needs a reply.";
  return deskScene(dBar(`<div class="dhead"><span class="tt"><b>Edit routine</b><small>${name}</small></span></div>
    <div class="dform" style="padding-top:12px;flex:1;min-height:0;overflow:auto;scrollbar-width:none;align-content:start">
      <label class="dfield on"><span>Name</span><span class="f">${name}</span></label>
      <div class="dfield"><span>What Beep does</span><div class="dmuted" style="display:block;font-size:13.5px;color:var(--d-fg)">${task}</div></div>
      ${when}
      <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri" data-dgo="dRoutines" data-toast="Routine saved">Save<kbd>⌘${DI.ret}</kbd></button><button class="dbtn quiet" data-dgo="dRoutines">Cancel</button>
        <span class="dmuted" style="margin-left:auto">To change what it does, ask Beep in a chat</span></div>
    </div>` + dHints([["tab", "next field"], [`⌘${DI.ret}`, "save"], ["esc", "cancel"]])));
};

// Memories: the reads that added or confirmed memories (ListMemoryReads) sit after the subjects.
// Tab and shift-Tab step through the chips.
const DREAD = [
  ["Work", [["Design review every Thursday at 10:30", "From your email · 7 Oct"], ["On the Lisbon project until December", "From your email · 7 Oct"]]],
  ["People", [["Alex is the landlord and prefers email to calls", "From your email · 7 Oct"]]],
];
const dScopeChipsBase = dScopeChips;
dScopeChips = () => dScopeChipsBase().replace(/(<\/div>)/, `<span class="dsep"></span><button class="dchip" aria-pressed="${S.d.mscope === "read"}" data-dact="dset:mscope:read">Gmail, today <span style="color:var(--d-muted)">3</span></button>$1`);
VARIANTS.dMemories.splice(2, 0, ["One read", () => { S.d.mem = "list"; S.d.mscope = "read"; render(); }, () => S.d.mem === "list" && S.d.mscope === "read"]);

// Machines (ListMachines): each one's state, system and the coding tools on it. This Mac's switch stays in Settings.
const DCODERS = new Set(["claude", "codex", "gemini", "opencode", "cursor-agent", "amp", "aider"]);
const dMacAgTile = (a, cls) => `<span class="${cls} ${a}${window.__MARKS?.[a] ? " mark" : ""}">${window.__MARKS?.[a] || a.slice(0, 2).toUpperCase()}</span>`;
D.dMachines = () => {
  const add = `<button class="dbtn" data-dgo="dAddMachine">Add a machine</button>`;
  const head = say => `<div class="dhead"><span class="tt"><b>Machines</b><small>${say}</small></span>${add}</div>`;
  if (S.d.machines === "none") return deskScene(dBar(head("Where Beep runs commands and coding agents") + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--d-dim);display:grid">${I.server}</span><b style="font-size:14px">No machines yet.</b><span class="dmuted" style="display:block">Add one to run commands and coding agents on it.</span></div>` + dHints([["esc", "back"]])));
  const COLS = ["claude", "codex", "opencode"];
  const list = [
    { name: "build-box", online: true, spec: "Ubuntu 24.04 · x86_64 · 16 CPU · 64 GB", ver: "0.4.1", behind: true, agents: [["claude", "2.1.4", true], ["codex", "0.48.0", false], ["opencode", "1.19.4", null]], tools: ["gh", "docker", "git", "cargo", "just"] },
    { name: "gpu-box", online: false, seen: "Seen 3 h ago", spec: "Debian 13 · x86_64 · 32 CPU · 128 GB", ver: "0.4.2", agents: [], tools: ["python3", "nvidia-smi", "git"] },
    { name: "ci-runner", online: true, temp: true, spec: "Debian 13 · x86_64 · 4 CPU · 8 GB", ver: "0.4.2", agents: [["claude", "2.1.4", true]], tools: ["git", "cargo"], adapters: [["Codex", "could not download Node.js: no curl or wget"], ["Gemini", null]] },
  ];
  const card = m => {
    const asking = S.d.mrmb === m.name;
    const remove = asking ? "" : `<button class="dbtn quiet" data-dact="dset:mrmb:${m.name}" data-dgo="dMachines" aria-label="Remove ${m.name}">Remove…</button>`;
    const what = m.online ? "It stops taking work; its sessions and projects go with it." : "Its sessions and projects go with it. If it comes back online, it is turned away.";
    const ask = asking ? `<div class="ask"><span>Remove ${m.name} from your account? ${what} To use it again, add it with a new setup command.</span>
      <span class="acts"><button class="dbtn warn" data-dact="dset:mrmb:" data-dgo="dMachines" data-toast="${m.name} removed">Remove</button><button class="dbtn quiet" data-dact="dset:mrmb:" data-dgo="dMachines">Cancel</button></span></div>` : "";
    const l1 = `<span class="l1"><span class="ddot${m.online ? "" : " off"}" title="${m.online ? "Online" : "Offline"}"></span><b>${m.name}</b>${m.temp ? '<span class="dtag" title="Removed after a day offline">container</span>' : ""}${m.seen ? `<span class="seen">${m.seen}</span>` : ""}<span class="spec" title="${m.spec}">${m.spec}</span><span class="dmnv" title="${m.behind ? `beep-node ${m.ver}; 0.4.2 is out` : `beep-node ${m.ver}`}">beep-node ${m.ver}${m.behind ? '<span class="up">update</span>' : ""}</span>${remove}</span>`;
    const l2 = m.agents.length ? m.agents.map(([a, v, s]) => `<span class="dmag" style="grid-column:${COLS.indexOf(a) + 1}" title="${a} ${v}${s === null ? "" : s ? ", signed in" : ", not signed in"}">${dMacAgTile(a, "dmag-ic")}<span class="v">${v}</span>${s === null ? "" : `<span class="${s ? "in" : "out"}"></span>`}</span>`).join("") : '<span class="none">No coding agents</span>';
    // An agent whose ACP adapter the node is installing, or could not install (#425).
    const l4 = (m.adapters ?? []).map(([n, why]) => `<span class="dmad${why ? " bad" : ""}" title="${why ?? ""}">${why ? `Could not install the ${n} adapter: ${why}` : `Installing the ${n} adapter`}${why ? `<button class="dbtn small" data-toast="Trying ${n} again on ${m.name}">Try again</button>` : ""}</span>`).join("");
    return `<div class="dmach">${l1}${l2}<span class="l3">${m.tools.map(t => `<span class="dmt-t">${t}</span>`).join("")}</span>${l4}${ask}</div>`;
  };
  return deskScene(dBar(head("2 online, 1 offline") + `<div class="dlist"><div class="dmt" style="--ag:${COLS.length}">${list.map(card).join("")}</div></div>` + dHints([["esc", "back"]])));
};
VARIANTS.dMachines = [["Two machines", () => { S.d.machines = "list"; render(); }, () => S.d.machines !== "none"], ["None", () => { S.d.machines = "none"; render(); }, () => S.d.machines === "none"]];
DESK_SCREENS.find(g => g.group === "Machines").items.forEach(it => { if (it[0] === "dMachines") it[2] = "MachinesView"; });

// Model view (admins): Agents turns agents on or off for everyone (SetAgents), with the agents listed.
const dModelBase = D.dModel;
D.dModel = () => {
  const html = dModelBase();
  if (S.d.model === "hints") return html;
  const ag = S.d.agents;
  const block = `<div class="dsec flat" style="padding:4px 0 0;display:flex;gap:6px;align-items:center">Agents <span class="dtag">Admin</span></div>
    <div class="dtoggle"><span><b>Agents</b><small>${ag ? "On for everyone" : "Off for everyone"}</small></span><button class="dswitch" role="switch" aria-checked="${ag}" aria-label="Agents" data-dact="dagents"></button></div>
    <div class="dmuted" style="display:block;opacity:${ag ? 1 : .6}">${AGENTS.map(([n]) => n).join(", ")}</div>`;
  const at = html.lastIndexOf('</div><div class="dhints">');
  return (at < 0 ? html : html.slice(0, at) + block + html.slice(at)).replace('<div class="dform" style="padding-top:12px">', '<div class="dform" style="padding-top:12px;flex:1;min-height:0;overflow:auto;scrollbar-width:none">');
};
window.addEventListener("click", e => {
  if (!e.target.closest('[data-dact="dagents"]')) return;
  S.d.agents = !S.d.agents;
  toast(S.d.agents ? "Agents on for everyone" : "Agents off");
  render();
}, true);

// Model view on a chat kept on this Mac (no server): search over OpenRouter's catalog, the
// catalog's feature toggles (flashmind CLI order), and a preferred upstream provider that loads
// only when asked. The server's picker above is unchanged.

// The agent that answered a turn (GetSessionAgents), after Copy. General shows nothing.
const dCopyBase = D.dCopy;
D.dCopy = () => {
  const html = dCopyBase();
  if (!S.d.agentTurn) return html;
  return html.replace(/(<div class="dreplyacts">[\s\S]*?)(<\/div>)/, `$1<span class="dagent" title="The agent that answered">Research</span>$2`);
};
VARIANTS.dCopy = VARIANTS.dCopy.map(([l, f, on]) => [l, () => { S.d.agentTurn = false; f(); }, () => !S.d.agentTurn && on()]);
VARIANTS.dCopy.push(["Answered by an agent", () => { S.d.copied = ""; S.d.agentTurn = true; render(); }, () => !!S.d.agentTurn]);

// A routine asking for a standing permission (request_grant): Allow or Deny only, since Always would not fit a grant.
const dApproveBase = D.dApprove;
D.dApprove = () => {
  const st = S.d.approve;
  if (st !== "grant" && st !== "granted") return dApproveBase();
  const ask = st === "grant";
  const card = ask ? `<div class="dcard">
      <div class="head"><span class="dapp">G</span><div style="flex:1;min-width:0;display:grid"><b style="font-size:13.5px">Reply to thread for a routine?</b><span style="font-size:12px;color:var(--d-muted)">Gmail asks first</span></div></div>
      <div class="kv"><span>Routine</span><span>Inbox sweep</span><span>Allows</span><span>1 reply each time it runs, without asking</span><span>To</span><span>ana@example.com only</span><span>Ends</span><span>When the routine ends, or in 8 h</span></div>
      <div class="acts"><button class="dbtn pri" data-dact="dset:approve:granted">Allow<kbd>⌘${DI.ret}</kbd></button><button class="dbtn warn" data-dact="dset:approve:denied">Deny<kbd>⌘D</kbd></button></div>
    </div>` : `<div class="dmuted">${I.shield.replace('width="18" height="18"', 'width="14" height="14"')}Allowed for this routine</div>`;
  const steps = [["Setting a reminder"], ["Asking to allow a routine", ask ? "run" : ""]];
  return deskScene(dBar((ask ? dAskF() : D_ASK) + `<div class="dbody">${dLog(`${dYou("Whenever Ana replies about Saturday, reply back for me")}
      ${ask ? dWork(steps, 3) : dDone(steps, 5)}
      <div class="dcoderen agent md">I set up Inbox sweep to watch that thread. To reply without asking each time, it needs your OK once.</div>
      ${card}${ask ? "" : '<div class="dcoderen agent md">Done. Inbox sweep can now reply in that thread on its own, for the next 8 hours. Revoke it in Routines, Grants.</div>'}`)}
    </div>` + dHints(ask ? [[`⌘${DI.ret}`, "allow"], ["⌘D", "deny"], ["esc", "to hide"]] : undefined)), { badge: ask });
};
VARIANTS.dApprove.push(["For a routine", () => { S.d.approve = "grant"; render(); }, () => S.d.approve === "grant"], ["Allowed for a routine", () => { S.d.approve = "granted"; render(); }, () => S.d.approve === "granted"]);

const dGroupMR = DESK_SCREENS.find(g => g.group === "Memories and routines").items;
dGroupMR.splice(dGroupMR.findIndex(it => it[0] === "dNewRoutine") + 1, 0, ["dEditRoutine", "Edit routine", "RoutineForm, edit"], ["dGrants", "Routine grants", "GrantView"]);
VARIANTS.dGrants = dv("gr", [["List", "list"], ["Revoke, confirm", "confirm"], ["Revoked", "revoked"], ["Empty", "empty"]]);
VARIANTS.dEditRoutine = dv("er", [["Time", "time"], ["Place", "place"]]);
window.addEventListener("keydown", e => {
  if (e.key !== "Escape" || (S.screen !== "dGrants" && S.screen !== "dEditRoutine")) return;
  e.preventDefault(); e.stopImmediatePropagation();
  if (S.screen === "dGrants" && S.d.gr === "confirm") { S.d.gr = "list"; return render(); }
  go("dRoutines");
}, true);
