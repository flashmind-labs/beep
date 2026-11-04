// machines/: the Machines list, Home's machine table (table.ts) and the
// Add a machine sheet. Entry: this file.
import { openAddMachine } from "./addmachine.ts";
import { harnessTile } from "../agents/harnessicon.ts";
import { bareVersion, coderColumns, coders, errorText, isCoder, newestNode, nodeBehind, type NodeUpdate, specLine, toolChips, updatedTo } from "./machines-state.ts";
import { bar, esc, ICON, invoke, S } from "../bar/core.ts";
import { bytes } from "../center/home-state.ts";
import { relayLine, trafficRows, type MachineTraffic } from "./traffic-state.ts";
import { hints } from "../bar/hints.ts";
import { emptyPage, errLine } from "../lists/memories.ts";
import { span } from "../lists/routines.ts";

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
  /// The machines Update works on.
  updatable: Set<string>;
  /// Update's progress by machine.
  updates: Map<string, NodeUpdate>;
  /// Tools behind their newest release (ToolUpdate).
  tools: ToolUpdate[];
  /// A tool's Update by "<machine> <tool>": installing, or why it failed.
  toolUps: Map<string, { busy: boolean; error: string }>;
  /// What each machine moved over iroh (ListMachineTraffic).
  traffic: MachineTraffic[];
} = { list: null, error: "", mac: null, asking: null, updatable: new Set(), updates: new Map(), tools: [], toolUps: new Map(), traffic: [] };

/// Where the open machine looks for projects (GetMachineSearch): null until
/// read, or when its beep-node cannot say; `draft` is the field while
/// changing it.
let search: { id: string; folder: string | null; draft: string | null; busy: boolean; error: string } | null = null;

/// A tool on a machine with a newer release (ToolUpdate).
interface ToolUpdate {
  machine_id: string;
  tool: string;
  version: string;
  latest: string;
}
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
  if (id) void loadSearch(id);
  drawMachines();
  try {
    const [list, mac, updatable, traffic] = await Promise.all([
      invoke<Machine[]>("list_machines"),
      invoke<{ id: string | null }>("mac_control").catch(e => { console.warn("this Mac's state:", e); return null; }),
      invoke<{ id: string }[]>("updatable_machines").catch(e => { console.warn("updatable machines:", e); return []; }),
      invoke<MachineTraffic[]>("machine_traffic").catch(e => { console.warn("machine traffic:", e); return []; }),
    ]);
    MC.list = list;
    MC.traffic = traffic;
    MC.updatable = new Set(updatable.map(u => u.id));
    MC.mac = mac?.id || null;
    MC.error = "";
  } catch (e) {
    MC.error = String(e);
  }
  if (shown()) drawMachines();
  // Looking the newest releases up can take a while; the page shows first.
  try {
    MC.tools = await invoke<ToolUpdate[]>("tool_updates");
    if (shown()) drawMachines();
  } catch (e) {
    console.warn("tool updates:", e);
  }
}

async function loadSearch(id: string) {
  search = { id, folder: null, draft: null, busy: false, error: "" };
  try {
    const folder = await invoke<string>("machine_search", { machineId: id });
    if (search?.id === id) search.folder = folder;
  } catch (e) {
    console.warn("search folder:", e);
  }
  if (shown()) drawMachines();
}

/// Has the open machine look for projects in `folder` and search again.
async function saveSearch(folder: string) {
  const at = search;
  if (!at || at.busy) return;
  at.busy = true;
  at.draft = folder;
  at.error = "";
  drawMachines();
  try {
    at.folder = await invoke<string>("set_machine_search", { machineId: at.id, search: folder });
    at.draft = null;
  } catch (e) {
    at.error = errorText(e);
  }
  at.busy = false;
  if (shown()) drawMachines();
}

