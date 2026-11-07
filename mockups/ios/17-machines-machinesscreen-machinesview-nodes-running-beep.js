// ---- Machines (MachinesScreen, MachinesView): nodes running beep-node, and a Mac with "Let Beep use this Mac" on ----
const MACHINES = [
  { name: "build-box", online: true, seen: "Online", ver: "0.4.2", os: "Ubuntu 24.04", arch: "x86_64", cpus: 16, mem: 64, home: "/srv/project",
    tools: [["claude", "2.1.4"], ["codex", "0.48.0"], ["git", "2.43.0"], ["cargo", "1.93.0"], ["docker", "27.3.1"], ["node", "22.11.0"], ["just", "1.36.0"]],
    logins: [["gh", "build-user"], ["claude", ""], ["codex", null]], avds: ["Medium_Phone_API_35"] },
  { name: "Alex's MacBook Air", online: true, seen: "Online", ver: "Beep 0.3.16", os: "macOS 26.1", arch: "arm64", cpus: 10, mem: 16, home: "/srv/project",
    tools: [["desktop", ""], ["git", "2.50.1"], ["gemini", "0.9.0"]], logins: [["gemini", ""]], avds: [] },
  { name: "gpu-box", online: false, seen: "Last seen 3 hours ago", ver: "0.4.1", os: "Debian 13", arch: "x86_64", cpus: 32, mem: 128, home: "/srv/project",
    tools: [["python", "3.12.7"], ["nvidia-smi", "570.86"], ["git", "2.47.2"]], logins: [], avds: [] },
];
// What the node reports live (ListMachineStats): memory in use, load and each volume. Offline machines have none.
MACHINES[0].stats = { mem: "9.1 / 64 GB", load: "1.25 on 16 cores", disks: [["/srv/project", 212, "212 GB", "931 GB"], ["/srv/data", 38, "38 GB", "1.9 TB", 0.02]] };
MACHINES[1].stats = { mem: "5.4 / 16 GB", load: "0.82 on 10 cores", disks: [["/srv/project", 38, "38 GB", "500 GB", 0.076]] };
const signIns = m => m.logins.filter(([, a]) => a !== null).map(([t, a]) => a ? `${t} as ${a}` : `${t} signed in`).join(", ");
const mIcon = (m, size = 40) => `<span class="logo" style="width:${size}px;height:${size}px;border-radius:${Math.round(size / 3.5)}px;background:${TINTS[m.online ? 1 : 2]}">${size > 44 ? I.server.replace('width="20" height="20"', 'width="26" height="26"') : I.server}</span>`;
const mTag = t => `<span style="font-size:11px;font-weight:500;padding:1px 8px;border-radius:999px;border:1px solid var(--m-soft)">${t}</span>`;
function machineCard(m, i) {
  const tools = m.tools.slice(0, 4).map(([t]) => mTag(t)).join("") + (m.tools.length > 4 ? `<span class="muted" style="font-size:11px;align-self:center">+${m.tools.length - 4}</span>` : "");
  const report = S.machD === "noreport" && i === 0 ? `<span style="font-size:12px;color:var(--m-dim)">No report yet</span>`
    : `<span class="muted" style="font-size:12px">${m.os}, ${m.arch}</span><span style="display:flex;flex-wrap:wrap;gap:4px 6px;margin-top:4px">${tools}</span>${signIns(m) ? `<span class="muted" style="font-size:12px">${signIns(m)}</span>` : ""}`;
  // Disk free and memory in use on one line, coral when the first volume is under a tenth free.
  const sx = m.online && m.stats && !(S.machD === "noreport" && i === 0) ? m.stats : null, d0 = sx && sx.disks[0], low = d0 && d0[4];
  const statsLine = sx ? `<span style="display:flex;font-size:12px;margin-top:2px;color:var(--m-muted)">${d0 ? `<span style="${low ? "color:var(--m-error);font-weight:500" : ""}">${d0[2]} free</span><span style="color:var(--m-dim);white-space:pre"> · </span>` : ""}<span>${sx.mem} memory</span></span>` : "";
  return `<button class="card row" style="text-align:left;width:100%;color:inherit;font:inherit" data-go="machine" data-set="machI:${i}">${mIcon(m)}
    <div class="grow"><b style="font-weight:600;font-size:15px">${m.name}</b><span class="muted" style="font-size:12px;display:flex;align-items:center;gap:6px"><span class="dot${m.online ? "" : " off"}" style="${m.online ? "" : "background:var(--m-dim)"}"></span>${m.seen}</span>${report}${statsLine}</div></button>`;
}
R.machines = () => {
  const st = S.mach || "list";
  const list = st === "loading" ? `<div style="display:grid;place-items:center;padding:40px"><span class="spin" style="width:22px;height:22px"></span></div>`
    : st === "empty" ? `<div class="muted" style="text-align:center;font-size:13px;line-height:20px;padding:40px 12px">No machines yet.<br>Run beep-node link on a Mac or Linux box, then paste its link here.</div>`
    : MACHINES.map(machineCard).join("") + `<div class="muted" style="font-size:12px;line-height:18px;padding:0 4px">Beep runs commands and coding agents on these when you ask. Each one runs beep-node and is linked like a computer.</div>`;
  return `<div class="bar" style="padding-left:6px"><button class="ib" data-go="settings" aria-label="Back">${I.back}</button><div class="title" style="flex:1">Machines</div></div>
    <div class="body" style="gap:8px">${list}<div style="flex:1"></div>
    <button class="primary" style="display:flex;align-items:center;justify-content:center;gap:8px;flex:none" data-go="linkMachine" data-set="linkM:typing">${small(I.server, 18)}Link a machine</button></div>`;
};
R.machine = () => {
  const m = MACHINES[+(S.machI || 0)];
  const facts = (rows, mono = []) => `<div class="card" style="padding:10px 14px;display:grid;gap:6px">${rows.map(([k, v]) => `<div style="display:flex;font-size:13px"><span style="${v === "" ? "" : "width:96px;flex:none;color:var(--m-muted)"}">${k}</span>${v === "" ? "" : `<span style="min-width:0;overflow-wrap:anywhere${mono.includes(k) ? ";font-family:var(--mono);font-size:12px" : ""}">${v}</span>`}</div>`).join("")}</div>`;
  const sec = t => `<div class="label" style="padding:8px 4px 0">${t}</div>`;
  const noreport = S.machD === "noreport";
  // Memory and load, then each volume measured with how much is free; only while online and measured.
  const sx = m.online && m.stats;
  const live = !sx ? "" : sec("Memory") + facts([["In use", sx.mem], ["Load", sx.load]])
    + sec("Disk") + `<div class="card" style="padding:10px 14px;display:grid;gap:8px">${sx.disks.map(([path, , free, total, frac]) => `<div style="display:grid;gap:1px"><span style="font-family:var(--mono);font-size:13px">${path}</span><span style="font-size:12px;${frac && frac < 0.1 ? "color:var(--m-error);font-weight:500" : "color:var(--m-muted)"}">${free} free of ${total}</span></div>`).join("")}</div>`;
  const body = noreport ? `<div class="muted" style="font-size:13px;padding:4px">It has not reported what it has yet.</div>`
    : sec("System") + facts([["OS", m.os], ["Chip", m.arch], ["CPUs", m.cpus], ["Memory", `${m.mem} GB`], ["Host", m.name.includes(" ") ? "alex-mba" : m.name], ["Home", m.home]], ["Home"])
      + (m.logins.length ? sec("Signed in") + facts(m.logins.map(([t, a]) => [t, a === null ? "Not signed in" : a || "Signed in"])) : "")
      + sec("Tools") + facts(m.tools.map(([t, v]) => [t, v]))
      + (m.avds.length ? sec("Android emulators") + facts(m.avds.map(a => [a, ""])) : "");
  return `<div class="bar" style="padding-left:6px"><button class="ib" data-go="machines" aria-label="Back">${I.back}</button><div class="title" style="flex:1;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${m.name}</div></div>
    <div class="body" style="gap:8px"><div class="row" style="align-items:center;padding:0 4px">${mIcon(m, 52)}<div class="grow">
      <b style="font-weight:500;font-size:14px;display:flex;align-items:center;gap:6px"><span class="dot" style="${m.online ? "" : "background:var(--m-dim)"}"></span>${m.online ? "Online" : "Offline, " + m.seen.toLowerCase()}</b>
      <span class="muted" style="font-size:12px">${m.ver.startsWith("Beep") ? m.ver + ", Let Beep use this Mac" : "beep-node " + m.ver}</span></div></div>${live}${body}</div>`;
};
// "Link a machine" opens the same Type the code dialog; a beep-link: link pasted whole is accepted (isCode).
R.linkMachine = () => {
  const st = S.linkM || "typing";
  if (st === "confirm") return R.machines() + `<div class="scrim" data-go="machines"></div>
    <div class="sheet" role="dialog" aria-label="Link lab-box">
      <div class="grab"></div>
      <div class="row" style="align-items:center"><span class="logo" style="width:52px;height:52px;border-radius:var(--radius);background:var(--m-t2)">${I.server}</span>
        <div class="grow"><h3>Link lab-box?</h3><span class="muted" style="font-size:13px">Computer</span></div></div>
      <div style="font-size:15px;line-height:22px">It will see your chats and memories.</div>
      <div class="card" style="padding:10px 14px;display:grid;grid-template-columns:auto 1fr;gap:4px 14px;font-size:13px">
        <span class="muted">Code</span><span style="font-family:var(--mono);letter-spacing:.1em">Q3M 8KD</span>
        <span class="muted">Stays here</span><span>Contacts, alarms, location</span></div>
      <div class="muted" style="font-size:12px">Check that the code matches the one on the computer.</div>
      <div class="end"><button class="text-btn" data-go="machines">Cancel</button><button class="go" style="box-shadow:0 3px 0 var(--m-ledge)" data-go="machines" data-toast="lab-box linked. Run beep-node run there">Link</button></div>
    </div>`;
  return R.machines() + `<div class="dialog-wrap"><div class="dialog" role="dialog" aria-label="Type the code"><h3>Type the code</h3>
    <div class="field" style="display:flex;align-items:center;font-family:var(--mono);font-size:13px;overflow:hidden;white-space:nowrap;text-overflow:ellipsis">${st === "empty" ? '<span style="color:var(--m-dim);letter-spacing:.12em">K7QM4-X9TRP</span>' : "beep-link:eyJuIjoibGFiLWJveCIsImsiOiJ…"}</div>
    <div class="muted" style="font-size:12px;margin-top:-6px">The code your other device shows under Settings, Devices, or the link beep-node link prints.</div>
    <div class="end"><button class="text-btn" data-go="machines">Cancel</button><button class="go" ${st === "empty" ? 'disabled style="opacity:.45"' : 'data-set="linkM:confirm"'}>Next</button></div></div></div>`;
};
VARIANTS.machines = vs("mach", [["Three machines", "list"], ["Loading", "loading"], ["None yet", "empty"]]);
VARIANTS.machine = [["build-box", () => { S.machI = "0"; S.machD = ""; render(); }, () => (S.machI || "0") === "0" && S.machD !== "noreport"],
  ["This Mac", () => { S.machI = "1"; S.machD = ""; render(); }, () => S.machI === "1"],
  ["Offline", () => { S.machI = "2"; S.machD = ""; render(); }, () => S.machI === "2"],
  ["No report yet", () => { S.machI = "0"; S.machD = "noreport"; render(); }, () => S.machD === "noreport"]];
VARIANTS.linkMachine = vs("linkM", [["Link pasted", "typing"], ["Empty", "empty"], ["Confirm", "confirm"]]);

Object.assign(I_GO, { machines: "iMachines", machine: "iMachine", linkMachine: "iLinkMachine", settings: "iSettings" });
IOS_SCREENS.find(g => g.group === "Settings").items.splice(1, 0, ["iMachines", "Machines", "MachinesView"], ["iMachine", "Machine", "MachinesView, detail"], ["iLinkMachine", "Link a machine", "LinkCodes, beep-link"]);
[["iMachines", "machines"], ["iMachine", "machine"], ["iLinkMachine", "linkMachine"]].forEach(([i, a]) => { P[i] = () => iGroup(R[a]()); VARIANTS[i] = VARIANTS[a]; });
