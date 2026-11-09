import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { MachineList } from "./list-view.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
// machines/: the Machines list, Home's machine table (table.ts) and the
// Add a machine sheet. Entry: this file.
import { openAddMachine } from "./addmachine.ts";
import { openJoinTicket } from "./jointicket.ts";
import { coders, errorText, isCoder, machineTag, nodeBehind, offeredNode, type NodeUpdate, shownVersion, updatedNotes, updatedTo, updateOffered } from "./machines-state.ts";
import { bar, S } from "../bar/core.ts";
import { relayLine, trafficRows, type MachineTraffic } from "./traffic-state.ts";
import { hints } from "../bar/hints.ts";
import { copy } from "../ui/parts.ts";
import { notice } from "../chat/media.ts";
import { errLine } from "../lists/memories.ts";
import { span } from "../lists/routines.ts";
import { bytes, esc } from "../ui/format.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { loadClocks } from "./clock.ts";
import { openSignIn } from "./signin.ts";
import { signInOffer } from "./signin-state.ts";
import { type AdapterInstall } from "./adapters-state.ts";

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

/// A machine or phone in the mesh (MachineView).
export interface Machine {
  id: string;
  name: string;
  online: boolean;
  can_host?: boolean;
  /// When it last connected or reported, in milliseconds since the Unix epoch.
  seen_at: number;
  version: string;
  /// Null until its first report.
  env: MachineEnv | null;
  /// Removed after a day offline, such as a container.
  temporary?: boolean;
  /// The agents whose ACP adapter it is installing or could not install.
  adapters?: AdapterInstall[];
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
  /// The newest beep-node release, null until read or while offline.
  released: string | null;
  /// Update's progress by machine.
  updates: Map<string, NodeUpdate>;
  /// Tools behind their newest release (ToolUpdate).
  tools: ToolUpdate[];
  /// A tool's Update by "<machine> <tool>": installing, or why it failed.
  toolUps: Map<string, { busy: boolean; error: string }>;
  /// The version each Update installed this session, by "<machine> <tool>".
  installed: Map<string, string>;
  /// What each machine moved over iroh (ListMachineTraffic).
  traffic: MachineTraffic[];
  /// Try again on a machine's adapters: asking, or why it failed.
  adapterTries: Map<string, { busy: boolean; error: string }>;
} = { list: null, error: "", mac: null, asking: null, updatable: new Set(), released: null, updates: new Map(), tools: [], toolUps: new Map(), installed: new Map(), traffic: [], adapterTries: new Map() };

/// Where the open machine looks for projects (GetMachineSearch): null until
/// read, or when its beep-node cannot say; `draft` is the field while
/// changing it.
let search: { id: string; folder: string | null; draft: string | null; busy: boolean; error: string } | null = null;

/// The open machine's log (GetMachineLog): `text` is null until read.
interface MachineLog { id: string; text: string | null; busy: boolean; error: string }
let log: MachineLog | null = null;

/// A tool on a machine with a newer release (ToolUpdate).
interface ToolUpdate {
  machine_id: string;
  tool: string;
  version: string;
  latest: string;
}

// ---------- Where Machines draws ----------

/// The window's Settings sheet shows Machines in its right pane: `root` is
/// the box it draws into instead of the page, `machine` the machine open
/// there (null for the list), and `opened` tells Settings when a click
/// opens one or goes back, so its breadcrumb follows.
interface MachinesHost {
  root: HTMLElement;
  machine: string | null;
  opened(id: string | null): void;
  /// The list changed: a machine came or went.
  changed(): void;
}
let host: MachinesHost | null = null;

/// Draws Machines into `root` from now on, until `machinesOut` or until
/// `root` leaves the page. The page's own view (`S.view`) is left alone.
export function machinesIn(root: HTMLElement, opened: MachinesHost["opened"], changed: MachinesHost["changed"]) {
  host = { root, machine: null, opened, changed };
}
export function machinesOut() {
  if (listRoot?.host === host?.root) clearList();
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
  if (log?.id !== id) log = null;
  drawMachines();
  await readList();
  if (id && MC.list?.some(m => m.id === id && m.can_host !== false)) void loadSearch(id);
  if (id && MC.list?.some(m => m.id === id && m.can_host === false)) return;
  // Looking the newest releases up can take a while; the page shows first.
  void loadNodeRelease();
  void loadToolUpdates();
}