/// The "Projects in" row: the folder and Change, or the field to change it.
function searchRow(m: Machine) {
  const at = search?.id === m.id ? search : null;
  if (!at || at.folder === null) return "";
  const value = at.draft === null
    ? `<span class="dmono">${esc(at.folder)}</span>${m.online ? ' <button class="dbtn" id="mcSearchEdit">Change</button>' : ""}`
    : `<span class="dfield" style="display:flex;gap:6px;align-items:center"><input class="dmono" id="mcSearch" value="${esc(at.draft)}" aria-label="Search for projects in" spellcheck="false"${at.busy ? " disabled" : ""}><button class="dbtn pri" id="mcSearchSave"${at.busy ? " disabled" : ""}>Save</button><button class="dbtn quiet" id="mcSearchNo">Cancel</button></span>`;
  return `<span>Projects in</span><span>${value}</span>`;
}

/// Installs the newest release of `tool` on machine `id`.
async function updateTool(id: string, tool: string) {
  const key = `${id} ${tool}`;
  if (MC.toolUps.get(key)?.busy) return;
  MC.toolUps.set(key, { busy: true, error: "" });
  if (shown()) drawMachines();
  try {
    const to = await invoke<string>("update_machine_tool", { machineId: id, tool });
    MC.toolUps.delete(key);
    MC.tools = MC.tools.filter(u => !(u.machine_id === id && u.tool === tool));
    const found = MC.list?.find(m => m.id === id)?.env?.tools.find(t => t.name === tool);
    if (found) found.version = to;
  } catch (e) {
    MC.toolUps.set(key, { busy: false, error: errorText(e) });
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

/// Has the server update machine `id`'s beep-node, then waits for it to
/// come back on the new version.
async function runUpdate(id: string) {
  MC.updates.set(id, { step: "updating" });
  if (shown()) drawMachines();
  try {
    const done = await invoke<{ from: string; to: string; restarting: boolean; note: string }>("update_machine", { id });
    if (done.restarting) {
      MC.updates.set(id, { step: "restarting", to: done.to, note: done.note });
      void waitForNode(id, done.to, done.note ? IDLE_POLLS : RESTART_POLLS);
    } else if (done.from === done.to) {
      MC.updates.set(id, { step: "note", text: `beep-node ${done.to} is the newest release.` });
    } else {
      MC.updates.set(id, { step: "note", text: `Installed beep-node ${done.to}. ${done.note}` });
    }
  } catch (e) {
    MC.updates.set(id, { step: "failed", error: errorText(e) });
  }
  if (shown()) drawMachines();
}

/// How often, and how many times, the list is read while a node restarts;
/// one waiting for its sessions has up to half an hour more.
const RESTART_POLL_MS = 3000, RESTART_POLLS = 60, IDLE_POLLS = RESTART_POLLS + 600;

/// Reads the list `polls` times until machine `id` is back on `to`, or
/// gives up.
async function waitForNode(id: string, to: string, polls: number) {
  for (let i = 0; i < polls; i++) {
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
  if (!m.online || !nodeBehind(m.version, newest())) return "";
  if (MC.updatable.has(m.id)) return ` <button class="dbtn" data-nodeup="${esc(m.id)}">${up?.step === "failed" ? "Try again" : "Update"}</button>`;
  if (m.id === MC.mac) return "";
  return ' <span class="dmuted">Run <span class="dmono">beep-node update</span> on the machine</span>';
}

/// Under the facts: Update's error or its note.
function updateLine(m: Machine) {
  const up = MC.updates.get(m.id);
  if (up?.step === "failed") return errLine(up.error);
  if (up?.step === "note" || (up?.step === "restarting" && up.note)) return `<div class="dmuted">${esc(up.step === "note" ? up.text : up.note)}</div>`;
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
      + kv("Host name", `<span class="dmono">${esc(env.hostname)}</span>`) + kv("Home", `<span class="dmono">${esc(env.home)}</span>`) + searchRow(m)
      + (env.avds.length ? kv("Android emulators", esc(env.avds.join(", "))) : "")
      : kv("Report", "None yet"));
  let html = `<div class="dset-box"><div class="dset-kv">${facts}</div></div>` + updateLine(m) + (search?.id === m.id ? errLine(search.error) : "");
  if (env?.logins.length) {
    const rows = env.logins.map(l => {
      const who = l.signed_in ? esc(l.account ?? "Signed in") : '<span class="dtag ask">Not signed in</span>';
      return `<tr><td class="dmono">${esc(l.tool)}</td><td>${who}</td></tr>`;
    }).join("");
    html += `<div class="dset-h">Sign-ins</div><table class="dset-box dset-tbl"><thead><tr><th>Tool</th><th>Account</th></tr></thead><tbody>${rows}</tbody></table>`;
  }
  if (env?.tools.length) {
    const tools = [...coders(env.tools), ...env.tools.filter(t => !isCoder(t.name))];
    const rows = tools.map(t => `<tr><td class="dmono">${esc(t.name)}</td><td>${toolVersion(m, t)}</td><td>${isCoder(t.name) ? "Coding agent" : "Tool"}</td></tr>`).join("");
    html += `<div class="dset-h">Tools</div><table class="dset-box dset-tbl"><thead><tr><th>Tool</th><th>Version</th><th>Kind</th></tr></thead><tbody>${rows}</tbody></table>`;
    html += [...MC.toolUps].filter(([k, up]) => k.startsWith(`${m.id} `) && up.error).map(([, up]) => errLine(up.error)).join("");
  }
  html += trafficTable(m);
  return html + `<div class="dmach dset-rm">${button ? `<span class="nm">${button}</span>` : ""}${ask}</div>`;
}

/// What the machine moved per protocol, once it has reported.
function trafficTable(m: Machine) {
  const traffic = MC.traffic.find(t => t.id === m.id), rows = trafficRows(traffic);
  if (!rows.length) return "";
  const body = rows.map(r => `<tr><td>${esc(r.label)}</td><td class="dmono">${bytes(r.sent)}</td><td class="dmono">${bytes(r.received)}</td></tr>`).join("");
  const relay = relayLine(traffic);
  return `<div class="dset-h">Traffic</div><table class="dset-box dset-tbl"><thead><tr><th>Protocol</th><th>Sent</th><th>Received</th></tr></thead><tbody>${body}</tbody></table>`
    + (relay ? `<div class="dmuted">Through a relay: ${relay}.</div>` : "");
}

/// A tool's version on a machine's page, with "to <latest>" and Update
/// when a newer release is out.
function toolVersion(m: Machine, t: MachineEnv["tools"][number]) {
  const version = `<span class="dmono">${esc(bareVersion(t.name, t.version) ?? "")}</span>`;
  const up = MC.tools.find(u => u.machine_id === m.id && u.tool === t.name);
  if (!up || !m.online) return version;
  const busy = MC.toolUps.get(`${m.id} ${t.name}`)?.busy;
  return `${version} <span class="dmuted">to</span> <span class="dmono">${esc(up.latest)}</span> <button class="dbtn" data-toolup="${esc(t.name)}" data-id="${esc(m.id)}"${busy ? " disabled" : ""}>${busy ? "Updating…" : "Update"}</button>`;
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
    // Rows that stand in for the machine until the list comes.
    el.innerHTML = `<div class="dskel-rows">${[78, 56, 68].map(w => `<div class="dskel" style="width:${w}%"></div>`).join("")}</div>`;
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
  for (const b of el.querySelectorAll<HTMLElement>("[data-nodeup]")) b.addEventListener("click", () => { void runUpdate(b.dataset.nodeup!); });
  for (const b of el.querySelectorAll<HTMLElement>("[data-toolup]")) b.addEventListener("click", () => { void updateTool(b.dataset.id!, b.dataset.toolup!); });
  el.querySelector("#rmNo")?.addEventListener("click", () => { MC.asking = null; drawMachines(); });
  el.querySelector("#mcSearchEdit")?.addEventListener("click", () => {
    if (!search) return;
    search.draft = search.folder;
    drawMachines();
    el.querySelector<HTMLInputElement>("#mcSearch")?.focus();
  });
  const field = el.querySelector<HTMLInputElement>("#mcSearch");
  field?.addEventListener("keydown", e => {
    if (e.key === "Enter") void saveSearch(field.value);
    if (e.key === "Escape") { e.stopPropagation(); stopEditing(); }
  });
  el.querySelector("#mcSearchSave")?.addEventListener("click", () => { if (field) void saveSearch(field.value); });
  el.querySelector("#mcSearchNo")?.addEventListener("click", stopEditing);
}

function stopEditing() {
  if (!search) return;
  search.draft = null;
  search.error = "";
  drawMachines();
}
