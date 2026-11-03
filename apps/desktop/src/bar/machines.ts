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
} = { list: null, error: "", mac: null, asking: null };
const SERVER_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>';
/// The coding agents Beep can drive on a machine; its other tools follow them.
const CODERS = new Set(["claude", "codex", "gemini", "opencode", "cursor-agent", "amp", "aider"]);

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
    const [list, mac] = await Promise.all([
      invoke<Machine[]>("list_machines"),
      invoke<{ id: string | null }>("mac_control").catch(e => { console.warn("this Mac's state:", e); return null; }),
    ]);
    MC.list = list;
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

/// A machine in the list. In Settings it opens on its own page, and
/// Remove waits there.
function machineCard(m: Machine) {
  const env = m.env, tag = statusTag(m), version = nodeVersion(m);
  const opens = !!hosted();
  const { button, ask } = opens ? { button: ICON.chev, ask: "" } : removeParts(m);
  const open = opens ? ` data-machine="${esc(m.id)}" role="button" tabindex="0" aria-label="${esc(m.name)}"` : "";
  if (!env) return `<div class="dmach"${open}><span class="nm">${esc(m.name)}${tag}${button}</span><small>${esc(version)} · No report yet</small>${ask}</div>`;
  const coders = env.tools.filter(t => CODERS.has(t.name)), rest = env.tools.filter(t => !CODERS.has(t.name)).map(t => t.name);
  const logins = env.logins.filter(l => l.signed_in).map(l => l.account ? `${l.tool} as ${l.account}` : `${l.tool} signed in`);
  const also = [rest.length ? `Also ${rest.join(", ")}` : "", logins.join(", ")].filter(Boolean).join(" · ");
  return `<div class="dmach"${open}><span class="nm">${esc(m.name)}${tag}${button}</span>
    <small>${esc(`${env.os_version}, ${env.arch} · ${env.cpus} CPUs, ${Math.round(env.memory_mb / 1024)} GB · ${version}`)}</small>
    <span class="cd">${coders.length ? esc("Coding: " + coders.map(t => t.version ? `${t.name} ${t.version}` : t.name).join(", ")) : '<span style="color:var(--muted)">No coding tools</span>'}</span>
    ${also ? `<small>${esc(also)}</small>` : ""}${ask}</div>`;
}

/// One machine on its own page in Settings: what it reported, every tool
/// with its version and sign-in, and Remove.
function machinePage(m: Machine) {
  const env = m.env, { button, ask } = removeParts(m);
  const kv = (k: string, v: string) => `<span>${k}</span><span>${v}</span>`;
  const facts = kv("Status", statusTag(m)) + kv("beep-node", esc(nodeVersion(m)))
    + (env ? kv("System", esc(`${env.os_version}, ${env.arch}`)) + kv("Hardware", esc(`${env.cpus} CPUs, ${Math.round(env.memory_mb / 1024)} GB`))
      + kv("Host name", `<span class="dmono">${esc(env.hostname)}</span>`) + kv("Home", `<span class="dmono">${esc(env.home)}</span>`)
      + (env.avds.length ? kv("Android emulators", esc(env.avds.join(", "))) : "")
      : kv("Report", "None yet"));
  let html = `<div class="dset-box"><div class="dset-kv">${facts}</div></div>`;
  if (env?.tools.length) {
    const login = (tool: string) => env.logins.find(l => l.tool === tool);
    const rows = [...env.tools].sort((a, b) => Number(CODERS.has(b.name)) - Number(CODERS.has(a.name))).map(t => {
      const l = login(t.name);
      const who = !l ? '<span class="dmuted">n/a</span>' : l.signed_in ? esc(l.account ?? "Signed in") : '<span class="dtag ask">Signed out</span>';
      return `<tr><td class="dmono">${esc(t.name)}</td><td class="dmono">${esc(t.version ?? "")}</td><td>${CODERS.has(t.name) ? "Coding agent" : "Tool"}</td><td>${who}</td></tr>`;
    }).join("");
    html += `<div class="dset-h">Tools</div><table class="dset-box dset-tbl"><thead><tr><th>Tool</th><th>Version</th><th>Kind</th><th>Signed in</th></tr></thead><tbody>${rows}</tbody></table>`;
  }
  return html + `<div class="dmach dset-rm">${button ? `<span class="nm">${button}</span>` : ""}${ask}</div>`;
}

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
    const head = at ? `<div class="dmuted" style="display:block;padding:0 0 10px">${say}</div>` : `<div class="dhead"><span class="tt"><b>Machines</b><small>${say}</small></span></div>`;
    const setup = '<code>beep-node setup</code>';
    el.innerHTML = head + errLine(MC.error) + (MC.list && !list.length
      ? emptyPage(SERVER_ICON, "No machines yet.", `Press Add a machine in the Beep window and run the ${setup} command it shows on that computer, or turn on Let Beep use this Mac in Settings.`)
      : `<div class="dlist${at ? " dset-box" : ""}" style="padding-top:6px">${list.map(machineCard).join("")}</div>
        ${MC.list ? `<div class="dmuted" style="display:block;padding:0 ${at ? 0 : 18}px 12px">To add one, press Add a machine in the Beep window and run the ${setup} command it shows.</div>` : ""}`)
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
  el.querySelector("#rmYes")?.addEventListener("click", removeMachine);
  el.querySelector("#rmNo")?.addEventListener("click", () => { MC.asking = null; drawMachines(); });
}

/// Rows that stand in for a machine until the list comes.
const setSkeletonRows = () => `<div class="dskel-rows">${[78, 56, 68].map(w => `<div class="dskel" style="width:${w}%"></div>`).join("")}</div>`;
