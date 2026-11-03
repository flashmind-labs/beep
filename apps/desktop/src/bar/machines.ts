import { openAddMachine } from "../addmachine.ts";
import { harnessTile } from "../harnessicon.ts";
import { asksToStop, bareVersion, coderColumns, coders, errorText, isCoder, newestNode, nodeBehind, type NodeUpdate, sessionsStop, specLine, toolChips, updatedTo } from "../machines-state.ts";
import { bar, esc, ICON, invoke, S } from "./core.ts";
import { hints } from "./hints.ts";
import { emptyPage, errLine } from "./memories.ts";
import { span } from "./routines.ts";

// ---------- Machines ----------

/// What a machine reported about itself (MachineEnv).
export interface MachineEnv {
  /// `linux` or `macos`.
  os: string;
  /// Such as "Ubuntu 24.04" or "macOS 26.3".
  os_version: string;
  arch: string;
  hostname: string;
  cpus: number;
  memory_mb: number;
  home: string;
  tools: { name: string; version: string | null }[];
  /// Whether a CLI is signed in, such as gh.
  logins: { tool: string; signed_in: boolean; account: string | null }[];
  avds: string[];
}

/// A machine running beep-node (MachineView).
export interface Machine {
  id: string;
  name: string;
  online: boolean;
  /// When it last connected or reported, in milliseconds since the Unix epoch.
  seen_at: number;
  version: string;
  /// Null until its first report.
  env: MachineEnv | null;
}

// The machines running beep-node (ListMachines), from Settings. `mac` is
// this Mac's own id, which unlinks from Settings rather than here; `asking`
// the machine whose Remove waits for an answer.
export const MC: {
  list: Machine[] | null;
  error: string;
  mac: string | null;
  asking: { id: string; busy: boolean; error: string } | null;
  /// The machines Update works on, with the agent sessions it would stop.
  updatable: Map<string, number>;
  /// Update's progress by machine.
  updates: Map<string, NodeUpdate>;
} = { list: null, error: "", mac: null, asking: null, updatable: new Map(), updates: new Map() };
const SERVER_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>';

// ---------- Where Machines draws ----------

/// The window's Settings sheet shows Machines in its right pane: `root` is
/// the box it draws into instead of the page, `machine` the machine open
/// there (null for the list), and `opened` tells Settings when a click
/// opens one or goes back, so its breadcrumb follows.
interface MachinesHost {
  root: HTMLElement;
  machine: string | null;
  opened(id: string | null): void;
}
let host: MachinesHost | null = null;

/// Draws Machines into `root` from now on, until `machinesOut` or until
/// `root` leaves the page. The page's own view (`S.view`) is left alone.
export function machinesIn(root: HTMLElement, opened: MachinesHost["opened"]) {
  host = { root, machine: null, opened };
}
export function machinesOut() {
  host = null;
}
const hosted = () => (host?.root.isConnected ? host : null);
const root = () => hosted()?.root ?? bar;
/// Whether Machines is on screen, in Settings or on the page.
const shown = () => !!hosted() || S.view === "machines";

/// The list, or in Settings machine `id` on its own page.
export async function renderMachines(id: string | null = null) {
  const at = hosted();
  if (at) {
    at.machine = id;
  } else {
    S.view = "machines";
    invoke("close_group").catch(() => {});
  }
  MC.asking = null;
  drawMachines();
  try {
    const [list, mac, updatable] = await Promise.all([
      invoke<Machine[]>("list_machines"),
      invoke<{ id: string | null }>("mac_control").catch(e => { console.warn("this Mac's state:", e); return null; }),
      invoke<{ id: string; agent_sessions: number }[]>("updatable_machines").catch(e => { console.warn("updatable machines:", e); return []; }),
    ]);
    MC.list = list;
    MC.updatable = new Map(updatable.map(u => [u.id, u.agent_sessions]));
    MC.mac = mac?.id || null;
    MC.error = "";
  } catch (e) {
    MC.error = String(e);
  }
  if (shown()) drawMachines();
}

/// Unlinks the machine Remove asked about, online or not.
async function removeMachine() {
  const asking = MC.asking;
  if (!asking || asking.busy) return;
  asking.busy = true;
  asking.error = "";
  drawMachines();
  try {
    await invoke("remove_machine", { id: asking.id });
    MC.list = (MC.list || []).filter(m => m.id !== asking.id);
    MC.asking = null;
  } catch (e) {
    asking.busy = false;
    asking.error = String(e);
  }
  if (shown()) drawMachines();
}

