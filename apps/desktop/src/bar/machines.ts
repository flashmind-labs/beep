import { bar, esc, invoke, S } from "./core.ts";
import { hints } from "./hints.ts";
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

// The machines running beep-node (ListMachines), from Settings.
export const MC: { list: Machine[] | null; error: string } = { list: null, error: "" };
const SERVER_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>';
/// The coding agents Beep can drive on a machine; its other tools follow them.
const CODERS = new Set(["claude", "codex", "gemini", "opencode", "cursor-agent", "amp", "aider"]);

export async function renderMachines() {
  S.view = "machines";
  invoke("close_group").catch(() => {});
  drawMachines();
  try {
    MC.list = await invoke<Machine[]>("list_machines");
    MC.error = "";
  } catch (e) {
    MC.error = String(e);
  }
  if (S.view === "machines") drawMachines();
}

function machineCard(m: Machine) {
  const env = m.env, now = Date.now();
  const tag = m.online ? '<span class="dtag ok">Online</span>' : `<span class="dtag">Seen ${span(now - m.seen_at)} ago</span>`;
  const version = /^\d/.test(m.version) ? `beep-node ${m.version}` : m.version;
  if (!env) return `<div class="dmach"><span class="nm">${esc(m.name)}${tag}</span><small>${esc(version)} · No report yet</small></div>`;
  const coders = env.tools.filter(t => CODERS.has(t.name)), rest = env.tools.filter(t => !CODERS.has(t.name)).map(t => t.name);
  const logins = env.logins.filter(l => l.signed_in).map(l => l.account ? `${l.tool} as ${l.account}` : `${l.tool} signed in`);
  const also = [rest.length ? `Also ${rest.join(", ")}` : "", logins.join(", ")].filter(Boolean).join(" · ");
  return `<div class="dmach"><span class="nm">${esc(m.name)}${tag}</span>
    <small>${esc(`${env.os_version}, ${env.arch} · ${env.cpus} CPUs, ${Math.round(env.memory_mb / 1024)} GB · ${version}`)}</small>
    <span class="cd">${coders.length ? esc("Coding: " + coders.map(t => t.version ? `${t.name} ${t.version}` : t.name).join(", ")) : '<span style="color:var(--muted)">No coding tools</span>'}</span>
    ${also ? `<small>${esc(also)}</small>` : ""}</div>`;
}

function drawMachines() {
  const list = MC.list ?? [], on = list.filter(m => m.online).length;
  const sum = !MC.list ? "" : [on && `${on} online`, list.length - on && `${list.length - on} offline`].filter(Boolean).join(", ");
  const head = `<div class="dhead"><span class="tt"><b>Machines</b><small>${esc(sum || "Where Beep runs commands and coding agents")}</small></span></div>`;
  const err = MC.error ? `<div class="derr" style="padding:8px 18px 0">${esc(MC.error)}</div>` : "";
  const setup = '<code>beep-node setup</code>';
  bar.innerHTML = head + err + (MC.list && !list.length
    ? `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${SERVER_ICON}</span><b style="font-size:14px">No machines yet.</b>
      <span class="dmuted" style="display:block">Press Add a machine in the Beep window and run the ${setup} command it shows on that computer, or turn on Let Beep use this Mac in Settings.</span></div>`
    : `<div class="dlist" style="padding-top:6px">${list.map(machineCard).join("")}</div>
      ${MC.list ? `<div class="dmuted" style="display:block;padding:0 18px 12px">To add one, press Add a machine in the Beep window and run the ${setup} command it shows.</div>` : ""}`)
    + hints([["esc", "back"]]);
}
