import { bar, esc, invoke, S } from "./core.ts";
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

export async function renderMachines() {
  S.view = "machines";
  invoke("close_group").catch(() => {});
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
  if (S.view === "machines") drawMachines();
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
  if (S.view === "machines") drawMachines();
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

function machineCard(m: Machine) {
  const env = m.env, now = Date.now();
  const tag = m.online ? '<span class="dtag ok">Online</span>' : `<span class="dtag">Seen ${span(now - m.seen_at)} ago</span>`;
  const version = /^\d/.test(m.version) ? `beep-node ${m.version}` : m.version;
  const { button, ask } = removeParts(m);
  if (!env) return `<div class="dmach"><span class="nm">${esc(m.name)}${tag}${button}</span><small>${esc(version)} · No report yet</small>${ask}</div>`;
  const coders = env.tools.filter(t => CODERS.has(t.name)), rest = env.tools.filter(t => !CODERS.has(t.name)).map(t => t.name);
  const logins = env.logins.filter(l => l.signed_in).map(l => l.account ? `${l.tool} as ${l.account}` : `${l.tool} signed in`);
  const also = [rest.length ? `Also ${rest.join(", ")}` : "", logins.join(", ")].filter(Boolean).join(" · ");
  return `<div class="dmach"><span class="nm">${esc(m.name)}${tag}${button}</span>
    <small>${esc(`${env.os_version}, ${env.arch} · ${env.cpus} CPUs, ${Math.round(env.memory_mb / 1024)} GB · ${version}`)}</small>
    <span class="cd">${coders.length ? esc("Coding: " + coders.map(t => t.version ? `${t.name} ${t.version}` : t.name).join(", ")) : '<span style="color:var(--muted)">No coding tools</span>'}</span>
    ${also ? `<small>${esc(also)}</small>` : ""}${ask}</div>`;
}

function drawMachines() {
  const list = MC.list ?? [], on = list.filter(m => m.online).length;
  const sum = !MC.list ? "" : [on && `${on} online`, list.length - on && `${list.length - on} offline`].filter(Boolean).join(", ");
  const head = `<div class="dhead"><span class="tt"><b>Machines</b><small>${esc(sum || "Where Beep runs commands and coding agents")}</small></span></div>`;
  const setup = '<code>beep-node setup</code>';
  bar.innerHTML = head + errLine(MC.error) + (MC.list && !list.length
    ? emptyPage(SERVER_ICON, "No machines yet.", `Press Add a machine in the Beep window and run the ${setup} command it shows on that computer, or turn on Let Beep use this Mac in Settings.`)
    : `<div class="dlist" style="padding-top:6px">${list.map(machineCard).join("")}</div>
      ${MC.list ? `<div class="dmuted" style="display:block;padding:0 18px 12px">To add one, press Add a machine in the Beep window and run the ${setup} command it shows.</div>` : ""}`)
    + hints([["esc", "back"]]);
  for (const b of bar.querySelectorAll<HTMLElement>("[data-rm]")) {
    b.addEventListener("click", () => {
      MC.asking = { id: b.dataset.rm!, busy: false, error: "" };
      drawMachines();
      bar.querySelector<HTMLElement>("#rmNo")?.focus();
    });
  }
  bar.querySelector("#rmYes")?.addEventListener("click", removeMachine);
  bar.querySelector("#rmNo")?.addEventListener("click", () => { MC.asking = null; drawMachines(); });
}