/// Update on a machine's page: asks first when agent sessions run there.
function startUpdate(id: string) {
  const sessions = MC.updatable.get(id) ?? 0;
  if (sessions > 0) {
    MC.updates.set(id, { step: "confirm", text: sessionsStop(sessions) });
    if (shown()) drawMachines();
    return;
  }
  void runUpdate(id, false);
}

/// Has the server update machine `id`'s beep-node, then waits for it to
/// come back on the new version.
async function runUpdate(id: string, stopSessions: boolean) {
  MC.updates.set(id, { step: "updating" });
  if (shown()) drawMachines();
  try {
    const done = await invoke<{ from: string; to: string; restarting: boolean; note: string }>("update_machine", { id, stopSessions });
    if (done.restarting) {
      MC.updates.set(id, { step: "restarting", to: done.to });
      void waitForNode(id, done.to);
    } else if (done.from === done.to) {
      MC.updates.set(id, { step: "note", text: `beep-node ${done.to} is the newest release.` });
    } else {
      MC.updates.set(id, { step: "note", text: `Installed beep-node ${done.to}. ${done.note}` });
    }
  } catch (e) {
    const text = errorText(e);
    MC.updates.set(id, asksToStop(text) ? { step: "confirm", text } : { step: "failed", error: text });
  }
  if (shown()) drawMachines();
}

/// How often, and how many times, the list is read while a node restarts.
const RESTART_POLL_MS = 3000, RESTART_POLLS = 60;

/// Reads the list until machine `id` is back on `to`, or gives up.
async function waitForNode(id: string, to: string) {
  for (let i = 0; i < RESTART_POLLS; i++) {
    await new Promise(r => setTimeout(r, RESTART_POLL_MS));
    const now = MC.updates.get(id);
    if (now?.step !== "restarting" || now.to !== to) return;
    try {
      MC.list = await invoke<Machine[]>("list_machines");
    } catch (e) {
      console.warn("machines while a node restarts:", e);
      continue;
    }
    const m = MC.list.find(m => m.id === id);
    if (m && updatedTo(m, to)) {
      MC.updates.delete(id);
      if (shown()) drawMachines();
      return;
    }
  }
  MC.updates.set(id, { step: "failed", error: `It did not come back on beep-node ${to} yet. Check the machine.` });
  if (shown()) drawMachines();
}

/// The beep-node row's end on a machine's page: Update, its progress, or
/// how to update from the machine.
function updateControl(m: Machine) {
  const up = MC.updates.get(m.id);
  if (up?.step === "updating") return ' <span class="dtag ask">Updating…</span>';
  if (up?.step === "restarting") return ` <span class="dtag ask">Restarting on ${esc(up.to)}…</span>`;
  if (!m.online || !nodeBehind(m.version, newest()) || up?.step === "confirm") return "";
  if (MC.updatable.has(m.id)) return ` <button class="dbtn" data-nodeup="${esc(m.id)}">${up?.step === "failed" ? "Try again" : "Update"}</button>`;
  if (m.id === MC.mac) return "";
  return ' <span class="dmuted">Run <span class="dmono">beep-node update</span> on the machine</span>';
}

/// Under the facts: Update's question, its error or its note.
function updateLine(m: Machine) {
  const up = MC.updates.get(m.id);
  if (up?.step === "confirm") {
    return `<div class="dmach dset-rm"><div class="ask"><span>${esc(up.text)}</span>
      <span class="acts"><button class="dbtn warn" id="nuYes" data-id="${esc(m.id)}">Update anyway</button><button class="dbtn" id="nuNo" data-id="${esc(m.id)}">Cancel</button></span></div></div>`;
  }
  if (up?.step === "failed") return errLine(up.error);
  if (up?.step === "note") return `<div class="dmuted">${esc(up.text)}</div>`;
  return "";
}

/// Remove on every machine but this Mac, and the question it asks.
function removeParts(m: Machine) {
  if (m.id === MC.mac) return { button: "", ask: "" };
  const button = `<button class="dbtn quiet" data-rm="${esc(m.id)}" aria-label="Remove ${esc(m.name)}">Remove…</button>`;
  const asking = MC.asking?.id === m.id ? MC.asking : null;
  if (!asking) return { button, ask: "" };
  const what = m.online ? "It stops taking work." : "If it comes back online, it is turned away.";
  return {
    button: "",
    ask: `<div class="ask"><span>Remove ${esc(m.name)} from your account? ${what} To use it again, add it with a new setup command.</span>
      ${asking.error ? `<span class="derr">${esc(asking.error)}</span>` : ""}
      <span class="acts"><button class="dbtn warn" id="rmYes"${asking.busy ? " disabled" : ""}>Remove</button><button class="dbtn" id="rmNo">Cancel</button></span></div>`,
  };
}

