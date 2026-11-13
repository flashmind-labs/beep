// ---- First run, and joining machines by ticket ----
// Desktops and beep-nodes work with each other directly, with no account.
{
  const TICKET = "beepjoin1example7qf3m9xa2kdpwt4h";
  const MONO = "font-family:var(--mono,monospace);font-size:12.5px;padding:9px 11px;border-radius:8px;background:#0f1011;border:1px solid var(--d-border)";

  // First run, over Home: the ticket card, as it is.
  const frCard = body => `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5">
    <div role="dialog" aria-label="First run" style="width:min(440px,86%);padding:20px 22px;border-radius:14px;border:1px solid var(--d-border);background:#151617;box-shadow:0 20px 60px rgba(0,0,0,.5);display:grid;gap:12px">${body}</div></div>`;
  const frOver = body => { const cc = S.d.cc; S.d.cc = "empty"; try { return D.dHome().replace(/<\/div><\/div>\s*$/, frCard(body) + "</div></div>"); } finally { S.d.cc = cc; } };
  const FR_LOGO = '<svg width="34" height="34" viewBox="0 0 28 28" fill="currentColor" aria-hidden="true"><path d="M14 1 27 27h-3.1L15.4 10v17h-2.8V10L4.1 27H1Z"/></svg>';
  const frBig = (title, text, attrs, pri) => `<button class="dbtn${pri ? " pri" : ""}" ${attrs} style="display:grid;gap:3px;justify-items:start;width:100%;height:auto;padding:13px 15px;text-align:left;white-space:normal"><b style="font-size:14px;font-weight:600">${title}</b><span style="font-size:12.5px;font-weight:400;opacity:.8;line-height:1.4">${text}</span></button>`;
  const frTicket = () => frOver(S.d.frlink ? `<div>${FR_LOGO}</div><div><b style="font-size:16px">Link another desktop</b><div style="font-size:12.5px;color:var(--d-muted);margin-top:4px;line-height:1.45">On your other desktop, open Machines, then Add a machine, choose Desktop and copy the ticket. This computer then shares its machines, projects and keys.</div></div>
      <input id="jt" aria-label="Ticket" placeholder="Paste a ticket" style="${MONO};width:100%;box-sizing:border-box">
      <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri" disabled>Link</button><button class="dbtn quiet" data-dact="dset:frlink:" data-dgo="dFirstRun">Back</button></div>` : `<div style="display:grid;gap:8px">${FR_LOGO}<div><b style="font-size:16px">Welcome to Beep</b><div style="font-size:12.5px;color:var(--d-muted);margin-top:2px">Run coding agents on your machines.</div></div></div>
      ${frBig("Start", "Use Beep on this computer. You can add machines later.", 'data-dact="dset:frpage:setup" data-dgo="dFirstRun"', true)}
      ${frBig("Link another desktop", "You already use Beep on another computer. Share its machines, projects and keys with this one.", 'data-dact="dset:frlink:1" data-dgo="dFirstRun"', false)}`);
  // First run, after Start: set up this computer. One button installs what is missing, then
  // opens each sign-in in the browser in turn. Every row can be skipped.
  const SU_TOOLS = [
    ["git", "Git", "2.47.1", "Projects and commits"],
    ["node", "Node.js", "", "Needed to install the agents"],
    ["claude", "Claude Code", "", "Coding agent"],
    ["codex", "Codex", "", "Coding agent"],
  ];
  const suRow = (name, sub, status, tone, action) => `<div style="display:grid;grid-template-columns:1fr auto;gap:10px;align-items:center;padding:10px 12px;border-top:1px solid var(--d-border)">
      <div style="min-width:0"><b style="font-size:13px;font-weight:600">${name}</b><div style="font-size:12px;color:var(--d-muted)">${sub}</div></div>
      <div style="display:flex;gap:8px;align-items:center;white-space:nowrap;font-size:12.5px;color:${tone}">${status}${action || ""}</div></div>`;
  const OK = "var(--d-emerald)", MUTED = "var(--d-muted)";
  const skip = k => S.d["suskip_" + k] ? "" : ` <button class="dbtn quiet small" data-dact="dset:suskip_${k}:1" data-dgo="dFirstRun">Skip</button>`;
  const frSetup = () => {
    const st = S.d.su || "ready";
    const rows = SU_TOOLS.map(([k, name, version, sub]) => {
      if (S.d["suskip_" + k]) return suRow(name, sub, "Skipped", MUTED);
      if (k === "git") return suRow(name, sub, `Installed · v${version}`, OK);
      if (st === "ready") return suRow(name, sub, "Not installed", MUTED, skip(k));
      if (st === "installing") return k === "node" ? suRow(name, sub, "Installed · v24.11.0", OK)
        : k === "claude" ? suRow(name, sub, '<span class="dspin"></span>Installing', MUTED) : suRow(name, sub, "Not installed", MUTED);
      if (st === "signin") return k === "node" ? suRow(name, sub, "Installed · v24.11.0", OK)
        : k === "claude" ? suRow(name, sub, "Signed in as you@example.com", OK)
        : suRow(name, sub, '<span class="dspin"></span>Signing in', MUTED);
      return suRow(name, sub, k === "node" ? "Installed · v24.11.0" : "Signed in as you@example.com", OK);
    }).join("");
    const missing = SU_TOOLS.filter(([k]) => k !== "git" && !S.d["suskip_" + k]).length;
    const foot = st === "ready" ? `<button class="dbtn pri" data-dact="dset:su:installing" data-dgo="dFirstRun"${missing ? "" : " disabled"}>Set up</button><span style="font-size:12px;color:var(--d-muted)">Installs what is missing, then signs in.</span><span style="margin-left:auto"><button class="dbtn quiet" data-dact="dset:frpage:" data-dgo="dHome">Later</button></span>`
      : st === "installing" ? `<button class="dbtn pri" disabled>Setting up…</button><span style="font-size:12px;color:var(--d-muted)">Installing Claude Code</span><span style="margin-left:auto"><button class="dbtn quiet" data-dact="dset:frpage:" data-dgo="dHome">Later</button></span>`
      : st === "signin" ? `<button class="dbtn pri" disabled>Setting up…</button><span style="font-size:12px;color:var(--d-muted)">Signing in to Codex in the sheet above</span><span style="margin-left:auto"><button class="dbtn quiet" data-dact="dset:frpage:" data-dgo="dHome">Later</button></span>`
      : `<button class="dbtn pri" data-dact="dset:frpage:" data-dgo="dHome">Open Beep</button><span style="font-size:12px;color:var(--d-muted)">This computer is ready to run agents.</span>`;
    return frOver(`<div style="display:grid;gap:8px">${FR_LOGO}<div><b style="font-size:16px">Set up this computer</b><div style="font-size:12.5px;color:var(--d-muted);margin-top:2px">What Beep needs to run coding agents here.</div></div></div>
      <div style="border:1px solid var(--d-border);border-radius:10px;overflow:hidden;margin-top:4px">${rows.replace('border-top:1px solid var(--d-border)', '')}</div>
      <div style="display:flex;gap:10px;align-items:center">${foot}</div>`);
  };
  D.dFirstRun = () => S.d.frpage === "setup" ? frSetup() : frTicket();
  VARIANTS.dFirstRun = [["Welcome", () => { S.d.frpage = ""; S.d.frlink = ""; render(); }, () => !S.d.frpage && !S.d.frlink], ["Link another desktop", () => { S.d.frpage = ""; S.d.frlink = "1"; render(); }, () => !S.d.frpage && !!S.d.frlink]];
  VARIANTS.dFirstRun.push(...[["Setup: ready", "ready"], ["Setup: installing", "installing"], ["Setup: signing in", "signin"], ["Setup: done", "done"]]
    .map(([l, k]) => [l, () => { S.d.frpage = "setup"; S.d.su = k; render(); }, () => S.d.frpage === "setup" && S.d.su === k]));

  // Add a machine by ticket. The ticket works once and lasts an hour.
  const line = (txt, toast) => `<div style="display:flex;gap:8px;align-items:center;margin-bottom:14px"><div style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;${MONO}">${txt}</div><button class="dbtn" data-toast="${toast}" style="flex:none">Copy</button></div>`;
  const step = t => `<div style="font-size:13px;color:var(--d-muted);margin-bottom:8px">${t}</div>`;
  const kindCard = (k, t, sub, on) => `<button class="dbtn" role="radio" aria-checked="${on}" aria-pressed="${on}" data-dact="dset:tk:${k}" data-dgo="dAddTicket" style="display:grid;gap:2px;text-align:left;padding:9px 12px;height:auto;${on ? "border-color:var(--d-emerald);background:rgba(76,183,130,.08)" : ""}"><b style="font-weight:600">${t}</b><span style="font-size:12px;color:var(--d-muted);font-weight:400">${sub}</span></button>`;
  const waiting = who => `<div style="display:flex;gap:8px;align-items:center;font-size:12.5px;color:var(--d-muted)"><span class="dspin"></span>Waiting for ${who} · 58 min left<span style="margin-left:auto"><button class="dbtn quiet" data-dact="dset:ts:expired" data-dgo="dAddTicket">Cancel</button></span></div>`;
  const note = `<div style="font-size:12.5px;color:var(--d-muted);margin-bottom:12px">Works once and expires in an hour.</div>`;

  const otherMac = () => {
    const s = S.d.ts || "empty";
    const val = ["pasted", "joining", "bad"].includes(s) ? TICKET : "";
    const msg = s === "joining" ? `<div style="display:flex;gap:8px;align-items:center;font-size:12.5px;color:var(--d-muted)"><span class="dspin"></span>Linking</div>`
      : s === "bad" ? `<div style="font-size:12.5px;color:var(--d-err)">This ticket expired or was already used. Ask for a new one.</div>` : "";
    const sheet = `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5"><div style="width:min(560px,86%);padding:18px 20px;border-radius:14px;border:1px solid var(--d-border);background:#151617;display:grid;gap:12px">
      <div style="display:flex;justify-content:space-between"><b style="font-size:15px">Link another desktop</b><kbd style="font-size:11px;opacity:.7">esc</kbd></div>
      <div style="font-size:12.5px;color:var(--d-muted);line-height:1.45">On the other desktop, open Machines, then Add a machine, choose Desktop and copy the ticket. Paste it here to share its machines, projects and keys.</div>
      <input id="jt" aria-label="Desktop ticket" placeholder="Paste a ticket" value="${val}" style="${MONO};width:100%;box-sizing:border-box">
      ${msg}
      <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri"${val && s !== "joining" ? ` data-dact="dset:ts:joining" data-dgo="dAddTicket"` : " disabled"}>Link</button><button class="dbtn quiet" data-dgo="dHome">Cancel</button></div></div></div>`;
    return D.dHome().replace(/<\/div><\/div>\s*$/, sheet + "</div></div>");
  };

  D.dAddTicket = () => {
    const k = S.d.tk || "node", s = S.d.ts || "ready";
    if (k === "other") return otherMac();
    const node = k !== "desktop" && k !== "phone", box = k === "container", who = k === "phone" ? "Phone" : box ? "ci-runner" : node ? "build-box" : "MacBook Air";
    let body;
    if (s === "joined") body = `<div style="display:flex;gap:10px;align-items:center;font-size:14px"><span class="ddot"></span><b>${k === "phone" ? "Phone paired" : who + " joined"}</b></div>
       <div style="font-size:12.5px;color:var(--d-muted);margin:8px 0 14px">${k === "phone" ? "Observe projects and agents from your phone. Phones do not host projects or run code, and stay out of Machines." : box ? "Linux, Claude Code. It shows under Machines and can run agents now." : node ? "Linux, Docker, Claude Code, Codex. It shows under Machines and can run agents now." : "macOS, Claude Code, Codex. It shows under Machines and can run agents now."}</div>
       <button class="dbtn pri" data-dgo="dHome">Done</button>`;
    else if (s === "expired") body = `<div style="font-size:13px;margin-bottom:12px">${k === "phone" ? "This code expired. Phone codes work once and last 5 minutes." : "This ticket expired. A ticket works once and lasts an hour."}</div><button class="dbtn pri" data-dact="dset:ts:ready" data-dgo="dAddTicket">New ticket</button>`;
    else if (node) body = `${step("1. Install beep-node on the machine")}
       ${line("curl -fsSL https://releases.flashmind.com/beep/node/install.sh | sh", "Copied")}
       ${step("2. Run this there")}
       ${line(`beep-node mesh join ${TICKET}`, "Copied")}
       ${s === "waiting" ? waiting("the machine") : note}`;
    else if (k === "phone") body = `${step("Scan with your phone")}<div style="width:200px;height:200px;background:#fff;color:#111;display:grid;place-items:center;margin:12px auto" aria-label="Phone pairing QR code">QR code</div><p>In Beep on your phone, open Linked devices and scan this code.</p><details><summary>Copy ticket instead</summary>${line(TICKET, "Copied")}</details><p>Expires in 5 minutes. Keep this ticket private.</p>${waiting("the phone")}`;
    else body = `${step("Ticket")}
       ${line(TICKET, "Copied")}
       <div style="font-size:12.5px;color:var(--d-muted);margin-bottom:12px">On the other desktop, open Machines, then Link another desktop, and paste this ticket.</div>
       ${s === "waiting" ? waiting("the other desktop") : note}`;
    const sheet = `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:flex-start;padding-top:90px;justify-content:center;z-index:5">
      <div style="width:min(560px,86%);padding:18px 20px;border-radius:14px;border:1px solid var(--d-border);background:#151617;box-shadow:0 20px 60px rgba(0,0,0,.5)">
      <div style="display:flex;justify-content:space-between;margin-bottom:12px"><b style="font-size:15px">Add a machine</b><kbd style="font-size:11px;opacity:.7">esc</kbd></div>
      ${s === "joined" || s === "expired" ? "" : `<div role="radiogroup" aria-label="Kind of machine" style="display:grid;grid-template-columns:1fr 1fr;gap:8px;margin-bottom:14px">${kindCard("node", "Node", "A server or any always-on box", k === "node")}${kindCard("container", "Container", "Removed after a day offline", box)}${kindCard("desktop", "Desktop", "macOS or Windows", k === "desktop")}${kindCard("phone", "Phone", "Observe projects and agents", k === "phone")}</div>`}${body}</div></div>`;
    return D.dHome().replace(/<\/div><\/div>\s*$/, sheet + "</div></div>");
  };
  const T = (l, kind, st) => [l, () => { S.d.tk = kind; S.d.ts = st; render(); }, () => (S.d.tk || "node") === kind && (S.d.ts || "ready") === st];
  VARIANTS.dAddTicket = [
    T("Node: ticket ready", "node", "ready"), T("Node: waiting", "node", "waiting"), T("Node: joined", "node", "joined"), T("Node: expired", "node", "expired"),
    T("Container: ticket ready", "container", "ready"), T("Container: joined", "container", "joined"),
    T("Desktop: ticket ready", "desktop", "ready"), T("Desktop: waiting", "desktop", "waiting"), T("Desktop: joined", "desktop", "joined"), T("Desktop: expired", "desktop", "expired"),
    T("Other desktop: Link another desktop", "other", "empty"), T("Other desktop: ticket pasted", "other", "pasted"), T("Other desktop: linking", "other", "joining"), T("Other desktop: ticket expired", "other", "bad"),
  ];

  DESK_SCREENS.find(g => g.group === "Command center").items.push(["dFirstRun", "First run", "CommandCenterWindow, FirstRun"], ["dAddTicket", "Add a machine by ticket", "CommandCenterWindow, AddMachineSheet, JoinWindow"]);
}
