// What Home's rows say and how its sections are laid out, apart from the
// DOM so it can be tested.

const CAPS = { gpu: "GPU", cuda: "CUDA", blender: "Blender", unreal: "Unreal", xcode: "Xcode", android: "Android", docker: "Docker" };
const AGENTS = { claude: { cls: "claude", ini: "CC", name: "Claude Code" }, codex: { cls: "codex", ini: "CX", name: "Codex" } };

export function base(path) { return String(path || "").split("/").filter(Boolean).pop() || ""; }

export function agentOf(name) {
  return AGENTS[name] || { cls: "", ini: String(name || "?").slice(0, 2).toUpperCase(), name: name || "Agent" };
}

/// What a machine can run: its abilities, graphics cards and apps, once
/// each. A named card stands in for the plain "GPU".
export function machineCaps(m) {
  const gpus = m.gpus.map(g => g.model);
  const out = m.capabilities.filter(c => !(c === "gpu" && gpus.length)).map(c => CAPS[c] || c);
  out.push(...gpus, ...m.apps.map(a => a.name));
  return [...new Set(out)];
}

/// The machine table's Now column.
export function machineNow(m) {
  if (!m.online) return "offline";
  return m.agents ? "working" : "idle";
}

/// A size in bytes, decimal as disks are sold: "850 MB", "4.3 GB",
/// "212 GB", "1.2 TB".
export function bytes(n) {
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let i = 0;
  while (n >= 1000 && i < units.length - 1) { n /= 1000; i++; }
  return `${n >= 10 || i === 0 ? Math.round(n) : n.toFixed(1)} ${units[i]}`;
}

/// GB from MiB, one decimal under 10: "9.1", "32".
function gb(mb) {
  const n = mb / 1024;
  return n >= 10 ? String(Math.round(n)) : n.toFixed(1);
}

/// The Disk column: free space where the machine keeps the user's files,
/// low under a tenth free. The title lists each volume measured.
export function machineDisk(m) {
  const disks = (m.online && m.stats?.disks) || [];
  const d = disks[0];
  if (!d || !d.total_bytes) return { text: "–", low: false, title: "" };
  return {
    text: `${bytes(d.free_bytes)} free`,
    low: d.free_bytes / d.total_bytes < 0.1,
    title: disks.map(d => `${d.path}: ${bytes(d.free_bytes)} free of ${bytes(d.total_bytes)}`).join("\n"),
  };
}

/// The Memory column: used of total, with the load in the title.
export function machineMemory(m) {
  const s = m.online && m.stats;
  if (!s || !s.memory_total_mb) return { text: "–", title: "" };
  const cores = m.env?.cpus ? ` on ${m.env.cpus} cores` : "";
  return {
    text: `${gb(s.memory_used_mb)} / ${gb(s.memory_total_mb)} GB`,
    title: s.load == null ? "" : `Load ${s.load.toFixed(2)}${cores}`,
  };
}

/// The OS line under a machine's name.
export function machineOs(m) {
  return m.env ? m.env.os_version || m.env.os : "";
}

/// A Running now row: a title and one muted line.
export function runningLine(r) {
  if (r.kind === "research") return { title: r.topic, sub: `Research · round ${r.round}` };
  const ag = agentOf(r.agent), project = base(r.cwd);
  return { title: r.title || project || ag.name, sub: [project || ag.name, r.machine_name].filter(Boolean).join(" · ") };
}

/// A Needs you row: a title, one muted line and its one button. A chat
/// approval is allowed from the row; an agent's question opens its session.
export function needsLine(n) {
  if (n.kind === "chat") {
    const a = n.pending.approval;
    const action = a.action ? a.action[0].toUpperCase() + a.action.slice(1) : "Allow this";
    return { title: `${action}?`, sub: [a.app, n.session_title].filter(Boolean).join(" · "), button: "Allow" };
  }
  return { title: n.question.text, sub: [agentOf(n.agent).name, n.title || n.machine_name].filter(Boolean).join(" · "), button: "Answer" };
}

/// Home's rows of sections under Machines, each one or two wide, imported
/// projects last. A section
/// with nothing in it is left out, and a row left empty goes too.
export function homeRows(home) {
  const has = list => Array.isArray(list) && list.length > 0;
  const pairs = [
    [has(home.running) && "running", has(home.needs_you) && "needs"],
    [has(home.updates) && "updates", has(home.coming_up) && "coming"],
    [has(home.projects) && "projects"],
  ];
  return pairs.map(p => p.filter(Boolean)).filter(p => p.length);
}

/// How wide Home's view must be, in CSS pixels, to split into two columns.
export const WIDE_HOME_PX = 1280;

/// Home's two columns on a wide window: what to act on at the left (Needs
/// you, Running now, Updates), Machines, Projects and Coming up at the right. Machines
/// counts whenever the server lists them, empty or not, as it offers Add a
/// machine. Null when a side would be empty, so Home keeps one column.
export function homeColumns(home) {
  const has = list => Array.isArray(list) && list.length > 0;
  const left = [has(home.needs_you) && "needs", has(home.running) && "running", has(home.updates) && "updates"].filter(Boolean);
  const right = [Array.isArray(home.machines) && "machines", has(home.projects) && "projects", has(home.coming_up) && "coming"].filter(Boolean);
  return left.length && right.length ? { left, right } : null;
}