const statusTag = (m: Machine) => m.online ? '<span class="dtag ok">Online</span>' : `<span class="dtag">Seen ${span(Date.now() - m.seen_at)} ago</span>`;
const nodeVersion = (m: Machine) => /^\d/.test(m.version) ? `beep-node ${m.version}` : m.version;
/// The newest beep-node on the account's machines, which the others are behind.
const newest = () => newestNode((MC.list ?? []).map(m => m.version));

/// The beep-node chip, marked when an update is out.
function nodeChip(m: Machine) {
  const behind = nodeBehind(m.version, newest());
  const title = behind ? `beep-node ${m.version}; ${newest()} is out` : nodeVersion(m);
  return `<span class="dmnv" title="${esc(title)}">${esc(nodeVersion(m))}${behind ? '<span class="up">update</span>' : ""}</span>`;
}

/// A coding agent's chip in its column: mark, bare version, and a dot for
/// its sign-in, green when signed in and red when not.
function agentChip(env: MachineEnv, t: MachineEnv["tools"][number], column: number) {
  const version = bareVersion(t.name, t.version) ?? "";
  const login = env.logins.find(l => l.tool === t.name);
  const dot = !login ? "" : `<span class="${login.signed_in ? "in" : "out"}"></span>`;
  const title = `${t.name} ${version}${!login ? "" : login.signed_in ? ", signed in" : ", not signed in"}`;
  return `<span class="dmag" style="grid-column:${column}" title="${esc(title)}">${harnessTile(t.name, "dmag-ic")}<span class="v">${esc(version)}</span>${dot}</span>`;
}

/// A machine in the list: name, status, spec and beep-node on the first line, a
/// column per coding agent on the second, the other tools on the third.
function machineCard(m: Machine, columns: string[]) {
  const env = m.env;
  const opens = !!hosted();
  const { button, ask } = opens ? { button: "", ask: "" } : removeParts(m);
  const open = opens ? ` data-machine="${esc(m.id)}" role="button" tabindex="0" aria-label="${esc(m.name)}"` : "";
  const seen = m.online ? "" : `<span class="seen">Seen ${span(Date.now() - m.seen_at)} ago</span>`;
  const spec = env ? specLine(env) : "No report yet";
  const line1 = `<span class="l1"><span class="ddot${m.online ? "" : " off"}" title="${m.online ? "Online" : "Offline"}"></span><b>${esc(m.name)}</b>${seen}<span class="spec" title="${esc(spec)}">${esc(spec)}</span>${nodeChip(m)}${button}</span>`;
  const chev = opens ? `<span class="ch">${ICON.chev}</span>` : "";
  if (!env) return `<div class="dmach"${open}>${line1}${chev}${ask}</div>`;
  const agents = coders(env.tools);
  const line2 = agents.length
    ? agents.map(t => agentChip(env, t, columns.indexOf(t.name) + 1)).join("")
    : '<span class="none">No coding agents</span>';
  const { shown, more } = toolChips(env.tools);
  const line3 = shown.length
    ? `<span class="l3">${shown.map(n => `<span class="dmt-t">${esc(n)}</span>`).join("")}${more.length ? `<span class="dmt-t" title="${esc(more.join(", "))}">+${more.length}</span>` : ""}</span>`
    : "";
  return `<div class="dmach"${open}>${line1}${chev}${line2}${line3}${ask}</div>`;
}

/// One machine on its own page in Settings: what it reported, its
/// sign-ins, every tool with its version, and Remove.
function machinePage(m: Machine) {
  const env = m.env, { button, ask } = removeParts(m);
  const kv = (k: string, v: string) => `<span>${k}</span><span>${v}</span>`;
  const behind = nodeBehind(m.version, newest()) ? ` <span class="dtag ask">${esc(newest() ?? "")} is out</span>` : "";
  const facts = kv("Status", statusTag(m)) + kv("beep-node", esc(nodeVersion(m)) + behind + updateControl(m))
    + (env ? kv("System", esc(`${env.os_version}, ${env.arch}`)) + kv("Hardware", esc(`${env.cpus} CPUs, ${Math.round(env.memory_mb / 1024)} GB`))
      + kv("Host name", `<span class="dmono">${esc(env.hostname)}</span>`) + kv("Home", `<span class="dmono">${esc(env.home)}</span>`)
      + (env.avds.length ? kv("Android emulators", esc(env.avds.join(", "))) : "")
      : kv("Report", "None yet"));
  let html = `<div class="dset-box"><div class="dset-kv">${facts}</div></div>` + updateLine(m);
  if (env?.logins.length) {
    const rows = env.logins.map(l => {
      const who = l.signed_in ? esc(l.account ?? "Signed in") : '<span class="dtag ask">Not signed in</span>';
      return `<tr><td class="dmono">${esc(l.tool)}</td><td>${who}</td></tr>`;
    }).join("");
    html += `<div class="dset-h">Sign-ins</div><table class="dset-box dset-tbl"><thead><tr><th>Tool</th><th>Account</th></tr></thead><tbody>${rows}</tbody></table>`;
  }
  if (env?.tools.length) {
    const tools = [...coders(env.tools), ...env.tools.filter(t => !isCoder(t.name))];
    const rows = tools.map(t => `<tr><td class="dmono">${esc(t.name)}</td><td class="dmono">${esc(bareVersion(t.name, t.version) ?? "")}</td><td>${isCoder(t.name) ? "Coding agent" : "Tool"}</td></tr>`).join("");
    html += `<div class="dset-h">Tools</div><table class="dset-box dset-tbl"><thead><tr><th>Tool</th><th>Version</th><th>Kind</th></tr></thead><tbody>${rows}</tbody></table>`;
  }
  return html + `<div class="dmach dset-rm">${button ? `<span class="nm">${button}</span>` : ""}${ask}</div>`;
}