let toolUpdatesLoading = false;
let toolUpdatesAgain = false;
async function loadToolUpdates() {
  if (toolUpdatesLoading) { toolUpdatesAgain = true; return; }
  toolUpdatesLoading = true;

  try {
    const tools = await invoke<ToolUpdate[]>("tool_updates");
    // The page only changes when a newer release shows up.
    const same = JSON.stringify(tools) === JSON.stringify(MC.tools);
    MC.tools = tools;
    if (shown() && !same) drawMachines();
  } catch (e) {
    console.warn("tool updates:", e);
  } finally {
    toolUpdatesLoading = false;

    if (toolUpdatesAgain) {
      toolUpdatesAgain = false;
      if (shown()) void loadToolUpdates();
    }
  }
}

/// Reads the machines and draws them while the page shows.
async function readList() {
  try {
    const [list, mac, updatable, traffic] = await Promise.all([
      invoke<Machine[]>("list_machines"),
      invoke<{ id: string | null }>("mac_control").catch(e => { console.warn("this computer's state:", e); return null; }),
      invoke<{ id: string }[]>("updatable_machines").catch(e => { console.warn("updatable machines:", e); return []; }),
      invoke<MachineTraffic[]>("machine_traffic").catch(e => { console.warn("machine traffic:", e); return []; }),
      loadClocks(invoke),
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
}

/// Starts the machine updates once per webview, from its entry point.
export function followToolUpdates() {
  // Opened before the machines connected: the page follows them as they come.
  for (const name of ["home-changed", "reconnected"]) listen(name, () => { if (shown()) { void readList(); void loadToolUpdates(); } });
  subscribeChanges(topic("machines"), () => readList(), null, shown);
}

async function loadNodeRelease() {
  const released = await invoke<string | null>("node_release").catch(e => { console.warn("beep-node release:", e); return null; });
  if (released === MC.released) return;
  MC.released = released;
  if (shown()) drawMachines();
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
  if (m.can_host === false) return "";
  const at = search?.id === m.id ? search : null;
  if (!at || at.folder === null) return "";
  const value = at.draft === null
    ? `<span class="dmono">${esc(at.folder)}</span>${m.online ? ' <button class="dbtn" id="mcSearchEdit">Change</button>' : ""}`
    : `<span class="dfield dline tight"><input class="dmono" id="mcSearch" value="${esc(at.draft)}" aria-label="Search for projects in" spellcheck="false"${at.busy ? " disabled" : ""}><button class="dbtn pri" id="mcSearchSave"${at.busy ? " disabled" : ""}>Save</button><button class="dbtn quiet" id="mcSearchNo">Cancel</button></span>`;
  return `<span>Projects in</span><span>${value}</span>`;
}

/// Reads machine `id`'s log.
async function readLog(id: string) {
  const at: MachineLog = log = { id, text: log?.id === id ? log.text : null, busy: true, error: "" };
  drawMachines();

  try {
    at.text = await invoke<string>("machine_log", { id });
  } catch (e) {
    at.error = errorText(e);
  }

  at.busy = false;
  if (shown()) drawMachines();
}

async function saveLog(m: Machine) {
  try {
    const path = await invoke<string>("save_machine_log", { name: m.name, text: log?.text ?? "" });
    notice(`Saved ${path.split("/").pop()} to Downloads`, { path });
  } catch (e) {
    notice(`Could not save it. ${e}`, { bad: true });
  }
}

/// The Log section: Show, then the lines with Refresh, Copy and Save.
function logSection(m: Machine) {
  const at = log?.id === m.id ? log : null;
  if (!at && !m.online) return "";
  const read = (label: string) => `<button class="dbtn" id="mcLog"${at?.busy ? " disabled" : ""}>${label}</button>`;
  let body = at ? "" : read("Show");
  if (at?.busy && at.text === null) body = '<div class="dmuted">Reading…</div>';
  else if (at?.error) body = errLine(at.error) + read("Try again");
  else if (at?.text === "") body = `<div class="dmuted">Nothing logged lately.</div>${read("Refresh")}`;
  else if (at?.text) body = `<pre class="dlog dmono">${esc(at.text)}</pre><div class="dlog-acts">${read("Refresh")}<button class="dbtn" id="mcLogCopy">Copy</button><button class="dbtn" id="mcLogSave">Save</button></div>`;
  return `<div class="dset-h">Log</div>${body}`;
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
    MC.installed.set(key, to);
    MC.tools = MC.tools.filter(u => !(u.machine_id === id && u.tool === tool));
    const found = MC.list?.find(m => m.id === id)?.env?.tools.find(t => t.name === tool);
    if (found) found.version = to;
  } catch (e) {
    MC.toolUps.set(key, { busy: false, error: errorText(e) });
  }

  if (shown()) drawMachines();
}

/// Try again: the machine installs the adapters it could not, now.
async function retryAdapters(id: string) {
  if (MC.adapterTries.get(id)?.busy) return;
  MC.adapterTries.set(id, { busy: true, error: "" });
  if (shown()) drawMachines();

  try {
    await invoke("agent_adapters_install", { machineId: id });
    MC.adapterTries.delete(id);
    // The node reads them as installing at once; its report follows.
    for (const a of MC.list?.find(m => m.id === id)?.adapters ?? []) a.failed = null;
  } catch (e) {
    MC.adapterTries.set(id, { busy: false, error: errorText(e) });
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
    hosted()?.changed();
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

/// Restart deadlines, including extra time for running sessions to finish.
const RESTART_POLL_MS = 3000, RESTART_POLLS = 60, IDLE_POLLS = RESTART_POLLS + 600;

/// Follows restart reports until the requested version appears, with the existing deadline.
async function waitForNode(id: string, to: string, polls: number) {
  await new Promise<void>(resolve => {
    let reading = false, again = false, done = false;
    const finish = () => { done = true; clearTimeout(deadline); unfollow(); resolve(); };
    const check = async () => {
      if (reading) { again = true; return; }
      reading = true;

      do {
        again = false;
        const now = MC.updates.get(id);
        if (done) break;
        if (now?.step !== "restarting" || now.to !== to) { finish(); break; }

        try {
          const list = await invoke<Machine[]>("list_machines");
          if (done) break;
          MC.list = list;
          const machine = list.find(machine => machine.id === id);

          if (machine && updatedTo(machine, to)) {
            MC.updates.delete(id);
            if (shown()) drawMachines();
            finish();
          }
        } catch (error) { console.warn("machines while a node restarts:", error); }
      } while (again && !done);

      reading = false;
    };
    const unfollow = subscribeChanges(topic("machines"), () => check());
    const deadline = setTimeout(() => {
      const now = MC.updates.get(id);

      if (now?.step === "restarting" && now.to === to) {
        MC.updates.set(id, { step: "failed", error: `It did not come back on beep-node ${to} yet. Check the machine.` });
        if (shown()) drawMachines();
      }

      finish();
    }, RESTART_POLL_MS * polls);
    void check();
  });
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

const removeNote = (m: Machine) => m.can_host === false ? "It disconnects from your mesh." : m.online ? "It stops taking work; its sessions and projects go with it." : "Its sessions and projects go with it. If it comes back online, it is turned away.";

/// Remove on every machine but this Mac, and the question it asks.
function removeParts(m: Machine) {
  if (m.id === MC.mac) return { button: "", ask: "" };
  const button = `<button class="dbtn warn" data-rm="${esc(m.id)}" aria-label="Remove ${esc(m.name)}">Remove…</button>`;
  const asking = MC.asking?.id === m.id ? MC.asking : null;
  if (!asking) return { button, ask: "" };
  const what = removeNote(m);
  return {
    button: "",
    ask: `<div class="ask"><span>Remove ${esc(m.name)} from your account? ${what} ${m.can_host === false ? "To reconnect, pair it again." : "To use it again, add it with a new setup command."}</span>
      ${asking.error ? `<span class="derr">${esc(asking.error)}</span>` : ""}
      <span class="acts"><button class="dbtn warn" id="rmYes"${asking.busy ? " disabled" : ""}>Remove</button><button class="dbtn quiet" id="rmNo">Cancel</button></span></div>`,
  };
}

/// "container" after the name of a machine removed after a day offline.
function tagChip(m: Machine) {
  const tag = machineTag(m);
  return tag ? `<span class="dtag" title="Removed after a day offline">${esc(tag)}</span>` : "";
}

const statusTag = (m: Machine) => m.online ? '<span class="dtag ok">Online</span>' : `<span class="dtag">Seen ${span(Date.now() - m.seen_at)} ago</span>`;
const nodeVersion = (m: Machine) => /^\d/.test(m.version) ? `beep-node ${m.version}` : m.version;
/// The newest beep-node released or on the account's machines, which the
/// others are behind.
// Only nodes that update themselves count: the Mac app's built-in node is
// whatever version the app was built with, which may never have been released.
const newest = () => offeredNode(MC.released, (MC.list ?? []).filter(m => MC.updatable.has(m.id)).map(m => m.version));

/// One machine on its own page in Settings: what it reported, its
/// sign-ins, every tool with its version, and Remove.
function machinePage(m: Machine) {
  const env = m.env, { button, ask } = removeParts(m);
  const kv = (k: string, v: string) => `<span>${k}</span><span>${v}</span>`;
  const behind = m.id !== MC.mac && nodeBehind(m.version, newest()) ? ` <span class="dtag ask">${esc(newest() ?? "")} is out</span>` : "";
  const facts = kv("Status", statusTag(m) + (machineTag(m) ? ` ${tagChip(m)}` : "")) + (m.can_host === false ? kv("Device", "Phone") + kv("Role", "Controller") : kv("beep-node", esc(nodeVersion(m)) + behind + updateControl(m)))
    + (env ? kv("System", esc(`${env.os_version}, ${env.arch}`)) + kv("Hardware", esc(`${env.cpus} CPUs, ${Math.round(env.memory_mb / 1024)} GB`))
      + kv("Host name", `<span class="dmono">${esc(env.hostname)}</span>`) + kv("Home", `<span class="dmono">${esc(env.home)}</span>`) + searchRow(m)
      + (env.avds.length ? kv("Android emulators", esc(env.avds.join(", "))) : "")
      : m.can_host === false ? "" : kv("Report", "None yet"));
  let html = `<div class="dset-box"><div class="dset-kv">${facts}</div></div>` + updateLine(m) + (search?.id === m.id ? errLine(search.error) : "");

  if (m.can_host !== false && env?.logins.length) {
    const rows = env.logins.map(l => {
      const who = l.signed_in ? esc(l.account ?? "Signed in") : '<span class="dtag ask">Not signed in</span>';
      const offer = l.signed_in ? null : signInOffer(l.tool, m.name, m.online, m.id === MC.mac);
      const act = !offer ? "" : "how" in offer ? `<small class="dmuted">${esc(offer.how)}</small>`
        : `<button class="dbtn small" data-signin="${esc(l.tool)}">Sign in</button>`;
      return `<tr><td class="dmono">${esc(l.tool)}</td><td>${who}</td><td style="text-align:right">${act}</td></tr>`;
    }).join("");
    html += `<div class="dset-h">Sign-ins</div><table class="dset-box dset-tbl"><thead><tr><th>Tool</th><th>Account</th><th></th></tr></thead><tbody>${rows}</tbody></table>`;
  }

  if (m.can_host !== false && env?.tools.length) {
    const tools = [...coders(env.tools), ...env.tools.filter(t => !isCoder(t.name))];
    const rows = tools.map(t => `<tr><td class="dmono">${esc(t.name)}</td><td class="dmono">${esc(shownVersion(MC.installed, `${m.id} ${t.name}`, t.name, t.version) ?? "")}</td><td>${isCoder(t.name) ? "Coding agent" : "Tool"}</td></tr>`).join("");
    html += `<div class="dset-hrow"><div class="dset-h">Tools</div>${tools.map(t => toolUpdate(m, t)).join("")}${toolNotes(m, env.tools)}</div><table class="dset-box dset-tbl"><thead><tr><th>Tool</th><th>Version</th><th>Kind</th></tr></thead><tbody>${rows}</tbody></table>`;
  }

  html += trafficTable(m) + (m.can_host === false ? "" : logSection(m));
  return html + `<div class="dmach dset-rm">${button ? `<div class="dset-rm-row"><span class="t"><b>Remove this machine</b><small>${removeNote(m)}</small></span>${button}</div>` : ""}${ask}</div>`;
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

/// Update above a machine's Tools table, when a newer release of `t` is out.
function toolUpdate(m: Machine, t: MachineEnv["tools"][number]) {
  const up = MC.tools.find(u => u.machine_id === m.id && u.tool === t.name);
  if (!up || !m.online || !updateOffered(MC.installed, `${m.id} ${t.name}`, up.latest)) return "";
  const busy = MC.toolUps.get(`${m.id} ${t.name}`)?.busy;
  return `<button class="dbtn small" data-toolup="${esc(t.name)}" data-id="${esc(m.id)}"${busy ? " disabled" : ""}>${busy ? `Updating ${esc(t.name)}…` : `Update ${esc(t.name)} to ${esc(up.latest)}`}</button>`;
}

/// In the Tools row: what Update installed this session, and why one failed.
function toolNotes(m: Machine, tools: MachineEnv["tools"]) {
  const done = updatedNotes(MC.installed, m.id, tools).map(n => `<small class="dmuted">${esc(n)}</small>`);
  const failed = [...MC.toolUps].filter(([k, up]) => k.startsWith(`${m.id} `) && up.error).map(([k, up]) => `<small class="derr">${esc(`${k.slice(m.id.length + 1)}: ${up.error}`)}</small>`);
  return done.join("") + failed.join("");
}

let listRoot: { host: HTMLElement; mount: HTMLElement; root: Root } | null = null;

function clearList() {
  if (!listRoot) return;
  listRoot.root.unmount();
  listRoot.mount.remove();
  listRoot = null;
}

function drawList(el: HTMLElement, at: MachinesHost | null) {
  if (listRoot && (listRoot.host !== el || listRoot.mount.parentElement !== el)) clearList();
  if (!listRoot) {
    const mount = document.createElement("div");
    mount.style.display = "contents";
    el.replaceChildren(mount);
    listRoot = { host: el, mount, root: createRoot(mount) };
  }
  const refreshAfterSheet = () => { if (shown()) void renderMachines(hosted()?.machine ?? null).then(() => hosted()?.changed()); };
  flushSync(() => listRoot?.root.render(createElement(MachineList, {
    list: MC.list, error: MC.error, hosted: !!at, mac: MC.mac, newest: newest(), asking: MC.asking,
    adapterTries: MC.adapterTries, hints: at ? "" : hints([["esc", "back"]]), age: span,
    open: (id: string) => { if (at) { at.opened(id); void renderMachines(id); } },
    add: () => openAddMachine({ onClose: refreshAfterSheet }),
    link: () => openJoinTicket({ onClose: refreshAfterSheet }),
    askRemove: (id: string) => { MC.asking = { id, busy: false, error: "" }; drawMachines(); el.querySelector<HTMLElement>("#rmNo")?.focus(); },
    cancelRemove: () => { MC.asking = null; drawMachines(); },
    remove: () => { void removeMachine(); },
    retryAdapter: (id: string) => { void retryAdapters(id); },
  })));
}

function drawMachines() {
  const at = hosted(), el = root();
  const open = at?.machine ? MC.list?.find(m => m.id === at.machine) : undefined;

  if (at?.machine && MC.list && !open) {
    // Gone, such as just removed: back to the list.
    at.machine = null;
    at.opened(null);
  }

  if (!open && !at?.machine) {
    drawList(el, at);
    return;
  }

  clearList();
  if (open) {
    el.innerHTML = errLine(MC.error) + machinePage(open);
  } else {
    el.innerHTML = `<div class="dskel-rows">${[78, 56, 68].map(w => `<div class="dskel" style="width:${w}%"></div>`).join("")}</div>`;
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

  el.querySelector("#mcLinkDesktop")?.addEventListener("click", () => openJoinTicket({ onClose: () => { if (shown()) void renderMachines(hosted()?.machine ?? null).then(() => hosted()?.changed()); } }));
  el.querySelector("#mcAdd")?.addEventListener("click", () => openAddMachine({ onClose: () => { if (shown()) void renderMachines(hosted()?.machine ?? null).then(() => hosted()?.changed()); } }));
  el.querySelector("#rmYes")?.addEventListener("click", removeMachine);

  if (open) {
    for (const b of el.querySelectorAll<HTMLElement>("[data-signin]")) b.addEventListener("click", () => openSignIn(open, b.dataset.signin!, () => {
      if (shown()) void renderMachines(open.id).then(() => hosted()?.changed());
    }));
    el.querySelector("#mcLog")?.addEventListener("click", () => { void readLog(open.id); });
    el.querySelector<HTMLElement>("#mcLogCopy")?.addEventListener("click", e => { void copy(log?.text ?? "", e.currentTarget as HTMLElement); });
    el.querySelector("#mcLogSave")?.addEventListener("click", () => { void saveLog(open); });
    const lines = el.querySelector(".dlog");
    if (lines) lines.scrollTop = lines.scrollHeight;
  }

  for (const b of el.querySelectorAll<HTMLElement>("[data-nodeup]")) b.addEventListener("click", () => { void runUpdate(b.dataset.nodeup!); });
  for (const b of el.querySelectorAll<HTMLElement>("[data-toolup]")) b.addEventListener("click", () => { void updateTool(b.dataset.id!, b.dataset.toolup!); });
  for (const b of el.querySelectorAll<HTMLElement>("[data-adapters]")) b.addEventListener("click", e => { e.stopPropagation(); void retryAdapters(b.dataset.adapters!); });
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