/// Add a machine, which opens the same sheet as the window's Home; the
/// list loads again once it closes.
const addButton = '<button class="dbtn" id="mcAdd">Add a machine</button>';

function drawMachines() {
  const at = hosted(), el = root();
  const open = at?.machine ? MC.list?.find(m => m.id === at.machine) : undefined;
  if (at?.machine && MC.list && !open) {
    // Gone, such as just removed: back to the list.
    at.machine = null;
    at.opened(null);
  }
  if (open) {
    el.innerHTML = errLine(MC.error) + machinePage(open);
  } else if (at?.machine) {
    el.innerHTML = setSkeletonRows();
  } else {
    const list = MC.list ?? [], on = list.filter(m => m.online).length;
    const sum = !MC.list ? "" : [on && `${on} online`, list.length - on && `${list.length - on} offline`].filter(Boolean).join(", ");
    const say = esc(sum || "Where Beep runs commands and coding agents");
    // In Settings the breadcrumb names the page; the sum goes under it.
    const head = at ? `<div class="dmc-head"><span class="dmuted">${say}</span>${addButton}</div>` : `<div class="dhead"><span class="tt"><b>Machines</b><small>${say}</small></span>${addButton}</div>`;
    const columns = coderColumns(list.map(m => m.env?.tools ?? []));
    el.innerHTML = head + errLine(MC.error) + (MC.list && !list.length
      ? emptyPage(SERVER_ICON, "No machines yet.", "Add one to run commands and coding agents on it.")
      : `<div class="dlist${at ? " dset-box" : ""}"><div class="dmt" style="--ag:${columns.length}">${list.map(m => machineCard(m, columns)).join("")}</div></div>`)
      + (at ? "" : hints([["esc", "back"]]));
  }
  for (const b of el.querySelectorAll<HTMLElement>("[data-machine]")) {
    const go = () => {
      at!.opened(b.dataset.machine!);
      renderMachines(b.dataset.machine!);
    };
    b.addEventListener("click", go);
    b.addEventListener("keydown", e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); go(); } });
  }
  for (const b of el.querySelectorAll<HTMLElement>("[data-rm]")) {
    b.addEventListener("click", () => {
      MC.asking = { id: b.dataset.rm!, busy: false, error: "" };
      drawMachines();
      el.querySelector<HTMLElement>("#rmNo")?.focus();
    });
  }
  el.querySelector("#mcAdd")?.addEventListener("click", () => openAddMachine({ onClose: () => { if (shown()) renderMachines(hosted()?.machine ?? null); } }));
  el.querySelector("#rmYes")?.addEventListener("click", removeMachine);
  for (const b of el.querySelectorAll<HTMLElement>("[data-nodeup]")) {
    b.addEventListener("click", () => startUpdate(b.dataset.nodeup!));
  }
  el.querySelector<HTMLElement>("#nuYes")?.addEventListener("click", e => { void runUpdate((e.currentTarget as HTMLElement).dataset.id!, true); });
  el.querySelector<HTMLElement>("#nuNo")?.addEventListener("click", e => { MC.updates.delete((e.currentTarget as HTMLElement).dataset.id!); drawMachines(); });
  el.querySelector("#rmNo")?.addEventListener("click", () => { MC.asking = null; drawMachines(); });
}

/// Rows that stand in for a machine until the list comes.
const setSkeletonRows = () => `<div class="dskel-rows">${[78, 56, 68].map(w => `<div class="dskel" style="width:${w}%"></div>`).join("")}</div>`;
